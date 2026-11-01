use chrono::{NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DuplicateKind {
    ExactFile,
    ProbableInvoice,
    Similar,
}

impl DuplicateKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ExactFile => "exact_file",
            Self::ProbableInvoice => "probable_invoice",
            Self::Similar => "similar",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExistingInvoice {
    pub id: Uuid,
    pub file_hash: Option<String>,
    pub vendor_key: Option<String>,
    pub invoice_number_key: Option<String>,
    pub currency: Option<String>,
    pub total: Option<Decimal>,
    pub invoice_date: Option<NaiveDate>,
}

#[derive(Debug, Clone)]
pub struct CandidateInvoice<'a> {
    pub file_hash: Option<&'a str>,
    pub vendor_key: Option<&'a str>,
    pub invoice_number_key: Option<&'a str>,
    pub currency: Option<&'a str>,
    pub total: Option<Decimal>,
    pub invoice_date: Option<NaiveDate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateMatch {
    pub existing_invoice_id: Uuid,
    pub kind: DuplicateKind,
    pub score: f64,
    pub reasons: Vec<String>,
}

/// Layered duplicate detection. Similar amounts/dates alone never reject an
/// invoice; they only create a reviewable candidate. The caller resolves it.
pub fn detect_duplicates(
    candidate: &CandidateInvoice<'_>,
    existing: &[ExistingInvoice],
) -> Vec<DuplicateMatch> {
    let mut out: Vec<DuplicateMatch> = Vec::new();
    for other in existing {
        let mut kind: Option<DuplicateKind> = None;
        let mut score: f64 = 0.0;
        let mut reasons: Vec<String> = Vec::new();

        if let (Some(a), Some(b)) = (candidate.file_hash, other.file_hash.as_deref()) {
            if !a.is_empty() && a.eq_ignore_ascii_case(b) {
                kind = Some(DuplicateKind::ExactFile);
                score = 1.0;
                reasons.push("Identical file content hash".into());
            }
        }

        let same_vendor = matches!(
            (candidate.vendor_key, other.vendor_key.as_deref()),
            (Some(a), Some(b)) if !a.is_empty() && a == b
        );
        let same_number = matches!(
            (candidate.invoice_number_key, other.invoice_number_key.as_deref()),
            (Some(a), Some(b)) if !a.is_empty() && a == b
        );
        if same_vendor && same_number {
            if kind.is_none() || kind == Some(DuplicateKind::Similar) {
                kind = Some(DuplicateKind::ProbableInvoice);
                score = score.max(0.95);
            }
            reasons.push("Same normalized vendor and invoice number".into());
        }

        let same_currency = match (candidate.currency, other.currency.as_deref()) {
            (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
            _ => false,
        };
        let same_amount = match (candidate.total, other.total) {
            (Some(a), Some(b)) => a == b,
            _ => false,
        };
        let date_close = match (candidate.invoice_date, other.invoice_date) {
            (Some(a), Some(b)) => (a - b).num_days().abs() <= 7,
            _ => false,
        };

        if same_amount && same_currency && date_close && kind != Some(DuplicateKind::ExactFile) {
            if !same_number {
                kind = Some(DuplicateKind::Similar);
            }
            score = score.max(0.7);
            reasons.push("Same amount and currency within 7 days".into());
        }
        if same_vendor && same_amount && same_currency && kind.is_none() {
            kind = Some(DuplicateKind::Similar);
            score = score.max(0.6);
            reasons.push("Same vendor, amount and currency".into());
        }

        if let Some(kind) = kind {
            reasons.dedup();
            out.push(DuplicateMatch {
                existing_invoice_id: other.id,
                kind,
                score,
                reasons,
            });
        }
    }
    out.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out.dedup_by_key(|m| m.existing_invoice_id);
    out
}

pub fn duplicate_status_label(kind: DuplicateKind) -> &'static str {
    match kind {
        DuplicateKind::ExactFile => "Exact duplicate file",
        DuplicateKind::ProbableInvoice => "Probable duplicate invoice",
        DuplicateKind::Similar => "Similar — verify before rejecting",
    }
}

pub fn now_utc() -> chrono::DateTime<Utc> {
    Utc::now()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::dec;

    fn existing(id: Uuid) -> ExistingInvoice {
        ExistingInvoice {
            id,
            file_hash: Some("abc".into()),
            vendor_key: Some("acme supplies".into()),
            invoice_number_key: Some("INV1001".into()),
            currency: Some("INR".into()),
            total: Some(dec!(1180)),
            invoice_date: NaiveDate::from_ymd_opt(2026, 3, 1),
        }
    }

    #[test]
    fn exact_file_is_highest_confidence() {
        let id = Uuid::new_v4();
        let candidate = CandidateInvoice {
            file_hash: Some("abc"),
            vendor_key: Some("other vendor"),
            invoice_number_key: Some("XYZ"),
            currency: None,
            total: None,
            invoice_date: None,
        };
        let matches = detect_duplicates(&candidate, &[existing(id)]);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].kind, DuplicateKind::ExactFile);
        assert_eq!(matches[0].score, 1.0);
    }

    #[test]
    fn vendor_and_number_is_probable() {
        let id = Uuid::new_v4();
        let candidate = CandidateInvoice {
            file_hash: Some("different"),
            vendor_key: Some("acme supplies"),
            invoice_number_key: Some("INV1001"),
            currency: Some("INR"),
            total: Some(dec!(1180)),
            invoice_date: NaiveDate::from_ymd_opt(2026, 3, 1),
        };
        let matches = detect_duplicates(&candidate, &[existing(id)]);
        assert_eq!(matches[0].kind, DuplicateKind::ProbableInvoice);
    }

    #[test]
    fn similar_amounts_do_not_claim_exact_duplicate() {
        let id = Uuid::new_v4();
        let candidate = CandidateInvoice {
            file_hash: Some("different"),
            vendor_key: Some("acme supplies"),
            invoice_number_key: Some("INV-9999"),
            currency: Some("INR"),
            total: Some(dec!(1180)),
            invoice_date: NaiveDate::from_ymd_opt(2026, 3, 2),
        };
        let matches = detect_duplicates(&candidate, &[existing(id)]);
        assert_eq!(matches[0].kind, DuplicateKind::Similar);
        assert!(matches[0].score < 0.95);
    }

    #[test]
    fn no_candidate_for_unrelated_invoice() {
        let candidate = CandidateInvoice {
            file_hash: Some("zzz"),
            vendor_key: Some("other"),
            invoice_number_key: Some("A1"),
            currency: Some("USD"),
            total: Some(dec!(42)),
            invoice_date: NaiveDate::from_ymd_opt(2026, 1, 1),
        };
        assert!(detect_duplicates(&candidate, &[existing(Uuid::new_v4())]).is_empty());
    }
}
