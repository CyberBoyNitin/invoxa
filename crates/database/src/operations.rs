//! Operational queries: dashboard, audit log, API keys, webhooks, exports,
//! automation rules, usage/quota and integration status.

use crate::{Db, DbError};
use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde_json::Value;
use sqlx::FromRow;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Audit
// ---------------------------------------------------------------------------

pub struct AuditInput<'a> {
    pub organization_id: Uuid,
    pub actor_user_id: Option<Uuid>,
    pub actor_api_key_id: Option<Uuid>,
    pub action: &'a str,
    pub entity_type: Option<&'a str>,
    pub entity_id: Option<Uuid>,
    pub metadata: Value,
    pub ip: Option<&'a str>,
    pub request_id: Option<&'a str>,
}

pub async fn insert_audit(db: &Db, input: &AuditInput<'_>) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"INSERT INTO audit_events
           (organization_id, actor_user_id, actor_api_key_id, action, entity_type, entity_id, metadata, ip, request_id)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)"#,
    )
    .bind(input.organization_id)
    .bind(input.actor_user_id)
    .bind(input.actor_api_key_id)
    .bind(input.action)
    .bind(input.entity_type)
    .bind(input.entity_id)
    .bind(&input.metadata)
    .bind(input.ip)
    .bind(input.request_id)
    .execute(db)
    .await?;
    Ok(())
}

#[derive(Debug, Clone, FromRow)]
pub struct AuditEventRow {
    pub id: Uuid,
    pub action: String,
    pub entity_type: Option<String>,
    pub entity_id: Option<Uuid>,
    pub metadata: Value,
    pub actor_name: Option<String>,
    pub actor_email: Option<String>,
    pub actor_key_name: Option<String>,
    pub ip: Option<String>,
    pub request_id: Option<String>,
    pub created_at: DateTime<Utc>,
}

pub async fn list_audit_events(
    db: &Db,
    organization_id: Uuid,
    search: Option<&str>,
    limit: i64,
    offset: i64,
) -> Result<(Vec<AuditEventRow>, i64), sqlx::Error> {
    let rows = sqlx::query_as::<_, AuditEventRow>(
        r#"SELECT a.id, a.action, a.entity_type, a.entity_id, a.metadata,
                  u.name AS actor_name, u.email AS actor_email, k.name AS actor_key_name,
                  a.ip, a.request_id, a.created_at
           FROM audit_events a
           LEFT JOIN users u ON u.id = a.actor_user_id
           LEFT JOIN api_keys k ON k.id = a.actor_api_key_id
           WHERE a.organization_id = $1
             AND ($2::text IS NULL OR a.action ILIKE '%' || $2 || '%' OR a.entity_type ILIKE '%' || $2 || '%')
           ORDER BY a.created_at DESC
           LIMIT $3 OFFSET $4"#,
    )
    .bind(organization_id)
    .bind(search)
    .bind(limit)
    .bind(offset)
    .fetch_all(db)
    .await?;
    let (total,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM audit_events WHERE organization_id = $1 AND ($2::text IS NULL OR action ILIKE '%' || $2 || '%' OR entity_type ILIKE '%' || $2 || '%')",
    )
    .bind(organization_id)
    .bind(search)
    .fetch_one(db)
    .await?;
    Ok((rows, total))
}

// ---------------------------------------------------------------------------
// Dashboard
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, FromRow)]
pub struct DashboardCounts {
    pub documents_received: i64,
    pub invoices_extracted: i64,
    pub awaiting_review: i64,
    pub validation_failures: i64,
    pub duplicate_candidates: i64,
    pub approved_invoices: i64,
    pub exported_invoices: i64,
    pub processing_failures: i64,
    pub pages_processed: i64,
    pub avg_processing_ms: Option<f64>,
    pub total_ai_requests: i64,
}

#[derive(Debug, Clone, FromRow)]
pub struct StatusCountRow {
    pub status: String,
    pub count: i64,
}

#[derive(Debug, Clone, FromRow)]
pub struct DayCountRow {
    pub day: NaiveDate,
    pub count: i64,
}

