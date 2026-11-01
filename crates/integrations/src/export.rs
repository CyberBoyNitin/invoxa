//! Invoice export builders. Money is always formatted as an exact decimal
//! string; exports never use floating point. CSV text fields are protected
//! against spreadsheet formula injection.

use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::Serialize;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum ExportError {
    #[error("unsupported export format")]
    UnsupportedFormat,
    #[error("could not build the export file")]
    Build,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Csv,
    Json,
}

impl ExportFormat {
    pub fn parse(value: &str) -> Result<Self, ExportError> {
        match value.trim().to_ascii_lowercase().as_str() {
            "csv" => Ok(Self::Csv),
            "json" => Ok(Self::Json),
            _ => Err(ExportError::UnsupportedFormat),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Json => "json",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Json => "json",
        }
    }

    pub fn content_type(self) -> &'static str {
        match self {
            Self::Csv => "text/csv; charset=utf-8",
            Self::Json => "application/json; charset=utf-8",
        }
    }
}

/// Database-independent export row. Mapping from persistence types happens in
/// the workflow crate so this crate remains free of SQL concerns.
#[derive(Debug, Clone)]
pub struct ExportRecord {
    pub invoice_id: Uuid,
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

#[derive(Debug, Clone)]
pub struct ExportArtifact {
    pub bytes: Vec<u8>,
    pub filename: String,
    pub content_type: String,
    pub row_count: usize,
    pub format: ExportFormat,
}

pub fn generate_export(
    format: ExportFormat,
    records: &[ExportRecord],
    generated_at: DateTime<Utc>,
) -> Result<ExportArtifact, ExportError> {
    let timestamp = generated_at.format("%Y%m%dT%H%M%SZ");
    let filename = format!("invoxa-invoices-{timestamp}.{}", format.extension());
    let bytes = match format {
        ExportFormat::Csv => build_csv(records)?,
        ExportFormat::Json => build_json(records, generated_at)?,
    };
    Ok(ExportArtifact {
        bytes,
        filename,
        content_type: format.content_type().to_string(),
        row_count: records.len(),
        format,
    })
}

const CSV_HEADERS: &[&str] = &[
    "invoice_id",
    "invoice_number",
    "vendor_name",
    "vendor_tax_id",
    "invoice_date",
    "due_date",
    "currency",
    "subtotal",
    "discount",
    "shipping",
    "tax_total",
    "total",
    "amount_paid",
    "balance_due",
    "is_credit_note",
    "status",
    "document_type",
    "payment_terms",
    "purchase_order",
    "labels",
    "source_document_id",
    "source_filename",
    "approved_at",
];

fn build_csv(records: &[ExportRecord]) -> Result<Vec<u8>, ExportError> {
    let mut writer = csv::WriterBuilder::new()
        .has_headers(false)
        .flexible(false)
        .from_writer(Vec::new());
    writer
        .write_record(CSV_HEADERS)
        .map_err(|_| ExportError::Build)?;
    for record in records {
        let labels = record.labels.join(";");
        let approved_at = record.approved_at.map(|value| value.to_rfc3339());
        let fields = [
            record.invoice_id.to_string(),
            text(record.invoice_number.as_deref()),
            text(record.vendor_name.as_deref()),
            text(record.vendor_tax_id.as_deref()),
            record
                .invoice_date
                .map(|d| d.to_string())
                .unwrap_or_default(),
            record.due_date.map(|d| d.to_string()).unwrap_or_default(),
            text(record.currency.as_deref()),
            decimal(record.subtotal),
            decimal(record.discount),
            decimal(record.shipping),
            decimal(record.tax_total),
            decimal(record.total),
            decimal(record.amount_paid),
            decimal(record.balance_due),
            record.is_credit_note.to_string(),
            text(Some(&record.status)),
            text(Some(&record.document_type)),
            text(record.payment_terms.as_deref()),
            text(record.purchase_order.as_deref()),
            text(Some(&labels)),
            record.source_document_id.to_string(),
            text(Some(&record.source_filename)),
            approved_at
                .as_deref()
                .map(|v| text(Some(v)))
                .unwrap_or_default(),
        ];
        writer
            .write_record(fields)
            .map_err(|_| ExportError::Build)?;
    }
    writer.flush().map_err(|_| ExportError::Build)?;
    writer.into_inner().map_err(|_| ExportError::Build)
}

fn decimal(value: Option<Decimal>) -> String {
    value.map(|v| v.to_string()).unwrap_or_default()
}

/// Protect free-text fields from spreadsheet formula execution. Amount columns
/// are emitted separately and are not prefixed because they are validated
/// decimals controlled by the application.
fn text(value: Option<&str>) -> String {
    let Some(value) = value else {
        return String::new();
    };
    let normalized = value.replace('\0', "");
    let first_significant = normalized.trim_start().chars().next();
    let dangerous = matches!(
        first_significant,
        Some('=') | Some('+') | Some('-') | Some('@') | Some('\t') | Some('\r')
    );
    if dangerous {
        format!("'{normalized}")
    } else {
        normalized
    }
}

#[derive(Serialize)]
struct JsonExport {
    schema: &'static str,
    export_version: u32,
    generated_at: String,
    money_format: &'static str,
    date_format: &'static str,
    invoice_count: usize,
    invoices: Vec<JsonInvoice>,
}

#[derive(Serialize)]
struct JsonInvoice {
    invoice_id: String,
    invoice_number: Option<String>,
    vendor_name: Option<String>,
    vendor_tax_id: Option<String>,
    invoice_date: Option<String>,
    due_date: Option<String>,
    currency: Option<String>,
    subtotal: Option<String>,
    discount: Option<String>,
    shipping: Option<String>,
    tax_total: Option<String>,
    total: Option<String>,
    amount_paid: Option<String>,
    balance_due: Option<String>,
    is_credit_note: bool,
    status: String,
    document_type: String,
    payment_terms: Option<String>,
    purchase_order: Option<String>,
    labels: Vec<String>,
    approved_at: Option<String>,
    source_document_id: String,
    source_filename: String,
}

fn build_json(
    records: &[ExportRecord],
    generated_at: DateTime<Utc>,
) -> Result<Vec<u8>, ExportError> {
    let invoices: Vec<JsonInvoice> = records
        .iter()
        .map(|record| JsonInvoice {
            invoice_id: record.invoice_id.to_string(),
            invoice_number: record.invoice_number.clone(),
            vendor_name: record.vendor_name.clone(),
            vendor_tax_id: record.vendor_tax_id.clone(),
            invoice_date: record.invoice_date.map(|date| date.to_string()),
            due_date: record.due_date.map(|date| date.to_string()),
            currency: record.currency.clone(),
            subtotal: record.subtotal.map(|v| v.to_string()),
            discount: record.discount.map(|v| v.to_string()),
            shipping: record.shipping.map(|v| v.to_string()),
            tax_total: record.tax_total.map(|v| v.to_string()),
            total: record.total.map(|v| v.to_string()),
            amount_paid: record.amount_paid.map(|v| v.to_string()),
            balance_due: record.balance_due.map(|v| v.to_string()),
            is_credit_note: record.is_credit_note,
            status: record.status.clone(),
            document_type: record.document_type.clone(),
            payment_terms: record.payment_terms.clone(),
            purchase_order: record.purchase_order.clone(),
            labels: record.labels.clone(),
            approved_at: record
                .approved_at
                .map(|value| value.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)),
            source_document_id: record.source_document_id.to_string(),
            source_filename: record.source_filename.clone(),
        })
        .collect();
    let document = JsonExport {
        schema: "invoxa.invoices.v1",
        export_version: 1,
        generated_at: generated_at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        money_format: "decimal-string",
        date_format: "YYYY-MM-DD",
        invoice_count: invoices.len(),
        invoices,
    };
    serde_json::to_vec_pretty(&document).map_err(|_| ExportError::Build)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use rust_decimal::dec;

