//! 郵件寄送
//!
//! 抽成 trait 而非直接呼叫 lettre：測試不該真的寄信，
//! 而「寄信失敗時流程怎麼走」正是最需要測的部分。
//!
//! 未設定 SMTP_HOST 時整個功能停用（`Mailer::disabled()`），
//! 呼叫端仍會記稽核，只是不送出。開發環境不必為了跑流程而先架郵件伺服器。

use std::sync::Arc;

use lettre::message::{header::ContentType, Mailbox};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

/// 一封要寄出的信
#[derive(Debug, Clone)]
pub struct Mail {
    pub to: Vec<String>,
    pub subject: String,
    pub body: String,
}

/// 寄送結果
///
/// 分開「停用」與「失敗」：前者是預期的設定狀態，
/// 後者要寫進稽核讓人查。把兩者混為一談會讓稽核記錄
/// 充滿開發環境的假失敗。
#[derive(Debug, Clone, PartialEq)]
pub enum SendOutcome {
    Sent { accepted: usize },
    Disabled,
    Failed { reason: String },
}

/// 郵件傳輸層
///
/// 實作者要保證不 panic——寄信失敗是常態（網路、憑證、對方拒收），
/// 不該讓整個請求崩掉。
#[async_trait::async_trait]
pub trait Transport: Send + Sync {
    async fn send(&self, mail: &Mail) -> SendOutcome;
}

/// SMTP 設定
#[derive(Debug, Clone)]
pub struct SmtpConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    /// 寄件者。留空時用 username。
    pub from: String,
}

impl SmtpConfig {
    /// 從環境變數讀取
    ///
    /// SMTP_HOST 未設定時回 None——這是「功能停用」而非錯誤。
    /// 其餘欄位缺少才是設定錯誤，記 warning 並停用，
    /// 因為半套設定寄不出去，啟動時讓整個 API 掛掉更糟。
    pub fn from_env() -> Option<Self> {
        let host = std::env::var("SMTP_HOST").ok().filter(|s| !s.is_empty())?;

        let username = std::env::var("SMTP_USERNAME").unwrap_or_default();
        let password = std::env::var("SMTP_PASSWORD").unwrap_or_default();

        if username.is_empty() || password.is_empty() {
            tracing::warn!("已設定 SMTP_HOST 但缺少帳號或密碼，郵件功能停用");
            return None;
        }

        let port = std::env::var("SMTP_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(587);

        let from = std::env::var("SMTP_FROM")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| username.clone());

        Some(Self {
            host,
            port,
            username,
            password,
            from,
        })
    }
}

/// 真的走 SMTP 的傳輸
pub struct SmtpTransport {
    inner: AsyncSmtpTransport<Tokio1Executor>,
    from: String,
}

impl SmtpTransport {
    pub fn new(config: &SmtpConfig) -> Result<Self, String> {
        let creds = Credentials::new(config.username.clone(), config.password.clone());

        // STARTTLS 而非明文：Gmail 與多數服務都要求加密。
        // 587 埠的慣例就是 STARTTLS。
        let inner = AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&config.host)
            .map_err(|e| format!("建立 SMTP 連線失敗：{e}"))?
            .port(config.port)
            .credentials(creds)
            .build();

        Ok(Self {
            inner,
            from: config.from.clone(),
        })
    }
}

#[async_trait::async_trait]
impl Transport for SmtpTransport {
    async fn send(&self, mail: &Mail) -> SendOutcome {
        let from: Mailbox = match self.from.parse() {
            Ok(m) => m,
            Err(e) => {
                return SendOutcome::Failed {
                    reason: format!("寄件者位址無效（{}）：{e}", self.from),
                }
            }
        };

        let mut accepted = 0usize;
        let mut errors: Vec<String> = Vec::new();

        // 逐封寄送而非一次多個收件人：一個位址無效不該讓其他人也收不到。
        // 簽核提醒尤其如此——三個簽核人其中一個 email 打錯，
        // 另外兩個仍應收到。
        for addr in &mail.to {
            let to: Mailbox = match addr.parse() {
                Ok(m) => m,
                Err(e) => {
                    errors.push(format!("{addr}：位址無效（{e}）"));
                    continue;
                }
            };

            let message = match Message::builder()
                .from(from.clone())
                .to(to)
                .subject(&mail.subject)
                .header(ContentType::TEXT_PLAIN)
                .body(mail.body.clone())
            {
                Ok(m) => m,
                Err(e) => {
                    errors.push(format!("{addr}：組裝郵件失敗（{e}）"));
                    continue;
                }
            };

            match self.inner.send(message).await {
                Ok(_) => accepted += 1,
                Err(e) => errors.push(format!("{addr}：{e}")),
            }
        }

        if accepted > 0 && errors.is_empty() {
            return SendOutcome::Sent { accepted };
        }
        if accepted > 0 {
            // 部分成功仍算送出，但把失敗的記進稽核
            tracing::warn!(errors = ?errors, "部分收件人寄送失敗");
            return SendOutcome::Sent { accepted };
        }
        SendOutcome::Failed {
            reason: errors.join("；"),
        }
    }
}

