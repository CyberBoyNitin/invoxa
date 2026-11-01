use crate::{Db, DbError};
use chrono::{DateTime, Utc};
use sqlx::{FromRow, Postgres, Transaction};
use uuid::Uuid;

#[derive(Debug, Clone, FromRow)]
pub struct DocumentRow {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub uploaded_by: Option<Uuid>,
    pub source: String,
    pub original_filename: String,
    pub storage_key: String,
    pub content_type: String,
    pub sha256: String,
    pub size_bytes: i64,
    pub page_count: Option<i32>,
    pub document_type: String,
    pub processing_status: String,
    pub processing_attempts: i32,
    pub extraction_provider: Option<String>,
    pub extraction_model: Option<String>,
    pub processing_duration_ms: Option<i64>,
    pub last_error_code: Option<String>,
    pub last_error_message: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, FromRow)]
pub struct DocumentEventRow {
    pub id: Uuid,
    pub stage: String,
    pub status: String,
    pub message: Option<String>,
    pub duration_ms: Option<i64>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
pub struct DocumentListRow {
    pub id: Uuid,
    pub original_filename: String,
    pub content_type: String,
    pub size_bytes: i64,
    pub page_count: Option<i32>,
    pub document_type: String,
    pub processing_status: String,
    pub last_error_code: Option<String>,
    pub created_at: DateTime<Utc>,
    pub invoice_id: Option<Uuid>,
    pub vendor_name: Option<String>,
    pub invoice_number: Option<String>,
    pub currency: Option<String>,
    pub total: Option<rust_decimal::Decimal>,
    pub invoice_status: Option<String>,
    pub issue_count: i64,
    pub duplicate_count: i64,
}

pub struct NewDocument<'a> {
    pub organization_id: Uuid,
    pub uploaded_by: Uuid,
    pub source: &'a str,
    pub original_filename: &'a str,
    pub storage_key: &'a str,
    pub content_type: &'a str,
    pub sha256: &'a str,
    pub size_bytes: i64,
}

pub async fn create_document_with_job(
    db: &Db,
    new: &NewDocument<'_>,
) -> Result<(DocumentRow, Uuid), DbError> {
    let mut tx = db.begin().await?;
    let doc = sqlx::query_as::<_, DocumentRow>(
        r#"INSERT INTO documents
             (organization_id, uploaded_by, source, original_filename, storage_key, content_type,
              sha256, size_bytes, processing_status)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,'queued')
           RETURNING id, organization_id, uploaded_by, source, original_filename, storage_key,
                     content_type, sha256, size_bytes, page_count, document_type, processing_status,
                     processing_attempts, extraction_provider, extraction_model, processing_duration_ms,
                     last_error_code, last_error_message, created_at, updated_at, deleted_at"#,
    )
    .bind(new.organization_id)
    .bind(new.uploaded_by)
    .bind(new.source)
    .bind(new.original_filename)
    .bind(new.storage_key)
    .bind(new.content_type)
    .bind(new.sha256)
    .bind(new.size_bytes)
    .fetch_one(&mut *tx)
    .await?;

    let (job_id,): (Uuid,) = sqlx::query_as(
        "INSERT INTO document_processing_jobs (organization_id, document_id) VALUES ($1, $2) RETURNING id",
    )
    .bind(new.organization_id)
    .bind(doc.id)
    .fetch_one(&mut *tx)
    .await?;

    sqlx::query(
        "INSERT INTO document_events (organization_id, document_id, stage, status, message) VALUES ($1,$2,'upload','queued','Document stored and queued for processing')",
    )
    .bind(new.organization_id)
    .bind(doc.id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok((doc, job_id))
}

pub async fn get_document(
    db: &Db,
    organization_id: Uuid,
    id: Uuid,
) -> Result<Option<DocumentRow>, sqlx::Error> {
    sqlx::query_as::<_, DocumentRow>(
        r#"SELECT id, organization_id, uploaded_by, source, original_filename, storage_key,
                  content_type, sha256, size_bytes, page_count, document_type, processing_status,
                  processing_attempts, extraction_provider, extraction_model, processing_duration_ms,
                  last_error_code, last_error_message, created_at, updated_at, deleted_at
           FROM documents
           WHERE organization_id = $1 AND id = $2 AND deleted_at IS NULL"#,
    )
    .bind(organization_id)
    .bind(id)
    .fetch_optional(db)
    .await
}

