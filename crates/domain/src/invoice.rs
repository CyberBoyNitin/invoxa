use crate::normalization::{normalize_currency, normalize_invoice_number, vendor_match_key};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentKind {
    SupplierInvoice,
    CreditNote,
    Receipt,
    PurchaseOrder,
    Statement,
    OtherFinancial,
    Unknown,
}

impl DocumentKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SupplierInvoice => "supplier_invoice",
            Self::CreditNote => "credit_note",
            Self::Receipt => "receipt",
            Self::PurchaseOrder => "purchase_order",
            Self::Statement => "statement",
            Self::OtherFinancial => "other_financial",
            Self::Unknown => "unknown",
        }
    }

    pub fn is_invoice_like(self) -> bool {
        matches!(self, Self::SupplierInvoice | Self::CreditNote)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::SupplierInvoice => "Supplier invoice",
            Self::CreditNote => "Credit note",
            Self::Receipt => "Receipt",
            Self::PurchaseOrder => "Purchase order",
            Self::Statement => "Statement",
            Self::OtherFinancial => "Other financial document",
            Self::Unknown => "Unknown",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VendorInfo {
    pub name: Option<String>,
    pub tax_id: Option<String>,
    pub address: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LineItem {
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaxLine {
    /// CGST, SGST, IGST, VAT, GST, SALES_TAX, OTHER
    pub tax_type: Option<String>,
    pub rate: Option<Decimal>,
    pub taxable_amount: Option<Decimal>,
    pub amount: Option<Decimal>,
    pub source_page: Option<i32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FieldSource {
    pub page: Option<i32>,
    pub text: Option<String>,
    #[serde(default)]
    pub bbox: Option<[f64; 4]>,
}

/// Canonical invoice extraction. Every field is optional so that missing data
/// is distinct from zero. Monetary fields are exact decimals, never floats.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExtractedInvoice {
    pub document_kind: DocumentKind,
    pub vendor: VendorInfo,
    pub invoice_number: Option<String>,
    pub invoice_date: Option<String>,
    pub due_date: Option<String>,
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
    pub line_items: Vec<LineItem>,
    pub tax_lines: Vec<TaxLine>,
    /// Original textual representations keyed by field path.
    #[serde(default)]
    pub original_values: BTreeMap<String, String>,
    /// Page/text provenance derived from provider output and document search.
    #[serde(default)]
    pub field_sources: BTreeMap<String, FieldSource>,
    #[serde(default)]
    pub field_confidence: BTreeMap<String, f64>,
    /// Quality proxy from OCR/icon extraction. Not a calibrated AI confidence.
    #[serde(default)]
    pub extraction_quality: Option<f64>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

impl Default for DocumentKind {
    fn default() -> Self {
        Self::Unknown
    }
}

impl ExtractedInvoice {
    pub fn normalized_currency(&self) -> Option<String> {
        self.currency.as_deref().and_then(normalize_currency)
    }

    pub fn vendor_match_key(&self) -> Option<String> {
        self.vendor
            .name
            .as_deref()
            .map(vendor_match_key)
            .filter(|s| !s.is_empty())
    }

    pub fn invoice_number_key(&self) -> Option<String> {
        self.invoice_number
            .as_deref()
            .map(normalize_invoice_number)
            .filter(|s| !s.is_empty())
    }

    pub fn total_tax_from_lines(&self) -> Option<Decimal> {
        if self.tax_lines.is_empty() {
            return None;
        }
        let mut total = Decimal::ZERO;
        let mut seen = false;
        for line in &self.tax_lines {
            if let Some(amount) = line.amount {
                total += amount;
                seen = true;
            }
        }
        seen.then_some(total)
    }

    pub fn computed_line_total(item: &LineItem) -> Option<Decimal> {
        let quantity = item.quantity?;
        let unit_price = item.unit_price?;
        let base = quantity * unit_price;
        let discount = item.discount.unwrap_or(Decimal::ZERO);
        // Line totals are pre-tax. Tax is stored separately and must not be
        // double counted when reconciling the invoice subtotal.
        Some(base - discount)
    }

    pub fn sum_line_totals(&self) -> Option<Decimal> {
        if self.line_items.is_empty() {
            return None;
        }
        let mut total = Decimal::ZERO;
        for item in &self.line_items {
            total += item
                .line_total
                .or_else(|| Self::computed_line_total(item))?;
        }
        Some(total)
    }

    /// Display helper used by API responses / exports. Source fields remain
    /// untouched; this only computes if both inputs are present.
    pub fn balance_from_inputs(&self) -> Option<Decimal> {
        match (self.total, self.amount_paid) {
            (Some(total), Some(paid)) => Some(total - paid),
            _ => None,
        }
    }

    pub fn validate_shape(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        if let Some(name) = &self.vendor.name {
            if name.chars().count() > 300 {
                errors.push("vendor.name exceeds 300 characters".into());
            }
        }
        if let Some(number) = &self.invoice_number {
            if number.chars().count() > 120 {
                errors.push("invoice_number exceeds 120 characters".into());
            }
        }
        if self.line_items.len() > 500 {
            errors.push("more than 500 line items".into());
        }
        if self.tax_lines.len() > 100 {
            errors.push("more than 100 tax lines".into());
        }
        if self.document_kind == DocumentKind::Unknown && self.line_items.is_empty() {
            errors.push("document type is unknown".into());
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvoiceSummary {
    pub id: uuid::Uuid,
    pub document_id: uuid::Uuid,
    pub vendor_name: Option<String>,
    pub invoice_number: Option<String>,
    pub invoice_date: Option<String>,
    pub due_date: Option<String>,
    pub currency: Option<String>,
    pub total: Option<Decimal>,
    pub status: String,
    pub document_type: String,
    pub has_blocking_issues: bool,
    pub duplicate_status: Option<String>,
    pub version: i32,
    pub labels: Vec<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::dec;

    #[test]
    fn computes_tax_and_line_totals_without_mutating_source() {
        let invoice = ExtractedInvoice {
            tax_lines: vec![
                TaxLine {
                    tax_type: Some("CGST".into()),
                    amount: Some(dec!(9.00)),
                    ..Default::default()
                },
                TaxLine {
                    tax_type: Some("SGST".into()),
                    amount: Some(dec!(9.00)),
                    ..Default::default()
                },
            ],
            line_items: vec![LineItem {
                quantity: Some(dec!(2)),
                unit_price: Some(dec!(50)),
                discount: Some(dec!(10)),
                tax_amount: Some(dec!(18)),
                line_total: None,
                ..Default::default()
            }],
            ..Default::default()
        };
        assert_eq!(invoice.total_tax_from_lines(), Some(dec!(18.00)));
        assert_eq!(invoice.sum_line_totals(), Some(dec!(90)));
        assert!(invoice.line_items[0].line_total.is_none());
    }

    #[test]
    fn zero_is_not_missing() {
        let invoice = ExtractedInvoice {
            total: Some(Decimal::ZERO),
            ..Default::default()
        };
        assert_eq!(invoice.total, Some(Decimal::ZERO));
        assert_ne!(invoice.total, None);
    }
}
