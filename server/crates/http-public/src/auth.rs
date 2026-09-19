//! 認證與授權
//!
//! JWT 帶 tenant_id、user_id、roles。每個受保護的請求都會解析成
//! [`Actor`]，再由它開啟綁定租戶的資料庫交易。
//!
//! 設計重點：取得資料庫連線必須經過 Actor，無法繞過租戶設定。

use argon2::password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, SaltString};
use argon2::{Argon2, PasswordVerifier};
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::ApiError;

/// JWT payload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    /// user_id
    pub sub: Uuid,
    pub tenant_id: Uuid,
    pub name: String,
    pub roles: Vec<String>,
    pub exp: i64,
    pub iat: i64,
}

/// 已認證的使用者
///
/// 由 extractor 產生，handler 直接宣告為參數即可取得。
/// 沒有 token 或 token 無效時，請求在進入 handler 前就被擋下。
#[derive(Debug, Clone)]
pub struct Actor {
    pub user_id: Uuid,
    pub tenant_id: Uuid,
    pub name: String,
    pub roles: Vec<String>,
}

impl Actor {
    pub fn has_role(&self, role: &str) -> bool {
        self.roles.iter().any(|r| r == role)
    }

    /// admin 擁有所有權限，不需逐一授予
    pub fn is_admin(&self) -> bool {
        self.has_role("admin")
    }

    /// 看得到全租戶流程、並能介入處理
    ///
    /// 語意是「營運端負責盯流程、排除卡關」，單獨一個角色。
    /// 刻意不把 approver、finance_manager 一併放行：那些角色是
    /// 「會簽到某些單」不是「該看到全部單」，混為一談之後很難再拆開，
    /// 而且會讓監控變成全租戶的資料出口。
    ///
    /// 做成方法而非在各處 `has_role("process_monitor")`：
    /// 這個判斷散在清單、單筆、取消、改派、催辦五處，
    /// 任何一處漏掉 admin 或拼錯角色名都是權限漏洞。
    pub fn can_monitor_all(&self) -> bool {
        self.is_admin() || self.has_role("process_monitor")
    }

    /// 要求具備指定角色之一，否則回 403
    pub fn require_any(&self, roles: &[&str]) -> Result<(), ApiError> {
        if self.is_admin() || roles.iter().any(|r| self.has_role(r)) {
            return Ok(());
        }
        Err(ApiError::Forbidden(format!(
            "需要下列角色之一：{}",
            roles.join("、")
        )))
    }
}

impl<S> FromRequestParts<S> for Actor
where
    S: Send + Sync,
    JwtKeys: axum::extract::FromRef<S>,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        use axum::extract::FromRef;

        let token = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .ok_or_else(|| ApiError::Unauthorized("缺少 Authorization 標頭".into()))?;

        let keys = JwtKeys::from_ref(state);
        let claims = keys.verify(token)?;

        Ok(Actor {
            user_id: claims.sub,
            tenant_id: claims.tenant_id,
            name: claims.name,
            roles: claims.roles,
        })
    }
}

/// JWT 簽發與驗證
#[derive(Clone)]
pub struct JwtKeys {
    encoding: EncodingKey,
    decoding: DecodingKey,
    ttl_hours: i64,
}

impl std::fmt::Debug for JwtKeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // 不印出金鑰內容
        f.debug_struct("JwtKeys")
            .field("ttl_hours", &self.ttl_hours)
            .finish_non_exhaustive()
    }
}

impl JwtKeys {
    pub fn new(secret: &[u8], ttl_hours: i64) -> Self {
        Self {
            encoding: EncodingKey::from_secret(secret),
            decoding: DecodingKey::from_secret(secret),
            ttl_hours,
        }
    }

    pub fn issue(
        &self,
        user_id: Uuid,
        tenant_id: Uuid,
        name: String,
        roles: Vec<String>,
    ) -> Result<String, ApiError> {
        let now = Utc::now();
        let claims = Claims {
            sub: user_id,
            tenant_id,
            name,
            roles,
            iat: now.timestamp(),
            exp: (now + Duration::hours(self.ttl_hours)).timestamp(),
        };
        encode(&Header::default(), &claims, &self.encoding)
            .map_err(|e| ApiError::Internal(format!("簽發 token 失敗：{e}")))
    }

    pub fn verify(&self, token: &str) -> Result<Claims, ApiError> {
        decode::<Claims>(token, &self.decoding, &Validation::default())
            .map(|d| d.claims)
            .map_err(|e| {
                use jsonwebtoken::errors::ErrorKind;
                match e.kind() {
                    ErrorKind::ExpiredSignature => ApiError::Unauthorized("token 已過期".into()),
                    _ => ApiError::Unauthorized("token 無效".into()),
                }
            })
    }
}

