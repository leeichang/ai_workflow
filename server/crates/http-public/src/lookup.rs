//! 基本資料查詢
//!
//! 表單的下拉選單與關聯查詢欄位要能綁定組織資料——選部門、選人員、
//! 選角色。先前 `reference.source` 是個空字串，沒有東西可查。
//!
//! 為什麼不讓前端直接查 `app_user`：
//!   - 那會把 RLS 之外的欄位（password_hash、external_id）暴露出去
//!   - 下拉選單要的是 `{value, label}`，不是完整的資料列
//!   - 來源名稱要能穩定（`employee` 而非 `app_user`），日後換實體
//!     不影響既有表單定義
//!
//! 來源名稱刻意用 EIG canonical 的詞彙（`employee` 而非 `app_user`），
//! 與 `migrations/0009_organization.sql` 建立的 view 一致。

use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};
use crate::AppState;
use crate::auth::Actor;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/lookup/sources", get(list_sources))
        .route("/lookup/{source}", get(query_source))
}

/// 下拉選單的一個選項
///
/// `extra` 放額外屬性，供 `reference.fill`（選定後自動填入其他欄位）使用。
/// 例如選了員工之後自動帶出他的部門與職稱。
#[derive(Serialize)]
pub struct LookupItem {
    value: String,
    label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    extra: Option<serde_json::Value>,
}

/// 可用的資料來源
#[derive(Serialize)]
pub struct SourceInfo {
    /// 表單定義裡填的值
    name: &'static str,
    /// 給設計者看的中文名
    label: &'static str,
    /// 這個來源提供哪些額外屬性，供 reference.fill 使用
    extra_fields: &'static [&'static str],
}

/// 白名單
///
/// 刻意寫死而非從資料庫推導：來源是 API 契約的一部分，
/// 表單定義會存下這個名字。開放任意查詢等於讓表單設計者
/// 能讀到任何一張表。
const SOURCES: &[SourceInfo] = &[
    SourceInfo {
        name: "employee",
        label: "員工",
        extra_fields: &["employee_no", "job_title", "department_id", "email"],
    },
    SourceInfo {
        name: "department",
        label: "部門",
        extra_fields: &["code", "parent_id"],
    },
    SourceInfo {
        name: "role",
        label: "角色",
        extra_fields: &["code"],
    },
];

async fn list_sources(_actor: Actor) -> Json<&'static [SourceInfo]> {
    Json(SOURCES)
}

#[derive(Deserialize)]
pub struct LookupQuery {
    /// 關鍵字過濾。下拉選單輸入時用
    q: Option<String>,
    /// 上限。預設 50——下拉選單顯示不了更多，
    /// 使用者應該用關鍵字縮小範圍
    limit: Option<i64>,
}

const DEFAULT_LIMIT: i64 = 50;
const MAX_LIMIT: i64 = 200;

async fn query_source(
    State(state): State<AppState>,
    actor: Actor,
    Path(source): Path<String>,
    Query(q): Query<LookupQuery>,
) -> ApiResult<Json<Vec<LookupItem>>> {
    if !SOURCES.iter().any(|s| s.name == source) {
        let known = SOURCES
            .iter()
            .map(|s| s.name)
            .collect::<Vec<_>>()
            .join("、");
        return Err(ApiError::BadRequest(format!(
            "未知的資料來源「{source}」。可用的來源：{known}"
        )));
    }

    let limit = q.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    // 關鍵字用 ILIKE 的模糊比對。SQL 由參數綁定，不拼接。
    let pattern = q.q.as_deref().map(|s| format!("%{s}%"));

    let mut tx = state.db.tenant_tx(actor.tenant_id).await?;

    let items = match source.as_str() {
        "employee" => query_employee(&mut tx, pattern.as_deref(), limit).await?,
        "department" => query_department(&mut tx, pattern.as_deref(), limit).await?,
        "role" => query_role(&mut tx, pattern.as_deref(), limit).await?,
        // SOURCES 已經檢查過，這裡到不了
        other => return Err(ApiError::BadRequest(format!("未知的資料來源「{other}」"))),
    };

    tx.commit().await?;
    Ok(Json(items))
}

/// 員工
///
/// 只回在職且啟用的人——下拉選單選到離職者會讓流程指派給不存在的人。
/// 離職的判定用 `left_at`：已過離職日的不列入。
async fn query_employee(
    tx: &mut persistence::TenantTx<'_>,
    pattern: Option<&str>,
    limit: i64,
) -> ApiResult<Vec<LookupItem>> {
    let rows: Vec<(Uuid, String, Option<String>, Option<String>, Option<Uuid>, String)> =
        sqlx::query_as(
            "select id, name, employee_no, job_title, department_id, email \
             from app_user \
             where status = 'ACTIVE' \
               and (left_at is null or left_at > current_date) \
               and ($1::text is null \
                    or name ilike $1 \
                    or email ilike $1 \
                    or coalesce(employee_no, '') ilike $1) \
             order by coalesce(employee_no, ''), name \
             limit $2",
        )
        .bind(pattern)
        .bind(limit)
        .fetch_all(tx.executor())
        .await
        .map_err(|e| ApiError::Internal(format!("查詢員工失敗：{e}")))?;

    Ok(rows
        .into_iter()
        .map(|(id, name, employee_no, job_title, department_id, email)| {
            // 標籤帶員工編號，同名時才分得出來
            let label = match &employee_no {
                Some(no) if !no.is_empty() => format!("{no} {name}"),
                _ => name.clone(),
            };
            LookupItem {
                value: id.to_string(),
                label,
                extra: Some(serde_json::json!({
                    "employee_no": employee_no,
                    "job_title": job_title,
                    "department_id": department_id,
                    "email": email,
                })),
            }
        })
        .collect())
}

/// 部門
async fn query_department(
    tx: &mut persistence::TenantTx<'_>,
    pattern: Option<&str>,
    limit: i64,
) -> ApiResult<Vec<LookupItem>> {
    let rows: Vec<(Uuid, String, String, Option<Uuid>)> = sqlx::query_as(
        "select id, name, code, parent_id from department \
         where status = 'ACTIVE' \
           and ($1::text is null or name ilike $1 or code ilike $1) \
         order by sort_order, code \
         limit $2",
    )
    .bind(pattern)
    .bind(limit)
    .fetch_all(tx.executor())
    .await
    .map_err(|e| ApiError::Internal(format!("查詢部門失敗：{e}")))?;

    Ok(rows
        .into_iter()
        .map(|(id, name, code, parent_id)| LookupItem {
            value: id.to_string(),
            label: name,
            extra: Some(serde_json::json!({
                "code": code,
                "parent_id": parent_id,
            })),
        })
        .collect())
}

/// 角色
///
/// 值用 `code` 而非 id——流程的 resolver 寫的是角色代碼
/// （`role = approver`），表單存 id 的話兩邊對不起來。
async fn query_role(
    tx: &mut persistence::TenantTx<'_>,
    pattern: Option<&str>,
    limit: i64,
) -> ApiResult<Vec<LookupItem>> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "select code, name from role \
         where ($1::text is null or name ilike $1 or code ilike $1) \
         order by code \
         limit $2",
    )
    .bind(pattern)
    .bind(limit)
    .fetch_all(tx.executor())
    .await
    .map_err(|e| ApiError::Internal(format!("查詢角色失敗：{e}")))?;

    Ok(rows
        .into_iter()
        .map(|(code, name)| LookupItem {
            value: code.clone(),
            label: name,
            extra: Some(serde_json::json!({ "code": code })),
        })
        .collect())
}
