//! Exact KDC <-> proton conversion (1 KDC = 10^18 proton).
//! String-based: proton values exceed u64/i64 (up to 2^256 on-chain).

use anyhow::{bail, Result};

/// Parse decimal KDC ("1.234567") into exact proton string.
pub fn kdc_to_proton(kdc: &str) -> Result<String> {
    let kdc = kdc.trim();
    if kdc.is_empty() {
        bail!("empty amount");
    }
    let (whole, frac) = match kdc.split_once('.') {
        Some((w, f)) => (w, f),
        None => (kdc, ""),
    };
    if whole.len() > 1 && whole.starts_with('0') || !whole.chars().all(|c| c.is_ascii_digit()) {
        bail!("invalid amount: {kdc}");
    }
    if frac.len() > 18 || !frac.chars().all(|c| c.is_ascii_digit()) {
        bail!("too many decimals (max 18): {kdc}");
    }
    let mut frac = frac.to_string();
    while frac.len() < 18 {
        frac.push('0');
    }
    let whole = whole.trim_start_matches('0');
    let whole = if whole.is_empty() { "0" } else { whole };
    let raw = format!("{whole}{frac}");
    let raw = raw.trim_start_matches('0');
    Ok(if raw.is_empty() {
        "0".to_string()
    } else {
        raw.to_string()
    })
}

/// Format proton string as human KDC with up to 6 decimals shown.
pub fn proton_to_kdc(proton: &str) -> String {
    let p = proton.trim().trim_start_matches('0');
    let p = if p.is_empty() { "0" } else { p };
    if p.len() <= 18 {
        let frac = format!("{:0>18}", p);
        let frac = frac.trim_end_matches('0');
        if frac.is_empty() {
            return "0".to_string();
        }
        return format!("0.{frac}");
    }
    let (w, f) = p.split_at(p.len() - 18);
    let f = f.trim_end_matches('0');
    if f.is_empty() {
        w.to_string()
    } else {
        format!("{w}.{f}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_conversions() {
        assert_eq!(kdc_to_proton("1").unwrap(), "1000000000000000000");
        assert_eq!(kdc_to_proton("1.234567").unwrap(), "1234567000000000000");
        assert_eq!(kdc_to_proton("0.000001").unwrap(), "1000000000000");
        assert_eq!(kdc_to_proton("250").unwrap(), "250000000000000000000");
        assert_eq!(kdc_to_proton("0").unwrap(), "0");
        assert!(kdc_to_proton("1.0000000000000000001").is_err());
        assert!(kdc_to_proton("abc").is_err());
    }

    #[test]
    fn format_back() {
        assert_eq!(proton_to_kdc("1000000000000000000"), "1");
        assert_eq!(proton_to_kdc("1234567000000000000"), "1.234567");
        assert_eq!(proton_to_kdc("0"), "0");
        assert_eq!(proton_to_kdc("9999000000000000000000000"), "9999000");
    }
}