#[derive(Debug, Clone, FromRow)]
pub struct RecentActivityRow {
    pub id: Uuid,
    pub action: String,
    pub entity_type: Option<String>,
    pub entity_id: Option<Uuid>,
    pub actor_name: Option<String>,
    pub metadata: Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
pub struct RecentErrorRow {
    pub document_id: Uuid,
    pub original_filename: String,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub created_at: DateTime<Utc>,
}

pub async fn dashboard_counts(
    db: &Db,
    organization_id: Uuid,
    from: Option<DateTime<Utc>>,
    to: Option<DateTime<Utc>>,
) -> Result<DashboardCounts, sqlx::Error> {
    sqlx::query_as::<_, DashboardCounts>(
        r#"SELECT
             (SELECT count(*) FROM documents d WHERE d.organization_id = $1 AND d.deleted_at IS NULL
                AND ($2::timestamptz IS NULL OR d.created_at >= $2)
                AND ($3::timestamptz IS NULL OR d.created_at < $3)) AS documents_received,
             (SELECT count(*) FROM extracted_invoices i WHERE i.organization_id = $1
                AND ($2::timestamptz IS NULL OR i.created_at >= $2)
                AND ($3::timestamptz IS NULL OR i.created_at < $3)) AS invoices_extracted,
             (SELECT count(*) FROM extracted_invoices i WHERE i.organization_id = $1 AND i.status = 'review_required'
                AND ($2::timestamptz IS NULL OR i.created_at >= $2)
                AND ($3::timestamptz IS NULL OR i.created_at < $3)) AS awaiting_review,
             (SELECT count(*) FROM extracted_invoices i WHERE i.organization_id = $1
                AND EXISTS (SELECT 1 FROM validation_issues vi WHERE vi.invoice_id = i.id AND vi.resolved_at IS NULL AND vi.severity = 'error')
                AND ($2::timestamptz IS NULL OR i.created_at >= $2)
                AND ($3::timestamptz IS NULL OR i.created_at < $3)) AS validation_failures,
             (SELECT count(*) FROM duplicate_candidates dc WHERE dc.organization_id = $1 AND dc.status = 'open'
                AND ($2::timestamptz IS NULL OR dc.created_at >= $2)
                AND ($3::timestamptz IS NULL OR dc.created_at < $3)) AS duplicate_candidates,
             (SELECT count(*) FROM extracted_invoices i WHERE i.organization_id = $1 AND i.status IN ('approved','exported')
                AND ($2::timestamptz IS NULL OR i.created_at >= $2)
                AND ($3::timestamptz IS NULL OR i.created_at < $3)) AS approved_invoices,
             (SELECT count(*) FROM extracted_invoices i WHERE i.organization_id = $1 AND i.status = 'exported'
                AND ($2::timestamptz IS NULL OR i.created_at >= $2)
                AND ($3::timestamptz IS NULL OR i.created_at < $3)) AS exported_invoices,
             (SELECT count(*) FROM documents d WHERE d.organization_id = $1 AND d.processing_status = 'failed'
                AND ($2::timestamptz IS NULL OR d.created_at >= $2)
                AND ($3::timestamptz IS NULL OR d.created_at < $3)) AS processing_failures,
             (SELECT COALESCE(sum(d.page_count),0) FROM documents d WHERE d.organization_id = $1 AND d.deleted_at IS NULL
                AND ($2::timestamptz IS NULL OR d.created_at >= $2)
                AND ($3::timestamptz IS NULL OR d.created_at < $3)) AS pages_processed,
             (SELECT avg(d.processing_duration_ms)::float8 FROM documents d WHERE d.organization_id = $1 AND d.processing_duration_ms IS NOT NULL
                AND ($2::timestamptz IS NULL OR d.created_at >= $2)
                AND ($3::timestamptz IS NULL OR d.created_at < $3)) AS avg_processing_ms,
             (SELECT count(*) FROM usage_records u WHERE u.organization_id = $1 AND u.kind = 'ai_extraction'
                AND ($2::timestamptz IS NULL OR u.created_at >= $2)
                AND ($3::timestamptz IS NULL OR u.created_at < $3)) AS total_ai_requests"#,
    )
    .bind(organization_id)
    .bind(from)
    .bind(to)
    .fetch_one(db)
    .await
}

pub async fn status_breakdown(
    db: &Db,
    organization_id: Uuid,
    from: Option<DateTime<Utc>>,
    to: Option<DateTime<Utc>>,
) -> Result<Vec<StatusCountRow>, sqlx::Error> {
    sqlx::query_as::<_, StatusCountRow>(
        r#"SELECT processing_status AS status, count(*) AS count
           FROM documents
           WHERE organization_id = $1 AND deleted_at IS NULL
             AND ($2::timestamptz IS NULL OR created_at >= $2)
             AND ($3::timestamptz IS NULL OR created_at < $3)
           GROUP BY processing_status
           ORDER BY count DESC"#,
    )
    .bind(organization_id)
    .bind(from)
    .bind(to)
    .fetch_all(db)
    .await
}

pub async fn daily_documents(
    db: &Db,
    organization_id: Uuid,
    days: i32,
) -> Result<Vec<DayCountRow>, sqlx::Error> {
    sqlx::query_as::<_, DayCountRow>(
        r#"SELECT day::date AS day, count(d.id) AS count
           FROM generate_series((now() AT TIME ZONE 'UTC')::date - ($2::int - 1), (now() AT TIME ZONE 'UTC')::date, interval '1 day') day
           LEFT JOIN documents d
             ON d.organization_id = $1 AND (d.created_at AT TIME ZONE 'UTC')::date = day::date AND d.deleted_at IS NULL
           GROUP BY day
           ORDER BY day"#,
    )
    .bind(organization_id)
    .bind(days)
    .fetch_all(db)
    .await
}

pub async fn daily_approved(
    db: &Db,
    organization_id: Uuid,
    days: i32,
) -> Result<Vec<DayCountRow>, sqlx::Error> {
    sqlx::query_as::<_, DayCountRow>(
        r#"SELECT day::date AS day, count(i.id) AS count
           FROM generate_series((now() AT TIME ZONE 'UTC')::date - ($2::int - 1), (now() AT TIME ZONE 'UTC')::date, interval '1 day') day
           LEFT JOIN extracted_invoices i
             ON i.organization_id = $1 AND i.approved_at IS NOT NULL AND (i.approved_at AT TIME ZONE 'UTC')::date = day::date
           GROUP BY day
           ORDER BY day"#,
    )
    .bind(organization_id)
    .bind(days)
    .fetch_all(db)
    .await
}

pub async fn recent_activity(
    db: &Db,
    organization_id: Uuid,
    limit: i64,
) -> Result<Vec<RecentActivityRow>, sqlx::Error> {
    sqlx::query_as::<_, RecentActivityRow>(
        r#"SELECT a.id, a.action, a.entity_type, a.entity_id, u.name AS actor_name, a.metadata, a.created_at
           FROM audit_events a LEFT JOIN users u ON u.id = a.actor_user_id
           WHERE a.organization_id = $1
           ORDER BY a.created_at DESC LIMIT $2"#,
    )
    .bind(organization_id)
    .bind(limit)
    .fetch_all(db)
    .await
}

pub async fn recent_errors(
    db: &Db,
    organization_id: Uuid,
    limit: i64,
) -> Result<Vec<RecentErrorRow>, sqlx::Error> {
    sqlx::query_as::<_, RecentErrorRow>(
        r#"SELECT id AS document_id, original_filename, last_error_code AS error_code,
                  last_error_message AS error_message, created_at
           FROM documents
           WHERE organization_id = $1 AND processing_status = 'failed' AND deleted_at IS NULL
           ORDER BY updated_at DESC LIMIT $2"#,
    )
    .bind(organization_id)
    .bind(limit)
    .fetch_all(db)
    .await
}

// ---------------------------------------------------------------------------
// API keys
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, FromRow)]
pub struct ApiKeyRow {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub name: String,
    pub prefix: String,
    pub scopes: Vec<String>,
    pub created_by_name: Option<String>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
pub struct ApiKeyAuthRow {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub scopes: Vec<String>,
    pub is_demo: bool,
    pub organization_name: String,
    pub organization_slug: String,
    pub country: String,
    pub base_currency: String,
    pub settings: Value,
    pub created_by: Option<Uuid>,
}

pub async fn list_api_keys(db: &Db, organization_id: Uuid) -> Result<Vec<ApiKeyRow>, sqlx::Error> {
    sqlx::query_as::<_, ApiKeyRow>(
        r#"SELECT k.id, k.organization_id, k.name, k.prefix, k.scopes,
                  u.name AS created_by_name, k.last_used_at, k.expires_at, k.revoked_at, k.created_at
           FROM api_keys k LEFT JOIN users u ON u.id = k.created_by
           WHERE k.organization_id = $1
           ORDER BY k.created_at DESC"#,
    )
    .bind(organization_id)
    .fetch_all(db)
    .await
}

pub async fn create_api_key(
    db: &Db,
    organization_id: Uuid,
    name: &str,
    prefix: &str,
    key_hash: &str,
    scopes: &[String],
    created_by: Uuid,
    expires_at: Option<DateTime<Utc>>,
) -> Result<Uuid, sqlx::Error> {
    let (id,): (Uuid,) = sqlx::query_as(
        r#"INSERT INTO api_keys (organization_id, name, prefix, key_hash, scopes, created_by, expires_at)
           VALUES ($1,$2,$3,$4,$5,$6,$7) RETURNING id"#,
    )
    .bind(organization_id)
    .bind(name)
    .bind(prefix)
    .bind(key_hash)
    .bind(scopes)
    .bind(created_by)
    .bind(expires_at)
    .fetch_one(db)
    .await?;
    Ok(id)
}

pub async fn revoke_api_key(
    db: &Db,
    organization_id: Uuid,
    key_id: Uuid,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE api_keys SET revoked_at = now() WHERE organization_id = $1 AND id = $2 AND revoked_at IS NULL",
    )
    .bind(organization_id)
    .bind(key_id)
    .execute(db)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn find_api_key_by_hash(
    db: &Db,
    key_hash: &str,
) -> Result<Option<ApiKeyAuthRow>, sqlx::Error> {
    sqlx::query_as::<_, ApiKeyAuthRow>(
        r#"SELECT k.id, k.organization_id, k.scopes, o.is_demo, o.name AS organization_name,
                  o.slug AS organization_slug, o.country, o.base_currency, o.settings, k.created_by
           FROM api_keys k
           JOIN organizations o ON o.id = k.organization_id
           WHERE k.key_hash = $1 AND k.revoked_at IS NULL
             AND (k.expires_at IS NULL OR k.expires_at > now())
             AND o.deleted_at IS NULL"#,
    )
    .bind(key_hash)
    .fetch_optional(db)
    .await
}

pub async fn touch_api_key(db: &Db, key_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE api_keys SET last_used_at = now() WHERE id = $1")
        .bind(key_id)
        .execute(db)
        .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Webhooks
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, FromRow)]
pub struct WebhookEndpointRow {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub url: String,
    pub description: Option<String>,
    pub events: Vec<String>,
    pub enabled: bool,
    pub last_success_at: Option<DateTime<Utc>>,
    pub last_failure_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub delivery_count: i64,
    pub failed_count: i64,
}

