//! Server configuration. Secrets are read from the environment only and are
//! never serialized into API responses or logs.

use base64::Engine;
use invoxa_documents::ocr::OcrConfig;
use invoxa_documents::validation::UploadLimits;
use invoxa_extraction::{OpenAiConfig, StructuredOutputMode};
use std::collections::BTreeSet;
use std::env;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Environment {
    Development,
    Test,
    Production,
}

impl Environment {
    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "production" | "prod" => Self::Production,
            "test" => Self::Test,
            _ => Self::Development,
        }
    }

    pub fn is_production(self) -> bool {
        self == Self::Production
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AiProviderKind {
    Mock,
    OpenAiCompatible,
}

impl AiProviderKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Mock => "mock-heuristic",
            Self::OpenAiCompatible => "openai-compatible",
        }
    }
}

#[derive(Clone)]
pub struct AiSettings {
    pub kind: AiProviderKind,
    pub openai: Option<OpenAiConfig>,
}

#[derive(Clone)]
pub struct AppConfig {
    pub environment: Environment,
    pub bind_addr: SocketAddr,
    pub app_url: String,
    pub allowed_origins: Vec<String>,
    pub database_url: String,
    pub database_max_connections: u32,
    pub storage_root: PathBuf,
    pub storage_max_bytes: u64,
    pub encryption_key: Vec<u8>,
    pub csrf_secret: Vec<u8>,
    pub download_signing_key: Vec<u8>,
    pub session_ttl_hours: i64,
    pub cookie_secure: bool,
    pub trust_proxy: bool,
    pub upload_limits: UploadLimits,
    pub upload_batch_limit: usize,
    pub upload_body_limit: usize,
    pub ocr: OcrConfig,
    pub ai: AiSettings,
    pub allow_mock_ai: bool,
    pub expose_dev_tokens: bool,
    pub webhook_allow_http: bool,
    pub webhook_http_ports: BTreeSet<u16>,
    pub webhook_validation_timeout: Duration,
    pub auth_rate_limit_per_minute: u32,
    pub api_key_rate_limit_per_minute: u32,
    pub worker_poll_seconds: u64,
    pub worker_lease_seconds: i32,
    pub worker_concurrency: usize,
}

impl AppConfig {
    pub fn from_env() -> anyhow::Result<Self> {
        let environment = Environment::parse(&env_or("APP_ENV", "development"));
        let app_url = env_or("APP_URL", "http://localhost:3000")
            .trim_end_matches('/')
            .to_string();
        let mut allowed_origins = vec![app_url.clone()];
        for origin in env_or("ALLOWED_ORIGINS", "")
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            let origin = origin.trim_end_matches('/').to_string();
            if !allowed_origins.contains(&origin) {
                allowed_origins.push(origin);
            }
        }

        let bind_addr = env_or("BIND_ADDR", "127.0.0.1:8080")
            .parse::<SocketAddr>()
            .map_err(|_| {
                anyhow::anyhow!("BIND_ADDR must be a socket address such as 127.0.0.1:8080")
            })?;

        let encryption_key = decode_key(
            &env::var("ENCRYPTION_KEY").map_err(|_| {
                anyhow::anyhow!(
                    "ENCRYPTION_KEY is required: generate 32 random bytes and base64-encode them"
                )
            })?,
            "ENCRYPTION_KEY",
        )?;
        let csrf_secret = decode_key(
            &env::var("CSRF_SECRET").map_err(|_| {
                anyhow::anyhow!(
                    "CSRF_SECRET is required: generate 32 random bytes and base64-encode them"
                )
            })?,
            "CSRF_SECRET",
        )?;
        let download_signing_key = match env::var("DOWNLOAD_SIGNING_KEY") {
            Ok(value) if !value.trim().is_empty() => decode_key(&value, "DOWNLOAD_SIGNING_KEY")?,
            _ => derive_key(&encryption_key, b"invoxa-download-grants-v1"),
        };