    fn record() -> ExportRecord {
        ExportRecord {
            invoice_id: Uuid::parse_str("11111111-1111-4111-8111-111111111111").unwrap(),
            invoice_number: Some("INV-001".into()),
            invoice_date: NaiveDate::from_ymd_opt(2026, 3, 14),
            due_date: NaiveDate::from_ymd_opt(2026, 4, 13),
            vendor_name: Some("Acme Supplies Pvt Ltd".into()),
            vendor_tax_id: Some("27AAPFU0939F1ZV".into()),
            currency: Some("INR".into()),
            subtotal: Some(dec!(100.00)),
            discount: Some(dec!(0.00)),
            shipping: None,
            tax_total: Some(dec!(18.00)),
            total: Some(dec!(118.00)),
            amount_paid: None,
            balance_due: Some(dec!(118.00)),
            status: "approved".into(),
            document_type: "supplier_invoice".into(),
            payment_terms: Some("Net 30".into()),
            purchase_order: Some("PO-9".into()),
            is_credit_note: false,
            approved_at: Some(Utc.with_ymd_and_hms(2026, 3, 15, 10, 30, 0).unwrap()),
            labels: vec!["march".into(), "priority".into()],
            source_document_id: Uuid::parse_str("22222222-2222-4222-8222-222222222222").unwrap(),
            source_filename: "invoice.pdf".into(),
        }
    }