#[derive(Debug, Clone, FromRow)]
pub struct WebhookDeliveryRow {
    pub id: Uuid,
    pub endpoint_id: Uuid,
    pub endpoint_url: String,
    pub event_id: Uuid,
    pub event_type: String,
    pub status: String,
    pub attempts: i32,
    pub last_status_code: Option<i32>,
    pub last_error: Option<String>,
    pub next_attempt_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub delivered_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, FromRow)]
pub struct ClaimedDeliveryRow {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub endpoint_id: Uuid,
    pub endpoint_url: String,
    pub secret_ciphertext: Vec<u8>,
    pub event_id: Uuid,
    pub event_type: String,
    pub payload: Value,
    pub attempts: i32,
}

#[derive(Debug, Clone, FromRow)]
pub struct DeliveryAttemptRow {
    pub id: Uuid,
    pub attempt_number: i32,
    pub status_code: Option<i32>,
    pub error: Option<String>,
    pub response_snippet: Option<String>,
    pub duration_ms: Option<i64>,
    pub created_at: DateTime<Utc>,
}

pub async fn list_webhook_endpoints(
    db: &Db,
    organization_id: Uuid,
) -> Result<Vec<WebhookEndpointRow>, sqlx::Error> {
    sqlx::query_as::<_, WebhookEndpointRow>(
        r#"SELECT w.id, w.organization_id, w.url, w.description, w.events, w.enabled,
                  w.last_success_at, w.last_failure_at, w.created_at, w.updated_at,
                  (SELECT count(*) FROM webhook_deliveries d WHERE d.endpoint_id = w.id) AS delivery_count,
                  (SELECT count(*) FROM webhook_deliveries d WHERE d.endpoint_id = w.id AND d.status IN ('failed','dead_letter')) AS failed_count
           FROM webhook_endpoints w
           WHERE w.organization_id = $1
           ORDER BY w.created_at DESC"#,
    )
    .bind(organization_id)
    .fetch_all(db)
    .await
}

