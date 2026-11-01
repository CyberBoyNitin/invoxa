//! Deterministic, rule-based extraction provider used for local development and
//! automated tests.
//!
//! This provider is **not** an AI model. It never invents amounts, dates or
//! identifiers: every value is copied from a labelled line of the incoming
//! text. Output is always marked `is_mock = true` and carries an explicit
//! warning so that it cannot be mistaken for production AI extraction.

use crate::{
    ExtractionError, ExtractionInput, ExtractionProvider, ExtractionResult, ProviderInvoice,
    ProviderLineItem, ProviderPageReference, ProviderTaxLine,
};
use async_trait::async_trait;
use chrono::Utc;
use invoxa_domain::invoice::DocumentKind;
use invoxa_domain::normalization::{parse_invoice_date, truncate_chars};
use regex::Regex;
use rust_decimal::Decimal;
use std::str::FromStr;
use std::sync::OnceLock;

pub const MOCK_PROVIDER_NAME: &str = "mock-heuristic";
pub const MOCK_MODEL_NAME: &str = "deterministic-rules-v1";
pub const MOCK_WARNING: &str =
    "Mock extractor: values were parsed by deterministic rules, not AI. Review every field.";

#[derive(Debug, Default, Clone)]
pub struct MockExtractionProvider;

fn amount_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r#"(?i)(?:INR|USD|EUR|GBP|AED|SGD|AUD|CAD|JPY|CNY|CHF|NZD|ZAR|MYR|THB|IDR|PHP|VND|KES|NGN|BRL|MXN|RON|PLN|SEK|NOK|DKK|HKD|LKR|NPR|PKR|BDT|TRY|UAH|ILS|SAR|QAR|KWD|OMR|BHD|JOD|EGP|MAD|GHS|PEN|COP|CLP|CZK|HUF|ISK)?\s*(?:[₹$€£¥₩₦₽]\s*)?(\(?-?\d[\d,\s]*\.?\d{0,4}\)?)\s*$"#,
        )
        .expect("amount regex")
    })
}

fn gstin_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"\b([0-9]{2}[A-Z]{5}[0-9]{4}[A-Z][0-9A-Z]Z[0-9A-Z])\b").expect("gstin regex")
    })
}

fn email_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)\b([A-Z0-9._%+\-]+@[A-Z0-9.\-]+\.[A-Z]{2,})\b").expect("email regex")
    })
}

fn line_item_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r#"(?i)^\s*([A-Za-z][A-Za-z0-9 &().'\-/,]{2,80}?)\s{1,}(\d{1,4}(?:[.,]\d{1,3})?)\s+(?:[₹$€£¥]\s*)?(\d[\d,]*\.\d{2})\s+(?:[₹$€£¥]\s*)?(\d[\d,]*\.\d{2})\s*$"#,
        )
        .expect("line item regex")
    })
}

fn tax_line_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r#"(?i)^\s*(CGST|SGST|IGST|VAT|GST|SALES\s+TAX|SERVICE\s+TAX|TAX)\b[^0-9%\n]{0,20}(?:(\d+(?:\.\d+)?)\s*%)?[^0-9\n]{0,20}([₹$€£¥]?\s*\(?-?\d[\d,]*\.?\d{0,4}\)?)\s*$"#,
        )
        .expect("tax line regex")
    })
}

fn currency_code_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)\b(INR|USD|EUR|GBP|AED|SGD|AUD|CAD|JPY|CNY|CHF|NZD|ZAR|MYR|THB|IDR|PHP|VND|KES|NGN|BRL|MXN)\b",
        )
        .expect("currency regex")
    })
}

/// Parse an amount token exactly. Returns `None` rather than guessing when the
/// token is not a valid decimal. Parentheses denote a negative amount.
fn parse_amount(raw: &str) -> Option<Decimal> {
    let trimmed = raw.trim();
    let negative = trimmed.starts_with('(') && trimmed.ends_with(')');
    let cleaned: String = trimmed
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.' || *c == '-' || *c == ',')
        .collect();
    let cleaned = cleaned.replace(',', "");
    if cleaned.is_empty() || cleaned == "-" {
        return None;
    }
    let mut value = Decimal::from_str(&cleaned).ok()?;
    if negative {
        value = -value.abs();
    }
    Some(value)
}

fn line_amount(line: &str) -> Option<(Decimal, String)> {
    let caps = amount_re().captures(line)?;
    let token = caps.get(1)?.as_str();
    parse_amount(token).map(|value| (value, token.to_string()))
}

