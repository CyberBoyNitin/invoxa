//! Export worker. Exports only include approved invoices; the query itself is
//! tenant-scoped and filters on `status = 'approved'`. Monetary values are
//! carried as exact decimels through the database, builder, and file.

use chrono::{NaiveDate, Utc};
use invoxa_database::{invoices, operations, Db};
use invoxa_documents::storage::{storage_key_for_export, ObjectStorage};
use invoxa_integrations::export::{generate_export, ExportFormat, ExportRecord};
use serde_json::Value;
use std::sync::Arc;
use tracing::{error, info, warn};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ExportOutcome {
    pub export_id: Uuid,
    pub status: &'static str,
    pub row_count: usize,
    pub message: Option<String>,
}

#[derive(Debug, Default)]
struct ExportFilters {
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
    vendor_id: Option<Uuid>,
    invoice_ids: Vec<Uuid>,
    status: String,
}

pub struct ExportWorker {
    db: Db,
    storage: Arc<dyn ObjectStorage>,
    worker_id: String,
}

impl ExportWorker {
    pub fn new(db: Db, storage: Arc<dyn ObjectStorage>, worker_id: impl Into<String>) -> Self {
        Self {
            db,
            storage,
            worker_id: worker_id.into(),
        }
    }

    pub fn worker_id(&self) -> &str {
        &self.worker_id
    }

    pub async fn claim(
        &self,
        lease_seconds: i32,
    ) -> Result<Option<operations::ClaimedExportRow>, sqlx::Error> {
        operations::claim_export_job(&self.db, &self.worker_id, lease_seconds).await
    }

    pub async fn process_claimed(&self, job: operations::ClaimedExportRow) -> ExportOutcome {
        match self.run(job.clone()).await {
            Ok(outcome) => outcome,
            Err(message) => {
                let (code, friendly) = export_failure(&message);
                if let Err(error) =
                    operations::fail_export_job(&self.db, job.id, code, &friendly).await
                {
                    error!(export_id = %job.id, error = %error, "could not record export failure");
                }
                info!(export_id = %job.id, code, "export failed");
                ExportOutcome {
                    export_id: job.id,
                    status: "failed",
                    row_count: 0,
                    message: Some(friendly),
                }
            }
        }
    }

    async fn run(
        &self,
        job: operations::ClaimedExportRow,
    ) -> Result<ExportOutcome, ExportRunError> {
        let format = ExportFormat::parse(&job.format).map_err(|_| ExportRunError::Format)?;
        let filters = parse_filters(&job.filters).map_err(|_| ExportRunError::Filter)?;
        let rows = invoices::export_rows(
            &self.db,
            job.organization_id,
            filters.from,
            filters.to,
            filters.vendor_id,
            if filters.invoice_ids.is_empty() {
                None
            } else {
                Some(filters.invoice_ids.as_slice())
            },
            &filters.status,
        )
        .await
        .map_err(ExportRunError::Database)?;

        // Defensive belt-and-braces check: the SQL already restricts this to
        // the requested post-approval status. An export must never contain
        // anything unreviewed.
        if rows.iter().any(|row| row.status != filters.status) {
            return Err(ExportRunError::UnsafeRows);
        }

        let records = rows
            .iter()
            .map(|row| ExportRecord {
                invoice_id: row.id,
                invoice_number: row.invoice_number.clone(),
                invoice_date: row.invoice_date,
                due_date: row.due_date,
                vendor_name: row.vendor_name.clone(),
                vendor_tax_id: row.vendor_tax_id.clone(),
                currency: row.currency.clone(),
                subtotal: row.subtotal,
                discount: row.discount,
                shipping: row.shipping,
                tax_total: row.tax_total,
                total: row.total,
                amount_paid: row.amount_paid,
                balance_due: row.balance_due,
                status: row.status.clone(),
                document_type: row.document_type.clone(),
                payment_terms: row.payment_terms.clone(),
                is_credit_note: row.is_credit_note,
                approved_at: row.approved_at,
                labels: row.labels.clone(),
                source_document_id: row.source_document_id,
                source_filename: row.source_filename.clone(),
                purchase_order: row.purchase_order.clone(),
            })
            .collect::<Vec<_>>();

        let artifact =
            generate_export(format, &records, Utc::now()).map_err(|_| ExportRunError::Build)?;
        let temp = tempfile::Builder::new()
            .prefix("invoxa-export-")
            .suffix(&format!(".{}", format.extension()))
            .tempfile()
            .map_err(|error| {
                warn!(error = %error, "could not create export temp file");
                ExportRunError::Storage
            })?;
        tokio::fs::write(temp.path(), &artifact.bytes)
            .await
            .map_err(|error| {
                warn!(error = %error, "could not write export temp file");
                ExportRunError::Storage
            })?;

        let key = storage_key_for_export(job.organization_id, job.id, format.extension());
        self.storage
            .put_file(&key, temp.path())
            .await
            .map_err(|error| {
                warn!(export_id = %job.id, error = %error, "could not store export artifact");
                ExportRunError::Storage
            })?;

        operations::complete_export_job(
            &self.db,
            job.id,
            &key,
            &artifact.filename,
            artifact.row_count as i32,
        )
        .await
        .map_err(ExportRunError::Database)?;

        let invoice_ids = records
            .iter()
            .map(|record| record.invoice_id)
            .collect::<Vec<_>>();
        if !invoice_ids.is_empty() {
            match invoices::mark_exported(&self.db, job.organization_id, &invoice_ids).await {
                Ok(count) => {
                    info!(export_id = %job.id, marked = count, "marked approved invoices as exported");
                }
                Err(error) => {
                    error!(export_id = %job.id, error = %error, "export succeeded but invoice status updates failed");
                }
            }
        }

        // Events are best-effort after the durable artifact exists. They use
        // deterministic IDs, so the delivery table de-duplicates repeats.
        if let Err(error) = crate::events::emit_export_event(
            &self.db,
            job.organization_id,
            crate::events::EVENT_EXPORT_COMPLETED,
            job.id,
            format.as_str(),
            &artifact.filename,
            artifact.row_count,
            &invoice_ids,
        )
        .await
        {
            error!(export_id = %job.id, error = %error, "could not queue export.completed webhook");
        }
        for invoice_id in &invoice_ids {
            if let Err(error) = crate::events::emit_invoice_event(
                &self.db,
                job.organization_id,
                crate::events::EVENT_INVOICE_EXPORTED,
                *invoice_id,
                Some(serde_json::json!({ "export_id": job.id, "export_format": format.as_str() })),
            )
            .await
            {
                error!(invoice_id = %invoice_id, error = %error, "could not queue invoice.exported webhook");
            }
        }

        info!(
            export_id = %job.id,
            rows = artifact.row_count,
            format = format.as_str(),
            "export completed"
        );
        Ok(ExportOutcome {
            export_id: job.id,
            status: "succeeded",
            row_count: artifact.row_count,
            message: None,
        })
    }
}

