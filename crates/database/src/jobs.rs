use crate::{Db, DbError};
use chrono::{DateTime, Utc};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, FromRow)]
pub struct JobRow {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub document_id: Uuid,
    pub status: String,
    pub attempts: i32,
    pub max_attempts: i32,
    pub run_at: DateTime<Utc>,
    pub stage: Option<String>,
    pub last_error_code: Option<String>,
    pub last_error_message: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
pub struct ClaimedJobRow {
    pub job_id: Uuid,
    pub organization_id: Uuid,
    pub document_id: Uuid,
    pub attempts: i32,
    pub max_attempts: i32,
    pub storage_key: String,
    pub original_filename: String,
    pub content_type: String,
    pub sha256: String,
    pub page_count: Option<i32>,
    pub document_type: String,
    pub processing_status: String,
}

pub async fn claim_job(
    db: &Db,
    worker_id: &str,
    lease_seconds: i32,
) -> Result<Option<ClaimedJobRow>, sqlx::Error> {
    let job = sqlx::query_as::<_, JobRow>(
        r#"WITH candidate AS (
               SELECT id FROM document_processing_jobs
               WHERE status IN ('queued','retry') AND run_at <= now()
               ORDER BY run_at ASC
               LIMIT 1
               FOR UPDATE SKIP LOCKED
           )
           UPDATE document_processing_jobs j
           SET status = 'running',
               attempts = j.attempts + 1,
               locked_at = now(),
               lease_expires_at = now() + ($2::int * interval '1 second'),
               locked_by = $1,
               started_at = COALESCE(j.started_at, now()),
               updated_at = now()
           FROM candidate c
           WHERE j.id = c.id
           RETURNING j.id, j.organization_id, j.document_id, j.status, j.attempts, j.max_attempts,
                     j.run_at, j.stage, j.last_error_code, j.last_error_message, j.started_at,
                     j.completed_at, j.created_at, j.updated_at"#,
    )
    .bind(worker_id)
    .bind(lease_seconds)
    .fetch_optional(db)
    .await?;
    let Some(job) = job else {
        return Ok(None);
    };
    let doc: Option<ClaimedJobRow> = sqlx::query_as::<_, ClaimedJobRow>(
        r#"SELECT $1::uuid AS job_id, d.organization_id, d.id AS document_id, $2::int AS attempts,
                  $3::int AS max_attempts, d.storage_key, d.original_filename, d.content_type,
                  d.sha256, d.page_count, d.document_type, d.processing_status
           FROM documents d
           WHERE d.id = $4 AND d.organization_id = $5 AND d.deleted_at IS NULL"#,
    )
    .bind(job.id)
    .bind(job.attempts)
    .bind(job.max_attempts)
    .bind(job.document_id)
    .bind(job.organization_id)
    .fetch_optional(db)
    .await?;
    if doc.is_none() {
        let _ = fail_job(
            db,
            job.id,
            false,
            "DOCUMENT_MISSING",
            "Document was deleted before processing",
        )
        .await;
    }
    Ok(doc)
}

pub async fn renew_lease(db: &Db, job_id: Uuid, lease_seconds: i32) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE document_processing_jobs SET lease_expires_at = now() + ($2::int * interval '1 second'), updated_at = now() WHERE id = $1 AND status = 'running'",
    )
    .bind(job_id)
    .bind(lease_seconds)
    .execute(db)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn complete_job(db: &Db, job_id: Uuid, stage: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE document_processing_jobs SET status = 'succeeded', stage = $2, completed_at = now(), locked_at = NULL, lease_expires_at = NULL, locked_by = NULL, updated_at = now() WHERE id = $1",
    )
    .bind(job_id)
    .bind(stage)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn fail_job(
    db: &Db,
    job_id: Uuid,
    retry: bool,
    code: &str,
    message: &str,
) -> Result<(), sqlx::Error> {
    let delay_seconds: i64 = if retry {
        // Exponential backoff with jitter is applied by the caller via
        // retry_at; this fallback is used for infrastructure failures.
        30
    } else {
        0
    };
    sqlx::query(
        r#"UPDATE document_processing_jobs
           SET status = CASE WHEN $2 THEN 'retry' ELSE 'dead' END,
               stage = stage,
               run_at = now() + ($4::bigint * interval '1 second'),
               last_error_code = $3,
               last_error_message = $5,
               locked_at = NULL, lease_expires_at = NULL, locked_by = NULL,
               completed_at = CASE WHEN $2 THEN NULL ELSE now() END,
               updated_at = now()
           WHERE id = $1"#,
    )
    .bind(job_id)
    .bind(retry)
    .bind(code)
    .bind(delay_seconds)
    .bind(message.chars().take(1200).collect::<String>())
    .execute(db)
    .await?;
    Ok(())
}