fn label_key(text: &str) -> String {
    const CURRENCY_WORDS: &[&str] = &[
        "inr", "usd", "eur", "gbp", "aed", "sgd", "aud", "cad", "jpy", "cny", "chf", "nzd", "zar",
        "myr", "thb", "idr", "php", "vnd", "kes", "ngn", "brl", "mxn",
    ];
    let mut value = text.to_ascii_lowercase();
    if let Some(index) = value.find(':') {
        value.truncate(index);
    }
    let words = value
        .split_whitespace()
        .filter(|word| {
            let stripped = word
                .trim_matches(|c: char| c == '₹' || c == '$' || c == '€' || c == '£' || c == '¥');
            !stripped.is_empty() && !CURRENCY_WORDS.contains(&stripped)
        })
        .collect::<Vec<_>>();
    words.join(" ")
}

fn labelled_value<'a>(text: &'a str, labels: &[&str]) -> Option<&'a str> {
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let lower = trimmed.to_ascii_lowercase();
        for label in labels {
            let label = label.to_ascii_lowercase();
            if !lower.starts_with(&label) {
                continue;
            }
            let rest = &trimmed[label.len()..];
            // Require a separator so that "Total" does not match "Totally".
            if !rest.starts_with(':')
                && !rest.starts_with('-')
                && !rest.starts_with('#')
                && !rest.starts_with(' ')
            {
                continue;
            }
            let value = rest
                .trim_start_matches(|c: char| c == ':' || c == '-' || c == '#' || c.is_whitespace())
                .trim();
            if !value.is_empty() {
                return Some(value);
            }
        }
    }
    None
}

fn detect_document_kind(text: &str) -> (DocumentKind, bool) {
    let lower = text.to_ascii_lowercase();
    let credit = lower.contains("credit note") || lower.contains("credit memo");
    let receipt = lower.contains("receipt") && !lower.contains("invoice");
    let purchase_order = lower.contains("purchase order") && !lower.contains("invoice");
    let statement = lower.contains("statement") && !lower.contains("invoice");
    if credit {
        (DocumentKind::CreditNote, true)
    } else if receipt {
        (DocumentKind::Receipt, false)
    } else if purchase_order {
        (DocumentKind::PurchaseOrder, false)
    } else if statement {
        (DocumentKind::Statement, false)
    } else if lower.contains("invoice") {
        (DocumentKind::SupplierInvoice, false)
    } else {
        (DocumentKind::Unknown, false)
    }
}

fn find_vendor_name(text: &str) -> Option<String> {
    if let Some(value) = labelled_value(text, &["vendor", "supplier", "seller", "from"]) {
        let cleaned = value.trim();
        if cleaned.chars().any(|c| c.is_alphabetic())
            && cleaned.len() <= 120
            && !cleaned.to_ascii_lowercase().contains("invoice")
        {
            return Some(truncate_chars(cleaned, 300));
        }
    }
    // Fall back to the first plausible heading line. This is still copied text,
    // not an inferred name. Generic document titles and address-only lines are
    // skipped.
    for line in text.lines().take(12) {
        let candidate = line.trim();
        if candidate.len() < 2 || candidate.len() > 120 {
            continue;
        }
        let lower = candidate.to_ascii_lowercase();
        if lower.starts_with("[page") || lower.starts_with("page ") {
            continue;
        }
        if lower.contains("invoice")
            || lower.contains("bill ")
            || lower == "bill"
            || lower.contains("receipt")
            || lower.contains("credit note")
            || lower.contains("statement")
            || lower.contains("purchase order")
            || lower.contains("tax invoice")
            || lower.contains('@')
            || lower.contains("gstin")
            || lower.contains("tax id")
            || lower.chars().filter(|c| c.is_alphabetic()).count() < 2
            || candidate.chars().filter(|c| c.is_ascii_digit()).count() > 6
            || candidate.ends_with('.')
        {
            continue;
        }
        return Some(truncate_chars(candidate, 300));
    }
    None
}

