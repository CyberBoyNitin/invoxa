use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use invoxa_database::DbError;
use serde_json::{json, Value};

#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
    pub details: Option<Value>,
}

pub type ApiResult<T> = Result<T, ApiError>;

impl ApiError {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
            details: None,
        }
    }

    pub fn with_details(mut self, details: Value) -> Self {
        self.details = Some(details);
        self
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "bad_request", message)
    }

    pub fn validation(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "validation_failed",
            message,
        )
    }

    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "unauthorized", message)
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, "forbidden", message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", message)
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, "conflict", message)
    }

    pub fn payload_too_large(message: impl Into<String>) -> Self {
        Self::new(StatusCode::PAYLOAD_TOO_LARGE, "payload_too_large", message)
    }

    pub fn unsupported_media_type(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "unsupported_media_type",
            message,
        )
    }

    pub fn rate_limited(retry_after: u64) -> Self {
        Self {
            status: StatusCode::TOO_MANY_REQUESTS,
            code: "rate_limited",
            message: "Too many requests. Please retry later.".into(),
            details: Some(json!({ "retry_after_seconds": retry_after })),
        }
    }

    pub fn internal(context: &str, error: impl std::fmt::Display) -> Self {
        tracing::error!(context, error = %error, "internal API error");
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            "An internal error occurred. The request was not completed.",
        )
    }

    pub fn service_unavailable(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "service_unavailable",
            message,
        )
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut response = (
            self.status,
            Json(json!({
                "error": {
                    "code": self.code,
                    "message": self.message,
                    "details": self.details,
                }
            })),
        )
            .into_response();
        if self.status == StatusCode::TOO_MANY_REQUESTS {
            if let Some(seconds) = self
                .details
                .as_ref()
                .and_then(|details| details.get("retry_after_seconds"))
                .and_then(Value::as_u64)
            {
                if let Ok(value) = axum::http::HeaderValue::from_str(&seconds.to_string()) {
                    response
                        .headers_mut()
                        .insert(axum::http::header::RETRY_AFTER, value);
                }
            }
        }
        response
    }
}

impl From<DbError> for ApiError {
    fn from(error: DbError) -> Self {
        match error {
            DbError::NotFound => Self::not_found("The requested resource was not found."),
            DbError::Conflict(message) => Self::conflict(message),
            DbError::InvalidTransition(message) => {
                Self::new(StatusCode::CONFLICT, "invalid_transition", message)
            }
            DbError::Validation(message) => Self::validation(message),
            DbError::Sqlx(error) => Self::internal("database", error),
        }
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(error: sqlx::Error) -> Self {
        Self::internal("database", error)
    }
}

impl From<invoxa_security::SecurityError> for ApiError {
    fn from(error: invoxa_security::SecurityError) -> Self {
        Self::internal("security", error)
    }
}

impl From<invoxa_documents::storage::StorageError> for ApiError {
    fn from(error: invoxa_documents::storage::StorageError) -> Self {
        match error {
            invoxa_documents::storage::StorageError::NotFound => {
                Self::not_found("The stored file was not found.")
            }
            invoxa_documents::storage::StorageError::InvalidKey => {
                Self::internal("storage key", "invalid storage key")
            }
            other => Self::internal("storage", other),
        }
    }
}
