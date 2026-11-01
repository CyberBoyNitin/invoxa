use crate::{Db, DbError};
use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde_json::Value;
use sqlx::{FromRow, Postgres, Transaction};
use uuid::Uuid;

#[derive(Debug, Clone, FromRow)]
pub struct InvoiceRow {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub document_id: Uuid,
    pub vendor_id: Option<Uuid>,
    pub vendor_name: Option<String>,
    pub document_type: String,
    pub invoice_number: Option<String>,
    pub invoice_number_normalized: Option<String>,
    pub invoice_date: Option<NaiveDate>,
    pub due_date: Option<NaiveDate>,
    pub invoice_date_text: Option<String>,
    pub due_date_text: Option<String>,
    pub purchase_order: Option<String>,
    pub currency: Option<String>,
    pub payment_terms: Option<String>,
    pub subtotal: Option<Decimal>,
    pub discount: Option<Decimal>,
    pub shipping: Option<Decimal>,
    pub tax_total: Option<Decimal>,
    pub total: Option<Decimal>,
    pub amount_paid: Option<Decimal>,
    pub balance_due: Option<Decimal>,
    pub is_credit_note: bool,
    pub status: String,
    pub version: i32,
    pub extraction_provider: Option<String>,
    pub extraction_model: Option<String>,
    pub extraction_method: Option<String>,
    pub extraction_quality: Option<Decimal>,
    pub raw_extraction: Option<Value>,
    pub labels: Vec<String>,
    pub assigned_reviewer_id: Option<Uuid>,
    pub assigned_reviewer_name: Option<String>,
    pub approved_at: Option<DateTime<Utc>>,
    pub approved_by: Option<Uuid>,
    pub rejected_at: Option<DateTime<Utc>>,
    pub rejected_by: Option<Uuid>,
    pub rejection_reason: Option<String>,
    pub exported_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
pub struct InvoiceListRow {
    pub id: Uuid,
    pub document_id: Uuid,
    pub vendor_name: Option<String>,
    pub invoice_number: Option<String>,
    pub invoice_date: Option<NaiveDate>,
    pub due_date: Option<NaiveDate>,
    pub currency: Option<String>,
    pub total: Option<Decimal>,
    pub status: String,
    pub document_type: String,
    pub issue_count: i64,
    pub blocking_issue_count: i64,
    pub duplicate_count: i64,
    pub open_duplicate_count: i64,
    pub source_filename: String,
    pub extraction_provider: Option<String>,
    pub version: i32,
    pub labels: Vec<String>,
    pub assigned_reviewer_name: Option<String>,
    pub document_status: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
pub struct LineItemRow {
    pub id: Uuid,
    pub position: i32,
    pub description: Option<String>,
    pub sku: Option<String>,
    pub quantity: Option<Decimal>,
    pub unit_of_measure: Option<String>,
    pub unit_price: Option<Decimal>,
    pub discount: Option<Decimal>,
    pub tax_rate: Option<Decimal>,
    pub tax_amount: Option<Decimal>,
    pub line_total: Option<Decimal>,
    pub source_page: Option<i32>,
    pub raw_text: Option<String>,
}

#[derive(Debug, Clone, FromRow)]
pub struct TaxLineRow {
    pub id: Uuid,
    pub position: i32,
    pub tax_type: Option<String>,
    pub rate: Option<Decimal>,
    pub taxable_amount: Option<Decimal>,
    pub amount: Option<Decimal>,
    pub source_page: Option<i32>,
}

#[derive(Debug, Clone, FromRow)]
pub struct ProvenanceRow {
    pub id: Uuid,
    pub field_path: String,
    pub extracted_value: Option<String>,
    pub normalized_value: Option<String>,
    pub confidence: Option<Decimal>,
    pub extraction_method: Option<String>,
    pub source_page: Option<i32>,
    pub source_text: Option<String>,
    pub bbox: Option<Value>,
    pub validation_status: String,
    pub human_edited: bool,
    pub previous_value: Option<String>,
    pub edited_by: Option<Uuid>,
    pub edited_at: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
pub struct ValidationIssueRow {
    pub id: Uuid,
    pub code: String,
    pub severity: String,
    pub field_path: Option<String>,
    pub message: String,
    pub resolved_at: Option<DateTime<Utc>>,
    pub resolution_note: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
pub struct DuplicateRow {
    pub id: Uuid,
    pub duplicate_of_invoice_id: Uuid,
    pub kind: String,
    pub score: Decimal,
    pub status: String,
    pub details: Value,
    pub resolution_note: Option<String>,
    pub resolved_at: Option<DateTime<Utc>>,
    pub other_vendor_name: Option<String>,
    pub other_invoice_number: Option<String>,
    pub other_total: Option<Decimal>,
    pub other_currency: Option<String>,
    pub other_status: String,
    pub other_created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
pub struct ApprovalEventRow {
    pub id: Uuid,
    pub action: String,
    pub reason: Option<String>,
    pub actor_name: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
pub struct ReviewRow {
    pub id: Uuid,
    pub action: String,
    pub notes: Option<String>,
    pub changed_fields: Value,
    pub reviewer_name: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct NewLineItem {
    pub description: Option<String>,
    pub sku: Option<String>,
    pub quantity: Option<Decimal>,
    pub unit_of_measure: Option<String>,
    pub unit_price: Option<Decimal>,
    pub discount: Option<Decimal>,
    pub tax_rate: Option<Decimal>,
    pub tax_amount: Option<Decimal>,
    pub line_total: Option<Decimal>,
    pub source_page: Option<i32>,
    pub raw_text: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NewTaxLine {
    pub tax_type: Option<String>,
    pub rate: Option<Decimal>,
    pub taxable_amount: Option<Decimal>,
    pub amount: Option<Decimal>,
    pub source_page: Option<i32>,
}

#[derive(Debug, Clone)]
pub struct NewProvenance {
    pub field_path: String,
    pub extracted_value: Option<String>,
    pub normalized_value: Option<String>,
    pub confidence: Option<Decimal>,
    pub extraction_method: Option<String>,
    pub source_page: Option<i32>,
    pub source_text: Option<String>,
    pub bbox: Option<Value>,
    pub validation_status: String,
    pub human_edited: bool,
    pub previous_value: Option<String>,
    pub edited_by: Option<Uuid>,
}

#[derive(Debug, Clone)]
pub struct NewInvoice {
    pub organization_id: Uuid,
    pub document_id: Uuid,
    pub vendor_id: Option<Uuid>,
    pub document_type: String,
    pub invoice_number: Option<String>,
    pub invoice_number_normalized: Option<String>,
    pub invoice_date: Option<NaiveDate>,
    pub due_date: Option<NaiveDate>,
    pub invoice_date_text: Option<String>,
    pub due_date_text: Option<String>,
    pub purchase_order: Option<String>,
    pub currency: Option<String>,
    pub payment_terms: Option<String>,
    pub subtotal: Option<Decimal>,
    pub discount: Option<Decimal>,
    pub shipping: Option<Decimal>,
    pub tax_total: Option<Decimal>,
    pub total: Option<Decimal>,
    pub amount_paid: Option<Decimal>,
    pub balance_due: Option<Decimal>,
    pub is_credit_note: bool,
    pub status: String,
    pub extraction_provider: Option<String>,
    pub extraction_model: Option<String>,
    pub extraction_method: Option<String>,
    pub extraction_quality: Option<Decimal>,
    pub raw_extraction: Option<Value>,
    pub labels: Vec<String>,
    pub line_items: Vec<NewLineItem>,
    pub tax_lines: Vec<NewTaxLine>,
    pub provenance: Vec<NewProvenance>,
}

const INVOICE_SELECT: &str = r#"SELECT i.id, i.organization_id, i.document_id, i.vendor_id, v.name AS vendor_name,
       i.document_type, i.invoice_number, i.invoice_number_normalized, i.invoice_date, i.due_date,
       i.invoice_date_text, i.due_date_text, i.purchase_order, i.currency, i.payment_terms,
       i.subtotal, i.discount, i.shipping, i.tax_total, i.total, i.amount_paid, i.balance_due,
       i.is_credit_note, i.status, i.version, i.extraction_provider, i.extraction_model,
       i.extraction_method, i.extraction_quality, i.raw_extraction, i.labels, i.assigned_reviewer_id,
       u.name AS assigned_reviewer_name, i.approved_at, i.approved_by, i.rejected_at, i.rejected_by,
       i.rejection_reason, i.exported_at, i.created_at, i.updated_at
FROM extracted_invoices i
LEFT JOIN vendors v ON v.id = i.vendor_id AND v.organization_id = i.organization_id
LEFT JOIN users u ON u.id = i.assigned_reviewer_id
"#;

async fn insert_children(
    tx: &mut Transaction<'_, Postgres>,
    organization_id: Uuid,
    invoice_id: Uuid,
    line_items: &[NewLineItem],
    tax_lines: &[NewTaxLine],
    provenance: &[NewProvenance],
) -> Result<(), sqlx::Error> {
    for (position, item) in line_items.iter().enumerate() {
        sqlx::query(
            r#"INSERT INTO invoice_line_items
               (organization_id, invoice_id, position, description, sku, quantity, unit_of_measure,
                unit_price, discount, tax_rate, tax_amount, line_total, source_page, raw_text)
               VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)"#,
        )
        .bind(organization_id)
        .bind(invoice_id)
        .bind(position as i32)
        .bind(&item.description)
        .bind(&item.sku)
        .bind(item.quantity)
        .bind(&item.unit_of_measure)
        .bind(item.unit_price)
        .bind(item.discount)
        .bind(item.tax_rate)
        .bind(item.tax_amount)
        .bind(item.line_total)
        .bind(item.source_page)
        .bind(&item.raw_text)
        .execute(&mut **tx)
        .await?;
    }
    for (position, tax) in tax_lines.iter().enumerate() {
        sqlx::query(
            r#"INSERT INTO invoice_tax_lines
               (organization_id, invoice_id, position, tax_type, rate, taxable_amount, amount, source_page)
               VALUES ($1,$2,$3,$4,$5,$6,$7,$8)"#,
        )
        .bind(organization_id)
        .bind(invoice_id)
        .bind(position as i32)
        .bind(&tax.tax_type)
        .bind(tax.rate)
        .bind(tax.taxable_amount)
        .bind(tax.amount)
        .bind(tax.source_page)
        .execute(&mut **tx)
        .await?;
    }
    for prov in provenance {
        sqlx::query(
            r#"INSERT INTO invoice_field_provenance
               (organization_id, invoice_id, field_path, extracted_value, normalized_value, confidence,
                extraction_method, source_page, source_text, bbox, validation_status, human_edited,
                previous_value, edited_by, edited_at)
               VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,
                       CASE WHEN $12 THEN now() ELSE NULL END)"#,
        )
        .bind(organization_id)
        .bind(invoice_id)
        .bind(&prov.field_path)
        .bind(&prov.extracted_value)
        .bind(&prov.normalized_value)
        .bind(prov.confidence)
        .bind(&prov.extraction_method)
        .bind(prov.source_page)
        .bind(&prov.source_text)
        .bind(&prov.bbox)
        .bind(&prov.validation_status)
        .bind(prov.human_edited)
        .bind(&prov.previous_value)
        .bind(prov.edited_by)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn insert_invoice_row(
    tx: &mut Transaction<'_, Postgres>,
    new: &NewInvoice,
) -> Result<Uuid, sqlx::Error> {
    let (id,): (Uuid,) = sqlx::query_as(
        r#"INSERT INTO extracted_invoices
           (organization_id, document_id, vendor_id, document_type, invoice_number,
            invoice_number_normalized, invoice_date, due_date, invoice_date_text, due_date_text,
            purchase_order, currency, payment_terms, subtotal, discount, shipping, tax_total, total,
            amount_paid, balance_due, is_credit_note, status, extraction_provider, extraction_model,
            extraction_method, extraction_quality, raw_extraction, labels)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,
                   $22,$23,$24,$25,$26,$27,$28)
           RETURNING id"#,
    )
    .bind(new.organization_id)
    .bind(new.document_id)
    .bind(new.vendor_id)
    .bind(&new.document_type)
    .bind(&new.invoice_number)
    .bind(&new.invoice_number_normalized)
    .bind(new.invoice_date)
    .bind(new.due_date)
    .bind(&new.invoice_date_text)
    .bind(&new.due_date_text)
    .bind(&new.purchase_order)
    .bind(&new.currency)
    .bind(&new.payment_terms)
    .bind(new.subtotal)
    .bind(new.discount)
    .bind(new.shipping)
    .bind(new.tax_total)
    .bind(new.total)
    .bind(new.amount_paid)
    .bind(new.balance_due)
    .bind(new.is_credit_note)
    .bind(&new.status)
    .bind(&new.extraction_provider)
    .bind(&new.extraction_model)
    .bind(&new.extraction_method)
    .bind(new.extraction_quality)
    .bind(&new.raw_extraction)
    .bind(&new.labels)
    .fetch_one(&mut **tx)
    .await?;
    Ok(id)
}

pub async fn create_invoice(db: &Db, new: &NewInvoice) -> Result<InvoiceRow, DbError> {
    let mut tx = db.begin().await?;
    // A document can have one invoice. Replacing an unapproved extraction
    // prevents retry duplication and is safe because the doc is not approved.
    let existing: Option<(String,)> = sqlx::query_as(
        "SELECT status FROM extracted_invoices WHERE organization_id = $1 AND document_id = $2 FOR UPDATE",
    )
    .bind(new.organization_id)
    .bind(new.document_id)
    .fetch_optional(&mut *tx)
    .await?;
    if let Some((status,)) = &existing {
        if matches!(status.as_str(), "approved" | "exported") {
            return Err(DbError::Conflict(
                "an approved invoice already exists for this document".into(),
            ));
        }
        sqlx::query(
            "DELETE FROM extracted_invoices WHERE organization_id = $1 AND document_id = $2",
        )
        .bind(new.organization_id)
        .bind(new.document_id)
        .execute(&mut *tx)
        .await?;
    }
    let id = insert_invoice_row(&mut tx, new).await?;
    insert_children(
        &mut tx,
        new.organization_id,
        id,
        &new.line_items,
        &new.tax_lines,
        &new.provenance,
    )
    .await?;
    tx.commit().await?;
    get_invoice(db, new.organization_id, id)
        .await?
        .ok_or(DbError::NotFound)
}

pub async fn get_invoice(
    db: &Db,
    organization_id: Uuid,
    invoice_id: Uuid,
) -> Result<Option<InvoiceRow>, sqlx::Error> {
    sqlx::query_as::<_, InvoiceRow>(&format!(
        "{INVOICE_SELECT} WHERE i.organization_id = $1 AND i.id = $2"
    ))
    .bind(organization_id)
    .bind(invoice_id)
    .fetch_optional(db)
    .await
}

pub async fn find_invoice_by_document(
    db: &Db,
    organization_id: Uuid,
    document_id: Uuid,
) -> Result<Option<InvoiceRow>, sqlx::Error> {
    sqlx::query_as::<_, InvoiceRow>(&format!(
        "{INVOICE_SELECT} WHERE i.organization_id = $1 AND i.document_id = $2"
    ))
    .bind(organization_id)
    .bind(document_id)
    .fetch_optional(db)
    .await
}

pub async fn list_invoices(
    db: &Db,
    organization_id: Uuid,
    status: Option<&str>,
    search: Option<&str>,
    vendor_id: Option<Uuid>,
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
    currency: Option<&str>,
    assigned_to_me: Option<Uuid>,
    limit: i64,
    offset: i64,
) -> Result<(Vec<InvoiceListRow>, i64), sqlx::Error> {
    let rows = sqlx::query_as::<_, InvoiceListRow>(
        r#"SELECT i.id, i.document_id, v.name AS vendor_name, i.invoice_number, i.invoice_date,
                  i.due_date, i.currency, i.total, i.status, i.document_type,
                  COALESCE((SELECT count(*) FROM validation_issues vi
                            WHERE vi.invoice_id = i.id AND vi.resolved_at IS NULL
                              AND vi.severity IN ('error','warning')), 0) AS issue_count,
                  COALESCE((SELECT count(*) FROM validation_issues vi
                            WHERE vi.invoice_id = i.id AND vi.resolved_at IS NULL
                              AND vi.severity = 'error'), 0) AS blocking_issue_count,
                  COALESCE((SELECT count(*) FROM duplicate_candidates dc
                            WHERE dc.invoice_id = i.id AND dc.status = 'open'), 0) AS duplicate_count,
                  COALESCE((SELECT count(*) FROM duplicate_candidates dc
                            WHERE dc.invoice_id = i.id AND dc.status = 'open'), 0) AS open_duplicate_count,
                  d.original_filename AS source_filename,
                  i.extraction_provider,
                  i.version, i.labels, u.name AS assigned_reviewer_name,
                  d.processing_status AS document_status, i.created_at, i.updated_at
           FROM extracted_invoices i
           JOIN documents d ON d.id = i.document_id
           LEFT JOIN vendors v ON v.id = i.vendor_id
           LEFT JOIN users u ON u.id = i.assigned_reviewer_id
           WHERE i.organization_id = $1
             AND ($2::text IS NULL OR i.status = $2)
             AND ($3::text IS NULL OR i.invoice_number ILIKE '%' || $3 || '%' OR v.name ILIKE '%' || $3 || '%')
             AND ($4::uuid IS NULL OR i.vendor_id = $4)
             AND ($5::date IS NULL OR i.invoice_date >= $5)
             AND ($6::date IS NULL OR i.invoice_date <= $6)
             AND ($7::text IS NULL OR i.currency = $7)
             AND ($8::uuid IS NULL OR i.assigned_reviewer_id = $8)
           ORDER BY i.created_at DESC
           LIMIT $9 OFFSET $10"#,
    )
    .bind(organization_id)
    .bind(status)
    .bind(search)
    .bind(vendor_id)
    .bind(from)
    .bind(to)
    .bind(currency)
    .bind(assigned_to_me)
    .bind(limit)
    .bind(offset)
    .fetch_all(db)
    .await?;

    let (total,): (i64,) = sqlx::query_as(
        r#"SELECT count(*) FROM extracted_invoices i
           LEFT JOIN vendors v ON v.id = i.vendor_id
           WHERE i.organization_id = $1
             AND ($2::text IS NULL OR i.status = $2)
             AND ($3::text IS NULL OR i.invoice_number ILIKE '%' || $3 || '%' OR v.name ILIKE '%' || $3 || '%')
             AND ($4::uuid IS NULL OR i.vendor_id = $4)
             AND ($5::date IS NULL OR i.invoice_date >= $5)
             AND ($6::date IS NULL OR i.invoice_date <= $6)
             AND ($7::text IS NULL OR i.currency = $7)
             AND ($8::uuid IS NULL OR i.assigned_reviewer_id = $8)"#,
    )
    .bind(organization_id)
    .bind(status)
    .bind(search)
    .bind(vendor_id)
    .bind(from)
    .bind(to)
    .bind(currency)
    .bind(assigned_to_me)
    .fetch_one(db)
    .await?;
    Ok((rows, total))
}

pub async fn line_items(
    db: &Db,
    organization_id: Uuid,
    invoice_id: Uuid,
) -> Result<Vec<LineItemRow>, sqlx::Error> {
    sqlx::query_as::<_, LineItemRow>(
        r#"SELECT id, position, description, sku, quantity, unit_of_measure, unit_price, discount,
                  tax_rate, tax_amount, line_total, source_page, raw_text
           FROM invoice_line_items WHERE organization_id = $1 AND invoice_id = $2 ORDER BY position"#,
    )
    .bind(organization_id)
    .bind(invoice_id)
    .fetch_all(db)
    .await
}

pub async fn tax_lines(
    db: &Db,
    organization_id: Uuid,
    invoice_id: Uuid,
) -> Result<Vec<TaxLineRow>, sqlx::Error> {
    sqlx::query_as::<_, TaxLineRow>(
        r#"SELECT id, position, tax_type, rate, taxable_amount, amount, source_page
           FROM invoice_tax_lines WHERE organization_id = $1 AND invoice_id = $2 ORDER BY position"#,
    )
    .bind(organization_id)
    .bind(invoice_id)
    .fetch_all(db)
    .await
}

pub async fn provenance(
    db: &Db,
    organization_id: Uuid,
    invoice_id: Uuid,
) -> Result<Vec<ProvenanceRow>, sqlx::Error> {
    sqlx::query_as::<_, ProvenanceRow>(
        r#"SELECT id, field_path, extracted_value, normalized_value, confidence, extraction_method,
                  source_page, source_text, bbox, validation_status, human_edited, previous_value,
                  edited_by, edited_at, updated_at
           FROM invoice_field_provenance WHERE organization_id = $1 AND invoice_id = $2
           ORDER BY field_path"#,
    )
    .bind(organization_id)
    .bind(invoice_id)
    .fetch_all(db)
    .await
}

pub async fn validation_issues(
    db: &Db,
    organization_id: Uuid,
    invoice_id: Uuid,
) -> Result<Vec<ValidationIssueRow>, sqlx::Error> {
    sqlx::query_as::<_, ValidationIssueRow>(
        r#"SELECT id, code, severity, field_path, message, resolved_at, resolution_note, created_at
           FROM validation_issues WHERE organization_id = $1 AND invoice_id = $2
           ORDER BY CASE severity WHEN 'error' THEN 0 WHEN 'warning' THEN 1 ELSE 2 END, created_at"#,
    )
    .bind(organization_id)
    .bind(invoice_id)
    .fetch_all(db)
    .await
}

pub async fn replace_validation_issues(
    db: &Db,
    organization_id: Uuid,
    invoice_id: Uuid,
    document_id: Uuid,
    issues: &[(String, String, Option<String>, String)],
) -> Result<(), DbError> {
    let mut tx = db.begin().await?;
    sqlx::query(
        "DELETE FROM validation_issues WHERE organization_id = $1 AND invoice_id = $2 AND resolved_at IS NULL",
    )
    .bind(organization_id)
    .bind(invoice_id)
    .execute(&mut *tx)
    .await?;
    for (code, severity, field, message) in issues {
        sqlx::query(
            "INSERT INTO validation_issues (organization_id, invoice_id, document_id, code, severity, field_path, message) VALUES ($1,$2,$3,$4,$5,$6,$7)",
        )
        .bind(organization_id)
        .bind(invoice_id)
        .bind(document_id)
        .bind(code)
        .bind(severity)
        .bind(field)
        .bind(message)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

pub async fn duplicates(
    db: &Db,
    organization_id: Uuid,
    invoice_id: Uuid,
) -> Result<Vec<DuplicateRow>, sqlx::Error> {
    sqlx::query_as::<_, DuplicateRow>(
        r#"SELECT dc.id, dc.duplicate_of_invoice_id, dc.kind, dc.score, dc.status, dc.details,
                  dc.resolution_note, dc.resolved_at, v.name AS other_vendor_name,
                  oi.invoice_number AS other_invoice_number, oi.total AS other_total,
                  oi.currency AS other_currency, oi.status AS other_status,
                  oi.created_at AS other_created_at
           FROM duplicate_candidates dc
           JOIN extracted_invoices oi ON oi.id = dc.duplicate_of_invoice_id
           LEFT JOIN vendors v ON v.id = oi.vendor_id
           WHERE dc.organization_id = $1 AND dc.invoice_id = $2
           ORDER BY dc.score DESC"#,
    )
    .bind(organization_id)
    .bind(invoice_id)
    .fetch_all(db)
    .await
}

pub async fn approval_events(
    db: &Db,
    organization_id: Uuid,
    invoice_id: Uuid,
) -> Result<Vec<ApprovalEventRow>, sqlx::Error> {
    sqlx::query_as::<_, ApprovalEventRow>(
        r#"SELECT ae.id, ae.action, ae.reason, u.name AS actor_name, ae.created_at
           FROM approval_events ae LEFT JOIN users u ON u.id = ae.actor_id
           WHERE ae.organization_id = $1 AND ae.invoice_id = $2 ORDER BY ae.created_at DESC"#,
    )
    .bind(organization_id)
    .bind(invoice_id)
    .fetch_all(db)
    .await
}

pub async fn reviews(
    db: &Db,
    organization_id: Uuid,
    invoice_id: Uuid,
) -> Result<Vec<ReviewRow>, sqlx::Error> {
    sqlx::query_as::<_, ReviewRow>(
        r#"SELECT r.id, r.action, r.notes, r.changed_fields, u.name AS reviewer_name, r.created_at
           FROM invoice_reviews r LEFT JOIN users u ON u.id = r.reviewer_id
           WHERE r.organization_id = $1 AND r.invoice_id = $2 ORDER BY r.created_at DESC"#,
    )
    .bind(organization_id)
    .bind(invoice_id)
    .fetch_all(db)
    .await
}

pub async fn update_invoice(
    db: &Db,
    organization_id: Uuid,
    invoice_id: Uuid,
    expected_version: i32,
    actor_id: Uuid,
    new: &NewInvoice,
    status: &str,
    issues: &[(String, String, Option<String>, String)],
    changed_fields: &Value,
    reviewer_action: &str,
) -> Result<InvoiceRow, DbError> {
    let mut tx = db.begin().await?;
    let row: Option<(i32, String)> = sqlx::query_as(
        "SELECT version, status FROM extracted_invoices WHERE organization_id = $1 AND id = $2 FOR UPDATE",
    )
    .bind(organization_id)
    .bind(invoice_id)
    .fetch_optional(&mut *tx)
    .await?;
    let (current_version, current_status) = row.ok_or(DbError::NotFound)?;
    if current_version != expected_version {
        return Err(DbError::Conflict(format!(
            "invoice was modified (current version {current_version}); reload before saving"
        )));
    }
    if matches!(current_status.as_str(), "approved" | "exported") {
        return Err(DbError::Conflict(
            "approved or exported invoices cannot be edited; reopen is an owner action".into(),
        ));
    }
    sqlx::query(
        r#"UPDATE extracted_invoices
           SET vendor_id = $3, document_type = $4, invoice_number = $5, invoice_number_normalized = $6,
               invoice_date = $7, due_date = $8, invoice_date_text = $9, due_date_text = $10,
               purchase_order = $11, currency = $12, payment_terms = $13, subtotal = $14,
               discount = $15, shipping = $16, tax_total = $17, total = $18, amount_paid = $19,
               balance_due = $20, is_credit_note = $21, status = $22, labels = $23,
               version = version + 1, updated_at = now()
           WHERE organization_id = $1 AND id = $2"#,
    )
    .bind(organization_id)
    .bind(invoice_id)
    .bind(new.vendor_id)
    .bind(&new.document_type)
    .bind(&new.invoice_number)
    .bind(&new.invoice_number_normalized)
    .bind(new.invoice_date)
    .bind(new.due_date)
    .bind(&new.invoice_date_text)
    .bind(&new.due_date_text)
    .bind(&new.purchase_order)
    .bind(&new.currency)
    .bind(&new.payment_terms)
    .bind(new.subtotal)
    .bind(new.discount)
    .bind(new.shipping)
    .bind(new.tax_total)
    .bind(new.total)
    .bind(new.amount_paid)
    .bind(new.balance_due)
    .bind(new.is_credit_note)
    .bind(status)
    .bind(&new.labels)
    .execute(&mut *tx)
    .await?;

    sqlx::query("DELETE FROM invoice_line_items WHERE organization_id = $1 AND invoice_id = $2")
        .bind(organization_id)
        .bind(invoice_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM invoice_tax_lines WHERE organization_id = $1 AND invoice_id = $2")
        .bind(organization_id)
        .bind(invoice_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM validation_issues WHERE organization_id = $1 AND invoice_id = $2 AND resolved_at IS NULL")
        .bind(organization_id)
        .bind(invoice_id)
        .execute(&mut *tx)
        .await?;

    insert_children(
        &mut tx,
        organization_id,
        invoice_id,
        &new.line_items,
        &new.tax_lines,
        &[],
    )
    .await?;

    for prov in &new.provenance {
        let existing: Option<(Uuid, Option<String>)> = sqlx::query_as(
            "SELECT id, normalized_value FROM invoice_field_provenance WHERE organization_id = $1 AND invoice_id = $2 AND field_path = $3",
        )
        .bind(organization_id)
        .bind(invoice_id)
        .bind(&prov.field_path)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some((id, previous)) = existing {
            let changed = previous.as_deref() != prov.normalized_value.as_deref();
            sqlx::query(
                r#"UPDATE invoice_field_provenance
                   SET normalized_value = $3,
                       human_edited = human_edited OR $4,
                       previous_value = CASE WHEN $4 AND $5 THEN $6 ELSE previous_value END,
                       edited_by = CASE WHEN $4 THEN $7 ELSE edited_by END,
                       edited_at = CASE WHEN $4 THEN now() ELSE edited_at END,
                       validation_status = $8, updated_at = now()
                   WHERE id = $1 AND organization_id = $2"#,
            )
            .bind(id)
            .bind(organization_id)
            .bind(&prov.normalized_value)
            .bind(changed)
            .bind(prov.human_edited)
            .bind(&previous)
            .bind(actor_id)
            .bind(&prov.validation_status)
            .execute(&mut *tx)
            .await?;
        } else {
            let mut p = prov.clone();
            p.human_edited = true;
            insert_children(
                &mut tx,
                organization_id,
                invoice_id,
                &[],
                &[],
                std::slice::from_ref(&p),
            )
            .await?;
        }
    }

    for (code, severity, field, message) in issues {
        sqlx::query(
            "INSERT INTO validation_issues (organization_id, invoice_id, document_id, code, severity, field_path, message) VALUES ($1,$2,$3,$4,$5,$6,$7)",
        )
        .bind(organization_id)
        .bind(invoice_id)
        .bind(new.document_id)
        .bind(code)
        .bind(severity)
        .bind(field)
        .bind(message)
        .execute(&mut *tx)
        .await?;
    }

    sqlx::query(
        "INSERT INTO invoice_reviews (organization_id, invoice_id, reviewer_id, action, changed_fields) VALUES ($1,$2,$3,$4,$5)",
    )
    .bind(organization_id)
    .bind(invoice_id)
    .bind(actor_id)
    .bind(reviewer_action)
    .bind(changed_fields)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    get_invoice(db, organization_id, invoice_id)
        .await?
        .ok_or(DbError::NotFound)
}

pub async fn set_invoice_status(
    db: &Db,
    organization_id: Uuid,
    invoice_id: Uuid,
    expected_version: i32,
    status: &str,
    actor_id: Uuid,
    _action: &str,
    reason: Option<&str>,
) -> Result<InvoiceRow, DbError> {
    let mut tx = db.begin().await?;
    let current: Option<(String, Uuid, i32)> = sqlx::query_as(
        "SELECT status, document_id, version FROM extracted_invoices WHERE organization_id = $1 AND id = $2 FOR UPDATE",
    )
    .bind(organization_id)
    .bind(invoice_id)
    .fetch_optional(&mut *tx)
    .await?;
    let (current_status, document_id, version) = current.ok_or(DbError::NotFound)?;
    if version != expected_version {
        return Err(DbError::Conflict(
            "the invoice was modified by someone else; reload it and try again".into(),
        ));
    }
    let allowed = match status {
        "approved" => matches!(
            current_status.as_str(),
            "review_required" | "ready_for_approval"
        ),
        "rejected" => matches!(
            current_status.as_str(),
            "review_required" | "ready_for_approval"
        ),
        "review_required" => matches!(current_status.as_str(), "ready_for_approval" | "rejected"),
        _ => false,
    };
    if !allowed {
        return Err(DbError::InvalidTransition(format!(
            "invoice cannot move from {current_status} to {status}"
        )));
    }
    sqlx::query(
        r#"UPDATE extracted_invoices SET status = $3,
             approved_at = CASE WHEN $3 = 'approved' THEN now() ELSE approved_at END,
             approved_by = CASE WHEN $3 = 'approved' THEN $4::uuid ELSE approved_by END,
             rejected_at = CASE WHEN $3 = 'rejected' THEN now() ELSE rejected_at END,
             rejected_by = CASE WHEN $3 = 'rejected' THEN $4::uuid ELSE rejected_by END,
             rejection_reason = CASE WHEN $3 = 'rejected' THEN $5 ELSE rejection_reason END,
             version = version + 1, updated_at = now()
           WHERE organization_id = $1 AND id = $2"#,
    )
    .bind(organization_id)
    .bind(invoice_id)
    .bind(status)
    .bind(actor_id)
    .bind(reason)
    .execute(&mut *tx)
    .await?;
    let doc_status = match status {
        "approved" => "approved",
        "rejected" => "rejected",
        _ => "review_required",
    };
    sqlx::query(
        "UPDATE documents SET processing_status = $3, updated_at = now() WHERE organization_id = $1 AND id = $2",
    )
    .bind(organization_id)
    .bind(document_id)
    .bind(doc_status)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO document_events (organization_id, document_id, stage, status, message) VALUES ($1,$2,'review',$3,$4)",
    )
    .bind(organization_id)
    .bind(document_id)
    .bind(doc_status)
    .bind(format!("Invoice {status} by reviewer"))
    .execute(&mut *tx)
    .await?;
    let approval_action = match status {
        "approved" => {
            if reason.is_some() {
                "override_approved"
            } else {
                "approved"
            }
        }
        "rejected" => "rejected",
        _ => "returned",
    };
    sqlx::query(
        "INSERT INTO approval_events (organization_id, invoice_id, actor_id, action, reason) VALUES ($1,$2,$3,$4,$5)",
    )
    .bind(organization_id)
    .bind(invoice_id)
    .bind(actor_id)
    .bind(approval_action)
    .bind(reason)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    get_invoice(db, organization_id, invoice_id)
        .await?
        .ok_or(DbError::NotFound)
}

/// Automated triage is limited to the two review states. It can promote an
/// invoice only when no blocking validation errors or open duplicate
/// candidates exist, so a stale worker can never make an unsafe invoice
/// approvable. Approval itself remains a separate, explicit transition.
pub async fn set_invoice_triage_status(
    db: &Db,
    organization_id: Uuid,
    invoice_id: Uuid,
    status: &str,
) -> Result<InvoiceRow, DbError> {
    if !matches!(status, "review_required" | "ready_for_approval") {
        return Err(DbError::Validation(
            "triage status must be review_required or ready_for_approval".into(),
        ));
    }
    let mut tx = db.begin().await?;
    let current: Option<(String,)> = sqlx::query_as(
        "SELECT status FROM extracted_invoices WHERE organization_id = $1 AND id = $2 FOR UPDATE",
    )
    .bind(organization_id)
    .bind(invoice_id)
    .fetch_optional(&mut *tx)
    .await?;
    let (current,) = current.ok_or(DbError::NotFound)?;
    if !matches!(current.as_str(), "review_required" | "ready_for_approval") {
        return Err(DbError::InvalidTransition(format!(
            "invoice in status {current} cannot be triaged"
        )));
    }
    if status == "ready_for_approval" {
        let (errors,): (i64,) = sqlx::query_as(
            "SELECT count(*) FROM validation_issues WHERE organization_id = $1 AND invoice_id = $2 AND resolved_at IS NULL AND severity = 'error'",
        )
        .bind(organization_id)
        .bind(invoice_id)
        .fetch_one(&mut *tx)
        .await?;
        let (duplicates,): (i64,) = sqlx::query_as(
            "SELECT count(*) FROM duplicate_candidates WHERE organization_id = $1 AND invoice_id = $2 AND status = 'open'",
        )
        .bind(organization_id)
        .bind(invoice_id)
        .fetch_one(&mut *tx)
        .await?;
        if errors > 0 || duplicates > 0 {
            return Err(DbError::Validation(
                "invoice cannot be marked ready for approval while blocking issues or duplicates are open"
                    .into(),
            ));
        }
    }
    sqlx::query(
        "UPDATE extracted_invoices SET status = $3, updated_at = now() WHERE organization_id = $1 AND id = $2",
    )
    .bind(organization_id)
    .bind(invoice_id)
    .bind(status)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    get_invoice(db, organization_id, invoice_id)
        .await?
        .ok_or(DbError::NotFound)
}

/// Apply labels and an optional reviewer chosen by deterministic automation.
/// Approval and export are deliberately not automation actions.
pub async fn apply_invoice_automation(
    db: &Db,
    organization_id: Uuid,
    invoice_id: Uuid,
    labels: &[String],
    reviewer: Option<Uuid>,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        r#"UPDATE extracted_invoices
           SET labels = COALESCE(
                   (SELECT array_agg(DISTINCT label ORDER BY label)
                    FROM unnest(labels || $3::text[]) AS label),
                   '{}'::text[]),
               assigned_reviewer_id = COALESCE($4, assigned_reviewer_id),
               updated_at = now()
           WHERE organization_id = $1 AND id = $2"#,
    )
    .bind(organization_id)
    .bind(invoice_id)
    .bind(labels)
    .bind(reviewer)
    .execute(db)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn resolve_duplicate(
    db: &Db,
    organization_id: Uuid,
    invoice_id: Uuid,
    duplicate_id: Uuid,
    status: &str,
    note: Option<&str>,
    actor_id: Uuid,
) -> Result<(), DbError> {
    if !matches!(status, "confirmed" | "dismissed" | "open") {
        return Err(DbError::Validation("invalid duplicate resolution".into()));
    }
    let mut tx = db.begin().await?;
    let affected = sqlx::query(
        "UPDATE duplicate_candidates SET status = $4, resolved_by = $5, resolved_at = CASE WHEN $4 = 'open' THEN NULL ELSE now() END, resolution_note = $6 WHERE organization_id = $1 AND invoice_id = $2 AND id = $3",
    )
    .bind(organization_id)
    .bind(invoice_id)
    .bind(duplicate_id)
    .bind(status)
    .bind(actor_id)
    .bind(note)
    .execute(&mut *tx)
    .await?;
    if affected.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    sqlx::query(
        "INSERT INTO audit_events (organization_id, actor_user_id, action, entity_type, entity_id, metadata) VALUES ($1,$2,'duplicate.resolved','invoice',$3,$4)",
    )
    .bind(organization_id)
    .bind(actor_id)
    .bind(invoice_id)
    .bind(serde_json::json!({"duplicate_id": duplicate_id, "status": status, "note": note}))
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

pub async fn unresolved_blocking_issues(
    db: &Db,
    organization_id: Uuid,
    invoice_id: Uuid,
) -> Result<Vec<(String, String)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT code, message FROM validation_issues WHERE organization_id = $1 AND invoice_id = $2 AND resolved_at IS NULL AND severity = 'error' ORDER BY created_at",
    )
    .bind(organization_id)
    .bind(invoice_id)
    .fetch_all(db)
    .await
}

pub async fn open_duplicates(
    db: &Db,
    organization_id: Uuid,
    invoice_id: Uuid,
) -> Result<i64, sqlx::Error> {
    let (count,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM duplicate_candidates WHERE organization_id = $1 AND invoice_id = $2 AND status = 'open'",
    )
    .bind(organization_id)
    .bind(invoice_id)
    .fetch_one(db)
    .await?;
    Ok(count)
}

pub async fn upsert_duplicate_candidates(
    db: &Db,
    organization_id: Uuid,
    invoice_id: Uuid,
    candidates: &[(Uuid, String, f64, Value)],
) -> Result<(), DbError> {
    for (other_id, kind, score, details) in candidates {
        let score = Decimal::try_from(*score).unwrap_or(Decimal::ZERO);
        sqlx::query(
            r#"INSERT INTO duplicate_candidates
               (organization_id, invoice_id, duplicate_of_invoice_id, kind, score, details)
               VALUES ($1,$2,$3,$4,$5,$6)
               ON CONFLICT (invoice_id, duplicate_of_invoice_id) DO UPDATE
               SET kind = EXCLUDED.kind, score = EXCLUDED.score, details = EXCLUDED.details"#,
        )
        .bind(organization_id)
        .bind(invoice_id)
        .bind(other_id)
        .bind(kind)
        .bind(score)
        .bind(details)
        .execute(db)
        .await?;
    }
    Ok(())
}

#[derive(Debug, Clone, FromRow)]
pub struct ExistingFingerprint {
    pub id: Uuid,
    pub file_hash: Option<String>,
    pub vendor_name: Option<String>,
    pub invoice_number: Option<String>,
    pub currency: Option<String>,
    pub total: Option<Decimal>,
    pub invoice_date: Option<NaiveDate>,
}

pub async fn fingerprints(
    db: &Db,
    organization_id: Uuid,
    exclude_invoice: Uuid,
) -> Result<Vec<ExistingFingerprint>, sqlx::Error> {
    sqlx::query_as::<_, ExistingFingerprint>(
        r#"SELECT i.id, d.sha256 AS file_hash, v.name AS vendor_name, i.invoice_number,
                  i.currency, i.total, i.invoice_date
           FROM extracted_invoices i
           JOIN documents d ON d.id = i.document_id
           LEFT JOIN vendors v ON v.id = i.vendor_id
           WHERE i.organization_id = $1 AND i.id <> $2 AND d.deleted_at IS NULL
             AND i.status <> 'rejected'"#,
    )
    .bind(organization_id)
    .bind(exclude_invoice)
    .fetch_all(db)
    .await
}

#[derive(Debug, Clone, FromRow)]
pub struct InvoiceExportRow {
    pub id: Uuid,
    pub invoice_number: Option<String>,
    pub invoice_date: Option<NaiveDate>,
    pub due_date: Option<NaiveDate>,
    pub vendor_name: Option<String>,
    pub vendor_tax_id: Option<String>,
    pub currency: Option<String>,
    pub subtotal: Option<Decimal>,
    pub discount: Option<Decimal>,
    pub shipping: Option<Decimal>,
    pub tax_total: Option<Decimal>,
    pub total: Option<Decimal>,
    pub amount_paid: Option<Decimal>,
    pub balance_due: Option<Decimal>,
    pub status: String,
    pub document_type: String,
    pub payment_terms: Option<String>,
    pub purchase_order: Option<String>,
    pub is_credit_note: bool,
    pub approved_at: Option<DateTime<Utc>>,
    pub labels: Vec<String>,
    pub source_document_id: Uuid,
    pub source_filename: String,
}

pub async fn export_rows(
    db: &Db,
    organization_id: Uuid,
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
    vendor_id: Option<Uuid>,
    invoice_ids: Option<&[Uuid]>,
    status: &str,
) -> Result<Vec<InvoiceExportRow>, sqlx::Error> {
    sqlx::query_as::<_, InvoiceExportRow>(
        r#"SELECT i.id, i.invoice_number, i.invoice_date, i.due_date, v.name AS vendor_name,
                  v.tax_id AS vendor_tax_id, i.currency, i.subtotal, i.discount, i.shipping,
                  i.tax_total, i.total, i.amount_paid, i.balance_due, i.status, i.document_type,
                  i.payment_terms, i.purchase_order, i.is_credit_note, i.approved_at, i.labels,
                  d.id AS source_document_id, d.original_filename AS source_filename
           FROM extracted_invoices i
           JOIN documents d ON d.id = i.document_id
           LEFT JOIN vendors v ON v.id = i.vendor_id
           WHERE i.organization_id = $1 AND i.status = $6
             AND ($2::date IS NULL OR i.invoice_date >= $2)
             AND ($3::date IS NULL OR i.invoice_date <= $3)
             AND ($4::uuid IS NULL OR i.vendor_id = $4)
             AND ($5::uuid[] IS NULL OR i.id = ANY($5))
           ORDER BY i.invoice_date NULLS LAST, i.created_at"#,
    )
    .bind(organization_id)
    .bind(from)
    .bind(to)
    .bind(vendor_id)
    .bind(invoice_ids)
    .bind(status)
    .fetch_all(db)
    .await
}

/// Return the organization-scoped status for each requested invoice. This is
/// used before an export is queued so a client cannot silently select an
/// invoice that belongs to another tenant or is not approved.
pub async fn selected_invoice_statuses(
    db: &Db,
    organization_id: Uuid,
    invoice_ids: &[Uuid],
) -> Result<Vec<(Uuid, String)>, sqlx::Error> {
    if invoice_ids.is_empty() {
        return Ok(Vec::new());
    }
    sqlx::query_as(
        "SELECT id, status FROM extracted_invoices WHERE organization_id = $1 AND id = ANY($2)",
    )
    .bind(organization_id)
    .bind(invoice_ids)
    .fetch_all(db)
    .await
}

pub async fn mark_exported(
    db: &Db,
    organization_id: Uuid,
    invoice_ids: &[Uuid],
) -> Result<u64, DbError> {
    if invoice_ids.is_empty() {
        return Ok(0);
    }
    let mut tx = db.begin().await?;
    let result = sqlx::query(
        r#"UPDATE extracted_invoices SET status = 'exported', exported_at = now(), updated_at = now()
           WHERE organization_id = $1 AND id = ANY($2) AND status = 'approved'"#,
    )
    .bind(organization_id)
    .bind(invoice_ids)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        r#"UPDATE documents d SET processing_status = 'exported', updated_at = now()
           WHERE d.organization_id = $1 AND d.id IN (
               SELECT document_id FROM extracted_invoices WHERE organization_id = $1 AND id = ANY($2)
           )"#,
    )
    .bind(organization_id)
    .bind(invoice_ids)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(result.rows_affected())
}

pub async fn insert_review_note(
    db: &Db,
    organization_id: Uuid,
    invoice_id: Uuid,
    actor_id: Uuid,
    action: &str,
    notes: &str,
) -> Result<(), DbError> {
    sqlx::query(
        "INSERT INTO invoice_reviews (organization_id, invoice_id, reviewer_id, action, notes) VALUES ($1,$2,$3,$4,$5)",
    )
    .bind(organization_id)
    .bind(invoice_id)
    .bind(actor_id)
    .bind(action)
    .bind(notes)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn review_queue(
    db: &Db,
    organization_id: Uuid,
    limit: i64,
) -> Result<Vec<InvoiceListRow>, sqlx::Error> {
    let (rows, _) = list_invoices(
        db,
        organization_id,
        Some("review_required"),
        None,
        None,
        None,
        None,
        None,
        None,
        limit,
        0,
    )
    .await?;
    Ok(rows)
}