pub async fn list_documents(
    db: &Db,
    organization_id: Uuid,
    status: Option<&str>,
    search: Option<&str>,
    limit: i64,
    offset: i64,
) -> Result<(Vec<DocumentListRow>, i64), sqlx::Error> {
    let rows = sqlx::query_as::<_, DocumentListRow>(
        r#"SELECT d.id, d.original_filename, d.content_type, d.size_bytes, d.page_count,
                  d.document_type, d.processing_status, d.last_error_code, d.created_at,
                  i.id AS invoice_id, v.name AS vendor_name, i.invoice_number, i.currency,
                  i.total, i.status AS invoice_status,
                  COALESCE((SELECT count(*) FROM validation_issues vi
                            WHERE vi.invoice_id = i.id AND vi.resolved_at IS NULL
                              AND vi.severity IN ('error','warning')), 0) AS issue_count,
                  COALESCE((SELECT count(*) FROM duplicate_candidates dc
                            WHERE dc.invoice_id = i.id AND dc.status = 'open'), 0) AS duplicate_count
           FROM documents d
           LEFT JOIN extracted_invoices i ON i.document_id = d.id AND i.organization_id = d.organization_id
           LEFT JOIN vendors v ON v.id = i.vendor_id AND v.organization_id = d.organization_id
           WHERE d.organization_id = $1 AND d.deleted_at IS NULL
             AND ($2::text IS NULL OR d.processing_status = $2)
             AND ($3::text IS NULL OR d.original_filename ILIKE '%' || $3 || '%'
                  OR i.invoice_number ILIKE '%' || $3 || '%'
                  OR v.name ILIKE '%' || $3 || '%')
           ORDER BY d.created_at DESC
           LIMIT $4 OFFSET $5"#,
    )
    .bind(organization_id)
    .bind(status)
    .bind(search)
    .bind(limit)
    .bind(offset)
    .fetch_all(db)
    .await?;

    let (total,): (i64,) = sqlx::query_as(
        r#"SELECT count(*)
           FROM documents d
           LEFT JOIN extracted_invoices i ON i.document_id = d.id AND i.organization_id = d.organization_id
           LEFT JOIN vendors v ON v.id = i.vendor_id AND v.organization_id = d.organization_id
           WHERE d.organization_id = $1 AND d.deleted_at IS NULL
             AND ($2::text IS NULL OR d.processing_status = $2)
             AND ($3::text IS NULL OR d.original_filename ILIKE '%' || $3 || '%'
                  OR i.invoice_number ILIKE '%' || $3 || '%'
                  OR v.name ILIKE '%' || $3 || '%')"#,
    )
    .bind(organization_id)
    .bind(status)
    .bind(search)
    .fetch_one(db)
    .await?;
    Ok((rows, total))
}

pub async fn insert_document_event(
    db: &Db,
    organization_id: Uuid,
    document_id: Uuid,
    stage: &str,
    status: &str,
    message: Option<&str>,
    duration_ms: Option<i64>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO document_events (organization_id, document_id, stage, status, message, duration_ms) VALUES ($1,$2,$3,$4,$5,$6)",
    )
    .bind(organization_id)
    .bind(document_id)
    .bind(stage)
    .bind(status)
    .bind(message)
    .bind(duration_ms)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn document_events(
    db: &Db,
    organization_id: Uuid,
    document_id: Uuid,
) -> Result<Vec<DocumentEventRow>, sqlx::Error> {
    sqlx::query_as::<_, DocumentEventRow>(
        "SELECT id, stage, status, message, duration_ms, created_at FROM document_events WHERE organization_id = $1 AND document_id = $2 ORDER BY created_at",
    )
    .bind(organization_id)
    .bind(document_id)
    .fetch_all(db)
    .await
}

