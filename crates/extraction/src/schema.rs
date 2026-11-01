use invoxa_domain::invoice::{
    DocumentKind, ExtractedInvoice, FieldSource, LineItem, TaxLine, VendorInfo,
};
use invoxa_domain::normalization::{normalize_currency, truncate_chars};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

fn default_kind() -> DocumentKind {
    DocumentKind::Unknown
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderLineItem {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderTaxLine {
    pub tax_type: Option<String>,
    pub rate: Option<Decimal>,
    pub taxable_amount: Option<Decimal>,
    pub amount: Option<Decimal>,
    pub source_page: Option<i32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderPageReference {
    pub vendor_name: Option<i32>,
    pub invoice_number: Option<i32>,
    pub invoice_date: Option<i32>,
    pub due_date: Option<i32>,
    pub currency: Option<i32>,
    pub total: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderInvoice {
    #[serde(default = "default_kind")]
    pub document_kind: DocumentKind,
    pub vendor_name: Option<String>,
    pub vendor_tax_id: Option<String>,
    pub vendor_address: Option<String>,
    pub vendor_email: Option<String>,
    pub vendor_phone: Option<String>,
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
    #[serde(default)]
    pub is_credit_note: bool,
    #[serde(default)]
    pub line_items: Vec<ProviderLineItem>,
    #[serde(default)]
    pub tax_lines: Vec<ProviderTaxLine>,
    pub original_invoice_date: Option<String>,
    pub original_due_date: Option<String>,
    pub original_currency: Option<String>,
    #[serde(default)]
    pub page_references: ProviderPageReference,
    pub extraction_quality: Option<f64>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

impl Default for ProviderInvoice {
    fn default() -> Self {
        Self {
            document_kind: DocumentKind::Unknown,
            vendor_name: None,
            vendor_tax_id: None,
            vendor_address: None,
            vendor_email: None,
            vendor_phone: None,
            invoice_number: None,
            invoice_date: None,
            due_date: None,
            purchase_order: None,
            currency: None,
            payment_terms: None,
            subtotal: None,
            discount: None,
            shipping: None,
            tax_total: None,
            total: None,
            amount_paid: None,
            balance_due: None,
            is_credit_note: false,
            line_items: Vec::new(),
            tax_lines: Vec::new(),
            original_invoice_date: None,
            original_due_date: None,
            original_currency: None,
            page_references: ProviderPageReference::default(),
            extraction_quality: None,
            warnings: Vec::new(),
        }
    }
}

impl ProviderInvoice {
    pub fn into_domain(
        self,
        pages: &[super::PageTextInput],
    ) -> Result<ExtractedInvoice, Vec<String>> {
        let clean = |value: Option<String>| {
            value
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        };

        let mut original_values = BTreeMap::new();
        if let Some(v) = clean(self.original_invoice_date.clone()) {
            original_values.insert("invoice_date".to_string(), truncate_chars(&v, 120));
        }
        if let Some(v) = clean(self.original_due_date.clone()) {
            original_values.insert("due_date".to_string(), truncate_chars(&v, 120));
        }
        if let Some(v) = clean(self.original_currency.clone()) {
            original_values.insert("currency".to_string(), truncate_chars(&v, 60));
        }

        let mut field_sources = BTreeMap::new();
        let lookups = [
            (
                "vendor.name",
                self.vendor_name.as_deref(),
                self.page_references.vendor_name,
            ),
            (
                "invoice_number",
                self.invoice_number.as_deref(),
                self.page_references.invoice_number,
            ),
            (
                "invoice_date",
                self.invoice_date.as_deref(),
                self.page_references.invoice_date,
            ),
            (
                "due_date",
                self.due_date.as_deref(),
                self.page_references.due_date,
            ),
            (
                "currency",
                self.currency.as_deref(),
                self.page_references.currency,
            ),
            ("total", None, self.page_references.total),
        ];
        for (field, value, page_hint) in lookups {
            if let Some(source) = find_source(pages, value, page_hint) {
                field_sources.insert(field.to_string(), source);
            }
        }

        let invoice = ExtractedInvoice {
            document_kind: self.document_kind,
            vendor: VendorInfo {
                name: clean(self.vendor_name),
                tax_id: clean(self.vendor_tax_id),
                address: clean(self.vendor_address),
                email: clean(self.vendor_email),
                phone: clean(self.vendor_phone),
            },
            invoice_number: clean(self.invoice_number),
            invoice_date: clean(self.invoice_date),
            due_date: clean(self.due_date),
            purchase_order: clean(self.purchase_order),
            currency: clean(self.currency),
            payment_terms: clean(self.payment_terms),
            subtotal: self.subtotal,
            discount: self.discount,
            shipping: self.shipping,
            tax_total: self.tax_total,
            total: self.total,
            amount_paid: self.amount_paid,
            balance_due: self.balance_due,
            is_credit_note: self.is_credit_note,
            line_items: self
                .line_items
                .into_iter()
                .map(|item| LineItem {
                    description: clean(item.description),
                    sku: clean(item.sku),
                    quantity: item.quantity,
                    unit_of_measure: clean(item.unit_of_measure),
                    unit_price: item.unit_price,
                    discount: item.discount,
                    tax_rate: item.tax_rate,
                    tax_amount: item.tax_amount,
                    line_total: item.line_total,
                    source_page: item.source_page,
                    raw_text: item.raw_text.map(|v| truncate_chars(&v, 500)),
                })
                .collect(),
            tax_lines: self
                .tax_lines
                .into_iter()
                .map(|tax| TaxLine {
                    tax_type: clean(tax.tax_type).map(|t| t.to_ascii_uppercase()),
                    rate: tax.rate,
                    taxable_amount: tax.taxable_amount,
                    amount: tax.amount,
                    source_page: tax.source_page,
                })
                .collect(),
            original_values,
            field_sources,
            field_confidence: BTreeMap::new(),
            extraction_quality: self.extraction_quality.map(|q| q.clamp(0.0, 1.0)),
            warnings: self
                .warnings
                .into_iter()
                .map(|w| truncate_chars(&w, 300))
                .collect(),
        };
        invoice.validate_shape()?;

        // Normalize recognized currency codes while preserving the original.
        let mut invoice = invoice;
        if let Some(currency) = invoice.currency.as_deref().and_then(normalize_currency) {
            invoice.currency = Some(currency);
        }
        Ok(invoice)
    }

    pub fn strict_json_schema() -> serde_json::Value {
        use serde_json::json;
        let nullable_string = || json!({"type": ["string", "null"]});
        let nullable_decimal = || {
            json!({
                "type": ["string", "null"],
                "description": "Exact decimal amount as a string, for example \"1234.50\". Never use floating point."
            })
        };
        let line_item = json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["description","sku","quantity","unit_of_measure","unit_price","discount","tax_rate","tax_amount","line_total","source_page","raw_text"],
            "properties": {
                "description": nullable_string(),
                "sku": nullable_string(),
                "quantity": nullable_decimal(),
                "unit_of_measure": nullable_string(),
                "unit_price": nullable_decimal(),
                "discount": nullable_decimal(),
                "tax_rate": nullable_decimal(),
                "tax_amount": nullable_decimal(),
                "line_total": nullable_decimal(),
                "source_page": {"type": ["integer","null"]},
                "raw_text": nullable_string()
            }
        });
        let tax_line = json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["tax_type","rate","taxable_amount","amount","source_page"],
            "properties": {
                "tax_type": {"type": ["string","null"], "description": "CGST, SGST, IGST, VAT, GST, SALES_TAX or OTHER"},
                "rate": nullable_decimal(),
                "taxable_amount": nullable_decimal(),
                "amount": nullable_decimal(),
                "source_page": {"type": ["integer","null"]}
            }
        });
        json!({
            "type": "object",
            "additionalProperties": false,
            "required": [
                "document_kind","vendor_name","vendor_tax_id","vendor_address","vendor_email","vendor_phone",
                "invoice_number","invoice_date","due_date","purchase_order","currency","payment_terms",
                "subtotal","discount","shipping","tax_total","total","amount_paid","balance_due",
                "is_credit_note","line_items","tax_lines","original_invoice_date","original_due_date",
                "original_currency","page_references","extraction_quality","warnings"
            ],
            "properties": {
                "document_kind": {
                    "type": "string",
                    "enum": ["supplier_invoice","credit_note","receipt","purchase_order","statement","other_financial","unknown"]
                },
                "vendor_name": nullable_string(),
                "vendor_tax_id": nullable_string(),
                "vendor_address": nullable_string(),
                "vendor_email": nullable_string(),
                "vendor_phone": nullable_string(),
                "invoice_number": nullable_string(),
                "invoice_date": {"type": ["string","null"], "description": "ISO date YYYY-MM-DD, or null if not present. Never infer."},
                "due_date": {"type": ["string","null"]},
                "purchase_order": nullable_string(),
                "currency": {"type": ["string","null"], "description": "ISO 4217 code exactly as indicated, or null."},
                "payment_terms": nullable_string(),
                "subtotal": nullable_decimal(),
                "discount": nullable_decimal(),
                "shipping": nullable_decimal(),
                "tax_total": nullable_decimal(),
                "total": nullable_decimal(),
                "amount_paid": nullable_decimal(),
                "balance_due": nullable_decimal(),
                "is_credit_note": {"type": "boolean"},
                "line_items": {"type": "array", "items": line_item},
                "tax_lines": {"type": "array", "items": tax_line},
                "original_invoice_date": {"type": ["string","null"], "description": "Original date text from the document, if present."},
                "original_due_date": {"type": ["string","null"]},
                "original_currency": {"type": ["string","null"]},
                "page_references": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["vendor_name","invoice_number","invoice_date","due_date","currency","total"],
                    "properties": {
                        "vendor_name": {"type": ["integer","null"]},
                        "invoice_number": {"type": ["integer","null"]},
                        "invoice_date": {"type": ["integer","null"]},
                        "due_date": {"type": ["integer","null"]},
                        "currency": {"type": ["integer","null"]},
                        "total": {"type": ["integer","null"]}
                    }
                },
                "extraction_quality": {"type": ["number","null"], "description": "Uncalibrated extraction quality between 0 and 1. Never treat as a probability of correctness."},
                "warnings": {"type": "array", "items": {"type": "string"}}
            }
        })
    }
}

