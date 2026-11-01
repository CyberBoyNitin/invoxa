//! Explicit API response mapping. Internal storage keys, hashes and encrypted
//! secrets are never serialized. Monetary values are always strings.

use invoxa_database::{
    auth::{InvitationRow, MemberRow},
    documents::{DocumentEventRow, DocumentListRow, DocumentRow},
    invoices::{
        ApprovalEventRow, DuplicateRow, InvoiceListRow, InvoiceRow, LineItemRow, ProvenanceRow,
        ReviewRow, TaxLineRow, ValidationIssueRow,
    },
    operations::{
        ApiKeyRow, AuditEventRow, AutomationRuleRow, ExportJobRow, IntegrationRow,
        WebhookDeliveryRow, WebhookEndpointRow,
    },
    vendors::{VendorInvoiceRow, VendorListRow, VendorRow},
};
use invoxa_domain::invoice::DocumentKind;
use rust_decimal::Decimal;
use serde_json::{json, Value};

pub fn dec(value: Option<Decimal>) -> Value {
    match value {
        Some(value) => Value::String(value.to_string()),
        None => Value::Null,
    }
}

pub fn dec_required(value: Decimal) -> Value {
    Value::String(value.to_string())
}

pub fn is_mock_provider(provider: Option<&str>) -> bool {
    provider == Some(invoxa_extraction::mock::MOCK_PROVIDER_NAME)
}

pub fn document_kind_label(value: &str) -> &'static str {
    match value {
        "supplier_invoice" => "Supplier invoice",
        "credit_note" => "Credit note",
        "receipt" => "Receipt",
        "purchase_order" => "Purchase order",
        "statement" => "Statement",
        "other_financial" => "Other financial document",
        _ => "Unknown document",
    }
}

pub fn document_kind_json(value: &str) -> Value {
    let kind = match value {
        "supplier_invoice" => DocumentKind::SupplierInvoice,
        "credit_note" => DocumentKind::CreditNote,
        "receipt" => DocumentKind::Receipt,
        "purchase_order" => DocumentKind::PurchaseOrder,
        "statement" => DocumentKind::Statement,
        "other_financial" => DocumentKind::OtherFinancial,
        _ => DocumentKind::Unknown,
    };
    json!({
        "value": value,
        "label": document_kind_label(value),
        "is_invoice_like": kind.is_invoice_like(),
    })
}

pub fn document_json(row: &DocumentRow) -> Value {
    json!({
        "id": row.id,
        "source": row.source,
        "filename": row.original_filename,
        "content_type": row.content_type,
        "size_bytes": row.size_bytes,
        "sha256": row.sha256,
        "page_count": row.page_count,
        "document_type": document_kind_json(&row.document_type),
        "status": row.processing_status,
        "processing_attempts": row.processing_attempts,
        "extraction_provider": row.extraction_provider,
        "extraction_model": row.extraction_model,
        "processing_duration_ms": row.processing_duration_ms,
        "error_code": row.last_error_code,
        "error_message": row.last_error_message,
        "created_at": row.created_at,
        "updated_at": row.updated_at,
        "content_url": format!("/api/v1/documents/{}/content", row.id),
        "download_url_endpoint": format!("/api/v1/documents/{}/download-url", row.id),
    })
}

pub fn document_list_json(row: &DocumentListRow) -> Value {
    json!({
        "id": row.id,
        "filename": row.original_filename,
        "content_type": row.content_type,
        "size_bytes": row.size_bytes,
        "page_count": row.page_count,
        "document_type": document_kind_json(&row.document_type),
        "status": row.processing_status,
        "error_code": row.last_error_code,
        "created_at": row.created_at,
        "invoice": row.invoice_id.map(|id| json!({
            "id": id,
            "vendor_name": row.vendor_name,
            "invoice_number": row.invoice_number,
            "currency": row.currency,
            "total": dec(row.total),
            "status": row.invoice_status,
            "issue_count": row.issue_count,
            "duplicate_count": row.duplicate_count,
        })),
    })
}

pub fn document_event_json(row: &DocumentEventRow) -> Value {
    json!({
        "id": row.id,
        "stage": row.stage,
        "status": row.status,
        "message": row.message,
        "duration_ms": row.duration_ms,
        "created_at": row.created_at,
    })
}