pub async fn create_webhook_endpoint(
    db: &Db,
    organization_id: Uuid,
    url: &str,
    description: Option<&str>,
    secret_ciphertext: &[u8],
    events: &[String],
    created_by: Uuid,
) -> Result<Uuid, sqlx::Error> {
    let (id,): (Uuid,) = sqlx::query_as(
        r#"INSERT INTO webhook_endpoints (organization_id, url, description, secret_ciphertext, events, created_by)
           VALUES ($1,$2,$3,$4,$5,$6) RETURNING id"#,
    )
    .bind(organization_id)
    .bind(url)
    .bind(description)
    .bind(secret_ciphertext)
    .bind(events)
    .bind(created_by)
    .fetch_one(db)
    .await?;
    Ok(id)
}

pub async fn update_webhook_endpoint(
    db: &Db,
    organization_id: Uuid,
    endpoint_id: Uuid,
    url: &str,
    description: Option<&str>,
    events: &[String],
    enabled: bool,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE webhook_endpoints SET url = $3, description = $4, events = $5, enabled = $6, updated_at = now() WHERE organization_id = $1 AND id = $2",
    )
    .bind(organization_id)
    .bind(endpoint_id)
    .bind(url)
    .bind(description)
    .bind(events)
    .bind(enabled)
    .execute(db)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn rotate_webhook_secret(
    db: &Db,
    organization_id: Uuid,
    endpoint_id: Uuid,
    secret_ciphertext: &[u8],
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE webhook_endpoints SET secret_ciphertext = $3, updated_at = now() WHERE organization_id = $1 AND id = $2",
    )
    .bind(organization_id)
    .bind(endpoint_id)
    .bind(secret_ciphertext)
    .execute(db)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn delete_webhook_endpoint(
    db: &Db,
    organization_id: Uuid,
    endpoint_id: Uuid,
) -> Result<bool, sqlx::Error> {
    let result =
        sqlx::query("DELETE FROM webhook_endpoints WHERE organization_id = $1 AND id = $2")
            .bind(organization_id)
            .bind(endpoint_id)
            .execute(db)
            .await?;
    Ok(result.rows_affected() == 1)
}

/// Create one delivery per matching endpoint and event. The unique
/// (endpoint,event_id) constraint makes retries and duplicate emissions safe.
pub async fn enqueue_webhook_event(
    db: &Db,
    organization_id: Uuid,
    event_type: &str,
    event_id: Uuid,
    payload: &Value,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        r#"INSERT INTO webhook_deliveries
             (organization_id, endpoint_id, event_id, event_type, payload)
           SELECT w.organization_id, w.id, $2::uuid, $3::text, $4::jsonb
           FROM webhook_endpoints w
           WHERE w.organization_id = $1 AND w.enabled = TRUE
             AND ($3::text = ANY(w.events) OR '*'::text = ANY(w.events))
           ON CONFLICT (endpoint_id, event_id) DO NOTHING"#,
    )
    .bind(organization_id)
    .bind(event_id)
    .bind(event_type)
    .bind(payload)
    .execute(db)
    .await?;
    Ok(result.rows_affected())
}

pub async fn list_webhook_deliveries(
    db: &Db,
    organization_id: Uuid,
    endpoint_id: Option<Uuid>,
    limit: i64,
) -> Result<Vec<WebhookDeliveryRow>, sqlx::Error> {
    sqlx::query_as::<_, WebhookDeliveryRow>(
        r#"SELECT d.id, d.endpoint_id, w.url AS endpoint_url, d.event_id, d.event_type, d.status,
                  d.attempts, d.last_status_code, d.last_error, d.next_attempt_at, d.created_at, d.delivered_at
           FROM webhook_deliveries d JOIN webhook_endpoints w ON w.id = d.endpoint_id
           WHERE d.organization_id = $1 AND ($2::uuid IS NULL OR d.endpoint_id = $2)
           ORDER BY d.created_at DESC LIMIT $3"#,
    )
    .bind(organization_id)
    .bind(endpoint_id)
    .bind(limit)
    .fetch_all(db)
    .await
}