fn find_source(
    pages: &[super::PageTextInput],
    value: Option<&str>,
    page_hint: Option<i32>,
) -> Option<FieldSource> {
    let value = value.unwrap_or_default().trim();
    let mut candidates = Vec::new();
    if !value.is_empty() {
        candidates.push(value.to_string());
        let plain = value.replace(',', "");
        if plain != value {
            candidates.push(plain);
        }
        if let Ok(amount) = value.parse::<Decimal>() {
            candidates.push(format!("{amount:.2}"));
        }
    }
    for page in pages {
        if let Some(hint) = page_hint {
            if page.page_number as i32 != hint {
                continue;
            }
        }
        for candidate in &candidates {
            if candidate.is_empty() {
                continue;
            }
            if let Some(pos) = page.text.find(candidate) {
                let start = page.text[..pos].rfind('\n').map(|i| i + 1).unwrap_or(0);
                let end = page.text[pos..]
                    .find('\n')
                    .map(|i| pos + i)
                    .unwrap_or_else(|| page.text.len());
                return Some(FieldSource {
                    page: Some(page.page_number as i32),
                    text: Some(truncate_chars(page.text[start..end].trim(), 300)),
                    bbox: None,
                });
            }
        }
    }
    page_hint.map(|page| FieldSource {
        page: Some(page),
        text: None,
        bbox: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PageTextInput;

    #[test]
    fn maps_provider_json_and_unknown_fields_are_rejected() {
        let json = serde_json::json!({
            "document_kind": "supplier_invoice",
            "vendor_name": "Acme",
            "vendor_tax_id": null,
            "vendor_address": null,
            "vendor_email": null,
            "vendor_phone": null,
            "invoice_number": "INV-1",
            "invoice_date": "2026-03-01",
            "due_date": null,
            "purchase_order": null,
            "currency": "INR",
            "payment_terms": null,
            "subtotal": "100.00",
            "discount": null,
            "shipping": null,
            "tax_total": "18.00",
            "total": "118.00",
            "amount_paid": null,
            "balance_due": null,
            "is_credit_note": false,
            "line_items": [],
            "tax_lines": [],
            "original_invoice_date": "01/03/2026",
            "original_due_date": null,
            "original_currency": "INR",
            "page_references": {
                "vendor_name": 1, "invoice_number": 1, "invoice_date": 1,
                "due_date": null, "currency": 1, "total": 1
            },
            "extraction_quality": 0.9,
            "warnings": [],
            "unknown": true
        });
        assert!(serde_json::from_value::<ProviderInvoice>(json).is_err());

        let json = serde_json::json!({
            "document_kind": "supplier_invoice",
            "vendor_name": "Acme",
            "vendor_tax_id": null,
            "vendor_address": null,
            "vendor_email": null,
            "vendor_phone": null,
            "invoice_number": "INV-1",
            "invoice_date": "2026-03-01",
            "due_date": null,
            "purchase_order": null,
            "currency": "INR",
            "payment_terms": null,
            "subtotal": "100.00",
            "discount": null,
            "shipping": null,
            "tax_total": "18.00",
            "total": "118.00",
            "amount_paid": null,
            "balance_due": null,
            "is_credit_note": false,
            "line_items": [],
            "tax_lines": [],
            "original_invoice_date": "01/03/2026",
            "original_due_date": null,
            "original_currency": "INR",
            "page_references": {
                "vendor_name": 1, "invoice_number": 1, "invoice_date": 1,
                "due_date": null, "currency": 1, "total": 1
            },
            "extraction_quality": 0.9,
            "warnings": []
        });
        let provider: ProviderInvoice = serde_json::from_value(json).unwrap();
        let pages = vec![PageTextInput {
            page_number: 1,
            text: "Acme\nInvoice No: INV-1\nTotal 118.00".into(),
        }];
        let invoice = provider.into_domain(&pages).unwrap();
        assert_eq!(invoice.vendor.name.as_deref(), Some("Acme"));
        assert_eq!(invoice.field_sources.get("total").unwrap().page, Some(1));
        assert_eq!(
            invoice.original_values.get("invoice_date").unwrap(),
            "01/03/2026"
        );
    }
}
