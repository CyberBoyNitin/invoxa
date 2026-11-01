use chrono::{Datelike, NaiveDate};
use rust_decimal::Decimal;
use std::collections::BTreeSet;
use std::str::FromStr;

/// Normalized vendor name used for display comparisons.
pub fn normalize_vendor_name(name: &str) -> String {
    let lower = name.to_lowercase();
    let cleaned: String = lower
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect();
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Aggressive key for duplicate matching. Legal suffixes and punctuation are
/// removed, but the original vendor name is never overwritten.
pub fn vendor_match_key(name: &str) -> String {
    const SUFFIXES: &[&str] = &[
        "private limited",
        "pvt ltd",
        "pvt. ltd.",
        "p ltd",
        "limited",
        "ltd",
        "llp",
        "llc",
        "inc",
        "incorporated",
        "corporation",
        "corp",
        "company",
        "co",
        "gmbh",
        "sarl",
        "sas",
        "bv",
        "nv",
        "ag",
        "oy",
        "ab",
        "plc",
        "pte ltd",
        "pte",
        "pty ltd",
        "pty",
    ];
    let mut key = normalize_vendor_name(name);
    for suffix in SUFFIXES {
        let with_space = format!(" {}", suffix);
        if key.ends_with(&with_space) {
            key.truncate(key.len() - with_space.len());
            key = key.trim().to_string();
        }
    }
    key.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn normalize_invoice_number(number: &str) -> String {
    number
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_uppercase()
}

pub fn normalize_tax_id(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_uppercase()
}

pub fn normalize_currency(value: &str) -> Option<String> {
    let cleaned: String = value
        .trim()
        .chars()
        .filter(|c| c.is_ascii_alphabetic())
        .collect::<String>()
        .to_ascii_uppercase();
    if cleaned.len() == 3 {
        return Some(cleaned);
    }
    match value.trim().to_ascii_lowercase().as_str() {
        "dollar" | "dollars" | "us dollar" | "us dollars" => return Some("USD".into()),
        "rupee" | "rupees" | "indian rupee" | "indian rupees" => return Some("INR".into()),
        "euro" | "euros" => return Some("EUR".into()),
        "pound" | "pounds" | "sterling" => return Some("GBP".into()),
        "yen" => return Some("JPY".into()),
        _ => {}
    }
    detect_currency_symbol(value)
}

/// Detect only unambiguous common symbols. `$` is treated as USD but flagged
/// ambiguous by the validation layer because it is used by many currencies.
pub fn detect_currency_symbol(value: &str) -> Option<String> {
    let v = value.trim();
    let map = [
        ("₹", "INR"),
        ("€", "EUR"),
        ("£", "GBP"),
        ("¥", "JPY"),
        ("₩", "KRW"),
        ("₦", "NGN"),
        ("₽", "RUB"),
        ("R$", "BRL"),
        ("A$", "AUD"),
        ("C$", "CAD"),
        ("S$", "SGD"),
        ("HK$", "HKD"),
        ("NZ$", "NZD"),
    ];
    for (symbol, code) in map {
        if v.contains(symbol) {
            return Some(code.to_string());
        }
    }
    if v.contains('$') {
        return Some("USD".into());
    }
    None
}

pub fn is_known_currency(code: &str) -> bool {
    const CODES: &[&str] = &[
        "AED", "AFN", "AUD", "BDT", "BGN", "BHD", "BRL", "CAD", "CHF", "CLP", "CNY", "COP", "CZK",
        "DKK", "EGP", "EUR", "GBP", "GHS", "HKD", "HUF", "IDR", "ILS", "INR", "ISK", "JOD", "JPY",
        "KES", "KRW", "KWD", "LKR", "MAD", "MXN", "MYR", "NGN", "NOK", "NPR", "NZD", "OMR", "PEN",
        "PHP", "PKR", "PLN", "QAR", "RON", "RUB", "SAR", "SEK", "SGD", "THB", "TRY", "TWD", "UAH",
        "USD", "VND", "ZAR",
    ];
    CODES.contains(&code.to_ascii_uppercase().as_str())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedDate {
    pub date: NaiveDate,
    pub ambiguous: bool,
    pub original: String,
}

/// Parse dates found in invoices. Day-first is applied to genuinely ambiguous
/// numeric dates (common in India and much of the world) and the ambiguity is
/// surfaced as a warning rather than silently assumed.
pub fn parse_invoice_date(input: &str) -> Result<ParsedDate, String> {
    let original = input.trim().to_string();
    if original.is_empty() {
        return Err("empty date".into());
    }
    let cleaned = original
        .trim_matches(|c: char| c == '.' || c == ',' || c.is_whitespace())
        .to_string();

    for fmt in ["%Y-%m-%d", "%Y/%m/%d", "%Y.%m.%d"] {
        if let Ok(date) = NaiveDate::parse_from_str(&cleaned, fmt) {
            return Ok(ParsedDate {
                date,
                ambiguous: false,
                original,
            });
        }
    }
    for fmt in ["%d %b %Y", "%d %B %Y", "%d-%b-%Y", "%d %b, %Y", "%d-%B-%Y"] {
        if let Ok(date) = NaiveDate::parse_from_str(&cleaned, fmt) {
            return Ok(ParsedDate {
                date,
                ambiguous: false,
                original,
            });
        }
    }
    for fmt in ["%b %d, %Y", "%B %d, %Y", "%b %d %Y", "%B %d %Y"] {
        if let Ok(date) = NaiveDate::parse_from_str(&cleaned, fmt) {
            return Ok(ParsedDate {
                date,
                ambiguous: false,
                original,
            });
        }
    }

    let numeric: Vec<u32> = cleaned
        .split(|c| c == '/' || c == '-' || c == '.')
        .map(|p| p.trim().parse::<u32>())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "unrecognized date".to_string())?;
    if numeric.len() != 3 {
        return Err("unrecognized date".into());
    }
    let (a, b) = (numeric[0], numeric[1]);
    // A four-digit leading component is year-first.
    if numeric[0] > 31 {
        let (y, m, d) = (numeric[0] as i32, b, numeric[2]);
        return NaiveDate::from_ymd_opt(y, m, d)
            .map(|date| ParsedDate {
                date,
                ambiguous: false,
                original,
            })
            .ok_or_else(|| "impossible date".into());
    }
    let year = if numeric[2] < 100 {
        if numeric[2] >= 70 {
            1900 + numeric[2]
        } else {
            2000 + numeric[2]
        }
    } else {
        numeric[2]
    } as i32;

    let day_first_valid = NaiveDate::from_ymd_opt(year, b, a).is_some();
    let month_first_valid = NaiveDate::from_ymd_opt(year, a, b).is_some();
    let ambiguous = a <= 12 && b <= 12 && a != b && day_first_valid && month_first_valid;

    let date = if day_first_valid {
        NaiveDate::from_ymd_opt(year, b, a).unwrap()
    } else if month_first_valid {
        NaiveDate::from_ymd_opt(year, a, b).unwrap()
    } else {
        return Err("impossible date".into());
    };
    Ok(ParsedDate {
        date,
        ambiguous,
        original,
    })
}

pub fn date_to_iso(date: NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}

pub fn years_between(a: NaiveDate, b: NaiveDate) -> i64 {
    let mut years = b.year() - a.year();
    if (b.month(), b.day()) < (a.month(), a.day()) {
        years -= 1;
    }
    i64::from(years)
}

/// GSTIN checksum validation per the GSTN specification. Supports 15-character
/// Indian GSTINs only; it does not claim to verify that a registration is active.
pub fn validate_gstin_format(value: &str) -> Result<(), String> {
    let s = value.trim().to_ascii_uppercase();
    if s.len() != 15 {
        return Err("GSTIN must be 15 characters".into());
    }
    let bytes = s.as_bytes();
    let valid_char = |b: u8| b.is_ascii_digit() || (b'A'..=b'Z').contains(&b);
    if !bytes.iter().all(|b| valid_char(*b)) {
        return Err("GSTIN contains invalid characters".into());
    }
    if !bytes[..2].iter().all(|b| b.is_ascii_digit()) {
        return Err("GSTIN state code must be numeric".into());
    }
    let state: u32 = s[..2].parse().map_err(|_| "invalid state code")?;
    if !(1..=38).contains(&state) {
        return Err("GSTIN state code is not recognised".into());
    }
    if !bytes[2..7].iter().all(|b| b.is_ascii_uppercase()) {
        return Err("GSTIN PAN segment must be letters".into());
    }
    if !bytes[7..11].iter().all(|b| b.is_ascii_digit()) {
        return Err("GSTIN PAN digits are invalid".into());
    }
    if !bytes[11].is_ascii_uppercase() {
        return Err("GSTIN PAN letter is invalid".into());
    }
    if !bytes[12].is_ascii_alphanumeric() || bytes[13] != b'Z' {
        return Err("GSTIN entity/Z segment is invalid".into());
    }
    if s.as_bytes()[15 - 1] != checksum_char(&s[..14]) as u8 {
        return Err("GSTIN checksum is invalid".into());
    }
    Ok(())
}

fn char_value(c: char) -> u32 {
    c.to_digit(36).unwrap_or(0)
}

fn checksum_char(first14: &str) -> char {
    let chars: Vec<char> = first14.chars().collect();
    let mut sum = 0u32;
    for (i, c) in chars.iter().enumerate() {
        let factor = if i % 2 == 0 { 1 } else { 2 };
        let product = char_value(*c) * factor;
        sum += product / 36 + product % 36;
    }
    let remainder = sum % 36;
    let check_value = (36 - remainder) % 36;
    std::char::from_digit(check_value, 36)
        .unwrap_or('0')
        .to_ascii_uppercase()
}

pub fn unique_uppercase(values: &[&str]) -> BTreeSet<String> {
    values.iter().map(|v| v.to_ascii_uppercase()).collect()
}

pub fn decimal_or_zero(value: Option<Decimal>) -> Decimal {
    value.unwrap_or(Decimal::ZERO)
}

pub fn parse_boolish(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "yes" | "y" | "1" => Some(true),
        "false" | "no" | "n" | "0" => Some(false),
        _ => None,
    }
}

pub fn truncate_chars(value: &str, max: usize) -> String {
    value.chars().take(max).collect()
}

pub fn is_blank(value: &str) -> bool {
    value.trim().is_empty()
}

pub fn parse_decimal_text(value: &str) -> Option<Decimal> {
    let cleaned: String = value
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.' || *c == '-' || *c == ',')
        .collect();
    Decimal::from_str(&cleaned).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_vendor_names_for_matching() {
        assert_eq!(vendor_match_key("Acme Supplies Pvt. Ltd."), "acme supplies");
        assert_eq!(vendor_match_key("ACME  SUPPLIES LLC"), "acme supplies");
        assert_eq!(normalize_vendor_name("  Rävi & Sons  "), "rävi sons");
    }

    #[test]
    fn parses_unambiguous_dates() {
        let d = parse_invoice_date("2026-03-14").unwrap();
        assert_eq!(d.date.to_string(), "2026-03-14");
        assert!(!d.ambiguous);
        let d = parse_invoice_date("14 Mar 2026").unwrap();
        assert_eq!(d.date.to_string(), "2026-03-14");
        assert!(!d.ambiguous);
        let d = parse_invoice_date("25/03/2026").unwrap();
        assert_eq!(d.date.to_string(), "2026-03-25");
        assert!(!d.ambiguous);
    }

    #[test]
    fn flags_ambiguous_dates_instead_of_hiding_them() {
        let d = parse_invoice_date("03/04/2026").unwrap();
        assert_eq!(d.date.to_string(), "2026-04-03");
        assert!(d.ambiguous);
    }

    #[test]
    fn rejects_impossible_dates() {
        assert!(parse_invoice_date("31/02/2026").is_err());
        assert!(parse_invoice_date("2026-13-01").is_err());
    }

    #[test]
    fn validates_known_gstin() {
        // Public sample GSTIN used by GST documentation.
        assert!(validate_gstin_format("27AAPFU0939F1ZV").is_ok());
        assert!(validate_gstin_format("27AAPFU0939F1ZX").is_err());
        assert!(validate_gstin_format("27AAPFU0939F1Z").is_err());
    }

    #[test]
    fn currency_detection_is_limited() {
        assert_eq!(normalize_currency("inr").as_deref(), Some("INR"));
        assert_eq!(normalize_currency("₹ 500").as_deref(), Some("INR"));
        assert_eq!(normalize_currency("$ 500").as_deref(), Some("USD"));
        assert_eq!(normalize_currency("dollars").as_deref(), Some("USD"));
    }
}