pub async fn get_webhook_delivery(
    db: &Db,
    organization_id: Uuid,
    delivery_id: Uuid,
) -> Result<Option<WebhookDeliveryRow>, sqlx::Error> {
    sqlx::query_as::<_, WebhookDeliveryRow>(
        r#"SELECT d.id, d.endpoint_id, w.url AS endpoint_url, d.event_id, d.event_type, d.status,
                  d.attempts, d.last_status_code, d.last_error, d.next_attempt_at, d.created_at, d.delivered_at
           FROM webhook_deliveries d JOIN webhook_endpoints w ON w.id = d.endpoint_id
           WHERE d.organization_id = $1 AND d.id = $2"#,
    )
    .bind(organization_id)
    .bind(delivery_id)
    .fetch_optional(db)
    .await
}

pub async fn delivery_attempts(
    db: &Db,
    organization_id: Uuid,
    delivery_id: Uuid,
) -> Result<Vec<DeliveryAttemptRow>, sqlx::Error> {
    sqlx::query_as::<_, DeliveryAttemptRow>(
        r#"SELECT id, attempt_number, status_code, error, response_snippet, duration_ms, created_at
           FROM webhook_delivery_attempts
           WHERE organization_id = $1 AND delivery_id = $2
           ORDER BY attempt_number"#,
    )
    .bind(organization_id)
    .bind(delivery_id)
    .fetch_all(db)
    .await
}

pub async fn claim_webhook_delivery(
    db: &Db,
    worker_id: &str,
    lease_seconds: i32,
) -> Result<Option<ClaimedDeliveryRow>, sqlx::Error> {
    sqlx::query_as::<_, ClaimedDeliveryRow>(
        r#"WITH candidate AS (
               SELECT id FROM webhook_deliveries
               WHERE status IN ('pending','failed') AND next_attempt_at <= now()
               ORDER BY next_attempt_at LIMIT 1
               FOR UPDATE SKIP LOCKED
           )
           UPDATE webhook_deliveries d
           SET status = 'delivering', attempts = d.attempts + 1, locked_at = now(),
               lease_expires_at = now() + ($2::int * interval '1 second'),
               locked_by = $1, updated_at = now()
           FROM candidate c, webhook_endpoints w
           WHERE d.id = c.id AND w.id = d.endpoint_id
           RETURNING d.id, d.organization_id, d.endpoint_id, w.url AS endpoint_url,
                     w.secret_ciphertext, d.event_id, d.event_type, d.payload, d.attempts"#,
    )
    .bind(worker_id)
    .bind(lease_seconds)
    .fetch_optional(db)
    .await
}

pub async fn record_delivery_attempt(
    db: &Db,
    organization_id: Uuid,
    delivery_id: Uuid,
    attempt_number: i32,
    status_code: Option<i32>,
    error: Option<&str>,
    response_snippet: Option<&str>,
    duration_ms: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"INSERT INTO webhook_delivery_attempts
           (organization_id, delivery_id, attempt_number, status_code, error, response_snippet, duration_ms)
           VALUES ($1,$2,$3,$4,$5,$6,$7)"#,
    )
    .bind(organization_id)
    .bind(delivery_id)
    .bind(attempt_number)
    .bind(status_code)
    .bind(error)
    .bind(response_snippet)
    .bind(duration_ms)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn finish_webhook_delivery(
    db: &Db,
    delivery_id: Uuid,
    success: bool,
    status_code: Option<i32>,
    error: Option<&str>,
    response_snippet: Option<&str>,
    dead: bool,
    next_attempt_at: Option<DateTime<Utc>>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"UPDATE webhook_deliveries
           SET status = CASE WHEN $2 THEN 'succeeded' WHEN $7 THEN 'dead_letter' ELSE 'failed' END,
               last_status_code = $3, last_error = $4, response_snippet = $5,
               next_attempt_at = COALESCE($6, next_attempt_at),
               delivered_at = CASE WHEN $2 THEN now() ELSE delivered_at END,
               locked_at = NULL, lease_expires_at = NULL, locked_by = NULL, updated_at = now()
           WHERE id = $1"#,
    )
    .bind(delivery_id)
    .bind(success)
    .bind(status_code)
    .bind(error)
    .bind(response_snippet)
    .bind(next_attempt_at)
    .bind(dead)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn update_endpoint_delivery_status(
    db: &Db,
    endpoint_id: Uuid,
    success: bool,
) -> Result<(), sqlx::Error> {
    if success {
        sqlx::query("UPDATE webhook_endpoints SET last_success_at = now() WHERE id = $1")
            .bind(endpoint_id)
            .execute(db)
            .await?;
    } else {
        sqlx::query("UPDATE webhook_endpoints SET last_failure_at = now() WHERE id = $1")
            .bind(endpoint_id)
            .execute(db)
            .await?;
    }
    Ok(())
}

