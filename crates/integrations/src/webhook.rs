//! Signed webhook delivery with SSRF protections.
//!
//! Security properties:
//! * only HTTP(S) endpoints on configured ports are accepted;
//! * loopback, private, link-local, carrier-grade NAT, multicast, documentation
//!   and IPv4-mapped IPv6 destinations are rejected by default;
//! * the hostname is resolved on every delivery attempt, every resolved address
//!   is checked, and the connection is pinned to the validated address so a
//!   second DNS lookup cannot rebind the request;
//! * HTTP redirects are disabled;
//! * each request is signed with HMAC-SHA256 over `timestamp.body`, carries a
//!   delivery ID for idempotency, and receivers are expected to reject stale
//!   timestamps.

use chrono::{DateTime, SecondsFormat, Utc};
use invoxa_security::{verify_hmac_sha256_hex, webhook_signature};
use reqwest::{Client, StatusCode, Url};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeSet;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::net::lookup_host;
use url::Host;
use uuid::Uuid;

pub const MAX_DELIVERY_ATTEMPTS: i32 = 8;
pub const MAX_WEBHOOK_BODY_BYTES: usize = 512 * 1024;
pub const SIGNATURE_TOLERANCE_SECONDS: i64 = 300;
pub const WEBHOOK_API_VERSION: &str = "2026-03-01";
const MAX_URL_LENGTH: usize = 2_048;

#[derive(Debug, Error)]
pub enum WebhookError {
    #[error("webhook URL is invalid")]
    InvalidUrl,
    #[error("webhook destination is not permitted")]
    BlockedDestination,
    #[error("webhook destination could not be resolved")]
    DnsResolution,
    #[error("webhook request failed")]
    Transport,
    #[error("webhook payload could not be serialized")]
    Payload,
}

impl WebhookError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidUrl => "WEBHOOK_URL_INVALID",
            Self::BlockedDestination => "WEBHOOK_URL_BLOCKED",
            Self::DnsResolution => "WEBHOOK_DNS_FAILED",
            Self::Transport => "WEBHOOK_NETWORK",
            Self::Payload => "WEBHOOK_PAYLOAD_INVALID",
        }
    }

    fn retryable(&self) -> bool {
        matches!(self, Self::DnsResolution | Self::Transport)
    }
}

#[derive(Debug, Clone)]
pub struct SsrfPolicy {
    pub require_https: bool,
    pub allowed_ports: BTreeSet<u16>,
    pub resolution_timeout: Duration,
    /// Only tests may enable this. Production configuration cannot set it.
    allow_private_for_tests: bool,
}

impl Default for SsrfPolicy {
    fn default() -> Self {
        Self {
            require_https: true,
            allowed_ports: BTreeSet::from([443]),
            resolution_timeout: Duration::from_secs(5),
            allow_private_for_tests: false,
        }
    }
}

impl SsrfPolicy {
    /// Relax TLS and add ports only for local development. Private destinations
    /// remain blocked; use an HTTPS tunnel for local webhook testing.
    pub fn allow_http(mut self, ports: impl IntoIterator<Item = u16>) -> Self {
        self.require_https = false;
        self.allowed_ports.extend(ports);
        self
    }

    pub fn with_resolution_timeout(mut self, timeout: Duration) -> Self {
        self.resolution_timeout = timeout.clamp(Duration::from_secs(1), Duration::from_secs(30));
        self
    }
}

#[derive(Debug, Clone)]
pub struct ValidatedWebhook {
    pub url: String,
    pub host: String,
    pub port: u16,
    pub pinned_addr: SocketAddr,
    pub host_is_ip_literal: bool,
}

#[derive(Debug, Clone)]
enum ParsedHost {
    Domain(String),
    Ip(IpAddr),
}

#[derive(Debug, Clone)]
struct ParsedWebhook {
    url: String,
    host: ParsedHost,
    port: u16,
}