    #[test]
    fn csv_has_exact_decimal_strings_and_zero_is_distinct_from_missing() {
        let artifact = generate_export(
            ExportFormat::Csv,
            &[record()],
            Utc.with_ymd_and_hms(2026, 3, 15, 12, 0, 0).unwrap(),
        )
        .unwrap();
        assert_eq!(artifact.filename, "invoxa-invoices-20260315T120000Z.csv");
        assert_eq!(artifact.row_count, 1);
        let csv = String::from_utf8(artifact.bytes).unwrap();
        let headers = csv.lines().next().unwrap();
        assert!(headers.starts_with("invoice_id,invoice_number,vendor_name"));
        assert!(csv.contains(",100.00,0.00,,"));
        assert!(csv.contains(",118.00,,118.00,"));
        assert!(csv.contains("march;priority"));
    }

    #[test]
    fn csv_blocks_formula_injection_in_text_fields_but_not_amounts() {
        let mut row = record();
        row.vendor_name = Some("=HYPERLINK(\"http://evil\")".into());
        row.invoice_number = Some("+1+1".into());
        row.payment_terms = Some("  @SUM(A1)".into());
        row.source_filename = "-cmd".into();
        let mut csv = String::from_utf8(
            generate_export(
                ExportFormat::Csv,
                &[row],
                Utc.with_ymd_and_hms(2026, 3, 15, 12, 0, 0).unwrap(),
            )
            .unwrap()
            .bytes,
        )
        .unwrap();
        assert!(csv.contains("'=HYPERLINK"));
        assert!(csv.contains("'+1+1"));
        assert!(csv.contains("'  @SUM"));
        assert!(csv.contains("'-cmd"));
        // The validated decimal column is untouched.
        assert!(csv.contains(",100.00,"));
        csv.clear();
    }

    #[test]
    fn json_export_is_versioned_and_money_remains_strings() {
        let artifact = generate_export(
            ExportFormat::Json,
            &[record()],
            Utc.with_ymd_and_hms(2026, 3, 15, 12, 0, 0).unwrap(),
        )
        .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&artifact.bytes).unwrap();
        assert_eq!(value["schema"], "invoxa.invoices.v1");
        assert_eq!(value["export_version"], 1);
        assert_eq!(value["money_format"], "decimal-string");
        assert_eq!(value["invoice_count"], 1);
        assert_eq!(value["invoices"][0]["total"], "118.00");
        assert_eq!(value["invoices"][0]["discount"], "0.00");
        assert!(value["invoices"][0]["shipping"].is_null());
        assert!(value["invoices"][0]["total"].is_string());
    }

    #[test]
    fn unsupported_format_is_rejected() {
        assert!(ExportFormat::parse("xlsx").is_err());
        assert_eq!(ExportFormat::parse("CSV").unwrap(), ExportFormat::Csv);
    }
}
