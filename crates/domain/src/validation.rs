use crate::invoice::{DocumentKind, ExtractedInvoice, LineItem};
use crate::normalization::{
    date_to_iso, is_known_currency, normalize_currency, parse_invoice_date, validate_gstin_format,
};
use rust_decimal::{Decimal, RoundingStrategy};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Warning,
    Error,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }

    pub fn is_blocking(self) -> bool {
        self == Self::Error
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationIssue {
    pub code: String,
    pub severity: Severity,
    pub field: Option<String>,
    pub message: String,
}

impl ValidationIssue {
    pub fn new(
        code: &str,
        severity: Severity,
        field: Option<&str>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code: code.to_string(),
            severity,
            field: field.map(str::to_string),
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ValidationPolicy {
    pub country: String,
    pub require_invoice_date: bool,
    pub require_po_number: bool,
    pub require_vendor_tax_id: bool,
    pub require_gstin_for_india: bool,
    pub amount_tolerance: Decimal,
    pub max_due_date_days: i64,
}

impl Default for ValidationPolicy {
    fn default() -> Self {
        Self {
            country: "IN".into(),
            require_invoice_date: true,
            require_po_number: false,
            require_vendor_tax_id: false,
            require_gstin_for_india: false,
            amount_tolerance: Decimal::new(2, 2), // 0.02
            max_due_date_days: 730,
        }
    }
}

impl ValidationPolicy {
    /// Build the deterministic validation policy from organization settings.
    /// Invalid or out-of-range values fall back to safe defaults.
    pub fn from_settings(country: &str, settings: &Value) -> Self {
        let mut policy = Self {
            country: country.to_string(),
            ..Self::default()
        };
        let Some(settings) = settings.get("validation") else {
            return policy;
        };
        if let Some(value) = settings
            .get("require_invoice_date")
            .and_then(Value::as_bool)
        {
            policy.require_invoice_date = value;
        }
        if let Some(value) = settings.get("require_po_number").and_then(Value::as_bool) {
            policy.require_po_number = value;
        }
        if let Some(value) = settings
            .get("require_vendor_tax_id")
            .and_then(Value::as_bool)
        {
            policy.require_vendor_tax_id = value;
        }
        if let Some(value) = settings
            .get("require_gstin_for_india")
            .and_then(Value::as_bool)
        {
            policy.require_gstin_for_india = value;
        }
        if let Some(value) = settings.get("amount_tolerance") {
            let raw = value
                .as_str()
                .map(str::to_string)
                .or_else(|| value.as_number().map(ToString::to_string));
            if let Some(raw) = raw {
                if let Ok(tolerance) = Decimal::from_str(&raw) {
                    if tolerance >= Decimal::ZERO && tolerance <= Decimal::ONE {
                        policy.amount_tolerance = tolerance;
                    }
                }
            }
        }
        if let Some(value) = settings.get("max_due_date_days").and_then(Value::as_i64) {
            if (0..=3650).contains(&value) {
                policy.max_due_date_days = value;
            }
        }
        policy
    }
}
#[derive(Debug, Clone)]
pub struct ValidationContext {
    pub policy: ValidationPolicy,
    /// Quality proxy (OCR / extraction), not calibrated confidence.
    pub extraction_quality: Option<f64>,
}

#[derive(Debug, Clone, Default)]
pub struct ValidationResult {
    pub issues: Vec<ValidationIssue>,
}

impl ValidationResult {
    pub fn blocking(&self) -> bool {
        self.issues.iter().any(|i| i.severity.is_blocking())
    }

    pub fn review_required(&self) -> bool {
        self.issues
            .iter()
            .any(|i| matches!(i.severity, Severity::Error | Severity::Warning))
    }

    pub fn error_count(&self) -> usize {
        self.issues
            .iter()
            .filter(|i| i.severity == Severity::Error)
            .count()
    }

    pub fn warning_count(&self) -> usize {
        self.issues
            .iter()
            .filter(|i| i.severity == Severity::Warning)
            .count()
    }
}

fn round(value: Decimal) -> Decimal {
    value.round_dp_with_strategy(2, RoundingStrategy::MidpointAwayFromZero)
}

pub fn validate_invoice(invoice: &ExtractedInvoice, ctx: &ValidationContext) -> ValidationResult {
    let mut issues = Vec::new();
    let policy = &ctx.policy;
    let tolerance = policy.amount_tolerance;

    // Required fields.
    let vendor_name = invoice.vendor.name.as_deref().map(str::trim).unwrap_or("");
    if vendor_name.is_empty() {
        issues.push(ValidationIssue::new(
            "VENDOR_NAME_MISSING",
            Severity::Error,
            Some("vendor.name"),
            "Supplier name is required to identify the vendor.",
        ));
    }
    let invoice_number = invoice
        .invoice_number
        .as_deref()
        .map(str::trim)
        .unwrap_or("");
    if invoice_number.is_empty() {
        issues.push(ValidationIssue::new(
            "INVOICE_NUMBER_MISSING",
            Severity::Error,
            Some("invoice_number"),
            "Invoice number is required and could not be determined from the document.",
        ));
    }
    if policy.require_invoice_date && invoice.invoice_date.is_none() {
        issues.push(ValidationIssue::new(
            "INVOICE_DATE_MISSING",
            Severity::Error,
            Some("invoice_date"),
            "Invoice date is required by this organization's validation policy.",
        ));
    }
    if policy.require_po_number
        && invoice
            .purchase_order
            .as_deref()
            .unwrap_or("")
            .trim()
            .is_empty()
    {
        issues.push(ValidationIssue::new(
            "PO_NUMBER_MISSING",
            Severity::Error,
            Some("purchase_order"),
            "Purchase order number is required by this organization's validation policy.",
        ));
    }
    let currency = invoice.currency.as_deref().and_then(normalize_currency);
    if currency.is_none() {
        issues.push(ValidationIssue::new(
            "CURRENCY_MISSING",
            Severity::Error,
            Some("currency"),
            "Currency is required to interpret monetary amounts.",
        ));
    } else if let Some(code) = &currency {
        if !is_known_currency(code) {
            issues.push(ValidationIssue::new(
                "CURRENCY_UNKNOWN",
                Severity::Warning,
                Some("currency"),
                format!("Currency '{code}' is not in the recognized ISO 4217 list. Amounts were preserved."),
            ));
        }
    }
    if invoice.total.is_none() {
        issues.push(ValidationIssue::new(
            "TOTAL_MISSING",
            Severity::Error,
            Some("total"),
            "Invoice total is required and could not be determined.",
        ));
    }

    // Document type / credit note semantics.
    if invoice.document_kind == DocumentKind::Unknown {
        issues.push(ValidationIssue::new(
            "DOCUMENT_TYPE_UNCERTAIN",
            Severity::Warning,
            Some("document_kind"),
            "The document could not be confidently classified as an invoice.",
        ));
    }
    if invoice.is_credit_note != (invoice.document_kind == DocumentKind::CreditNote)
        && invoice.document_kind.is_invoice_like()
    {
        issues.push(ValidationIssue::new(
            "CREDIT_NOTE_FLAG_MISMATCH",
            Severity::Warning,
            Some("is_credit_note"),
            "The credit-note indicator does not match the extracted document type.",
        ));
    }
    if let Some(total) = invoice.total {
        if total < Decimal::ZERO
            && !invoice.is_credit_note
            && invoice.document_kind != DocumentKind::CreditNote
        {
            issues.push(ValidationIssue::new(
                "NEGATIVE_TOTAL_WITHOUT_CREDIT_NOTE",
                Severity::Warning,
                Some("total"),
                "Total is negative but the document is not marked as a credit note.",
            ));
        }
        if total > Decimal::ZERO
            && (invoice.is_credit_note || invoice.document_kind == DocumentKind::CreditNote)
        {
            issues.push(ValidationIssue::new(
                "CREDIT_NOTE_POSITIVE_TOTAL",
                Severity::Warning,
                Some("total"),
                "This is a credit note with a positive total. Verify the sign convention used by the supplier.",
            ));
        }
    }

    // Dates.
    if let Some(date_text) = &invoice.invoice_date {
        match parse_invoice_date(date_text) {
            Ok(parsed) => {
                if parsed.ambiguous {
                    issues.push(ValidationIssue::new(
                        "DATE_AMBIGUOUS",
                        Severity::Warning,
                        Some("invoice_date"),
                        "Invoice date has an ambiguous day/month order. Verify it against the document.",
                    ));
                }
                if let Some(original) = invoice.original_values.get("invoice_date") {
                    if let Ok(from_original) = parse_invoice_date(original) {
                        if from_original.ambiguous {
                            issues.push(ValidationIssue::new(
                                "DATE_AMBIGUOUS_ORIGINAL",
                                Severity::Warning,
                                Some("invoice_date"),
                                format!(
                                    "Original date text '{}' is ambiguous. Normalized to {} using day-first interpretation.",
                                    original,
                                    date_to_iso(parsed.date)
                                ),
                            ));
                        }
                    }
                }
                if let Some(due_text) = &invoice.due_date {
                    match parse_invoice_date(due_text) {
                        Ok(due) => {
                            if due.date < parsed.date {
                                issues.push(ValidationIssue::new(
                                    "DUE_DATE_BEFORE_INVOICE",
                                    Severity::Warning,
                                    Some("due_date"),
                                    "Due date is earlier than the invoice date.",
                                ));
                            } else if (due.date - parsed.date).num_days() > policy.max_due_date_days
                            {
                                issues.push(ValidationIssue::new(
                                    "DUE_DATE_TOO_FAR",
                                    Severity::Warning,
                                    Some("due_date"),
                                    "Due date is unusually far in the future. It was preserved, not changed.",
                                ));
                            }
                        }
                        Err(_) => issues.push(ValidationIssue::new(
                            "DUE_DATE_INVALID",
                            Severity::Error,
                            Some("due_date"),
                            "Due date is not a valid date.",
                        )),
                    }
                }
            }
            Err(_) => issues.push(ValidationIssue::new(
                "INVOICE_DATE_INVALID",
                Severity::Error,
                Some("invoice_date"),
                "Invoice date is not a parseable or possible calendar date.",
            )),
        }
    }

    // Tax totals.
    let tax_from_lines = invoice.total_tax_from_lines();
    if let (Some(stated), Some(from_lines)) = (invoice.tax_total, tax_from_lines) {
        if (stated - from_lines).abs() > tolerance {
            issues.push(ValidationIssue::new(
                "TAX_TOTAL_MISMATCH",
                Severity::Error,
                Some("tax_total"),
                format!(
                    "Stated tax total {} does not match the sum of tax lines {}.",
                    round(stated),
                    round(from_lines)
                ),
            ));
        }
    }
    let effective_tax = invoice.tax_total.or(tax_from_lines);

    // Main arithmetic identity: subtotal - discount + shipping + tax = total.
    if let (Some(subtotal), Some(total)) = (invoice.subtotal, invoice.total) {
        let discount = invoice.discount.unwrap_or(Decimal::ZERO);
        let shipping = invoice.shipping.unwrap_or(Decimal::ZERO);
        let tax = effective_tax.unwrap_or(Decimal::ZERO);
        let expected = subtotal - discount + shipping + tax;
        if (expected - total).abs() > tolerance {
            issues.push(ValidationIssue::new(
                "TOTAL_ARITHMETIC_MISMATCH",
                Severity::Error,
                Some("total"),
                format!(
                    "Subtotal {} − discount {} + shipping {} + tax {} = {}, but stated total is {}. Difference: {}.",
                    round(subtotal),
                    round(discount),
                    round(shipping),
                    round(tax),
                    round(expected),
                    round(total),
                    round(expected - total)
                ),
            ));
        }
    } else if invoice.subtotal.is_none() && invoice.total.is_some() {
        issues.push(ValidationIssue::new(
            "SUBTOTAL_MISSING",
            Severity::Warning,
            Some("subtotal"),
            "Subtotal is not available, so the total could not be independently verified.",
        ));
    }

    // Balance due, only validated when explicitly present.
    if let (Some(balance), Some(total)) = (invoice.balance_due, invoice.total) {
        let paid = invoice.amount_paid.unwrap_or(Decimal::ZERO);
        if (total - paid - balance).abs() > tolerance {
            issues.push(ValidationIssue::new(
                "BALANCE_DUE_MISMATCH",
                Severity::Warning,
                Some("balance_due"),
                "Balance due does not equal total minus amount paid.",
            ));
        }
    }
    if let (Some(paid), Some(total)) = (invoice.amount_paid, invoice.total) {
        if paid > total + tolerance && !invoice.is_credit_note {
            issues.push(ValidationIssue::new(
                "AMOUNT_PAID_EXCEEDS_TOTAL",
                Severity::Warning,
                Some("amount_paid"),
                "Amount paid exceeds the invoice total.",
            ));
        }
    }

    // Line item arithmetic.
    let mut computed_sum = Decimal::ZERO;
    let mut have_all_line_totals = !invoice.line_items.is_empty();
    for (index, item) in invoice.line_items.iter().enumerate() {
        let field = |name: &str| format!("line_items[{index}].{name}");
        if let (Some(stated), Some(computed)) =
            (item.line_total, ExtractedInvoice::computed_line_total(item))
        {
            let difference = if invoice.is_credit_note {
                (stated.abs() - computed.abs()).abs()
            } else {
                (stated - computed).abs()
            };
            if difference > tolerance {
                issues.push(ValidationIssue::new(
                    "LINE_TOTAL_MISMATCH",
                    Severity::Error,
                    Some(&field("line_total")),
                    format!(
                        "Line {} total {} does not match quantity × unit price − discount + tax ({}).",
                        index + 1,
                        round(stated),
                        round(computed)
                    ),
                ));
            }
        }
        match item
            .line_total
            .or_else(|| ExtractedInvoice::computed_line_total(item))
        {
            Some(value) => computed_sum += value,
            None => have_all_line_totals = false,
        }
        if item.quantity.is_some_and(|q| q < Decimal::ZERO) && !invoice.is_credit_note {
            issues.push(ValidationIssue::new(
                "NEGATIVE_QUANTITY",
                Severity::Warning,
                Some(&field("quantity")),
                "Line item has a negative quantity. It was preserved.",
            ));
        }
    }
    if !invoice.line_items.is_empty() {
        if let Some(subtotal) = invoice.subtotal {
            if have_all_line_totals && (computed_sum - subtotal).abs() > tolerance {
                issues.push(ValidationIssue::new(
                    "SUBTOTAL_LINE_SUM_MISMATCH",
                    Severity::Error,
                    Some("subtotal"),
                    format!(
                        "Line items sum to {}, but the stated subtotal is {}.",
                        round(computed_sum),
                        round(subtotal)
                    ),
                ));
            }
        } else {
            issues.push(ValidationIssue::new(
                "SUBTOTAL_LINE_ITEMS_UNVERIFIED",
                Severity::Warning,
                Some("subtotal"),
                "Line items exist but no subtotal was extracted; line totals cannot be reconciled.",
            ));
        }
    }

    // Indian GST rules, applied only when the organization jurisdiction is India.
    let is_india = policy.country.eq_ignore_ascii_case("IN")
        || policy.country.eq_ignore_ascii_case("IND")
        || policy.country.eq_ignore_ascii_case("INDIA");
    if is_india {
        if let Some(tax_id) = invoice
            .vendor
            .tax_id
            .as_deref()
            .filter(|v| !v.trim().is_empty())
        {
            if let Err(reason) = validate_gstin_format(tax_id) {
                issues.push(ValidationIssue::new(
                    "GSTIN_INVALID",
                    Severity::Error,
                    Some("vendor.tax_id"),
                    format!("Vendor GSTIN is invalid: {reason}."),
                ));
            }
        } else if policy.require_gstin_for_india {
            issues.push(ValidationIssue::new(
                "GSTIN_MISSING",
                Severity::Error,
                Some("vendor.tax_id"),
                "A vendor GSTIN is required by this organization's policy.",
            ));
        }

        let gst_types: BTreeSet<String> = invoice
            .tax_lines
            .iter()
            .filter_map(|t| t.tax_type.as_ref().map(|s| s.to_ascii_uppercase()))
            .collect();
        let has_split = gst_types.contains("CGST") || gst_types.contains("SGST");
        let has_igst = gst_types.contains("IGST");
        if has_split && has_igst {
            issues.push(ValidationIssue::new(
                "GST_STRUCTURE_MIXED",
                Severity::Warning,
                Some("tax_lines"),
                "Both intra-state (CGST/SGST) and inter-state (IGST) tax lines are present.",
            ));
        }
        let gst_sum: Decimal = invoice
            .tax_lines
            .iter()
            .filter(|t| {
                t.tax_type
                    .as_deref()
                    .map(|s| matches!(s.to_ascii_uppercase().as_str(), "CGST" | "SGST" | "IGST"))
                    .unwrap_or(false)
            })
            .filter_map(|t| t.amount)
            .sum();
        if let Some(stated) = invoice.tax_total.or(effective_tax) {
            if has_split || has_igst {
                if (stated - gst_sum).abs() > tolerance {
                    issues.push(ValidationIssue::new(
                        "GST_SPLIT_MISMATCH",
                        Severity::Error,
                        Some("tax_total"),
                        format!(
                            "CGST/SGST/IGST lines sum to {}, but total tax is {}.",
                            round(gst_sum),
                            round(stated)
                        ),
                    ));
                }
            }
        }
        if invoice.vendor.tax_id.is_none()
            && invoice
                .currency
                .as_deref()
                .and_then(normalize_currency)
                .as_deref()
                == Some("INR")
        {
            // Informational only: GSTIN requirements vary by supplier and
            // transaction. Organizations may make it mandatory via policy.
            issues.push(ValidationIssue::new(
                "GSTIN_NOT_PROVIDED",
                Severity::Info,
                Some("vendor.tax_id"),
                "No vendor GSTIN was extracted. Confirm against the source document if required.",
            ));
        }
    }

    if let Some(quality) = ctx.extraction_quality {
        if quality < 0.5 {
            issues.push(ValidationIssue::new(
                "LOW_OCR_QUALITY",
                Severity::Warning,
                None,
                "Text extraction quality is low. Verify values against the original document.",
            ));
        }
    }

    ValidationResult { issues }
}

/// Recalculate totals for an edited invoice. Returns (subtotal, tax, total)
/// computed from line items where possible. Display-only: callers must persist
/// the explicit values chosen by the reviewer.
pub fn recalculated_totals(
    invoice: &ExtractedInvoice,
) -> (Option<Decimal>, Option<Decimal>, Option<Decimal>) {
    let subtotal = if invoice.line_items.is_empty() {
        invoice.subtotal
    } else {
        let mut sum = Decimal::ZERO;
        let mut seen = false;
        for item in &invoice.line_items {
            if let Some(value) = item.line_total.or_else(|| {
                let q = item.quantity?;
                let p = item.unit_price?;
                Some(q * p - item.discount.unwrap_or(Decimal::ZERO))
            }) {
                sum += value;
                seen = true;
            }
        }
        seen.then_some(sum)
    };
    let tax = invoice.tax_total.or_else(|| invoice.total_tax_from_lines());
    let total = match (subtotal, tax) {
        (Some(s), Some(t)) => Some(
            s - invoice.discount.unwrap_or(Decimal::ZERO)
                + invoice.shipping.unwrap_or(Decimal::ZERO)
                + t,
        ),
        _ => invoice.total,
    };
    (subtotal, tax, total)
}

#[allow(dead_code)]
fn line_with_total(item: &LineItem) -> Option<Decimal> {
    item.line_total
        .or_else(|| ExtractedInvoice::computed_line_total(item))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::invoice::{TaxLine, VendorInfo};
    use rust_decimal::dec;

    fn clean_invoice() -> ExtractedInvoice {
        ExtractedInvoice {
            document_kind: DocumentKind::SupplierInvoice,
            vendor: VendorInfo {
                name: Some("Acme Supplies Pvt Ltd".into()),
                ..Default::default()
            },
            invoice_number: Some("INV-1001".into()),
            invoice_date: Some("2026-03-01".into()),
            due_date: Some("2026-03-31".into()),
            currency: Some("INR".into()),
            subtotal: Some(dec!(1000)),
            tax_total: Some(dec!(180)),
            total: Some(dec!(1180)),
            line_items: vec![LineItem {
                description: Some("Widgets".into()),
                quantity: Some(dec!(10)),
                unit_price: Some(dec!(100)),
                tax_amount: Some(dec!(180)),
                line_total: Some(dec!(1000)),
                ..Default::default()
            }],
            tax_lines: vec![TaxLine {
                tax_type: Some("IGST".into()),
                rate: Some(dec!(18)),
                amount: Some(dec!(180)),
                ..Default::default()
            }],
            extraction_quality: Some(0.95),
            ..Default::default()
        }
    }

    fn ctx() -> ValidationContext {
        ValidationContext {
            policy: ValidationPolicy::default(),
            extraction_quality: Some(0.95),
        }
    }

    #[test]
    fn clean_invoice_has_no_issues() {
        let result = validate_invoice(&clean_invoice(), &ctx());
        assert!(!result.blocking(), "{:?}", result.issues);
        assert!(!result.review_required());
        assert!(result.issues.iter().all(|i| i.severity == Severity::Info));
    }

    #[test]
    fn catches_total_arithmetic_mismatch() {
        let mut invoice = clean_invoice();
        invoice.total = Some(dec!(1200));
        let result = validate_invoice(&invoice, &ctx());
        assert!(result
            .issues
            .iter()
            .any(|i| i.code == "TOTAL_ARITHMETIC_MISMATCH"));
        assert!(result.blocking());
    }

    #[test]
    fn catches_missing_critical_fields() {
        let invoice = ExtractedInvoice {
            total: Some(dec!(100)),
            ..Default::default()
        };
        let result = validate_invoice(&invoice, &ctx());
        for code in [
            "VENDOR_NAME_MISSING",
            "INVOICE_NUMBER_MISSING",
            "CURRENCY_MISSING",
        ] {
            assert!(
                result.issues.iter().any(|i| i.code == code),
                "missing {code}"
            );
        }
    }

    #[test]
    fn catches_line_item_mismatch() {
        let mut invoice = clean_invoice();
        invoice.line_items[0].line_total = Some(dec!(99));
        let result = validate_invoice(&invoice, &ctx());
        assert!(result
            .issues
            .iter()
            .any(|i| i.code == "LINE_TOTAL_MISMATCH"));
    }

    #[test]
    fn catches_gst_split_mismatch() {
        let mut invoice = clean_invoice();
        invoice.tax_lines = vec![
            TaxLine {
                tax_type: Some("CGST".into()),
                amount: Some(dec!(90)),
                ..Default::default()
            },
            TaxLine {
                tax_type: Some("SGST".into()),
                amount: Some(dec!(80)),
                ..Default::default()
            },
        ];
        let result = validate_invoice(&invoice, &ctx());
        assert!(result.issues.iter().any(|i| i.code == "GST_SPLIT_MISMATCH"));
    }

    #[test]
    fn flags_ambiguous_original_date() {
        let mut invoice = clean_invoice();
        invoice.invoice_date = Some("2026-04-03".into());
        invoice
            .original_values
            .insert("invoice_date".into(), "03/04/2026".into());
        let result = validate_invoice(&invoice, &ctx());
        assert!(result
            .issues
            .iter()
            .any(|i| i.code == "DATE_AMBIGUOUS_ORIGINAL"));
    }

    #[test]
    fn credit_note_negative_total_is_clean() {
        let mut invoice = clean_invoice();
        invoice.document_kind = DocumentKind::CreditNote;
        invoice.is_credit_note = true;
        invoice.subtotal = Some(dec!(-1000));
        invoice.tax_total = Some(dec!(-180));
        invoice.total = Some(dec!(-1180));
        invoice.line_items[0].line_total = Some(dec!(-1000));
        invoice.tax_lines[0].amount = Some(dec!(-180));
        let result = validate_invoice(&invoice, &ctx());
        assert!(!result.blocking(), "{:?}", result.issues);
    }
}