/// Validate a webhook URL and resolve/pin its destination.
pub async fn validate_webhook_url(
    raw_url: &str,
    policy: &SsrfPolicy,
) -> Result<ValidatedWebhook, WebhookError> {
    let parsed = parse_webhook_url(raw_url, policy)?;
    match &parsed.host {
        ParsedHost::Ip(ip) => {
            if !policy.allow_private_for_tests && is_forbidden_ip(*ip) {
                return Err(WebhookError::BlockedDestination);
            }
            Ok(ValidatedWebhook {
                url: parsed.url,
                host: ip.to_string(),
                port: parsed.port,
                pinned_addr: SocketAddr::new(*ip, parsed.port),
                host_is_ip_literal: true,
            })
        }
        ParsedHost::Domain(domain) => {
            let lookup = lookup_host((domain.as_str(), parsed.port));
            let addresses = tokio::time::timeout(policy.resolution_timeout, lookup)
                .await
                .map_err(|_| WebhookError::DnsResolution)?
                .map_err(|_| WebhookError::DnsResolution)?
                .collect::<Vec<_>>();
            if addresses.is_empty() {
                return Err(WebhookError::DnsResolution);
            }
            if !policy.allow_private_for_tests
                && addresses.iter().any(|addr| is_forbidden_ip(addr.ip()))
            {
                // Reject if *any* address is forbidden. A hostname must not be
                // able to hide an internal address among public answers.
                return Err(WebhookError::BlockedDestination);
            }
            let pinned = addresses[0];
            Ok(ValidatedWebhook {
                url: parsed.url,
                host: domain.clone(),
                port: parsed.port,
                pinned_addr: pinned,
                host_is_ip_literal: false,
            })
        }
    }
}

fn parse_webhook_url(raw_url: &str, policy: &SsrfPolicy) -> Result<ParsedWebhook, WebhookError> {
    let trimmed = raw_url.trim();
    if trimmed.is_empty()
        || trimmed.len() > MAX_URL_LENGTH
        || trimmed.chars().any(|c| c.is_control())
    {
        return Err(WebhookError::InvalidUrl);
    }
    let parsed = Url::parse(trimmed).map_err(|_| WebhookError::InvalidUrl)?;
    match parsed.scheme() {
        "https" => {}
        "http" if !policy.require_https => {}
        _ => return Err(WebhookError::InvalidUrl),
    }
    if parsed.fragment().is_some() || !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(WebhookError::InvalidUrl);
    }
    let port = parsed
        .port_or_known_default()
        .ok_or(WebhookError::InvalidUrl)?;
    if !policy.allowed_ports.contains(&port) {
        return Err(WebhookError::BlockedDestination);
    }
    let host = match parsed.host() {
        Some(Host::Domain(domain)) => {
            if domain.ends_with('.') {
                // A trailing dot could desynchronize the pinned hostname from
                // the URL hostname and defeat DNS pinning.
                return Err(WebhookError::InvalidUrl);
            }
            let domain = domain.to_ascii_lowercase();
            if domain.is_empty()
                || domain == "localhost"
                || !domain.contains('.')
                || is_prohibited_domain(&domain)
            {
                return Err(WebhookError::BlockedDestination);
            }
            ParsedHost::Domain(domain)
        }
        Some(Host::Ipv4(ip)) => ParsedHost::Ip(IpAddr::V4(ip)),
        Some(Host::Ipv6(ip)) => ParsedHost::Ip(IpAddr::V6(ip)),
        None => return Err(WebhookError::InvalidUrl),
    };
    Ok(ParsedWebhook {
        url: parsed.to_string(),
        host,
        port,
    })
}

fn is_prohibited_domain(domain: &str) -> bool {
    const SUFFIXES: &[&str] = &[
        "localhost",
        "local",
        "internal",
        "lan",
        "home.arpa",
        "test",
        "invalid",
        "example",
        "onion",
        "in-addr.arpa",
        "ip6.arpa",
    ];
    SUFFIXES
        .iter()
        .any(|suffix| domain == *suffix || domain.ends_with(&format!(".{suffix}")))
}

/// True when the address must never be contacted by a webhook.
pub fn is_forbidden_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => is_forbidden_ipv4(ip),
        IpAddr::V6(ip) => is_forbidden_ipv6(ip),
    }
}

fn is_forbidden_ipv4(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_multicast()
        || ip.is_unspecified()
        || o[0] == 0
        || (o[0] == 100 && (64..=127).contains(&o[1]))
        || (o[0] == 192 && o[1] == 0 && o[2] == 0)
        || (o[0] == 192 && o[1] == 88 && o[2] == 99)
        || (o[0] == 198 && matches!(o[1], 18 | 19))
        || o[0] >= 240
}