pub fn invoice_json(row: &InvoiceRow) -> Value {
    json!({
        "id": row.id,
        "document_id": row.document_id,
        "vendor_id": row.vendor_id,
        "vendor_name": row.vendor_name,
        "document_type": document_kind_json(&row.document_type),
        "invoice_number": row.invoice_number,
        "invoice_date": row.invoice_date,
        "due_date": row.due_date,
        "invoice_date_text": row.invoice_date_text,
        "due_date_text": row.due_date_text,
        "purchase_order": row.purchase_order,
        "currency": row.currency,
        "payment_terms": row.payment_terms,
        "subtotal": dec(row.subtotal),
        "discount": dec(row.discount),
        "shipping": dec(row.shipping),
        "tax_total": dec(row.tax_total),
        "total": dec(row.total),
        "amount_paid": dec(row.amount_paid),
        "balance_due": dec(row.balance_due),
        "is_credit_note": row.is_credit_note,
        "status": row.status,
        "version": row.version,
        "extraction": {
            "provider": row.extraction_provider,
            "model": row.extraction_model,
            "method": row.extraction_method,
            "quality": dec(row.extraction_quality),
            "is_mock": is_mock_provider(row.extraction_provider.as_deref()),
        },
        "labels": row.labels,
        "assigned_reviewer_id": row.assigned_reviewer_id,
        "assigned_reviewer_name": row.assigned_reviewer_name,
        "approved_at": row.approved_at,
        "approved_by": row.approved_by,
        "rejected_at": row.rejected_at,
        "rejected_by": row.rejected_by,
        "rejection_reason": row.rejection_reason,
        "exported_at": row.exported_at,
        "created_at": row.created_at,
        "updated_at": row.updated_at,
    })
}

pub fn invoice_list_json(row: &InvoiceListRow) -> Value {
    json!({
        "id": row.id,
        "document_id": row.document_id,
        "vendor_name": row.vendor_name,
        "invoice_number": row.invoice_number,
        "invoice_date": row.invoice_date,
        "due_date": row.due_date,
        "currency": row.currency,
        "total": dec(row.total),
        "status": row.status,
        "document_type": document_kind_json(&row.document_type),
        "issue_count": row.issue_count,
        "blocking_issue_count": row.blocking_issue_count,
        "duplicate_count": row.duplicate_count,
        "open_duplicate_count": row.open_duplicate_count,
        "source_filename": row.source_filename,
        "is_mock": is_mock_provider(row.extraction_provider.as_deref()),
        "provider": row.extraction_provider,
        "version": row.version,
        "labels": row.labels,
        "assigned_reviewer_name": row.assigned_reviewer_name,
        "document_status": row.document_status,
        "created_at": row.created_at,
        "updated_at": row.updated_at,
    })
}

pub fn line_item_json(row: &LineItemRow) -> Value {
    json!({
        "id": row.id,
        "position": row.position,
        "description": row.description,
        "sku": row.sku,
        "quantity": dec(row.quantity),
        "unit_of_measure": row.unit_of_measure,
        "unit_price": dec(row.unit_price),
        "discount": dec(row.discount),
        "tax_rate": dec(row.tax_rate),
        "tax_amount": dec(row.tax_amount),
        "line_total": dec(row.line_total),
        "source_page": row.source_page,
        "raw_text": row.raw_text,
    })
}

pub fn tax_line_json(row: &TaxLineRow) -> Value {
    json!({
        "id": row.id,
        "position": row.position,
        "tax_type": row.tax_type,
        "rate": dec(row.rate),
        "taxable_amount": dec(row.taxable_amount),
        "amount": dec(row.amount),
        "source_page": row.source_page,
    })
}

pub fn provenance_json(row: &ProvenanceRow) -> Value {
    json!({
        "id": row.id,
        "field": row.field_path,
        "extracted_value": row.extracted_value,
        "normalized_value": row.normalized_value,
        "confidence": dec(row.confidence),
        "extraction_method": row.extraction_method,
        "source_page": row.source_page,
        "source_text": row.source_text,
        "validation_status": row.validation_status,
        "human_edited": row.human_edited,
        "previous_value": row.previous_value,
        "edited_by": row.edited_by,
        "edited_at": row.edited_at,
        "updated_at": row.updated_at,
    })
}

pub fn issue_json(row: &ValidationIssueRow) -> Value {
    json!({
        "id": row.id,
        "code": row.code,
        "severity": row.severity,
        "field": row.field_path,
        "message": row.message,
        "resolved_at": row.resolved_at,
        "resolution_note": row.resolution_note,
        "created_at": row.created_at,
    })
}

pub fn duplicate_json(row: &DuplicateRow) -> Value {
    json!({
        "id": row.id,
        "kind": row.kind,
        "score": dec(Some(row.score)),
        "status": row.status,
        "details": row.details,
        "resolution_note": row.resolution_note,
        "resolved_at": row.resolved_at,
        "other": {
            "id": row.duplicate_of_invoice_id,
            "vendor_name": row.other_vendor_name,
            "invoice_number": row.other_invoice_number,
            "total": dec(row.other_total),
            "currency": row.other_currency,
            "status": row.other_status,
            "created_at": row.other_created_at,
        },
    })
}

pub fn approval_event_json(row: &ApprovalEventRow) -> Value {
    json!({
        "id": row.id,
        "action": row.action,
        "reason": row.reason,
        "actor_name": row.actor_name,
        "created_at": row.created_at,
    })
}

