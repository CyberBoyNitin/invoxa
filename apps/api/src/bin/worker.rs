use invoxa_api::config::{AppConfig, Environment};
use invoxa_api::state::{
    build_text_extractor, initialize_tracing, text_extraction_config, AppState,
};
use invoxa_integrations::webhook::WebhookSender;
use invoxa_workflows::export::ExportWorker;
use invoxa_workflows::pipeline::DocumentPipeline;
use invoxa_workflows::webhook::WebhookWorker;
use invoxa_workflows::{spawn_workers, WorkerConfig};
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    initialize_tracing();

    let config = AppConfig::from_env()?;
    if config.environment == Environment::Production && config.allow_mock_ai {
        warn!("ALLOW_MOCK_AI is enabled in production; mock output is labelled and never presented as real AI extraction.");
    }
    if config.ai.kind == invoxa_api::config::AiProviderKind::Mock {
        warn!(
            "worker is using the deterministic mock extractor; results are labelled is_mock=true"
        );
    }

    let db =
        invoxa_database::connect(&config.database_url, config.database_max_connections).await?;
    invoxa_database::migrate(&db).await?;
    let state = AppState::new(config.clone(), db)?;

    let text_config = text_extraction_config(&config);
    let text_extractor = build_text_extractor(&config);
    match text_extractor.ocr_available().await {
        true => info!(
            version = %text_extractor.ocr().version().await,
            "OCR engine available"
        ),
        false => warn!(
            "OCR engine unavailable. Text-based PDFs still work; scanned images will fail with a configuration error. Set TESSERACT_PATH."
        ),
    }
    let text_extractor = Arc::new(text_extractor);
    let worker_suffix = format!("{}-{}", std::process::id(), uuid::Uuid::new_v4().simple());

    let pipeline = Arc::new(DocumentPipeline::new(
        state.db.clone(),
        state.storage.clone(),
        text_extractor,
        state.extractor.clone(),
        text_config,
    ));
    let export = Arc::new(ExportWorker::new(
        state.db.clone(),
        state.storage.clone(),
        format!("invoxa-export-{worker_suffix}"),
    ));
    let sender = Arc::new(WebhookSender::new(state.webhook_policy.clone()));
    let webhook = Arc::new(WebhookWorker::new(
        state.db.clone(),
        state.cipher.clone(),
        sender,
        format!("invoxa-webhook-{worker_suffix}"),
    ));

    let worker_config = WorkerConfig {
        lease_seconds: config.worker_lease_seconds,
        poll_interval: Duration::from_secs(config.worker_poll_seconds.max(1)),
        recovery_interval: Duration::from_secs(60),
        document_concurrency: config.worker_concurrency.max(1),
        export_concurrency: 1,
        webhook_concurrency: 1,
    };
    let workers = spawn_workers(
        pipeline,
        export,
        webhook,
        state.db.clone(),
        worker_config,
        &format!("invoxa-{worker_suffix}"),
    )
    .map_err(|error| anyhow::anyhow!("could not start workers: {error}"))?;

    info!(
        provider = %config.ai.kind.name(),
        environment = ?config.environment,
        document_workers = config.worker_concurrency.max(1),
        "Invoxa worker started"
    );
    wait_for_shutdown().await;
    info!("shutdown requested; finishing leased work");
    workers.shutdown().await;
    info!("Invoxa worker stopped");
    Ok(())
}

async fn wait_for_shutdown() {
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
}
