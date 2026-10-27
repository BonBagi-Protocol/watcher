//! Amounts are integer zatoshi (1 ZEC = 100,000,000 zat); only text uses decimal ZEC.

use crate::address::Network;

pub const ZAT_PER_ZEC: u64 = 100_000_000;
pub const MAX_ZAT: u64 = 21_000_000 * ZAT_PER_ZEC;

/// Parses a decimal ZEC string ("1.5", "0.00010000") into zatoshi.
pub fn parse_zec(text: &str) -> Result<u64, String> {
    let t = text.trim();
    let bad = || format!("invalid ZEC amount: \"{text}\"");
    let (whole, frac) = t.split_once('.').unwrap_or((t, ""));
    if whole.is_empty() || !whole.bytes().all(|c| c.is_ascii_digit()) {
        return Err(bad());
    }
    if t.contains('.')
        && (frac.is_empty() || frac.len() > 8 || !frac.bytes().all(|c| c.is_ascii_digit()))
    {
        return Err(bad());
    }
    let whole = whole.trim_start_matches('0');
    if whole.len() > 9 {
        return Err("amount exceeds 21,000,000 ZEC".into());
    }
    let whole: u64 = if whole.is_empty() {
        0
    } else {
        whole.parse().map_err(|_| bad())?
    };
    let frac: u64 = format!("{frac:0<8}").parse().map_err(|_| bad())?;
    let zat = whole * ZAT_PER_ZEC + frac;
    if zat > MAX_ZAT {
        return Err("amount exceeds 21,000,000 ZEC".into());
    }
    Ok(zat)
}

/// Shortest decimal form, as ZIP-321 writes it ("0.0018", "1").
pub fn format_zec(zat: u64) -> String {
    let whole = zat / ZAT_PER_ZEC;
    let frac = format!("{:08}", zat % ZAT_PER_ZEC);
    let frac = frac.trim_end_matches('0');
    if frac.is_empty() {
        whole.to_string()
    } else {
        format!("{whole}.{frac}")
    }
}

/// "0.0018 TAZ" on testnet, "0.0018 ZEC" on mainnet.
pub fn with_ticker(zat: u64, network: Network) -> String {
    format!("{} {}", format_zec(zat), network.ticker())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_formats() {
        assert_eq!(parse_zec("1.5").unwrap(), 150_000_000);
        assert_eq!(parse_zec("0.00010000").unwrap(), 10_000);
        assert_eq!(parse_zec(" 0.003 ").unwrap(), 300_000);
        assert_eq!(parse_zec("21000000").unwrap(), MAX_ZAT);
        assert!(parse_zec("21000000.00000001").is_err());
        assert!(parse_zec("1.").is_err());
        assert!(parse_zec(".5").is_err());
        assert!(parse_zec("1.123456789").is_err());
        assert!(parse_zec("-1").is_err());
        assert!(parse_zec("99999999999999999999999").is_err());
        assert_eq!(format_zec(180_000), "0.0018");
        assert_eq!(format_zec(100_000_000), "1");
        assert_eq!(format_zec(1), "0.00000001");
        assert_eq!(with_ticker(120_000, Network::Test), "0.0012 TAZ");
    }
}
