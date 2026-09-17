use anyhow::{Context, Result};
use http_public::{AppState, JwtKeys};
use persistence::Db;
use std::net::SocketAddr;

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    let database_url = std::env::var("DATABASE_URL")
        .context("必須設定 DATABASE_URL，且連線角色不可為 superuser")?;

    let jwt_secret = std::env::var("JWT_SECRET").unwrap_or_else(|_| {
        tracing::warn!("未設定 JWT_SECRET，使用開發預設值。正式環境必須設定");
        "dev-only-secret-do-not-use-in-production!!".into()
    });

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3001);

    // connect 內會檢查連線角色是否為 superuser，是的話直接失敗。
    // superuser 會繞過 RLS，租戶隔離形同虛設，不能讓它跑起來。
    let db = Db::connect(&database_url)
        .await
        .context("資料庫連線失敗")?;

    tracing::info!("資料庫已連線");

    let state = AppState {
        db,
        jwt: JwtKeys::new(jwt_secret.as_bytes(), 8),
        temporal: connect_temporal().await,
        internal_token: internal_token(),
        mailer: http_public::Mailer::from_env(),
    };

    let app = http_public::router(state).layer(
        tower_http::trace::TraceLayer::new_for_http()
            .make_span_with(tower_http::trace::DefaultMakeSpan::new().include_headers(false)),
    );

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("無法綁定 {addr}"))?;

    tracing::info!("API 啟動於 http://{addr}");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("伺服器異常結束")?;

    Ok(())
}

/// 讀取 internal API 密鑰
///
/// 未設定時回 None，整組 /internal 端點會回 403。
/// 預設停用而非給一組預設值：忘了設定時應該是「不能用」，
/// 不該是「用一組大家都知道的密鑰在跑」。
fn internal_token() -> Option<String> {
    match std::env::var("INTERNAL_API_TOKEN") {
        Ok(t) if !t.trim().is_empty() => {
            tracing::info!("internal API 已啟用");
            Some(t)
        }
        _ => {
            tracing::warn!(
                "未設定 INTERNAL_API_TOKEN，internal API 停用。\
                 Python Worker 將無法建立待辦，流程會停在第一個人工節點"
            );
            None
        }
    }
}

/// 連線 Temporal
///
/// 連不上時回 None 而非讓 API 啟動失敗。表單設計、權限矩陣這些
/// 功能不需要 Temporal，為了它讓整個系統起不來並不合理。
/// 需要 Temporal 的端點會各自回 503 並說明原因。
///
/// 代價是設定打錯時不會立刻發現。因此連線失敗記為 warn 而非 debug，
/// 並明確印出嘗試連線的位址。
async fn connect_temporal() -> Option<http_public::TemporalHandle> {
    let target = std::env::var("TEMPORAL_TARGET")
        .unwrap_or_else(|_| "http://localhost:7233".to_string());
    let namespace = std::env::var("TEMPORAL_NAMESPACE").unwrap_or_else(|_| "default".to_string());
    let task_queue =
        std::env::var("TEMPORAL_TASK_QUEUE").unwrap_or_else(|_| "workflow-platform".to_string());

    match temporal_client::TemporalClient::connect(&target, &namespace).await {
        Ok(client) => {
            tracing::info!(%target, %namespace, %task_queue, "Temporal 已連線");
            Some(http_public::TemporalHandle { client, task_queue })
        }
        Err(e) => {
            tracing::warn!(
                %target,
                error = %e,
                "Temporal 連線失敗，流程執行相關端點將回 503。其餘功能不受影響"
            );
            None
        }
    }
}

fn init_tracing() {
    use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,sqlx=warn,tower_http=debug"));

    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer())
        .init();
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c().await.expect("安裝 Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("安裝 SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!("收到關閉訊號，正在結束");
}
