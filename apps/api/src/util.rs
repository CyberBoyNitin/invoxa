use crate::error::{ApiError, ApiResult};
use axum::extract::{FromRequest, FromRequestParts, Path, Query, Request};
use axum::http::request::Parts;
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::Json;
use rust_decimal::Decimal;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::net::SocketAddr;
use std::str::FromStr;

// ---------------------------------------------------------------------------
// JSON / query / path extractors with a consistent JSON error shape
// ---------------------------------------------------------------------------

pub struct ApiJson<T>(pub T);

impl<S, T> FromRequest<S> for ApiJson<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        match Json::<T>::from_request(req, state).await {
            Ok(Json(value)) => Ok(Self(value)),
            Err(rejection) => {
                let status = rejection.status();
                if status == StatusCode::PAYLOAD_TOO_LARGE {
                    Err(ApiError::payload_too_large(
                        "The request body exceeds the configured limit.",
                    ))
                } else if status == StatusCode::UNSUPPORTED_MEDIA_TYPE {
                    Err(ApiError::unsupported_media_type(
                        "Content-Type must be application/json.",
                    ))
                } else {
                    Err(ApiError::bad_request("The JSON request body is invalid."))
                }
            }
        }
    }
}

pub struct ApiQuery<T>(pub T);

impl<S, T> FromRequestParts<S> for ApiQuery<T>
where
    T: DeserializeOwned + Send,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        Query::<T>::from_request_parts(parts, state)
            .await
            .map(|Query(value)| Self(value))
            .map_err(|_| ApiError::bad_request("The query string is invalid."))
    }
}

pub struct ApiPath<T>(pub T);

impl<S, T> FromRequestParts<S> for ApiPath<T>
where
    T: DeserializeOwned + Send,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        Path::<T>::from_request_parts(parts, state)
            .await
            .map(|Path(value)| Self(value))
            .map_err(|_| ApiError::not_found("The requested resource was not found."))
    }
}

// ---------------------------------------------------------------------------
// Cookies and client metadata
// ---------------------------------------------------------------------------

pub fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    let cookies = headers.get(header::COOKIE)?.to_str().ok()?;
    for part in cookies.split(';') {
        let part = part.trim();
        let (key, value) = part.split_once('=')?;
        if key == name {
            return Some(value.to_string());
        }
    }
    None
}

#[allow(clippy::too_many_arguments)]
pub fn build_cookie(
    name: &str,
    value: &str,
    path: &str,
    max_age_seconds: i64,
    secure: bool,
    http_only: bool,
    same_site: &str,
) -> HeaderValue {
    let mut cookie =
        format!("{name}={value}; Path={path}; Max-Age={max_age_seconds}; SameSite={same_site}");
    if secure {
        cookie.push_str("; Secure");
    }
    if http_only {
        cookie.push_str("; HttpOnly");
    }
    HeaderValue::from_str(&cookie)
        .unwrap_or_else(|_| HeaderValue::from_static("invoxa_invalid=; Max-Age=0"))
}

pub fn append_cookie(headers: &mut HeaderMap, cookie: HeaderValue) {
    headers.append(header::SET_COOKIE, cookie);
}

pub fn clear_cookie(name: &str, path: &str, secure: bool) -> HeaderValue {
    build_cookie(name, "", path, 0, secure, true, "Lax")
}

pub fn client_ip(headers: &HeaderMap, parts: &Parts, trust_proxy: bool) -> String {
    if trust_proxy {
        if let Some(value) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) {
            if let Some(first) = value.split(',').next() {
                let first = first.trim();
                if !first.is_empty() && first.len() <= 64 {
                    return first.to_string();
                }
            }
        }
        if let Some(value) = headers.get("x-real-ip").and_then(|v| v.to_str().ok()) {
            let value = value.trim();
            if !value.is_empty() && value.len() <= 64 {
                return value.to_string();
            }
        }
    }
    parts
        .extensions
        .get::<axum::extract::ConnectInfo<SocketAddr>>()
        .map(|axum::extract::ConnectInfo(address)| address.ip().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

pub struct ClientIp(pub String);

impl axum::extract::FromRequestParts<crate::state::AppState> for ClientIp {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &crate::state::AppState,
    ) -> Result<Self, Self::Rejection> {
        Ok(Self(client_ip(
            &parts.headers,
            parts,
            state.config.trust_proxy,
        )))
    }
}

pub fn request_id(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .map(|value| value.chars().take(100).collect())
}

pub fn user_agent(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::USER_AGENT)
        .and_then(|value| value.to_str().ok())
}

pub fn check_origin(state: &crate::state::AppState, headers: &HeaderMap) -> ApiResult<()> {
    if let Some(origin) = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
    {
        if !state.config.origin_allowed(origin) {
            return Err(ApiError::forbidden(
                "This origin is not permitted to make authenticated requests.",
            ));
        }
    }
    Ok(())
}

