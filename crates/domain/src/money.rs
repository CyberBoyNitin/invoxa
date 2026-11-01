use rust_decimal::{Decimal, RoundingStrategy};
use std::str::FromStr;

/// Round monetary values using the currency's minor-unit exponent.
/// INVX policy: half away from zero, documented in docs/architecture.md.
pub fn round_currency(value: Decimal, currency: Option<&str>) -> Decimal {
    let dp = currency_exponent(currency.unwrap_or("USD"));
    value.round_dp_with_strategy(dp, RoundingStrategy::MidpointAwayFromZero)
}

pub fn currency_exponent(currency: &str) -> u32 {
    match currency.to_ascii_uppercase().as_str() {
        "JPY" | "KRW" | "VND" | "CLP" | "ISK" | "UGX" | "XOF" | "XAF" | "GNF" | "PYG" => 0,
        "BHD" | "IQD" | "JOD" | "KWD" | "LYD" | "OMR" | "TND" | "BIF" | "DJF" => 3,
        _ => 2,
    }
}

/// Parse a human-entered or extracted amount without losing the original text.
/// Supports grouped thousands, decimal commas, currency symbols, parentheses
/// for negatives, and trailing minus signs. This is deliberately conservative:
/// ambiguous mixed separators are rejected rather than guessed.
pub fn parse_amount(input: &str) -> Result<Decimal, String> {
    let raw = input.trim();
    if raw.is_empty() {
        return Err("empty amount".into());
    }

    let negative =
        (raw.starts_with('(') && raw.ends_with(')')) || raw.ends_with('-') || raw.starts_with('-');
    let mut cleaned: String = raw
        .chars()
        .filter(|c| {
            c.is_ascii_digit()
                || *c == '.'
                || *c == ','
                || *c == '-'
                || *c == '\''
                || *c == ' '
                || *c == '\u{00A0}'
        })
        .collect();

    cleaned = cleaned
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '\'' && *c != '-')
        .collect();
    if cleaned.is_empty() {
        return Err("no numeric content".into());
    }

    let last_dot = cleaned.rfind('.');
    let last_comma = cleaned.rfind(',');
    let normalized = match (last_dot, last_comma) {
        (Some(d), Some(c)) => {
            if d > c {
                // 1,234.56
                if cleaned.matches('.').count() > 1 {
                    return Err("multiple decimal separators".into());
                }
                cleaned.replace(',', "")
            } else {
                // 1.234,56
                let parts: Vec<&str> = cleaned.split(',').collect();
                if parts.len() != 2 {
                    return Err("ambiguous decimal separator".into());
                }
                format!("{}.{}", parts[0].replace('.', ""), parts[1])
            }
        }
        (Some(_), None) => cleaned,
        (None, Some(_)) => {
            let parts: Vec<&str> = cleaned.split(',').collect();
            if parts.len() == 2 && parts[1].len() <= 2 {
                format!("{}.{}", parts[0], parts[1])
            } else {
                cleaned.replace(',', "")
            }
        }
        (None, None) => cleaned,
    };

    let value = Decimal::from_str(&normalized).map_err(|_| "invalid amount".to_string())?;
    Ok(if negative { -value } else { value })
}

pub fn money_difference_within(a: Decimal, b: Decimal, tolerance: Decimal) -> bool {
    (a - b).abs() <= tolerance
}

pub fn fmt_money(value: Decimal, currency: &str) -> String {
    let dp = currency_exponent(currency) as usize;
    format!("{} {}", currency, format!("{:.*}", dp, value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::dec;

    #[test]
    fn parses_common_formats() {
        assert_eq!(parse_amount("1,234.56").unwrap(), dec!(1234.56));
        assert_eq!(parse_amount("1.234,56").unwrap(), dec!(1234.56));
        assert_eq!(parse_amount("₹ 1,23,456.78").unwrap(), dec!(123456.78));
        assert_eq!(parse_amount("(45.00)").unwrap(), dec!(-45.00));
        assert_eq!(parse_amount("45.00-").unwrap(), dec!(-45.00));
        assert_eq!(parse_amount("$ 900").unwrap(), dec!(900));
    }

    #[test]
    fn rejects_empty_and_garbage() {
        assert!(parse_amount("   ").is_err());
        assert!(parse_amount("not money").is_err());
        assert!(parse_amount("1.2.3").is_err());
    }

    #[test]
    fn rounds_by_currency() {
        assert_eq!(round_currency(dec!(10.005), Some("USD")), dec!(10.01));
        assert_eq!(round_currency(dec!(10.5), Some("JPY")), dec!(11));
        assert_eq!(round_currency(dec!(10.5005), Some("KWD")), dec!(10.501));
    }
}