fn parse_filters(value: &Value) -> Result<ExportFilters, ()> {
    let object = match value {
        Value::Null => {
            return Ok(ExportFilters {
                status: "approved".into(),
                ..ExportFilters::default()
            })
        }
        Value::Object(object) => object,
        _ => return Err(()),
    };
    let date = |key: &str| -> Result<Option<NaiveDate>, ()> {
        match object.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(raw)) => NaiveDate::parse_from_str(raw, "%Y-%m-%d")
                .map(Some)
                .map_err(|_| ()),
            _ => Err(()),
        }
    };
    let from = date("from")?;
    let to = date("to")?;
    if let (Some(from), Some(to)) = (from, to) {
        if from > to {
            return Err(());
        }
    }
    let vendor_id = match object.get("vendor_id") {
        None | Some(Value::Null) => None,
        Some(Value::String(raw)) => Some(Uuid::parse_str(raw).map_err(|_| ())?),
        _ => return Err(()),
    };
    let invoice_ids = match object.get("invoice_ids") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| match item {
                Value::String(raw) => Uuid::parse_str(raw).map_err(|_| ()),
                _ => Err(()),
            })
            .collect::<Result<Vec<_>, _>>()?,
        _ => return Err(()),
    };
    let status = match object.get("status") {
        None | Some(Value::Null) => "approved".to_string(),
        Some(Value::String(raw)) if matches!(raw.as_str(), "approved" | "exported") => raw.clone(),
        _ => return Err(()),
    };
    Ok(ExportFilters {
        from,
        to,
        vendor_id,
        invoice_ids,
        status,
    })
}

#[derive(Debug)]
enum ExportRunError {
    Format,
    Filter,
    Build,
    Storage,
    UnsafeRows,
    Database(sqlx::Error),
}

fn export_failure(error: &ExportRunError) -> (&'static str, String) {
    match error {
        ExportRunError::Format => (
            "EXPORT_FORMAT_INVALID",
            "The export format is not supported.".into(),
        ),
        ExportRunError::Filter => (
            "EXPORT_FILTER_INVALID",
            "The export filters are invalid.".into(),
        ),
        ExportRunError::Build => (
            "EXPORT_BUILD_FAILED",
            "The export file could not be generated.".into(),
        ),
        ExportRunError::Storage => (
            "EXPORT_STORAGE_FAILED",
            "The export artifact could not be stored. Request the export again.".into(),
        ),
        ExportRunError::UnsafeRows => (
            "EXPORT_UNSAFE_ROWS",
            "The export was aborted because a non-approved invoice was selected.".into(),
        ),
        ExportRunError::Database(error) => {
            error!(error = %error, "export database operation failed");
            (
                "EXPORT_DATABASE_ERROR",
                "A database error interrupted the export. Request it again.".into(),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_empty_and_full_filters() {
        assert!(parse_filters(&json!({})).is_ok());
        let filters = parse_filters(&json!({
            "from": "2026-01-01",
            "to": "2026-12-31",
            "vendor_id": "11111111-1111-4111-8111-111111111111",
            "invoice_ids": [
                "22222222-2222-4222-8222-222222222222",
                "33333333-3333-4333-8333-333333333333"
            ],
            "status": "exported"
        }))
        .expect("valid filters");
        assert_eq!(filters.from.unwrap().to_string(), "2026-01-01");
        assert!(filters.vendor_id.is_some());
        assert_eq!(filters.invoice_ids.len(), 2);
        assert_eq!(filters.status, "exported");
    }

    #[test]
    fn defaults_to_approved_and_rejects_unsafe_filters() {
        let filters = parse_filters(&json!({})).expect("default filters");
        assert_eq!(filters.status, "approved");
        assert!(parse_filters(&json!({"from": "2026-05-01", "to": "2026-04-01"})).is_err());
        assert!(parse_filters(&json!({"from": "01/05/2026"})).is_err());
        assert!(parse_filters(&json!({"vendor_id": "not-a-uuid"})).is_err());
        assert!(parse_filters(&json!({"invoice_ids": ["not-a-uuid"]})).is_err());
        assert!(parse_filters(&json!({"invoice_ids": "not-an-array"})).is_err());
        assert!(parse_filters(&json!({"status": "review_required"})).is_err());
    }
}