pub async fn reschedule_job(
    db: &Db,
    job_id: Uuid,
    run_at: DateTime<Utc>,
    code: &str,
    message: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"UPDATE document_processing_jobs
           SET status = 'retry', run_at = $2, last_error_code = $3, last_error_message = $4,
               locked_at = NULL, lease_expires_at = NULL, locked_by = NULL,
               completed_at = NULL, updated_at = now()
           WHERE id = $1"#,
    )
    .bind(job_id)
    .bind(run_at)
    .bind(code)
    .bind(message.chars().take(1200).collect::<String>())
    .execute(db)
    .await?;
    Ok(())
}

pub async fn recover_expired_jobs(db: &Db) -> Result<(u64, u64), sqlx::Error> {
    let retry = sqlx::query(
        r#"UPDATE document_processing_jobs
           SET status = 'retry', run_at = now(), locked_at = NULL, lease_expires_at = NULL, locked_by = NULL,
               last_error_code = 'WORKER_LEASE_EXPIRED',
               last_error_message = 'Worker lease expired; job was recovered and requeued',
               updated_at = now()
           WHERE status = 'running' AND lease_expires_at < now() AND attempts < max_attempts"#,
    )
    .execute(db)
    .await?;
    let dead = sqlx::query(
        r#"UPDATE document_processing_jobs
           SET status = 'dead', locked_at = NULL, lease_expires_at = NULL, locked_by = NULL,
               last_error_code = 'WORKER_LEASE_EXPIRED',
               last_error_message = 'Worker lease expired after maximum attempts',
               completed_at = now(), updated_at = now()
           WHERE status = 'running' AND lease_expires_at < now() AND attempts >= max_attempts"#,
    )
    .execute(db)
    .await?;
    Ok((retry.rows_affected(), dead.rows_affected()))
}

pub async fn get_job(
    db: &Db,
    organization_id: Uuid,
    job_id: Uuid,
) -> Result<Option<JobRow>, sqlx::Error> {
    sqlx::query_as::<_, JobRow>(
        r#"SELECT id, organization_id, document_id, status, attempts, max_attempts, run_at, stage,
                  last_error_code, last_error_message, started_at, completed_at, created_at, updated_at
           FROM document_processing_jobs WHERE organization_id = $1 AND id = $2"#,
    )
    .bind(organization_id)
    .bind(job_id)
    .fetch_optional(db)
    .await
}

pub async fn jobs_for_document(
    db: &Db,
    organization_id: Uuid,
    document_id: Uuid,
) -> Result<Vec<JobRow>, sqlx::Error> {
    sqlx::query_as::<_, JobRow>(
        r#"SELECT id, organization_id, document_id, status, attempts, max_attempts, run_at, stage,
                  last_error_code, last_error_message, started_at, completed_at, created_at, updated_at
           FROM document_processing_jobs
           WHERE organization_id = $1 AND document_id = $2
           ORDER BY created_at DESC"#,
    )
    .bind(organization_id)
    .bind(document_id)
    .fetch_all(db)
    .await
}

pub async fn queue_depth(db: &Db) -> Result<i64, sqlx::Error> {
    let (count,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM document_processing_jobs WHERE status IN ('queued','retry','running')",
    )
    .fetch_one(db)
    .await?;
    Ok(count)
}

pub async fn cancel_for_document(
    db: &Db,
    organization_id: Uuid,
    document_id: Uuid,
) -> Result<u64, DbError> {
    let result = sqlx::query(
        "UPDATE document_processing_jobs SET status = 'cancelled', locked_at = NULL, lease_expires_at = NULL, updated_at = now() WHERE organization_id = $1 AND document_id = $2 AND status IN ('queued','retry','running')",
    )
    .bind(organization_id)
    .bind(document_id)
    .execute(db)
    .await?;
    Ok(result.rows_affected())
}