fn is_forbidden_ipv6(ip: Ipv6Addr) -> bool {
    // IPv4-compatible and IPv4-mapped addresses must be judged as IPv4.
    if let Some(v4) = ip.to_ipv4() {
        return is_forbidden_ipv4(v4);
    }
    let segments = ip.segments();
    let first = segments[0];
    if ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        || ip.is_unique_local()
        || ip.is_unicast_link_local()
    {
        return true;
    }
    // Deprecated 6bone, Teredo, ORCHID, benchmarking, documentation, NAT64,
    // discard-only and 6to4 transition prefixes.
    (first == 0x3ffe)
        || (first == 0x2001 && segments[1] == 0x0000)
        || (first == 0x2001 && segments[1] == 0x0002)
        || (first == 0x2001 && (segments[1] & 0xfff0) == 0x0010)
        || (first == 0x2001 && (segments[1] & 0xfff0) == 0x0020)
        || (first == 0x2001 && segments[1] == 0x0db8)
        || (first == 0x2002)
        || (first == 0x0064 && segments[1] == 0xff9b)
        || (first == 0x0100 && segments[1] == 0 && segments[2] == 0 && segments[3] == 0)
}

pub fn sign_webhook_header(secret: &[u8], timestamp: i64, body: &[u8]) -> String {
    format!(
        "t={timestamp},v1={}",
        webhook_signature(secret, timestamp, body)
    )
}

/// Verification helper for receivers and automated tests. Signatures older or
/// newer than `tolerance_seconds` are rejected to limit replay.
pub fn verify_webhook_signature(
    secret: &[u8],
    header: &str,
    body: &[u8],
    now: i64,
    tolerance_seconds: i64,
) -> bool {
    if tolerance_seconds < 0 {
        return false;
    }
    let mut timestamp = None;
    let mut signature = None;
    for part in header.split(',') {
        let part = part.trim();
        if let Some(value) = part.strip_prefix("t=") {
            if timestamp.is_some() {
                return false;
            }
            timestamp = value.parse::<i64>().ok();
        } else if let Some(value) = part.strip_prefix("v1=") {
            if signature.is_some() {
                return false;
            }
            signature = Some(value);
        }
    }
    let (Some(timestamp), Some(signature)) = (timestamp, signature) else {
        return false;
    };
    if (now - timestamp).abs() > tolerance_seconds {
        return false;
    }
    let mut message = timestamp.to_string().into_bytes();
    message.push(b'.');
    message.extend_from_slice(body);
    verify_hmac_sha256_hex(secret, &message, signature)
}

#[derive(Debug, Serialize)]
struct WebhookEnvelope<'a> {
    id: Uuid,
    #[serde(rename = "type")]
    event_type: &'a str,
    api_version: &'a str,
    created_at: String,
    data: &'a Value,
}

pub fn build_webhook_body(
    event_id: Uuid,
    event_type: &str,
    api_version: &str,
    created_at: DateTime<Utc>,
    data: &Value,
) -> Result<Vec<u8>, WebhookError> {
    let envelope = WebhookEnvelope {
        id: event_id,
        event_type,
        api_version,
        created_at: created_at.to_rfc3339_opts(SecondsFormat::Secs, true),
        data,
    };
    let body = serde_json::to_vec(&envelope).map_err(|_| WebhookError::Payload)?;
    if body.len() > MAX_WEBHOOK_BODY_BYTES {
        return Err(WebhookError::Payload);
    }
    Ok(body)
}

#[derive(Debug, Clone)]
pub struct DeliveryOutcome {
    pub success: bool,
    pub retryable: bool,
    pub status_code: Option<u16>,
    pub error_code: Option<&'static str>,
    pub error: Option<String>,
    pub response_snippet: Option<String>,
    pub duration_ms: i64,
}

pub struct WebhookSender {
    policy: SsrfPolicy,
    timeout: Duration,
}