/// 郵件服務
///
/// None 代表停用。呼叫端不必分辨「沒設定」與「設定錯誤」，
/// 兩者都是「不寄」。
#[derive(Clone)]
pub struct Mailer {
    transport: Option<Arc<dyn Transport>>,
}

impl Mailer {
    pub fn disabled() -> Self {
        Self { transport: None }
    }

    pub fn new(transport: Arc<dyn Transport>) -> Self {
        Self {
            transport: Some(transport),
        }
    }

    /// 依環境變數建立
    pub fn from_env() -> Self {
        let Some(config) = SmtpConfig::from_env() else {
            tracing::info!("未設定 SMTP，郵件功能停用（通知仍會記稽核）");
            return Self::disabled();
        };

        match SmtpTransport::new(&config) {
            Ok(t) => {
                tracing::info!(host = %config.host, port = config.port, "SMTP 已設定");
                Self::new(Arc::new(t))
            }
            Err(e) => {
                tracing::warn!("SMTP 初始化失敗，郵件功能停用：{e}");
                Self::disabled()
            }
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.transport.is_some()
    }

    pub async fn send(&self, mail: &Mail) -> SendOutcome {
        let Some(t) = &self.transport else {
            return SendOutcome::Disabled;
        };
        if mail.to.is_empty() {
            return SendOutcome::Failed {
                reason: "沒有收件人".into(),
            };
        }
        t.send(mail).await
    }
}

impl std::fmt::Debug for Mailer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Mailer")
            .field("enabled", &self.is_enabled())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// 記錄寄送請求但不真的送出
    struct Recorder {
        sent: Mutex<Vec<Mail>>,
        outcome: SendOutcome,
    }

    impl Recorder {
        fn new(outcome: SendOutcome) -> Arc<Self> {
            Arc::new(Self {
                sent: Mutex::new(Vec::new()),
                outcome,
            })
        }
    }

    #[async_trait::async_trait]
    impl Transport for Recorder {
        async fn send(&self, mail: &Mail) -> SendOutcome {
            self.sent.lock().expect("鎖").push(mail.clone());
            self.outcome.clone()
        }
    }

    fn mail() -> Mail {
        Mail {
            to: vec!["a@example.com".into()],
            subject: "測試".into(),
            body: "內容".into(),
        }
    }

    #[tokio::test]
    async fn disabled_mailer_does_not_send() {
        let m = Mailer::disabled();
        assert_eq!(m.send(&mail()).await, SendOutcome::Disabled);
        assert!(!m.is_enabled());
    }

    #[tokio::test]
    async fn enabled_mailer_sends() {
        let rec = Recorder::new(SendOutcome::Sent { accepted: 1 });
        let m = Mailer::new(rec.clone());

        assert_eq!(m.send(&mail()).await, SendOutcome::Sent { accepted: 1 });
        assert_eq!(rec.sent.lock().expect("鎖").len(), 1);
    }

    #[tokio::test]
    async fn empty_recipients_is_failure_not_send() {
        // 沒有收件人卻回報成功，會讓稽核記錄看起來正常而實際沒人收到
        let rec = Recorder::new(SendOutcome::Sent { accepted: 1 });
        let m = Mailer::new(rec.clone());

        let empty = Mail {
            to: vec![],
            ..mail()
        };
        assert!(matches!(m.send(&empty).await, SendOutcome::Failed { .. }));
        assert_eq!(rec.sent.lock().expect("鎖").len(), 0, "不該呼叫傳輸層");
    }

    #[tokio::test]
    async fn failure_is_reported() {
        let rec = Recorder::new(SendOutcome::Failed {
            reason: "連線被拒".into(),
        });
        let m = Mailer::new(rec);

        match m.send(&mail()).await {
            SendOutcome::Failed { reason } => assert!(reason.contains("連線被拒")),
            other => panic!("預期失敗，實際 {other:?}"),
        }
    }

    #[test]
    fn config_requires_host() {
        // 沒有 SMTP_HOST 就是停用，不是錯誤
        temp_env::with_var_unset("SMTP_HOST", || {
            assert!(SmtpConfig::from_env().is_none());
        });
    }

    #[test]
    fn config_without_credentials_is_disabled() {
        // 半套設定寄不出去。啟動時讓整個 API 掛掉更糟，因此停用。
        temp_env::with_vars(
            [
                ("SMTP_HOST", Some("smtp.example.com")),
                ("SMTP_USERNAME", Some("")),
                ("SMTP_PASSWORD", Some("")),
            ],
            || {
                assert!(SmtpConfig::from_env().is_none());
            },
        );
    }

    #[test]
    fn from_defaults_to_username() {
        temp_env::with_vars(
            [
                ("SMTP_HOST", Some("smtp.example.com")),
                ("SMTP_USERNAME", Some("bot@example.com")),
                ("SMTP_PASSWORD", Some("secret")),
                ("SMTP_FROM", None),
            ],
            || {
                let c = SmtpConfig::from_env().expect("應有設定");
                assert_eq!(c.from, "bot@example.com");
                assert_eq!(c.port, 587, "預設埠應為 587");
            },
        );
    }
}