fn find_invoice_number(text: &str) -> Option<String> {
    for line in text.lines() {
        let trimmed = line.trim();
        let lower = trimmed.to_ascii_lowercase();
        let label = [
            "invoice no",
            "invoice number",
            "invoice #",
            "bill no",
            "bill number",
            "bill #",
            "inv no",
            "inv #",
        ]
        .iter()
        .find(|label| lower.starts_with(**label));
        if let Some(label) = label {
            let value = trimmed[label.len()..]
                .trim_start_matches(|c: char| c == ':' || c == '-' || c == '#' || c.is_whitespace())
                .trim();
            let token = value
                .split_whitespace()
                .next()
                .unwrap_or("")
                .trim_matches(|c: char| c == ',' || c == ';');
            if token.len() >= 2
                && token.len() <= 60
                && token.chars().any(|c| c.is_ascii_alphanumeric())
                && !token.eq_ignore_ascii_case("number")
            {
                return Some(token.to_string());
            }
        }
    }
    None
}

fn find_date(text: &str, labels: &[&str]) -> Option<(String, String)> {
    for line in text.lines() {
        let trimmed = line.trim();
        let lower = trimmed.to_ascii_lowercase();
        for label in labels {
            if !lower.starts_with(label) {
                continue;
            }
            let rest = trimmed[label.len()..].trim_start_matches(|c: char| {
                c == ':' || c == '-' || c == '#' || c.is_whitespace()
            });
            let token = rest.split('|').next().unwrap_or("").trim();
            // Take the longest date-looking prefix. The domain parser handles
            // ambiguous formats, and returns an error rather than guessing when
            // the date is impossible.
            let mut best = None;
            for end in (6..=token.len()).rev() {
                if !token.is_char_boundary(end) {
                    continue;
                }
                let candidate = token[..end].trim();
                if let Ok(parsed) = parse_invoice_date(candidate) {
                    best = Some((parsed.date.format("%Y-%m-%d").to_string(), parsed.original));
                    break;
                }
            }
            if let Some(value) = best {
                return Some(value);
            }
        }
    }
    None
}

fn find_totals(text: &str) -> Totals {
    let mut totals = Totals::default();
    for line in text.lines() {
        let Some((value, token)) = line_amount(line) else {
            continue;
        };
        let prefix = line[..line.rfind(&token).unwrap_or(line.len())].trim();
        let key = label_key(prefix);
        if key.is_empty() {
            continue;
        }
        let is = |needle: &str| key == needle;
        if is("subtotal") || is("sub total") || is("net total") {
            totals.subtotal.get_or_insert(value);
        } else if is("grand total")
            || is("total amount")
            || is("invoice total")
            || is("total due")
            || is("amount due")
            || is("total")
        {
            totals.total.get_or_insert(value);
        } else if is("tax total") || is("total tax") {
            totals.tax_total.get_or_insert(value);
        } else if is("discount") || is("total discount") {
            totals.discount.get_or_insert(value);
        } else if is("shipping") || is("freight") || is("delivery") {
            totals.shipping.get_or_insert(value);
        } else if is("amount paid") || is("paid") || is("payment received") {
            totals.amount_paid.get_or_insert(value);
        } else if is("balance due") || is("balance") || is("outstanding") {
            totals.balance_due.get_or_insert(value);
        }
    }
    totals
}

#[derive(Debug, Default)]
struct Totals {
    subtotal: Option<Decimal>,
    discount: Option<Decimal>,
    shipping: Option<Decimal>,
    tax_total: Option<Decimal>,
    total: Option<Decimal>,
    amount_paid: Option<Decimal>,
    balance_due: Option<Decimal>,
}

fn find_tax_lines(text: &str, page: u32) -> Vec<ProviderTaxLine> {
    let mut out = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        let lower = trimmed.to_ascii_lowercase();
        // A "Tax Total" or "Total Tax" line is a summary of the tax lines, not
        // another tax line. Including it would double-count tax in validation.
        if lower.starts_with("tax total")
            || lower.starts_with("total tax")
            || lower.starts_with("taxable")
            || lower.starts_with("subtotal")
            || lower.starts_with("total ")
            || lower == "total"
        {
            continue;
        }
        let Some(caps) = tax_line_re().captures(trimmed) else {
            continue;
        };
        let label = caps.get(1).map(|m| m.as_str()).unwrap_or("TAX");
        let upper = label.to_ascii_uppercase();
        let tax_type = match upper.as_str() {
            "SALES TAX" => "SALES_TAX",
            "SERVICE TAX" => "SERVICE_TAX",
            other => other,
        };
        let rate = caps.get(2).and_then(|m| Decimal::from_str(m.as_str()).ok());
        let amount = caps.get(3).and_then(|m| parse_amount(m.as_str()));
        if rate.is_none() && amount.is_none() {
            continue;
        }
        out.push(ProviderTaxLine {
            tax_type: Some(tax_type.to_string()),
            rate,
            taxable_amount: None,
            amount,
            source_page: Some(page as i32),
        });
    }
    out
}

