//! Persistence layer. All tenant-scoped queries require an organization id;
//! callers must never rely on client-supplied authorization.

pub mod auth;
pub mod documents;
pub mod invoices;
pub mod jobs;
pub mod operations;
pub mod vendors;

use sqlx::postgres::{PgPoolOptions, PgRow};
use sqlx::{PgPool, Row};
use std::time::Duration;
use thiserror::Error;

pub type Db = PgPool;

#[derive(Debug, Error)]
pub enum DbError {
    #[error("database error")]
    Sqlx(#[from] sqlx::Error),
    #[error("not found")]
    NotFound,
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("invalid transition: {0}")]
    InvalidTransition(String),
    #[error("validation: {0}")]
    Validation(String),
}

impl DbError {
    pub fn is_unique_violation(&self) -> bool {
        matches!(self, Self::Sqlx(sqlx::Error::Database(e)) if e.is_unique_violation())
    }
}

pub async fn connect(database_url: &str, max_connections: u32) -> Result<Db, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(max_connections)
        .min_connections(1)
        .acquire_timeout(Duration::from_secs(10))
        .connect(database_url)
        .await
}

pub async fn migrate(db: &Db) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate!("../../migrations").run(db).await
}

/// Lightweight readiness probe used by /health/ready.
pub async fn ping(db: &Db) -> bool {
    sqlx::query("SELECT 1").execute(db).await.is_ok()
}

pub fn slugify(name: &str) -> String {
    let mut slug = String::new();
    let mut last_dash = false;
    for ch in name.chars() {
        let c = ch.to_ascii_lowercase();
        if c.is_ascii_alphanumeric() {
            slug.push(c);
            last_dash = false;
        } else if !last_dash {
            slug.push('-');
            last_dash = true;
        }
    }
    let slug = slug.trim_matches('-').to_string();
    let mut out: String = slug.chars().take(50).collect();
    if out.len() < 3 {
        out = format!("org-{out}");
    }
    out
}

pub async fn unique_org_slug(db: &Db, base: &str) -> Result<String, sqlx::Error> {
    let base = slugify(base);
    for attempt in 0..50 {
        let candidate = if attempt == 0 {
            base.clone()
        } else {
            format!("{base}-{attempt}")
        };
        let exists: Option<(i32,)> =
            sqlx::query_as("SELECT 1 FROM organizations WHERE slug = $1 AND deleted_at IS NULL")
                .bind(&candidate)
                .fetch_optional(db)
                .await?;
        if exists.is_none() {
            return Ok(candidate);
        }
    }
    Ok(format!("{base}-{}", uuid::Uuid::new_v4().simple()))
}

/// Run a query that must touch exactly one row, mapping zero rows to NotFound.
pub async fn one_row(
    result: Result<PgRow, sqlx::Error>,
    not_found: &str,
) -> Result<PgRow, DbError> {
    match result {
        Ok(row) => Ok(row),
        Err(sqlx::Error::RowNotFound) => Err(DbError::Validation(not_found.to_string())),
        Err(e) => Err(DbError::Sqlx(e)),
    }
}

pub fn json_len(row: &PgRow, column: &str) -> i64 {
    row.try_get::<Option<i64>, _>(column)
        .ok()
        .flatten()
        .unwrap_or(0)
}