        let database_url = env_or(
            "DATABASE_URL",
            "postgres://invoxa:invoxa_dev_password@localhost:55432/invoxa",
        );
        let database_max_connections = parse_env("DATABASE_MAX_CONNECTIONS", 20u32)?;
        let storage_root = PathBuf::from(env_or("STORAGE_ROOT", ".local/storage"));
        let storage_max_bytes = parse_env("STORAGE_MAX_FILE_BYTES", 20u64 * 1024 * 1024)?;
        let session_ttl_hours = parse_env("SESSION_TTL_HOURS", 24i64)?;
        if !(1..=720).contains(&session_ttl_hours) {
            anyhow::bail!("SESSION_TTL_HOURS must be between 1 and 720");
        }

        let cookie_secure = parse_env("COOKIE_SECURE", environment.is_production())?;
        let trust_proxy = parse_env("TRUST_PROXY", false)?;
        if environment.is_production() && !app_url.starts_with("https://") {
            anyhow::bail!("APP_URL must use https in production");
        }
        if environment.is_production() && !cookie_secure {
            anyhow::bail!("COOKIE_SECURE cannot be disabled in production");
        }

        let max_file_bytes = parse_env("UPLOAD_MAX_BYTES", 20u64 * 1024 * 1024)?;
        if !(1024..=100 * 1024 * 1024).contains(&max_file_bytes) {
            anyhow::bail!("UPLOAD_MAX_BYTES must be between 1 KiB and 100 MiB");
        }
        let upload_limits = UploadLimits {
            max_file_bytes,
            max_pages: parse_env("UPLOAD_MAX_PAGES", 50u32)?,
            max_image_pixels: parse_env("UPLOAD_MAX_IMAGE_PIXELS", 25_000_000u64)?,
        };
        let upload_batch_limit = parse_env("UPLOAD_MAX_BATCH", 20usize)?;
        if !(1..=20).contains(&upload_batch_limit) {
            anyhow::bail!("UPLOAD_MAX_BATCH must be between 1 and 20");
        }
        // Multipart envelopes add a little overhead. The per-file limit is
        // enforced while streaming; this is only an outer guard.
        let upload_body_limit = ((max_file_bytes as usize).saturating_mul(upload_batch_limit))
            .saturating_add(2 * 1024 * 1024);

        let ocr = OcrConfig {
            tesseract_path: env::var("TESSERACT_PATH")
                .ok()
                .filter(|v| !v.trim().is_empty()),
            language: env_or("OCR_LANGUAGE", "eng"),
            timeout_secs: parse_env("OCR_TIMEOUT_SECONDS", 60u64)?,
            page_segmentation_mode: env_or("OCR_PAGE_SEGMENTATION_MODE", "6"),
        };

        let ai_provider = env_or("AI_PROVIDER", "mock").to_ascii_lowercase();
        let allow_mock_ai = parse_env("ALLOW_MOCK_AI", !environment.is_production())?;
        let ai = match ai_provider.as_str() {
            "mock" => {
                if environment.is_production() && !allow_mock_ai {
                    anyhow::bail!(
                        "AI_PROVIDER=mock is not allowed in production unless ALLOW_MOCK_AI=true. \
                         Mock extraction is synthetic test data and must never be presented as real AI output."
                    );
                }
                AiSettings {
                    kind: AiProviderKind::Mock,
                    openai: None,
                }
            }
            "openai" | "openai-compatible" | "openai_compatible" => {
                let base_url = env::var("AI_BASE_URL").map_err(|_| {
                    anyhow::anyhow!("AI_BASE_URL is required when AI_PROVIDER=openai")
                })?;
                let model = env::var("AI_MODEL")
                    .map_err(|_| anyhow::anyhow!("AI_MODEL is required when AI_PROVIDER=openai"))?;
                let api_key = env::var("AI_API_KEY").ok().filter(|v| !v.trim().is_empty());
                let structured_output = match env_or("AI_STRUCTURED_OUTPUT", "json_schema")
                    .to_ascii_lowercase()
                    .as_str()
                {
                    "json_object" => StructuredOutputMode::JsonObject,
                    _ => StructuredOutputMode::JsonSchema,
                };
                let mut config = OpenAiConfig::new(base_url, api_key, model);
                config.timeout =
                    Duration::from_secs(parse_env("AI_TIMEOUT_SECONDS", 60u64)?.clamp(1, 300));
                config.max_retries = parse_env("AI_MAX_RETRIES", 2u32)?.min(5);
                config.max_input_chars = parse_env("AI_MAX_INPUT_CHARS", 60_000usize)?;
                config.structured_output = structured_output;
                config.organization = env::var("AI_ORGANIZATION").ok().filter(|v| !v.is_empty());
                config.circuit_failure_threshold = parse_env("AI_CIRCUIT_FAILURES", 5u32)?;
                config.circuit_cooldown =
                    Duration::from_secs(parse_env("AI_CIRCUIT_COOLDOWN_SECONDS", 60u64)?);
                config
                    .validate()
                    .map_err(|error| anyhow::anyhow!("{error}"))?;
                AiSettings {
                    kind: AiProviderKind::OpenAiCompatible,
                    openai: Some(config),
                }
            }
            other => {
                anyhow::bail!("unsupported AI_PROVIDER '{other}'; expected 'mock' or 'openai'")
            }
        };