fn find_line_items(text: &str, page: u32) -> Vec<ProviderLineItem> {
    let mut out = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        let lower = trimmed.to_ascii_lowercase();
        if lower.contains("total")
            || lower.contains("subtotal")
            || lower.contains("balance")
            || lower.contains("gstin")
            || lower.contains("invoice")
        {
            continue;
        }
        let Some(caps) = line_item_re().captures(trimmed) else {
            continue;
        };
        let description = caps.get(1).map(|m| m.as_str().trim().to_string());
        let quantity = caps
            .get(2)
            .and_then(|m| Decimal::from_str(&m.as_str().replace(',', "")).ok());
        let unit_price = caps.get(3).and_then(|m| parse_amount(m.as_str()));
        let line_total = caps.get(4).and_then(|m| parse_amount(m.as_str()));
        if description.is_none() || line_total.is_none() {
            continue;
        }
        out.push(ProviderLineItem {
            description,
            sku: None,
            quantity,
            unit_of_measure: None,
            unit_price,
            discount: None,
            tax_rate: None,
            tax_amount: None,
            line_total,
            source_page: Some(page as i32),
            raw_text: Some(truncate_chars(trimmed, 500)),
        });
    }
    out
}

impl MockExtractionProvider {
    fn parse(&self, input: &ExtractionInput) -> Result<ProviderInvoice, ExtractionError> {
        if input.pages.iter().all(|page| page.text.trim().is_empty()) {
            return Err(ExtractionError::InvalidDocument(
                "no text could be extracted from this document".into(),
            ));
        }
        let text = input.prompt_text(200_000);
        if text.trim().is_empty() {
            return Err(ExtractionError::InvalidDocument(
                "no text could be extracted from this document".into(),
            ));
        }
        let (kind, is_credit_note) = detect_document_kind(&text);
        let mut warnings = vec![MOCK_WARNING.to_string()];

        let vendor_name = find_vendor_name(&text);
        let invoice_number = find_invoice_number(&text);
        let (invoice_date, original_invoice_date) = match find_date(
            &text,
            &[
                "invoice date",
                "bill date",
                "date of invoice",
                "issued on",
                "issue date",
            ],
        ) {
            Some((iso, original)) => (Some(iso), Some(original)),
            None => (None, None),
        };
        let (due_date, original_due_date) =
            match find_date(&text, &["due date", "payment due", "due on", "pay by"]) {
                Some((iso, original)) => (Some(iso), Some(original)),
                None => (None, None),
            };

        let currency = currency_code_re()
            .captures(&text)
            .map(|c| c[1].to_ascii_uppercase())
            .or_else(|| {
                for (symbol, code) in [("₹", "INR"), ("€", "EUR"), ("£", "GBP"), ("¥", "JPY")]
                {
                    if text.contains(symbol) {
                        return Some(code.to_string());
                    }
                }
                if text.contains('$') {
                    warnings.push(
                        "Currency symbol '$' is ambiguous; confirm whether the invoice is in USD."
                            .to_string(),
                    );
                    return Some("USD".to_string());
                }
                None
            });

        let totals = find_totals(&text);
        let tax_lines = find_tax_lines(&text, 1);
        let line_items = find_line_items(&text, 1);

        let vendor_tax_id = gstin_re()
            .captures(&text)
            .map(|c| c[1].to_string())
            .or_else(|| {
                labelled_value(
                    &text,
                    &[
                        "gstin",
                        "gst no",
                        "tax id",
                        "vat no",
                        "vat reg no",
                        "abn",
                        "ein",
                    ],
                )
                .map(|v| truncate_chars(v.trim(), 60))
                .filter(|v| v.chars().any(|c| c.is_ascii_alphanumeric()))
            });
        let vendor_email = labelled_value(&text, &["email", "e-mail", "vendor email"])
            .and_then(|v| email_re().captures(v).map(|c| c[1].to_ascii_lowercase()));
        let payment_terms = labelled_value(&text, &["payment terms", "terms"]);
        let purchase_order = labelled_value(
            &text,
            &[
                "purchase order",
                "purchase order no",
                "po no",
                "po number",
                "po #",
            ],
        )
        .map(|v| truncate_chars(v.trim(), 60));

        let has_fields = invoice_number.is_some()
            || invoice_date.is_some()
            || totals.total.is_some()
            || !line_items.is_empty()
            || !tax_lines.is_empty();
        if kind == DocumentKind::Unknown && !has_fields {
            return Err(ExtractionError::InvalidDocument(
                "the document does not contain recognizable invoice fields".into(),
            ));
        }
        if kind == DocumentKind::Unknown {
            // A value was found but the document type is not stated. The domain
            // validation layer will require review instead of guessing.
            warnings.push("Document type was not stated on the page.".to_string());
        }

        let mut page_references = ProviderPageReference::default();
        page_references.vendor_name = vendor_name.as_ref().map(|_| 1);
        page_references.invoice_number = invoice_number.as_ref().map(|_| 1);
        page_references.invoice_date = invoice_date.as_ref().map(|_| 1);
        page_references.due_date = due_date.as_ref().map(|_| 1);
        page_references.currency = currency.as_ref().map(|_| 1);
        page_references.total = totals.total.map(|_| 1);

        if input.used_ocr {
            warnings.push(
                "Text was produced by OCR; verify amounts and identifiers against the image."
                    .to_string(),
            );
        }
        if let Some(quality) = input.ocr_quality {
            warnings.push(format!("OCR text quality signal: {quality:.2}."));
        }

        Ok(ProviderInvoice {
            document_kind: kind,
            vendor_name,
            vendor_tax_id,
            vendor_address: None,
            vendor_email,
            vendor_phone: None,
            invoice_number,
            invoice_date,
            due_date,
            purchase_order,
            currency,
            payment_terms: payment_terms.map(|v| truncate_chars(v.trim(), 120)),
            subtotal: totals.subtotal,
            discount: totals.discount,
            shipping: totals.shipping,
            tax_total: totals.tax_total,
            total: totals.total,
            amount_paid: totals.amount_paid,
            balance_due: totals.balance_due,
            is_credit_note,
            line_items,
            tax_lines,
            original_invoice_date,
            original_due_date,
            original_currency: None,
            page_references,
            extraction_quality: None,
            warnings,
        })
    }
}

