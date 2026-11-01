//! Durable webhook event construction. Events are stored in the outbox table
//! (`webhook_deliveries`) in the same database as the state transition, so a
//! crash after the transition does not silently lose the event.

use invoxa_database::{auth, documents, invoices, operations, Db};
use serde_json::{json, Value};
use uuid::Uuid;

pub const EVENT_INVOICE_PROCESSED: &str = "invoice.processed";
pub const EVENT_INVOICE_UPDATED: &str = "invoice.updated";
pub const EVENT_INVOICE_APPROVED: &str = "invoice.approved";
pub const EVENT_INVOICE_REJECTED: &str = "invoice.rejected";
pub const EVENT_INVOICE_EXPORTED: &str = "invoice.exported";
pub const EVENT_EXPORT_COMPLETED: &str = "export.completed";
pub const EVENT_EXPORT_FAILED: &str = "invoice.export_failed";
pub const EVENT_AUTOMATION_TRIGGERED: &str = "automation.triggered";
pub const EVENT_DOCUMENT_UPLOADED: &str = "document.uploaded";
pub const EVENT_DOCUMENT_PROCESSING_COMPLETED: &str = "document.processing_completed";
pub const EVENT_DOCUMENT_PROCESSING_FAILED: &str = "document.processing_failed";

/// Events an organization can subscribe to. Kept here so the API and worker
/// agree on the exact strings accepted by `webhook_endpoints.events`.
pub const ALL_EVENTS: &[&str] = &[
    EVENT_DOCUMENT_UPLOADED,
    EVENT_DOCUMENT_PROCESSING_COMPLETED,
    EVENT_DOCUMENT_PROCESSING_FAILED,
    EVENT_INVOICE_PROCESSED,
    EVENT_INVOICE_UPDATED,
    EVENT_INVOICE_APPROVED,
    EVENT_INVOICE_REJECTED,
    EVENT_INVOICE_EXPORTED,
    EVENT_EXPORT_COMPLETED,
    EVENT_EXPORT_FAILED,
    EVENT_AUTOMATION_TRIGGERED,
];

pub fn is_mock_provider(provider: Option<&str>) -> bool {
    matches!(provider, Some(invoxa_extraction::mock::MOCK_PROVIDER_NAME))
}

/// Build a stable, exact-decimal event snapshot for one invoice. Money is
/// serialized as a string so JSON consumers cannot accidentally round it
/// through a binary float.
pub async fn invoice_snapshot(
    db: &Db,
    organization_id: Uuid,
    invoice_id: Uuid,
) -> Result<Option<Value>, sqlx::Error> {
    let Some(invoice) = invoices::get_invoice(db, organization_id, invoice_id).await? else {
        return Ok(None);
    };
    let document = documents::get_document(db, organization_id, invoice.document_id).await?;
    let org = auth::organization_by_id(db, organization_id).await?;
    let issues = invoices::validation_issues(db, organization_id, invoice_id).await?;
    let duplicates = invoices::duplicates(db, organization_id, invoice_id).await?;
    let errors = issues
        .iter()
        .filter(|i| i.resolved_at.is_none() && i.severity == "error")
        .count();
    let warnings = issues
        .iter()
        .filter(|i| i.resolved_at.is_none() && i.severity == "warning")
        .count();
    let open_duplicates = duplicates.iter().filter(|d| d.status == "open").count();
    let currency = invoice
        .currency
        .clone()
        .or_else(|| org.as_ref().map(|o| o.base_currency.clone()));

    Ok(Some(json!({
        "invoice_id": invoice.id,
        "document_id": invoice.document_id,
        "organization_id": organization_id,
        "vendor_id": invoice.vendor_id,
        "vendor_name": invoice.vendor_name,
        "invoice_number": invoice.invoice_number,
        "invoice_date": invoice.invoice_date.map(|d| d.to_string()),
        "due_date": invoice.due_date.map(|d| d.to_string()),
        "currency": currency,
        "subtotal": invoice.subtotal.map(|v| v.to_string()),
        "discount": invoice.discount.map(|v| v.to_string()),
        "shipping": invoice.shipping.map(|v| v.to_string()),
        "tax_total": invoice.tax_total.map(|v| v.to_string()),
        "total": invoice.total.map(|v| v.to_string()),
        "amount_paid": invoice.amount_paid.map(|v| v.to_string()),
        "balance_due": invoice.balance_due.map(|v| v.to_string()),
        "status": invoice.status,
        "document_type": invoice.document_type,
        "version": invoice.version,
        "labels": invoice.labels,
        "extraction": {
            "provider": invoice.extraction_provider,
            "model": invoice.extraction_model,
            "method": invoice.extraction_method,
            "quality": invoice.extraction_quality.map(|v| v.to_string()),
            "is_mock": is_mock_provider(invoice.extraction_provider.as_deref()),
        },
        "validation": {
            "unresolved_errors": errors,
            "unresolved_warnings": warnings,
        },
        "open_duplicates": open_duplicates,
        "source": document.map(|d| json!({
            "filename": d.original_filename,
            "sha256": d.sha256,
            "content_type": d.content_type,
            "page_count": d.page_count,
        })),
    })))
}