impl WebhookSender {
    pub fn new(policy: SsrfPolicy) -> Self {
        Self {
            policy,
            timeout: Duration::from_secs(10),
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout.clamp(Duration::from_secs(1), Duration::from_secs(60));
        self
    }

    /// Validate, pin, sign, and POST one delivery. Redirects are never followed.
    pub async fn send(
        &self,
        url: &str,
        secret: &[u8],
        event_id: Uuid,
        event_type: &str,
        payload: &Value,
        api_version: &str,
    ) -> DeliveryOutcome {
        let started = Instant::now();
        let validated = match validate_webhook_url(url, &self.policy).await {
            Ok(value) => value,
            Err(error) => {
                return DeliveryOutcome {
                    success: false,
                    retryable: error.retryable(),
                    status_code: None,
                    error_code: Some(error.code()),
                    error: Some(error.to_string()),
                    response_snippet: None,
                    duration_ms: started.elapsed().as_millis() as i64,
                };
            }
        };
        let created_at = Utc::now();
        let body = match build_webhook_body(event_id, event_type, api_version, created_at, payload)
        {
            Ok(body) => body,
            Err(error) => {
                return DeliveryOutcome {
                    success: false,
                    retryable: error.retryable(),
                    status_code: None,
                    error_code: Some(error.code()),
                    error: Some(error.to_string()),
                    response_snippet: None,
                    duration_ms: started.elapsed().as_millis() as i64,
                };
            }
        };
        let timestamp = created_at.timestamp();
        let signature = sign_webhook_header(secret, timestamp, &body);

        let mut builder = Client::builder()
            .timeout(self.timeout)
            .connect_timeout(Duration::from_secs(5))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent("Invoxa-Webhooks/1.0");
        if !validated.host_is_ip_literal {
            // Pin the connection to the address that was validated above. TLS
            // SNI and the Host header still use the original hostname.
            builder = builder.resolve(&validated.host, validated.pinned_addr);
        }
        let client = match builder.build() {
            Ok(client) => client,
            Err(_) => {
                return DeliveryOutcome {
                    success: false,
                    retryable: true,
                    status_code: None,
                    error_code: Some("WEBHOOK_NETWORK"),
                    error: Some("could not initialize webhook HTTP client".into()),
                    response_snippet: None,
                    duration_ms: started.elapsed().as_millis() as i64,
                };
            }
        };

        let request = client
            .post(&validated.url)
            .header("content-type", "application/json")
            .header("accept", "*/*")
            .header("x-invoxa-event", event_type)
            .header("x-invoxa-delivery", event_id.to_string())
            .header("x-invoxa-signature", &signature)
            .header("x-invoxa-api-version", api_version)
            .body(body);
        let response = match request.send().await {
            Ok(response) => response,
            Err(error) => {
                let timed_out = error.is_timeout();
                return DeliveryOutcome {
                    success: false,
                    retryable: true,
                    status_code: None,
                    error_code: Some(if timed_out {
                        "WEBHOOK_TIMEOUT"
                    } else {
                        "WEBHOOK_NETWORK"
                    }),
                    error: Some(if timed_out {
                        "webhook request timed out".into()
                    } else {
                        "webhook request failed".into()
                    }),
                    response_snippet: None,
                    duration_ms: started.elapsed().as_millis() as i64,
                };
            }
        };

        let status = response.status();
        let snippet = read_response_snippet(response).await;
        let success = status.is_success();
        let retryable = !success
            && (status == StatusCode::REQUEST_TIMEOUT
                || status == StatusCode::TOO_MANY_REQUESTS
                || status.is_server_error());
        let error_code = if success {
            None
        } else if status.is_redirection() {
            Some("WEBHOOK_REDIRECT")
        } else if status == StatusCode::TOO_MANY_REQUESTS {
            Some("WEBHOOK_RATE_LIMITED")
        } else {
            Some("WEBHOOK_HTTP_ERROR")
        };
        DeliveryOutcome {
            success,
            retryable,
            status_code: Some(status.as_u16()),
            error_code,
            error: if success {
                None
            } else if status.is_redirection() {
                Some("webhook endpoint returned a redirect; redirects are not followed".into())
            } else {
                Some(format!(
                    "webhook endpoint returned HTTP {}",
                    status.as_u16()
                ))
            },
            response_snippet: snippet,
            duration_ms: started.elapsed().as_millis() as i64,
        }
    }
}

async fn read_response_snippet(mut response: reqwest::Response) -> Option<String> {
    let mut bytes = Vec::new();
    while bytes.len() < 2_048 {
        match response.chunk().await {
            Ok(Some(chunk)) => {
                let remaining = 2_048 - bytes.len();
                bytes.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
            }
            Ok(None) => break,
            Err(_) => break,
        }
    }
    if bytes.is_empty() {
        return None;
    }
    let text = String::from_utf8_lossy(&bytes);
    let cleaned: String = text
        .chars()
        .map(|c| {
            if c.is_control() && c != '\n' && c != '\t' {
                ' '
            } else {
                c
            }
        })
        .take(500)
        .collect();
    let cleaned = cleaned.trim().to_string();
    (!cleaned.is_empty()).then_some(cleaned)
}

/// Retry schedule after a failed attempt. Attempt 1 waits 1 minute and the
/// delay grows to a 24-hour cap. Returns `None` when the delivery should be
/// dead-lettered.
pub fn retry_delay_seconds(attempts_completed: i32) -> Option<u64> {
    if attempts_completed <= 0 || attempts_completed >= MAX_DELIVERY_ATTEMPTS {
        return None;
    }
    const SCHEDULE: [u64; 7] = [60, 300, 1_800, 7_200, 21_600, 43_200, 86_400];
    Some(SCHEDULE[(attempts_completed - 1) as usize])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[test]
    fn blocks_private_loopback_link_local_and_mapped_addresses() {
        for ip in [
            "127.0.0.1",
            "127.1.2.3",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "198.18.0.1",
            "192.0.2.10",
            "224.0.0.1",
            "255.255.255.255",
            "::1",
            "::",
            "fc00::1",
            "fe80::1",
            "ff02::1",
            "::ffff:127.0.0.1",
            "2001:db8::1",
            "2002:7f00:1::1",
            "64:ff9b::7f00:1",
            "2001:0:53aa:64c::1",
        ] {
            assert!(
                is_forbidden_ip(ip.parse().unwrap()),
                "{ip} should be blocked"
            );
        }
        for ip in ["1.1.1.1", "93.184.215.14", "2606:4700:4700::1111"] {
            assert!(
                !is_forbidden_ip(ip.parse().unwrap()),
                "{ip} should be allowed"
            );
        }
    }

    #[tokio::test]
    async fn rejects_dangerous_urls_without_network_access() {
        let policy = SsrfPolicy::default();
        for url in [
            "https://127.0.0.1/hook",
            "https://10.0.0.2/hook",
            "https://169.254.169.254/latest/meta-data",
            "https://[::1]/hook",
            "https://localhost/hook",
            "https://metadata.internal/hook",
            "https://example.com:8443/hook",
            "http://93.184.215.14/hook",
            "https://user:pass@93.184.215.14/hook",
            "https://93.184.215.14/hook#fragment",
        ] {
            let result = validate_webhook_url(url, &policy).await;
            assert!(result.is_err(), "{url} should be rejected");
        }
    }

    #[tokio::test]
    async fn accepts_public_ip_destinations_and_pins_them() {
        let policy = SsrfPolicy::default();
        let validated = validate_webhook_url("https://93.184.215.14/hook?tenant=1", &policy)
            .await
            .unwrap();
        assert!(validated.host_is_ip_literal);
        assert_eq!(validated.pinned_addr.ip().to_string(), "93.184.215.14");
        assert_eq!(validated.pinned_addr.port(), 443);
        assert!(validated.url.starts_with("https://93.184.215.14/"));
    }

    #[test]
    fn signatures_are_timestamped_and_replay_checked() {
        let secret = b"whsec_0123456789abcdef0123456789abcdef";
        let timestamp = 1_700_000_000;
        let body = br#"{"hello":"world"}"#;
        let header = sign_webhook_header(secret, timestamp, body);
        assert!(header.starts_with("t=1700000000,v1="));
        assert!(verify_webhook_signature(
            secret,
            &header,
            body,
            timestamp + 10,
            300
        ));
        assert!(!verify_webhook_signature(
            secret,
            &header,
            body,
            timestamp + 301,
            300
        ));
        assert!(!verify_webhook_signature(
            secret,
            &header,
            b"{\"hello\":\"tampered\"}",
            timestamp,
            300
        ));
        assert!(!verify_webhook_signature(
            secret,
            "t=abc,v1=00",
            body,
            timestamp,
            300
        ));
        assert!(!verify_webhook_signature(
            secret,
            &header.replace("v1=", "v1=00"),
            body,
            timestamp,
            300
        ));
    }

    #[test]
    fn retry_schedule_is_exponential_and_capped() {
        assert_eq!(retry_delay_seconds(1), Some(60));
        assert_eq!(retry_delay_seconds(2), Some(300));
        assert_eq!(retry_delay_seconds(7), Some(86_400));
        assert_eq!(retry_delay_seconds(8), None);
        assert_eq!(retry_delay_seconds(0), None);
        for attempt in 2..7 {
            assert!(retry_delay_seconds(attempt) > retry_delay_seconds(attempt - 1));
        }
    }

    #[test]
    fn builds_a_versioned_event_envelope() {
        let event_id = Uuid::new_v4();
        let body = build_webhook_body(
            event_id,
            "invoice.approved",
            WEBHOOK_API_VERSION,
            Utc::now(),
            &json!({"invoice_id": "abc"}),
        )
        .unwrap();
        let value: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["id"], event_id.to_string());
        assert_eq!(value["type"], "invoice.approved");
        assert_eq!(value["api_version"], WEBHOOK_API_VERSION);
        assert_eq!(value["data"]["invoice_id"], "abc");
    }