        let expose_dev_tokens = parse_env("EXPOSE_DEV_TOKENS", !environment.is_production())?;
        if environment.is_production() && expose_dev_tokens {
            anyhow::bail!("EXPOSE_DEV_TOKENS cannot be enabled in production");
        }

        let webhook_allow_http = parse_env("WEBHOOK_ALLOW_HTTP", false)?;
        let mut webhook_http_ports = BTreeSet::new();
        for port in env_or("WEBHOOK_HTTP_PORTS", "")
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            webhook_http_ports
                .insert(port.parse::<u16>().map_err(|_| {
                    anyhow::anyhow!("WEBHOOK_HTTP_PORTS must contain port numbers")
                })?);
        }

        Ok(Self {
            environment,
            bind_addr,
            app_url,
            allowed_origins,
            database_url,
            database_max_connections,
            storage_root,
            storage_max_bytes,
            encryption_key,
            csrf_secret,
            download_signing_key,
            session_ttl_hours,
            cookie_secure,
            trust_proxy,
            upload_limits,
            upload_batch_limit,
            upload_body_limit,
            ocr,
            ai,
            allow_mock_ai,
            expose_dev_tokens,
            webhook_allow_http,
            webhook_http_ports,
            webhook_validation_timeout: Duration::from_secs(parse_env(
                "WEBHOOK_VALIDATION_TIMEOUT_SECONDS",
                5u64,
            )?),
            auth_rate_limit_per_minute: parse_env("AUTH_RATE_LIMIT_PER_MINUTE", 30u32)?,
            api_key_rate_limit_per_minute: parse_env("API_KEY_RATE_LIMIT_PER_MINUTE", 120u32)?,
            worker_poll_seconds: parse_env("WORKER_POLL_SECONDS", 2u64)?,
            worker_lease_seconds: parse_env("WORKER_LEASE_SECONDS", 120i32)?,
            worker_concurrency: parse_env("WORKER_CONCURRENCY", 2usize)?,
        })
    }

    /// Origins that may make browser requests with credentials.
    pub fn origin_allowed(&self, origin: &str) -> bool {
        let origin = origin.trim_end_matches('/');
        self.allowed_origins
            .iter()
            .any(|allowed| allowed.eq_ignore_ascii_case(origin))
    }

    pub fn secure_cookies(&self) -> bool {
        self.cookie_secure
    }
}

fn env_or(key: &str, default: &str) -> String {
    env::var(key).unwrap_or_else(|_| default.to_string())
}

fn parse_env<T>(key: &str, default: T) -> anyhow::Result<T>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    match env::var(key) {
        Ok(value) if !value.trim().is_empty() => value
            .trim()
            .parse::<T>()
            .map_err(|error| anyhow::anyhow!("{key} is invalid: {error}")),
        _ => Ok(default),
    }
}

fn decode_key(value: &str, name: &str) -> anyhow::Result<Vec<u8>> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(value.trim())
        .map_err(|_| anyhow::anyhow!("{name} must be valid base64"))?;
    if bytes.len() != 32 {
        anyhow::bail!("{name} must decode to exactly 32 bytes");
    }
    Ok(bytes)
}

fn derive_key(secret: &[u8], context: &[u8]) -> Vec<u8> {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(secret);
    hasher.update(context);
    hasher.finalize().to_vec()
}