pub async fn emit_invoice_event(
    db: &Db,
    organization_id: Uuid,
    event_type: &str,
    invoice_id: Uuid,
    extra: Option<Value>,
) -> Result<u64, sqlx::Error> {
    let Some(mut payload) = invoice_snapshot(db, organization_id, invoice_id).await? else {
        return Ok(0);
    };
    let version = payload.get("version").and_then(Value::as_i64).unwrap_or(0);
    if let (Some(Value::Object(extra)), Some(map)) = (extra, payload.as_object_mut()) {
        for (key, value) in extra {
            map.insert(key, value);
        }
    }
    let event_id = stable_invoice_event_id(invoice_id, version, event_type);
    operations::enqueue_webhook_event(db, organization_id, event_type, event_id, &payload).await
}

/// Deterministic event id makes event emission idempotent across worker
/// retries. The invoice version is part of the key so a reopened and
/// re-approved invoice emits a fresh webhook.
pub fn stable_invoice_event_id(invoice_id: Uuid, version: i64, event_type: &str) -> Uuid {
    Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("invoxa:invoice:{invoice_id}:{version}:{event_type}").as_bytes(),
    )
}

pub fn stable_export_event_id(export_id: Uuid, event_type: &str) -> Uuid {
    Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("invoxa:export:{export_id}:{event_type}").as_bytes(),
    )
}

pub fn stable_document_event_id(document_id: Uuid, attempts: i32, event_type: &str) -> Uuid {
    Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("invoxa:document:{document_id}:{attempts}:{event_type}").as_bytes(),
    )
}

/// Emit a durable document lifecycle webhook. Document state is snapshotted at
/// emission time and the event id is deterministic for (document, attempt,
/// event type), so worker retries cannot produce duplicate deliveries.
pub async fn emit_document_event(
    db: &Db,
    organization_id: Uuid,
    event_type: &str,
    document_id: Uuid,
    attempts: i32,
    extra: Option<Value>,
) -> Result<u64, sqlx::Error> {
    let Some(document) = documents::get_document(db, organization_id, document_id).await? else {
        return Ok(0);
    };
    let invoice = invoices::find_invoice_by_document(db, organization_id, document_id).await?;
    let mut payload = json!({
        "document_id": document.id,
        "organization_id": document.organization_id,
        "filename": document.original_filename,
        "content_type": document.content_type,
        "size_bytes": document.size_bytes,
        "sha256": document.sha256,
        "page_count": document.page_count,
        "document_type": document.document_type,
        "status": document.processing_status,
        "attempts": document.processing_attempts,
        "extraction_provider": document.extraction_provider,
        "error_code": document.last_error_code,
        "error_message": document.last_error_message,
        "invoice_id": invoice.as_ref().map(|row| row.id),
        "invoice_status": invoice.as_ref().map(|row| row.status.clone()),
        "created_at": document.created_at,
        "updated_at": document.updated_at,
    });
    if let (Some(Value::Object(extra)), Some(map)) = (extra, payload.as_object_mut()) {
        for (key, value) in extra {
            map.insert(key, value);
        }
    }
    let event_id = stable_document_event_id(document_id, attempts, event_type);
    operations::enqueue_webhook_event(db, organization_id, event_type, event_id, &payload).await
}

pub async fn emit_export_event(
    db: &Db,
    organization_id: Uuid,
    event_type: &str,
    export_id: Uuid,
    format: &str,
    filename: &str,
    row_count: usize,
    invoice_ids: &[Uuid],
) -> Result<u64, sqlx::Error> {
    let payload = json!({
        "export_id": export_id,
        "organization_id": organization_id,
        "format": format,
        "filename": filename,
        "row_count": row_count,
        "invoice_ids": invoice_ids,
    });
    let event_id = stable_export_event_id(export_id, event_type);
    operations::enqueue_webhook_event(db, organization_id, event_type, event_id, &payload).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invoice_event_ids_are_stable_per_version() {
        let invoice = Uuid::new_v4();
        let first = stable_invoice_event_id(invoice, 1, EVENT_INVOICE_APPROVED);
        let repeat = stable_invoice_event_id(invoice, 1, EVENT_INVOICE_APPROVED);
        let reopened = stable_invoice_event_id(invoice, 2, EVENT_INVOICE_APPROVED);
        assert_eq!(first, repeat);
        assert_ne!(first, reopened);
        assert_ne!(
            first,
            stable_invoice_event_id(invoice, 1, EVENT_INVOICE_REJECTED)
        );
    }

    #[test]
    fn mock_provider_is_identified_from_stored_name() {
        assert!(is_mock_provider(Some(
            invoxa_extraction::mock::MOCK_PROVIDER_NAME
        )));
        assert!(!is_mock_provider(Some("openai-compatible")));
        assert!(!is_mock_provider(None));
    }
}
