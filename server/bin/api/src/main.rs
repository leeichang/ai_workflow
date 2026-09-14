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