pub async fn requeue_webhook_delivery(
    db: &Db,
    organization_id: Uuid,
    delivery_id: Uuid,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE webhook_deliveries SET status = 'pending', attempts = 0, next_attempt_at = now(), last_error = NULL, response_snippet = NULL, delivered_at = NULL, locked_at = NULL, lease_expires_at = NULL, locked_by = NULL, updated_at = now() WHERE organization_id = $1 AND id = $2",
    )
    .bind(organization_id)
    .bind(delivery_id)
    .execute(db)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn recover_webhook_deliveries(db: &Db) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE webhook_deliveries SET status = 'pending', next_attempt_at = now(), locked_at = NULL, lease_expires_at = NULL, locked_by = NULL WHERE status = 'delivering' AND lease_expires_at < now()",
    )
    .execute(db)
    .await?;
    Ok(result.rows_affected())
}

// ---------------------------------------------------------------------------
// Exports
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, FromRow)]
pub struct ExportJobRow {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub requested_by: Option<Uuid>,
    pub requested_by_name: Option<String>,
    pub format: String,
    pub status: String,
    pub filters: Value,
    pub storage_key: Option<String>,
    pub filename: Option<String>,
    pub row_count: Option<i32>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, FromRow)]
pub struct ClaimedExportRow {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub format: String,
    pub filters: Value,
    pub attempts: i32,
}

pub async fn create_export_job(
    db: &Db,
    organization_id: Uuid,
    requested_by: Uuid,
    format: &str,
    filters: &Value,
    idempotency_key: Option<&str>,
) -> Result<ExportJobRow, DbError> {
    if let Some(key) = idempotency_key {
        if let Some(existing) = sqlx::query_as::<_, ExportJobRow>(
            r#"SELECT e.id, e.organization_id, e.requested_by, u.name AS requested_by_name, e.format,
                      e.status, e.filters, e.storage_key, e.filename, e.row_count, e.error_code,
                      e.error_message, e.created_at, e.started_at, e.completed_at, e.expires_at
               FROM export_jobs e LEFT JOIN users u ON u.id = e.requested_by
               WHERE e.organization_id = $1 AND e.idempotency_key = $2"#,
        )
        .bind(organization_id)
        .bind(key)
        .fetch_optional(db)
        .await?
        {
            return Ok(existing);
        }
    }
    let id = sqlx::query_scalar::<_, Uuid>(
        r#"INSERT INTO export_jobs (organization_id, requested_by, format, filters, idempotency_key)
           VALUES ($1,$2,$3,$4,$5) RETURNING id"#,
    )
    .bind(organization_id)
    .bind(requested_by)
    .bind(format)
    .bind(filters)
    .bind(idempotency_key)
    .fetch_one(db)
    .await
    .map_err(|e| {
        if matches!(&e, sqlx::Error::Database(d) if d.is_unique_violation()) {
            DbError::Conflict("an export with this idempotency key already exists".into())
        } else {
            DbError::Sqlx(e)
        }
    })?;
    get_export_job(db, organization_id, id)
        .await?
        .ok_or(DbError::NotFound)
}

pub async fn get_export_job(
    db: &Db,
    organization_id: Uuid,
    export_id: Uuid,
) -> Result<Option<ExportJobRow>, sqlx::Error> {
    sqlx::query_as::<_, ExportJobRow>(
        r#"SELECT e.id, e.organization_id, e.requested_by, u.name AS requested_by_name, e.format,
                  e.status, e.filters, e.storage_key, e.filename, e.row_count, e.error_code,
                  e.error_message, e.created_at, e.started_at, e.completed_at, e.expires_at
           FROM export_jobs e LEFT JOIN users u ON u.id = e.requested_by
           WHERE e.organization_id = $1 AND e.id = $2"#,
    )
    .bind(organization_id)
    .bind(export_id)
    .fetch_optional(db)
    .await
}

pub async fn list_export_jobs(
    db: &Db,
    organization_id: Uuid,
    limit: i64,
) -> Result<Vec<ExportJobRow>, sqlx::Error> {
    sqlx::query_as::<_, ExportJobRow>(
        r#"SELECT e.id, e.organization_id, e.requested_by, u.name AS requested_by_name, e.format,
                  e.status, e.filters, e.storage_key, e.filename, e.row_count, e.error_code,
                  e.error_message, e.created_at, e.started_at, e.completed_at, e.expires_at
           FROM export_jobs e LEFT JOIN users u ON u.id = e.requested_by
           WHERE e.organization_id = $1
           ORDER BY e.created_at DESC LIMIT $2"#,
    )
    .bind(organization_id)
    .bind(limit)
    .fetch_all(db)
    .await
}

pub async fn claim_export_job(
    db: &Db,
    worker_id: &str,
    lease_seconds: i32,
) -> Result<Option<ClaimedExportRow>, sqlx::Error> {
    sqlx::query_as::<_, ClaimedExportRow>(
        r#"WITH candidate AS (
               SELECT id FROM export_jobs WHERE status = 'queued' ORDER BY created_at LIMIT 1 FOR UPDATE SKIP LOCKED
           )
           UPDATE export_jobs e
           SET status = 'running', attempts = e.attempts + 1, locked_at = now(),
               lease_expires_at = now() + ($2::int * interval '1 second'), locked_by = $1,
               started_at = COALESCE(e.started_at, now())
           FROM candidate c WHERE e.id = c.id
           RETURNING e.id, e.organization_id, e.format, e.filters, e.attempts"#,
    )
    .bind(worker_id)
    .bind(lease_seconds)
    .fetch_optional(db)
    .await
}

