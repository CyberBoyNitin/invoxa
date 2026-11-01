use crate::config::{AiProviderKind, AppConfig};
use crate::rate_limit::RateLimiter;
use invoxa_database::Db;
use invoxa_documents::storage::{LocalStorage, ObjectStorage};
use invoxa_documents::text::{TextExtractionConfig, TextExtractor};
use invoxa_extraction::{ExtractionProvider, MockExtractionProvider, OpenAiCompatibleProvider};
use invoxa_integrations::webhook::SsrfPolicy;
use invoxa_security::{CsrfProtector, SecretCipher};
use std::sync::Arc;
use std::time::Instant;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<AppConfig>,
    pub db: Db,
    pub storage: Arc<dyn ObjectStorage>,
    pub extractor: Arc<dyn ExtractionProvider>,
    pub cipher: Arc<SecretCipher>,
    pub csrf: Arc<CsrfProtector>,
    pub rate_limiter: Arc<RateLimiter>,
    pub webhook_policy: SsrfPolicy,
    pub started_at: Instant,
}

impl AppState {
    pub fn new(config: AppConfig, db: Db) -> anyhow::Result<Self> {
        let storage = LocalStorage::new(&config.storage_root, config.storage_max_bytes)?;
        let extractor: Arc<dyn ExtractionProvider> = match config.ai.kind {
            AiProviderKind::Mock => Arc::new(MockExtractionProvider),
            AiProviderKind::OpenAiCompatible => {
                let openai =
                    config.ai.openai.clone().ok_or_else(|| {
                        anyhow::anyhow!("OpenAI provider configuration is missing")
                    })?;
                Arc::new(OpenAiCompatibleProvider::new(openai).map_err(|error| {
                    anyhow::anyhow!("could not initialize AI provider: {error}")
                })?)
            }
        };
        let cipher = SecretCipher::from_bytes(&config.encryption_key)
            .map_err(|error| anyhow::anyhow!("invalid ENCRYPTION_KEY: {error}"))?;
        let csrf = CsrfProtector::new(&config.csrf_secret)
            .map_err(|error| anyhow::anyhow!("invalid CSRF_SECRET: {error}"))?;
        let policy = if config.webhook_allow_http {
            SsrfPolicy::default()
                .allow_http(config.webhook_http_ports.iter().copied())
                .with_resolution_timeout(config.webhook_validation_timeout)
        } else {
            SsrfPolicy::default().with_resolution_timeout(config.webhook_validation_timeout)
        };
        Ok(Self {
            config: Arc::new(config),
            db,
            storage: Arc::new(storage),
            extractor,
            cipher: Arc::new(cipher),
            csrf: Arc::new(csrf),
            rate_limiter: Arc::new(RateLimiter::new()),
            webhook_policy: policy,
            started_at: Instant::now(),
        })
    }

    pub fn provider_name(&self) -> &str {
        self.extractor.name()
    }

    pub fn provider_is_mock(&self) -> bool {
        self.extractor.is_mock()
    }

    pub fn storage_backend(&self) -> &'static str {
        self.storage.backend_name()
    }
}

pub fn text_extraction_config(config: &AppConfig) -> TextExtractionConfig {
    TextExtractionConfig {
        limits: config.upload_limits.clone(),
        ocr: config.ocr.clone(),
        ocr_enabled: true,
        ..TextExtractionConfig::default()
    }
}

/// OCR settings shared by the HTTP server and worker. The API's multipart
/// validation and the worker use the same page/pixel/file limits.
pub fn build_text_extractor(config: &AppConfig) -> TextExtractor {
    TextExtractor::new(text_extraction_config(config))
}

pub fn initialize_tracing() {
    use tracing_subscriber::EnvFilter;
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new("info,invoxa_api=debug,invoxa_workflows=debug,invoxa_documents=info")
    });
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .try_init();
}
