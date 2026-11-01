use invoxa_api::config::{AppConfig, Environment};
use invoxa_api::router::build_router;
use invoxa_api::state::{initialize_tracing, AppState};
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tracing::{info, warn};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    initialize_tracing();

    let config = AppConfig::from_env()?;
    if config.environment == Environment::Production && config.allow_mock_ai {
        warn!(
            "ALLOW_MOCK_AI is enabled in production. Mock extraction output is clearly labelled and must never be treated as a real AI result."
        );
    }
    if config.ai.kind == invoxa_api::config::AiProviderKind::Mock {
        warn!(
            "AI_PROVIDER=mock: invoice extraction uses deterministic labels only. All mock results are marked is_mock=true and require review."
        );
    }

    let db =
        invoxa_database::connect(&config.database_url, config.database_max_connections).await?;
    invoxa_database::migrate(&db).await?;
    info!("database migrations are up to date");

    let state = AppState::new(config.clone(), db)?;
    let app = build_router(state);
    let listener = TcpListener::bind(config.bind_addr).await?;
    info!(
        address = %config.bind_addr,
        environment = ?config.environment,
        provider = %config.ai.kind.name(),
        storage = %config.storage_root.display(),
        "Invoxa API listening"
    );

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;
    info!("Invoxa API stopped");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut signal) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            signal.recv().await;
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    info!("shutdown requested");
}