pub async fn complete_export_job(
    db: &Db,
    export_id: Uuid,
    storage_key: &str,
    filename: &str,
    row_count: i32,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"UPDATE export_jobs SET status = 'succeeded', storage_key = $2, filename = $3,
             row_count = $4, completed_at = now(), expires_at = now() + interval '7 days',
             locked_at = NULL, lease_expires_at = NULL, locked_by = NULL
           WHERE id = $1"#,
    )
    .bind(export_id)
    .bind(storage_key)
    .bind(filename)
    .bind(row_count)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn fail_export_job(
    db: &Db,
    export_id: Uuid,
    code: &str,
    message: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE export_jobs SET status = 'failed', error_code = $2, error_message = $3, completed_at = now(), locked_at = NULL, lease_expires_at = NULL, locked_by = NULL WHERE id = $1",
    )
    .bind(export_id)
    .bind(code)
    .bind(message.chars().take(1000).collect::<String>())
    .execute(db)
    .await?;
    Ok(())
}

/// Requeue exports whose worker lease expired. After three attempts the job is
/// failed instead of being requeued forever.
pub async fn recover_expired_exports(db: &Db) -> Result<u64, sqlx::Error> {
    let retry = sqlx::query(
        r#"UPDATE export_jobs
           SET status = 'queued', locked_at = NULL, lease_expires_at = NULL, locked_by = NULL,
               error_code = 'WORKER_LEASE_EXPIRED',
               error_message = 'Worker lease expired; the export was requeued'
           WHERE status = 'running' AND lease_expires_at < now() AND attempts < 3"#,
    )
    .execute(db)
    .await?;
    let failed = sqlx::query(
        r#"UPDATE export_jobs
           SET status = 'failed', locked_at = NULL, lease_expires_at = NULL, locked_by = NULL,
               error_code = 'WORKER_LEASE_EXPIRED',
               error_message = 'Worker lease expired after repeated attempts',
               completed_at = now()
           WHERE status = 'running' AND lease_expires_at < now() AND attempts >= 3"#,
    )
    .execute(db)
    .await?;
    Ok(retry.rows_affected() + failed.rows_affected())
}

// ---------------------------------------------------------------------------
// Automation
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, FromRow)]
pub struct AutomationRuleRow {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub enabled: bool,
    pub priority: i32,
    pub spec: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub async fn list_automation_rules(
    db: &Db,
    organization_id: Uuid,
) -> Result<Vec<AutomationRuleRow>, sqlx::Error> {
    sqlx::query_as::<_, AutomationRuleRow>(
        r#"SELECT id, organization_id, name, description, enabled, priority, spec, created_at, updated_at
           FROM automation_rules WHERE organization_id = $1 ORDER BY priority, created_at"#,
    )
    .bind(organization_id)
    .fetch_all(db)
    .await
}

pub async fn enabled_automation_rules(
    db: &Db,
    organization_id: Uuid,
) -> Result<Vec<AutomationRuleRow>, sqlx::Error> {
    sqlx::query_as::<_, AutomationRuleRow>(
        r#"SELECT id, organization_id, name, description, enabled, priority, spec, created_at, updated_at
           FROM automation_rules WHERE organization_id = $1 AND enabled = TRUE ORDER BY priority, created_at"#,
    )
    .bind(organization_id)
    .fetch_all(db)
    .await
}

pub async fn create_automation_rule(
    db: &Db,
    organization_id: Uuid,
    name: &str,
    description: Option<&str>,
    enabled: bool,
    priority: i32,
    spec: &Value,
    created_by: Uuid,
) -> Result<Uuid, sqlx::Error> {
    let (id,): (Uuid,) = sqlx::query_as(
        "INSERT INTO automation_rules (organization_id, name, description, enabled, priority, spec, created_by) VALUES ($1,$2,$3,$4,$5,$6,$7) RETURNING id",
    )
    .bind(organization_id)
    .bind(name)
    .bind(description)
    .bind(enabled)
    .bind(priority)
    .bind(spec)
    .bind(created_by)
    .fetch_one(db)
    .await?;
    Ok(id)
}

pub async fn update_automation_rule(
    db: &Db,
    organization_id: Uuid,
    rule_id: Uuid,
    name: &str,
    description: Option<&str>,
    enabled: bool,
    priority: i32,
    spec: &Value,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE automation_rules SET name = $3, description = $4, enabled = $5, priority = $6, spec = $7, updated_at = now() WHERE organization_id = $1 AND id = $2",
    )
    .bind(organization_id)
    .bind(rule_id)
    .bind(name)
    .bind(description)
    .bind(enabled)
    .bind(priority)
    .bind(spec)
    .execute(db)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn delete_automation_rule(
    db: &Db,
    organization_id: Uuid,
    rule_id: Uuid,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("DELETE FROM automation_rules WHERE organization_id = $1 AND id = $2")
        .bind(organization_id)
        .bind(rule_id)
        .execute(db)
        .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn insert_automation_run(
    db: &Db,
    organization_id: Uuid,
    rule_id: Uuid,
    invoice_id: Uuid,
    outcome: &Value,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO automation_runs (organization_id, rule_id, invoice_id, outcome) VALUES ($1,$2,$3,$4)",
    )
    .bind(organization_id)
    .bind(rule_id)
    .bind(invoice_id)
    .bind(outcome)
    .execute(db)
    .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Usage and quota
// ---------------------------------------------------------------------------

pub struct UsageInput<'a> {
    pub organization_id: Uuid,
    pub document_id: Option<Uuid>,
    pub kind: &'a str,
    pub quantity: Decimal,
    pub provider: Option<&'a str>,
    pub model: Option<&'a str>,
    pub input_tokens: Option<i32>,
    pub output_tokens: Option<i32>,
    pub duration_ms: Option<i64>,
    pub estimated_cost_usd: Option<Decimal>,
    pub metadata: Value,
}

pub async fn record_usage(db: &Db, input: &UsageInput<'_>) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"INSERT INTO usage_records
           (organization_id, document_id, kind, quantity, provider, model, input_tokens, output_tokens,
            duration_ms, estimated_cost_usd, metadata)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)"#,
    )
    .bind(input.organization_id)
    .bind(input.document_id)
    .bind(input.kind)
    .bind(input.quantity)
    .bind(input.provider)
    .bind(input.model)
    .bind(input.input_tokens)
    .bind(input.output_tokens)
    .bind(input.duration_ms)
    .bind(input.estimated_cost_usd)
    .bind(&input.metadata)
    .execute(db)
    .await?;
    Ok(())
}

