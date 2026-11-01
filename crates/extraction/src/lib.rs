//! Pluggable invoice extraction providers. AI output is always treated as
//! untrusted input and validated by the domain layer before persistence.
#![recursion_limit = "256"]

pub mod mock;
pub mod openai;
pub mod schema;

use async_trait::async_trait;
use thiserror::Error;

pub use mock::MockExtractionProvider;
pub use openai::{OpenAiCompatibleProvider, OpenAiConfig};
pub use schema::{ProviderInvoice, ProviderLineItem, ProviderPageReference, ProviderTaxLine};

/// How the configured OpenAI-compatible endpoint is asked to constrain output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructuredOutputMode {
    /// OpenAI-style strict JSON Schema response format.
    JsonSchema,
    /// Universal JSON object mode; the schema is supplied in the prompt.
    JsonObject,
}

#[derive(Debug, Clone)]
pub struct PageTextInput {
    pub page_number: u32,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct ExtractionInput {
    pub filename: String,
    pub country: String,
    pub base_currency: String,
    pub pages: Vec<PageTextInput>,
    pub used_ocr: bool,
    pub ocr_quality: Option<f64>,
}

impl ExtractionInput {
    pub fn prompt_text(&self, max_chars: usize) -> String {
        let mut out = String::new();
        for page in &self.pages {
            let header = format!("\n\n[Page {}]\n", page.page_number);
            if out.len() + header.len() + page.text.len() > max_chars {
                let remaining = max_chars.saturating_sub(out.len() + header.len());
                if remaining > 0 {
                    out.push_str(&header);
                    out.extend(page.text.chars().take(remaining));
                }
                break;
            }
            out.push_str(&header);
            out.push_str(&page.text);
        }
        out.trim().to_string()
    }
}

#[derive(Debug, Clone)]
pub struct ExtractionResult {
    pub invoice: invoxa_domain::invoice::ExtractedInvoice,
    pub raw: serde_json::Value,
    pub provider: String,
    pub model: String,
    pub is_mock: bool,
    pub input_tokens: Option<i32>,
    pub output_tokens: Option<i32>,
    pub duration_ms: i64,
    pub warnings: Vec<String>,
}

#[derive(Debug, Error)]
pub enum ExtractionError {
    #[error("extraction provider is not configured: {0}")]
    NotConfigured(String),
    #[error("extraction provider rejected the request: {0}")]
    Rejected(String),
    #[error("extraction provider rate limit exceeded")]
    RateLimited,
    #[error("extraction provider timed out")]
    Timeout,
    #[error("extraction provider is temporarily unavailable")]
    Unavailable,
    #[error("extraction provider circuit breaker is open")]
    CircuitOpen,
    #[error("extraction provider returned invalid structured data")]
    InvalidOutput,
    #[error("document text is not suitable for invoice extraction: {0}")]
    InvalidDocument(String),
}

impl ExtractionError {
    pub fn retryable(&self) -> bool {
        matches!(
            self,
            Self::RateLimited | Self::Timeout | Self::Unavailable | Self::CircuitOpen
        )
    }

    pub fn code(&self) -> &'static str {
        match self {
            Self::NotConfigured(_) => "AI_NOT_CONFIGURED",
            Self::Rejected(_) => "AI_REQUEST_REJECTED",
            Self::RateLimited => "AI_RATE_LIMITED",
            Self::Timeout => "AI_TIMEOUT",
            Self::Unavailable => "AI_UNAVAILABLE",
            Self::CircuitOpen => "AI_CIRCUIT_OPEN",
            Self::InvalidOutput => "AI_INVALID_OUTPUT",
            Self::InvalidDocument(_) => "AI_UNSUPPORTED_DOCUMENT",
        }
    }
}

#[async_trait]
pub trait ExtractionProvider: Send + Sync {
    fn name(&self) -> &str;
    fn model(&self) -> &str;
    fn is_mock(&self) -> bool;
    async fn extract(&self, input: ExtractionInput) -> Result<ExtractionResult, ExtractionError>;
}

pub fn build_mock() -> MockExtractionProvider {
    MockExtractionProvider::default()
}