// ── 密碼 ────────────────────────────────────────────────

pub fn hash_password(plain: &str) -> Result<String, ApiError> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(plain.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| ApiError::Internal(format!("密碼雜湊失敗：{e}")))
}

/// 驗證密碼
///
/// 回傳 bool 而非 Result，呼叫端不應區分「雜湊格式錯誤」與「密碼不符」，
/// 兩者對使用者都只是登入失敗。區分開來會給攻擊者額外資訊。
pub fn verify_password(plain: &str, hash: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(plain.as_bytes(), &parsed)
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn actor_with(roles: &[&str]) -> Actor {
        Actor {
            user_id: Uuid::nil(),
            tenant_id: Uuid::nil(),
            name: "測試".into(),
            roles: roles.iter().map(|r| (*r).to_string()).collect(),
        }
    }

    #[test]
    fn monitor_role_sees_everything() {
        assert!(actor_with(&["process_monitor"]).can_monitor_all());
        assert!(actor_with(&["admin"]).can_monitor_all());
    }

    #[test]
    fn approval_roles_are_not_monitors() {
        // **安全回歸。** 這是 N1 的核心決定：approver 與
        // finance_manager 的語意是「會簽到某些單」，不是
        // 「該看到全部單」。任何人把它們加進 can_monitor_all
        // 都會讓監控變成全租戶的資料出口，這條會紅。
        for role in ["approver", "finance_manager", "designer", "requester", "viewer"] {
            assert!(
                !actor_with(&[role]).can_monitor_all(),
                "{role} 不該看得到全部流程"
            );
        }
    }

    #[test]
    fn no_roles_sees_nothing() {
        assert!(!actor_with(&[]).can_monitor_all());
    }

    #[test]
    fn password_roundtrip() {
        let hash = hash_password("correct horse battery staple").unwrap();
        assert!(verify_password("correct horse battery staple", &hash));
        assert!(!verify_password("wrong password", &hash));
    }

    #[test]
    fn malformed_hash_fails_closed() {
        assert!(!verify_password("anything", "not-a-valid-hash"));
        assert!(!verify_password("anything", ""));
    }

    #[test]
    fn jwt_roundtrip_preserves_claims() {
        let keys = JwtKeys::new(b"test-secret-at-least-32-bytes-long!!", 1);
        let uid = Uuid::new_v4();
        let tid = Uuid::new_v4();

        let token = keys
            .issue(uid, tid, "王小明".into(), vec!["approver".into()])
            .unwrap();
        let claims = keys.verify(&token).unwrap();

        assert_eq!(claims.sub, uid);
        assert_eq!(claims.tenant_id, tid);
        assert_eq!(claims.name, "王小明");
        assert_eq!(claims.roles, vec!["approver"]);
    }

    #[test]
    fn rejects_token_signed_with_other_secret() {
        let a = JwtKeys::new(b"secret-a-at-least-32-bytes-long-ok!!", 1);
        let b = JwtKeys::new(b"secret-b-at-least-32-bytes-long-ok!!", 1);

        let token = a
            .issue(Uuid::new_v4(), Uuid::new_v4(), "x".into(), vec![])
            .unwrap();
        assert!(b.verify(&token).is_err(), "不同金鑰簽發的 token 必須被拒");
    }

    #[test]
    fn rejects_expired_token() {
        let keys = JwtKeys::new(b"test-secret-at-least-32-bytes-long!!", -1);
        let token = keys
            .issue(Uuid::new_v4(), Uuid::new_v4(), "x".into(), vec![])
            .unwrap();
        let err = keys.verify(&token).unwrap_err();
        assert!(matches!(err, ApiError::Unauthorized(_)));
    }

    #[test]
    fn admin_bypasses_role_check() {
        let admin = Actor {
            user_id: Uuid::new_v4(),
            tenant_id: Uuid::new_v4(),
            name: "管理員".into(),
            roles: vec!["admin".into()],
        };
        assert!(admin.require_any(&["designer"]).is_ok());
    }

    #[test]
    fn missing_role_is_forbidden() {
        let viewer = Actor {
            user_id: Uuid::new_v4(),
            tenant_id: Uuid::new_v4(),
            name: "訪客".into(),
            roles: vec!["viewer".into()],
        };
        assert!(matches!(
            viewer.require_any(&["designer", "admin"]),
            Err(ApiError::Forbidden(_))
        ));
    }
}