#[derive(Debug, Clone, FromRow)]
pub struct UsageSummaryRow {
    pub documents_this_month: i64,
    pub pages_this_month: i64,
    pub ai_requests_this_month: i64,
    pub ocr_pages_this_month: i64,
    pub storage_bytes: i64,
    pub estimated_ai_cost_usd: Option<Decimal>,
}

pub async fn usage_summary(db: &Db, organization_id: Uuid) -> Result<UsageSummaryRow, sqlx::Error> {
    sqlx::query_as::<_, UsageSummaryRow>(
        r#"SELECT
             (SELECT count(*) FROM documents WHERE organization_id = $1 AND created_at >= date_trunc('month', now()) AND deleted_at IS NULL) AS documents_this_month,
             (SELECT COALESCE(sum(quantity),0)::bigint FROM usage_records WHERE organization_id = $1 AND kind = 'page_processed' AND created_at >= date_trunc('month', now())) AS pages_this_month,
             (SELECT count(*) FROM usage_records WHERE organization_id = $1 AND kind = 'ai_extraction' AND created_at >= date_trunc('month', now())) AS ai_requests_this_month,
             (SELECT COALESCE(sum(quantity),0)::bigint FROM usage_records WHERE organization_id = $1 AND kind = 'ocr_page' AND created_at >= date_trunc('month', now())) AS ocr_pages_this_month,
             (SELECT COALESCE(sum(size_bytes),0)::bigint FROM documents WHERE organization_id = $1 AND deleted_at IS NULL) AS storage_bytes,
             (SELECT sum(estimated_cost_usd) FROM usage_records WHERE organization_id = $1 AND created_at >= date_trunc('month', now())) AS estimated_ai_cost_usd"#,
    )
    .bind(organization_id)
    .fetch_one(db)
    .await
}

pub async fn documents_this_month(db: &Db, organization_id: Uuid) -> Result<i64, sqlx::Error> {
    let (count,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM documents WHERE organization_id = $1 AND created_at >= date_trunc('month', now()) AND deleted_at IS NULL",
    )
    .bind(organization_id)
    .fetch_one(db)
    .await?;
    Ok(count)
}

pub async fn ai_requests_this_month(db: &Db, organization_id: Uuid) -> Result<i64, sqlx::Error> {
    let (count,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM usage_records WHERE organization_id = $1 AND kind = 'ai_extraction' AND created_at >= date_trunc('month', now())",
    )
    .bind(organization_id)
    .fetch_one(db)
    .await?;
    Ok(count)
}

// ---------------------------------------------------------------------------
// Integrations
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, FromRow)]
pub struct IntegrationRow {
    pub provider: String,
    pub status: String,
    pub config: Value,
    pub last_verified_at: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
}

pub async fn list_integrations(
    db: &Db,
    organization_id: Uuid,
) -> Result<Vec<IntegrationRow>, sqlx::Error> {
    sqlx::query_as::<_, IntegrationRow>(
        "SELECT provider, status, config, last_verified_at, updated_at FROM integration_connections WHERE organization_id = $1 ORDER BY provider",
    )
    .bind(organization_id)
    .fetch_all(db)
    .await
}

pub async fn upsert_integration(
    db: &Db,
    organization_id: Uuid,
    provider: &str,
    status: &str,
    config: &Value,
    encrypted_credentials: Option<&[u8]>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"INSERT INTO integration_connections (organization_id, provider, status, config, encrypted_credentials)
           VALUES ($1,$2,$3,$4,$5)
           ON CONFLICT (organization_id, provider) DO UPDATE
           SET status = EXCLUDED.status, config = EXCLUDED.config,
               encrypted_credentials = COALESCE(EXCLUDED.encrypted_credentials, integration_connections.encrypted_credentials),
               updated_at = now()"#,
    )
    .bind(organization_id)
    .bind(provider)
    .bind(status)
    .bind(config)
    .bind(encrypted_credentials)
    .execute(db)
    .await?;
    Ok(())
}