pub fn review_json(row: &ReviewRow) -> Value {
    json!({
        "id": row.id,
        "action": row.action,
        "notes": row.notes,
        "changed_fields": row.changed_fields,
        "reviewer_name": row.reviewer_name,
        "created_at": row.created_at,
    })
}

pub fn vendor_json(row: &VendorRow) -> Value {
    json!({
        "id": row.id,
        "name": row.name,
        "tax_id": row.tax_id,
        "email": row.email,
        "phone": row.phone,
        "address": row.address,
        "created_at": row.created_at,
        "updated_at": row.updated_at,
    })
}

pub fn vendor_list_json(row: &VendorListRow) -> Value {
    json!({
        "id": row.id,
        "name": row.name,
        "tax_id": row.tax_id,
        "email": row.email,
        "phone": row.phone,
        "address": row.address,
        "invoice_count": row.invoice_count,
        "review_count": row.review_count,
        "last_invoice_at": row.last_invoice_at,
        "created_at": row.created_at,
    })
}

pub fn vendor_invoice_json(row: &VendorInvoiceRow) -> Value {
    json!({
        "id": row.id,
        "document_id": row.document_id,
        "invoice_number": row.invoice_number,
        "invoice_date": row.invoice_date,
        "currency": row.currency,
        "total": dec(row.total),
        "status": row.status,
        "created_at": row.created_at,
    })
}

pub fn api_key_json(row: &ApiKeyRow) -> Value {
    json!({
        "id": row.id,
        "name": row.name,
        "prefix": row.prefix,
        "scopes": row.scopes,
        "created_by_name": row.created_by_name,
        "last_used_at": row.last_used_at,
        "expires_at": row.expires_at,
        "revoked_at": row.revoked_at,
        "created_at": row.created_at,
    })
}

pub fn webhook_json(row: &WebhookEndpointRow) -> Value {
    json!({
        "id": row.id,
        "url": row.url,
        "description": row.description,
        "events": row.events,
        "enabled": row.enabled,
        "last_success_at": row.last_success_at,
        "last_failure_at": row.last_failure_at,
        "delivery_count": row.delivery_count,
        "failed_count": row.failed_count,
        "created_at": row.created_at,
        "updated_at": row.updated_at,
    })
}

pub fn delivery_json(row: &WebhookDeliveryRow) -> Value {
    json!({
        "id": row.id,
        "endpoint_id": row.endpoint_id,
        "endpoint_url": row.endpoint_url,
        "event_id": row.event_id,
        "event_type": row.event_type,
        "status": row.status,
        "attempts": row.attempts,
        "last_status_code": row.last_status_code,
        "last_error": row.last_error,
        "next_attempt_at": row.next_attempt_at,
        "created_at": row.created_at,
        "delivered_at": row.delivered_at,
    })
}

pub fn export_json(row: &ExportJobRow) -> Value {
    json!({
        "id": row.id,
        "format": row.format,
        "status": row.status,
        "filters": row.filters,
        "filename": row.filename,
        "row_count": row.row_count,
        "error_code": row.error_code,
        "error_message": row.error_message,
        "requested_by_name": row.requested_by_name,
        "created_at": row.created_at,
        "started_at": row.started_at,
        "completed_at": row.completed_at,
        "expires_at": row.expires_at,
        "download_url": if row.status == "succeeded" {
            Some(format!("/api/v1/exports/{}/download", row.id))
        } else { None },
    })
}

pub fn automation_json(row: &AutomationRuleRow) -> Value {
    json!({
        "id": row.id,
        "name": row.name,
        "description": row.description,
        "enabled": row.enabled,
        "priority": row.priority,
        "spec": row.spec,
        "created_at": row.created_at,
        "updated_at": row.updated_at,
    })
}

pub fn audit_json(row: &AuditEventRow) -> Value {
    json!({
        "id": row.id,
        "action": row.action,
        "entity_type": row.entity_type,
        "entity_id": row.entity_id,
        "actor_name": row.actor_name,
        "actor_email": row.actor_email,
        "actor_key_name": row.actor_key_name,
        "metadata": row.metadata,
        "ip": row.ip,
        "request_id": row.request_id,
        "created_at": row.created_at,
    })
}

pub fn member_json(row: &MemberRow) -> Value {
    json!({
        "user_id": row.user_id,
        "email": row.email,
        "name": row.name,
        "role": row.role,
        "created_at": row.created_at,
        "last_seen_at": row.last_seen_at,
    })
}

pub fn invitation_json(row: &InvitationRow) -> Value {
    json!({
        "id": row.id,
        "email": row.email,
        "role": row.role,
        "invited_by_name": row.invited_by_name,
        "expires_at": row.expires_at,
        "accepted_at": row.accepted_at,
        "created_at": row.created_at,
    })
}

pub fn integration_json(row: &IntegrationRow) -> Value {
    json!({
        "provider": row.provider,
        "status": row.status,
        "config": row.config,
        "last_verified_at": row.last_verified_at,
        "updated_at": row.updated_at,
    })
}