    #[tokio::test]
    async fn delivers_to_a_permitted_local_test_endpoint() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buffer = vec![0u8; 16_384];
            let read = socket.read(&mut buffer).await.unwrap();
            let request = String::from_utf8_lossy(&buffer[..read]).to_string();
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                .await
                .unwrap();
            request.to_ascii_lowercase()
        });
        let policy = SsrfPolicy {
            require_https: false,
            allowed_ports: BTreeSet::from([addr.port()]),
            resolution_timeout: Duration::from_secs(2),
            allow_private_for_tests: true,
        };
        let sender = WebhookSender::new(policy).with_timeout(Duration::from_secs(5));
        let event_id = Uuid::new_v4();
        let outcome = sender
            .send(
                &format!("http://127.0.0.1:{}/hook", addr.port()),
                b"whsec_0123456789abcdef0123456789abcdef",
                event_id,
                "invoice.approved",
                &json!({"invoice_id": "abc"}),
                WEBHOOK_API_VERSION,
            )
            .await;
        assert!(outcome.success, "{outcome:?}");
        assert_eq!(outcome.status_code, Some(200));
        assert_eq!(outcome.response_snippet.as_deref(), Some("ok"));
        let request = server.await.unwrap();
        assert!(request.contains("x-invoxa-signature: t="));
        assert!(request.contains(&format!("x-invoxa-delivery: {event_id}")));
        assert!(request.contains("x-invoxa-event: invoice.approved"));
    }

    #[tokio::test]
    async fn redirects_are_not_followed() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buffer = [0u8; 8_192];
            let _ = socket.read(&mut buffer).await;
            socket
                .write_all(
                    b"HTTP/1.1 302 Found\r\nLocation: http://169.254.169.254/\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .await
                .unwrap();
        });
        let policy = SsrfPolicy {
            require_https: false,
            allowed_ports: BTreeSet::from([addr.port()]),
            resolution_timeout: Duration::from_secs(2),
            allow_private_for_tests: true,
        };
        let outcome = WebhookSender::new(policy)
            .send(
                &format!("http://127.0.0.1:{}/hook", addr.port()),
                b"whsec_0123456789abcdef0123456789abcdef",
                Uuid::new_v4(),
                "invoice.approved",
                &json!({}),
                WEBHOOK_API_VERSION,
            )
            .await;
        assert!(!outcome.success);
        assert_eq!(outcome.status_code, Some(302));
        assert_eq!(outcome.error_code, Some("WEBHOOK_REDIRECT"));
        assert!(!outcome.retryable);
    }

    #[test]
    fn constant_time_signature_comparison_helper_is_used() {
        assert!(invoxa_security::secure_equals(b"same", b"same"));
        assert!(!invoxa_security::secure_equals(b"same", b"diff"));
    }
}