#[async_trait]
impl ExtractionProvider for MockExtractionProvider {
    fn name(&self) -> &str {
        MOCK_PROVIDER_NAME
    }

    fn model(&self) -> &str {
        MOCK_MODEL_NAME
    }

    fn is_mock(&self) -> bool {
        true
    }

    async fn extract(&self, input: ExtractionInput) -> Result<ExtractionResult, ExtractionError> {
        let started = Utc::now();
        let parsed = self.parse(&input)?;
        let raw = serde_json::to_value(&parsed).map_err(|error| {
            ExtractionError::Rejected(format!("serialize mock output: {error}"))
        })?;
        let invoice = parsed
            .into_domain(&input.pages)
            .map_err(|_| ExtractionError::InvalidOutput)?;
        let duration_ms = (Utc::now() - started).num_milliseconds();
        Ok(ExtractionResult {
            invoice,
            raw,
            provider: MOCK_PROVIDER_NAME.to_string(),
            model: MOCK_MODEL_NAME.to_string(),
            is_mock: true,
            input_tokens: None,
            output_tokens: None,
            duration_ms,
            warnings: vec![MOCK_WARNING.to_string()],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::dec;

    fn sample_invoice() -> ExtractionInput {
        ExtractionInput {
            filename: "invoice.pdf".into(),
            country: "IN".into(),
            base_currency: "INR".into(),
            pages: vec![crate::PageTextInput {
                page_number: 1,
                text: "ACME SUPPLIES PVT LTD\n\
                       12 Industrial Road, Bengaluru\n\
                       GSTIN: 27AAPFU0939F1ZV\n\
                       TAX INVOICE\n\
                       Invoice No: ACME/2026/0042\n\
                       Invoice Date: 14 Mar 2026\n\
                       Due Date: 13 Apr 2026\n\
                       Payment Terms: Net 30\n\
                       Purchase Order: PO-9001\n\
                       Description              Qty   Unit Price    Amount\n\
                       Widget A                  2      50.00        100.00\n\
                       Widget B                  1     118.00        118.00\n\
                       Subtotal: INR 218.00\n\
                       CGST 9% 19.62\n\
                       SGST 9% 19.62\n\
                       Total: INR 257.24\n\
                       Currency: INR\n"
                    .into(),
            }],
            used_ocr: false,
            ocr_quality: None,
        }
    }

    #[tokio::test]
    async fn parses_labelled_fields_without_inventing_values() {
        let provider = MockExtractionProvider;
        assert!(provider.is_mock());
        let result = provider.extract(sample_invoice()).await.unwrap();
        let invoice = &result.invoice;
        assert_eq!(
            invoice.vendor.name.as_deref(),
            Some("ACME SUPPLIES PVT LTD")
        );
        assert_eq!(invoice.vendor.tax_id.as_deref(), Some("27AAPFU0939F1ZV"));
        assert_eq!(invoice.invoice_number.as_deref(), Some("ACME/2026/0042"));
        assert_eq!(invoice.invoice_date.as_deref(), Some("2026-03-14"));
        assert_eq!(invoice.due_date.as_deref(), Some("2026-04-13"));
        assert_eq!(invoice.currency.as_deref(), Some("INR"));
        assert_eq!(invoice.payment_terms.as_deref(), Some("Net 30"));
        assert_eq!(invoice.purchase_order.as_deref(), Some("PO-9001"));
        assert_eq!(invoice.subtotal, Some(dec!(218.00)));
        assert_eq!(invoice.total, Some(dec!(257.24)));
        assert_eq!(invoice.tax_lines.len(), 2);
        assert_eq!(invoice.tax_lines[0].tax_type.as_deref(), Some("CGST"));
        assert_eq!(invoice.tax_lines[0].rate, Some(dec!(9)));
        assert_eq!(invoice.tax_lines[0].amount, Some(dec!(19.62)));
        assert_eq!(invoice.line_items.len(), 2);
        assert_eq!(
            invoice.line_items[0].description.as_deref(),
            Some("Widget A")
        );
        assert_eq!(invoice.line_items[0].quantity, Some(dec!(2)));
        assert_eq!(invoice.line_items[0].unit_price, Some(dec!(50.00)));
        assert_eq!(invoice.line_items[0].line_total, Some(dec!(100.00)));
        assert_eq!(
            invoice.field_sources.get("invoice_number").unwrap().page,
            Some(1)
        );
        assert!(result.is_mock);
        assert_eq!(result.provider, MOCK_PROVIDER_NAME);
        assert!(result.warnings.iter().any(|w| w.contains("Mock extractor")));
        assert!(invoice.field_confidence.is_empty());
    }

    #[tokio::test]
    async fn tax_total_summary_is_not_counted_as_a_tax_line() {
        let mut input = sample_invoice();
        input.pages[0]
            .text
            .push_str("Tax Total: INR 39.24\nTotal Tax: 39.24\n");
        let result = MockExtractionProvider.extract(input).await.unwrap();
        assert_eq!(result.invoice.tax_lines.len(), 2);
        assert_eq!(result.invoice.tax_total, Some(dec!(39.24)));
    }

    #[tokio::test]
    async fn missing_values_stay_missing() {
        let input = ExtractionInput {
            filename: "receipt.txt".into(),
            country: "US".into(),
            base_currency: "USD".into(),
            pages: vec![crate::PageTextInput {
                page_number: 1,
                text: "Corner Cafe\nRECEIPT\nTotal: 12.50\n".into(),
            }],
            used_ocr: false,
            ocr_quality: None,
        };
        let result = MockExtractionProvider.extract(input).await.unwrap();
        assert_eq!(result.invoice.total, Some(dec!(12.50)));
        assert_eq!(result.invoice.invoice_number, None);
        assert_eq!(result.invoice.invoice_date, None);
        assert_eq!(result.invoice.document_kind, DocumentKind::Receipt);
    }

    #[tokio::test]
    async fn empty_or_unrecognizable_text_is_rejected() {
        let mut input = sample_invoice();
        input.pages[0].text = "   ".into();
        assert!(matches!(
            MockExtractionProvider.extract(input).await,
            Err(ExtractionError::InvalidDocument(_))
        ));

        let mut input = sample_invoice();
        input.pages[0].text = "Hello world\nNothing financial here".into();
        assert!(matches!(
            MockExtractionProvider.extract(input).await,
            Err(ExtractionError::InvalidDocument(_))
        ));
    }

    #[test]
    fn parses_parenthesised_negative_amounts() {
        assert_eq!(parse_amount("(1,234.50)"), Some(dec!(-1234.50)));
        assert_eq!(parse_amount("₹ 2,000"), Some(dec!(2000)));
        assert_eq!(parse_amount("not money"), None);
    }
}
