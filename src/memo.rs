//! The memo BonBagi puts in every member's output, and base64url (no padding) as the pay link
//! and ZIP-321 use it. The memo carries no personal data: an order reference, a short charter
//! fingerprint and which member the output is for. Only that member's wallet can read it.
//!
//! ```text
//! bonbagi:v1 o=<order ID> c=<first 16 hex of the charter hash> m=<member>/<members>
//! ```

use base64::alphabet::URL_SAFE;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
use base64::Engine;

pub const MAX_MEMO_BYTES: usize = 512;
pub const FINGERPRINT_LEN: usize = 16;

/// Unpadded base64url. Decoding tolerates non-zero trailing bits, as the browser's `atob` does.
const B64: GeneralPurpose = GeneralPurpose::new(
    &URL_SAFE,
    GeneralPurposeConfig::new()
        .with_encode_padding(false)
        .with_decode_padding_mode(DecodePaddingMode::RequireNone)
        .with_decode_allow_trailing_bits(true),
);

pub fn base64url(bytes: &[u8]) -> String {
    B64.encode(bytes)
}

pub fn from_base64url(text: &str) -> Result<Vec<u8>, String> {
    if !text
        .bytes()
        .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    {
        return Err("not unpadded base64url".into());
    }
    B64.decode(text)
        .map_err(|_| "not unpadded base64url".into())
}

/// Order references are 1–32 letters, digits or dashes (the pay page makes `ord-` + 8 random).
pub fn is_order_id(s: &str) -> bool {
    (1..=32).contains(&s.len()) && s.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
}

/// The memo text for one member, exactly as `bonbagiMemo` in @bonbagi/core builds it.
pub fn bonbagi_memo(
    order: &str,
    charter_hash: &str,
    member: u32,
    of: u32,
) -> Result<String, String> {
    if !is_order_id(order) {
        return Err("order ID must be 1–32 letters, digits or dashes".into());
    }
    if !crate::hex::is_lower(charter_hash, 64) {
        return Err("charter hash must be 64 lowercase hex characters".into());
    }
    Ok(format!(
        "bonbagi:v1 o={order} c={} m={member}/{of}",
        &charter_hash[..FINGERPRINT_LEN]
    ))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BonbagiMemo {
    pub order: String,
    pub fingerprint: String,
    pub member: u32,
    pub of: u32,
}

/// Reads a BonBagi memo. Anything that is not exactly this format is not a BonBagi memo.
pub fn parse(text: &str) -> Option<BonbagiMemo> {
    let rest = text.strip_prefix("bonbagi:v1 ")?;
    let mut parts = rest.split(' ');
    let order = parts.next()?.strip_prefix("o=")?;
    let fingerprint = parts.next()?.strip_prefix("c=")?;
    let (member, of) = parts.next()?.strip_prefix("m=")?.split_once('/')?;
    if parts.next().is_some()
        || !is_order_id(order)
        || !crate::hex::is_lower(fingerprint, FINGERPRINT_LEN)
    {
        return None;
    }
    let number = |s: &str| -> Option<u32> {
        if s.is_empty()
            || s.len() > 3
            || s.starts_with('0')
            || !s.bytes().all(|c| c.is_ascii_digit())
        {
            return None;
        }
        s.parse().ok()
    };
    let (member, of) = (number(member)?, number(of)?);
    if member > of {
        return None;
    }
    Some(BonbagiMemo {
        order: order.into(),
        fingerprint: fingerprint.into(),
        member,
        of,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_what_it_builds() {
        let hash = "3b25c168a1f0e2d4c5b6a7980112233445566778899aabbccddeeff001122334";
        let m = bonbagi_memo("ord-k7m2qx9p", hash, 1, 2).unwrap();
        assert_eq!(m, "bonbagi:v1 o=ord-k7m2qx9p c=3b25c168a1f0e2d4 m=1/2");
        assert_eq!(
            parse(&m),
            Some(BonbagiMemo {
                order: "ord-k7m2qx9p".into(),
                fingerprint: "3b25c168a1f0e2d4".into(),
                member: 1,
                of: 2
            })
        );
    }

    #[test]
    fn rejects_near_misses() {
        for m in [
            "bonbagi:v1 o=ord-1 c=3b25c168a1f0e2d4 m=3/2",
            "bonbagi:v1 o=ord-1 c=3b25c168a1f0e2d4 m=01/2",
            "bonbagi:v1 o=ord-1 c=3B25C168A1F0E2D4 m=1/2",
            "bonbagi:v1 o=ord-1 c=3b25c168a1f0e2 m=1/2",
            "bonbagi:v1 o=ord_1 c=3b25c168a1f0e2d4 m=1/2",
            "bonbagi:v1 o=ord-1 c=3b25c168a1f0e2d4 m=1/2 extra",
            "bonbagi:v2 o=ord-1 c=3b25c168a1f0e2d4 m=1/2",
            "bonbagi gate test 1/2",
        ] {
            assert_eq!(parse(m), None, "{m}");
        }
    }

    #[test]
    fn base64url_matches_the_browser() {
        assert_eq!(
            base64url(b"bonbagi gate test 1/2"),
            "Ym9uYmFnaSBnYXRlIHRlc3QgMS8y"
        );
        assert_eq!(from_base64url("_wEC").unwrap(), [0xff, 1, 2]);
        // atob ignores non-zero trailing bits ("_x" decodes like "_w").
        assert_eq!(from_base64url("_w").unwrap(), [0xff]);
        assert_eq!(from_base64url("_x").unwrap(), [0xff]);
        assert!(from_base64url("_wEC=").is_err());
        assert!(from_base64url("a").is_err());
    }
}