/// Explicit state transition with an audit event. The caller cannot update
/// statuses with a direct SQL write.
pub async fn transition_document(
    db: &Db,
    organization_id: Uuid,
    document_id: Uuid,
    next: invoxa_domain::status::DocumentStatus,
    stage: &str,
    message: Option<&str>,
    duration_ms: Option<i64>,
) -> Result<DocumentRow, DbError> {
    let mut tx = db.begin().await?;
    let current: Option<(String,)> = sqlx::query_as(
        "SELECT processing_status FROM documents WHERE organization_id = $1 AND id = $2 AND deleted_at IS NULL FOR UPDATE",
    )
    .bind(organization_id)
    .bind(document_id)
    .fetch_optional(&mut *tx)
    .await?;
    let (current_status,) = current.ok_or(DbError::NotFound)?;
    let current =
        invoxa_domain::status::DocumentStatus::parse(&current_status).ok_or_else(|| {
            DbError::InvalidTransition(format!("unknown document status {current_status}"))
        })?;
    if !current.can_transition_to(next) {
        return Err(DbError::InvalidTransition(format!(
            "document cannot move from {} to {}",
            current.as_str(),
            next.as_str()
        )));
    }
    let row = sqlx::query_as::<_, DocumentRow>(
        r#"UPDATE documents
           SET processing_status = $3, updated_at = now(),
               last_error_code = CASE WHEN $3 IN ('failed','review_required') THEN last_error_code ELSE NULL END,
               last_error_message = CASE WHEN $3 IN ('failed','review_required') THEN last_error_message ELSE NULL END
           WHERE organization_id = $1 AND id = $2
           RETURNING id, organization_id, uploaded_by, source, original_filename, storage_key,
                     content_type, sha256, size_bytes, page_count, document_type, processing_status,
                     processing_attempts, extraction_provider, extraction_model, processing_duration_ms,
                     last_error_code, last_error_message, created_at, updated_at, deleted_at"#,
    )
    .bind(organization_id)
    .bind(document_id)
    .bind(next.as_str())
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO document_events (organization_id, document_id, stage, status, message, duration_ms) VALUES ($1,$2,$3,$4,$5,$6)",
    )
    .bind(organization_id)
    .bind(document_id)
    .bind(stage)
    .bind(next.as_str())
    .bind(message)
    .bind(duration_ms)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(row)
}

pub async fn set_preflight_metadata(
    db: &Db,
    organization_id: Uuid,
    document_id: Uuid,
    page_count: i32,
    document_type: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE documents SET page_count = $3, document_type = $4, updated_at = now() WHERE organization_id = $1 AND id = $2",
    )
    .bind(organization_id)
    .bind(document_id)
    .bind(page_count)
    .bind(document_type)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn set_processing_error(
    db: &Db,
    organization_id: Uuid,
    document_id: Uuid,
    code: &str,
    message: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE documents SET last_error_code = $3, last_error_message = $4, updated_at = now() WHERE organization_id = $1 AND id = $2",
    )
    .bind(organization_id)
    .bind(document_id)
    .bind(code)
    .bind(message.chars().take(1000).collect::<String>())
    .execute(db)
    .await?;
    Ok(())
}

pub async fn set_extraction_meta(
    db: &Db,
    organization_id: Uuid,
    document_id: Uuid,
    provider: &str,
    model: &str,
    duration_ms: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"UPDATE documents SET extraction_provider = $3, extraction_model = $4,
             processing_duration_ms = $5, processing_attempts = processing_attempts + 1, updated_at = now()
           WHERE organization_id = $1 AND id = $2"#,
    )
    .bind(organization_id)
    .bind(document_id)
    .bind(provider)
    .bind(model)
    .bind(duration_ms)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn delete_document(
    db: &Db,
    organization_id: Uuid,
    document_id: Uuid,
) -> Result<Option<DocumentRow>, DbError> {
    let doc = get_document(db, organization_id, document_id).await?;
    if doc.is_none() {
        return Ok(None);
    }
    sqlx::query(
        "UPDATE documents SET deleted_at = now(), updated_at = now(), processing_status = CASE WHEN processing_status IN ('approved','exported') THEN processing_status ELSE 'cancelled' END WHERE organization_id = $1 AND id = $2",
    )
    .bind(organization_id)
    .bind(document_id)
    .execute(db)
    .await?;
    sqlx::query(
        "UPDATE document_processing_jobs SET status = 'cancelled', updated_at = now() WHERE organization_id = $1 AND document_id = $2 AND status IN ('queued','retry','running')",
    )
    .bind(organization_id)
    .bind(document_id)
    .execute(db)
    .await?;
    Ok(doc)
}