/// Require the signed double-submit CSRF token for session-authenticated
/// state-changing requests. API-key requests are exempt because the bearer
/// secret is not automatically attached by browsers.
pub fn require_csrf(state: &crate::state::AppState, headers: &HeaderMap) -> ApiResult<()> {
    check_origin(state, headers)?;
    let cookie = cookie_value(headers, "invoxa_csrf")
        .ok_or_else(|| ApiError::forbidden("CSRF token is missing. Refresh the page."))?;
    let header_token = headers
        .get("x-csrf-token")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| ApiError::forbidden("CSRF token header is missing. Refresh the page."))?;
    if !constant_time_eq(cookie.as_bytes(), header_token.as_bytes())
        || !state.csrf.verify(header_token)
    {
        return Err(ApiError::forbidden(
            "CSRF validation failed. Refresh the page and try again.",
        ));
    }
    Ok(())
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut difference = 0u8;
    for (a, b) in left.iter().zip(right) {
        difference |= a ^ b;
    }
    difference == 0
}

// ---------------------------------------------------------------------------
// Request validation
// ---------------------------------------------------------------------------

pub fn limit_offset(limit: Option<i64>, offset: Option<i64>, default: i64) -> (i64, i64) {
    let limit = limit.unwrap_or(default).clamp(1, 100);
    let offset = offset.unwrap_or(0).max(0);
    (limit, offset)
}

pub fn optional_text(
    value: Option<&Value>,
    field: &str,
    max_chars: usize,
) -> ApiResult<Option<String>> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(raw)) => {
            let raw = raw.trim();
            if raw.is_empty() {
                return Ok(None);
            }
            if raw.chars().count() > max_chars {
                return Err(ApiError::validation(format!(
                    "{field} must be at most {max_chars} characters"
                )));
            }
            Ok(Some(raw.to_string()))
        }
        _ => Err(ApiError::validation(format!(
            "{field} must be a string or null"
        ))),
    }
}

pub fn required_text(value: &str, field: &str, min: usize, max: usize) -> ApiResult<String> {
    let trimmed = value.trim();
    let length = trimmed.chars().count();
    if length < min || length > max {
        return Err(ApiError::validation(format!(
            "{field} must be between {min} and {max} characters"
        )));
    }
    if trimmed
        .chars()
        .any(|c| c.is_control() && c != '\n' && c != '\t')
    {
        return Err(ApiError::validation(format!(
            "{field} contains invalid control characters"
        )));
    }
    Ok(trimmed.to_string())
}

/// Parse money from a JSON string or integer. JSON floating point numbers are
/// rejected because they cannot be represented exactly.
pub fn decimal_value(value: &Value, field: &str) -> ApiResult<Option<Decimal>> {
    match value {
        Value::Null => Ok(None),
        Value::String(raw) => {
            let raw = raw.trim().replace(',', "");
            if raw.is_empty() {
                return Ok(None);
            }
            let amount = Decimal::from_str(&raw)
                .map_err(|_| ApiError::validation(format!("{field} must be a decimal number")))?;
            validate_decimal(amount, field)?;
            Ok(Some(amount))
        }
        Value::Number(number) => {
            if number.is_f64() && number.as_f64().map(|v| v.fract()).unwrap_or(0.0) != 0.0 {
                return Err(ApiError::validation(format!(
                    "{field} must be sent as a decimal string, not a floating-point number"
                )));
            }
            let amount = Decimal::from_str(&number.to_string())
                .map_err(|_| ApiError::validation(format!("{field} must be a decimal number")))?;
            validate_decimal(amount, field)?;
            Ok(Some(amount))
        }
        _ => Err(ApiError::validation(format!(
            "{field} must be a decimal string or null"
        ))),
    }
}

pub fn validate_decimal(amount: Decimal, field: &str) -> ApiResult<()> {
    if amount.scale() > 4 {
        return Err(ApiError::validation(format!(
            "{field} supports at most 4 decimal places"
        )));
    }
    let limit = Decimal::from(100_000_000_000_000i64);
    if amount.abs() >= limit {
        return Err(ApiError::validation(format!(
            "{field} exceeds the supported monetary range"
        )));
    }
    Ok(())
}

pub fn parse_iso_date(value: &str, field: &str) -> ApiResult<chrono::NaiveDate> {
    invoxa_domain::normalization::parse_invoice_date(value)
        .map(|parsed| parsed.date)
        .map_err(|_| ApiError::validation(format!("{field} must be a valid date (YYYY-MM-DD)")))
}

pub fn is_safe_filename(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 200
        && !name.contains(['/', '\\'])
        && !name.chars().any(|c| c.is_control())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn cookie_parser_handles_multiple_values() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_static("a=1; invoxa_csrf=v1.2.abc; b=2"),
        );
        assert_eq!(
            cookie_value(&headers, "invoxa_csrf").as_deref(),
            Some("v1.2.abc")
        );
        assert_eq!(cookie_value(&headers, "missing"), None);
    }

    #[test]
    fn money_rejects_floats_and_excess_scale() {
        assert!(decimal_value(&json!("10.25"), "total").unwrap().is_some());
        assert!(decimal_value(&json!(10), "total").unwrap().is_some());
        assert!(decimal_value(&json!(10.25), "total").is_err());
        assert!(decimal_value(&json!("10.12345"), "total").is_err());
        assert!(decimal_value(&json!("1e3"), "total").is_ok());
    }

    #[test]
    fn pagination_is_bounded() {
        assert_eq!(limit_offset(Some(1000), Some(-1), 25), (100, 0));
        assert_eq!(limit_offset(None, None, 25), (25, 0));
    }
}