pub async fn retry_document(
    db: &Db,
    organization_id: Uuid,
    document_id: Uuid,
) -> Result<Uuid, DbError> {
    let mut tx = db.begin().await?;
    let row: Option<(String, Option<String>)> = sqlx::query_as(
        "SELECT d.processing_status, i.status FROM documents d LEFT JOIN extracted_invoices i ON i.document_id = d.id WHERE d.organization_id = $1 AND d.id = $2 AND d.deleted_at IS NULL FOR UPDATE OF d",
    )
    .bind(organization_id)
    .bind(document_id)
    .fetch_optional(&mut *tx)
    .await?;
    let (status, invoice_status) = row.ok_or(DbError::NotFound)?;
    if let Some(invoice_status) = invoice_status {
        if matches!(invoice_status.as_str(), "approved" | "exported") {
            return Err(DbError::Validation(
                "approved or exported invoices must be reopened by an owner before reprocessing"
                    .into(),
            ));
        }
    }
    if !matches!(
        status.as_str(),
        "failed" | "review_required" | "ready_for_approval" | "rejected" | "queued"
    ) {
        return Err(DbError::Validation(format!(
            "document in status '{status}' cannot be queued for reprocessing"
        )));
    }
    // A reprocess deletes the previous unapproved extraction so retries cannot
    // create duplicate invoices. Approved records are blocked above.
    sqlx::query("DELETE FROM extracted_invoices WHERE organization_id = $1 AND document_id = $2")
        .bind(organization_id)
        .bind(document_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "UPDATE documents SET processing_status = 'queued', last_error_code = NULL, last_error_message = NULL, updated_at = now() WHERE organization_id = $1 AND id = $2",
    )
    .bind(organization_id)
    .bind(document_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE document_processing_jobs SET status = 'cancelled', updated_at = now() WHERE document_id = $1 AND status IN ('queued','retry','running')",
    )
    .bind(document_id)
    .execute(&mut *tx)
    .await?;
    let (job_id,): (Uuid,) = sqlx::query_as(
        "INSERT INTO document_processing_jobs (organization_id, document_id) VALUES ($1,$2) RETURNING id",
    )
    .bind(organization_id)
    .bind(document_id)
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO document_events (organization_id, document_id, stage, status, message) VALUES ($1,$2,'retry','queued','Document queued for reprocessing by a user')",
    )
    .bind(organization_id)
    .bind(document_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(job_id)
}

/// Restart a document whose worker lease was lost or whose previous attempt
/// failed. Approved/exported/rejected documents are never reset. This is an
/// infrastructure-only reset, not a user-facing lifecycle transition, and it
/// records a document event so the restart is auditable.
pub async fn reset_document_for_retry(
    db: &Db,
    organization_id: Uuid,
    document_id: Uuid,
) -> Result<DocumentRow, DbError> {
    let mut tx = db.begin().await?;
    let current: Option<(String,)> = sqlx::query_as(
        "SELECT processing_status FROM documents WHERE organization_id = $1 AND id = $2 AND deleted_at IS NULL FOR UPDATE",
    )
    .bind(organization_id)
    .bind(document_id)
    .fetch_optional(&mut *tx)
    .await?;
    let (current,) = current.ok_or(DbError::NotFound)?;
    if matches!(
        current.as_str(),
        "approved" | "exported" | "rejected" | "cancelled"
    ) {
        return Err(DbError::InvalidTransition(format!(
            "document in status {current} cannot be restarted by a worker"
        )));
    }
    let row = sqlx::query_as::<_, DocumentRow>(
        r#"UPDATE documents
           SET processing_status = 'queued', last_error_code = NULL, last_error_message = NULL,
               updated_at = now()
           WHERE organization_id = $1 AND id = $2
           RETURNING id, organization_id, uploaded_by, source, original_filename, storage_key,
                     content_type, sha256, size_bytes, page_count, document_type, processing_status,
                     processing_attempts, extraction_provider, extraction_model, processing_duration_ms,
                     last_error_code, last_error_message, created_at, updated_at, deleted_at"#,
    )
    .bind(organization_id)
    .bind(document_id)
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO document_events (organization_id, document_id, stage, status, message) VALUES ($1,$2,'retry','queued','Worker restarted processing after an interrupted or failed attempt')",
    )
    .bind(organization_id)
    .bind(document_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(row)
}

pub async fn with_tx<'a>(db: &'a Db) -> Result<Transaction<'a, Postgres>, sqlx::Error> {
    db.begin().await
}
