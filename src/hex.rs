//! Lowercase hex, the form @bonbagi/core uses for keys, signatures and hashes.

pub fn encode(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(DIGITS[(b >> 4) as usize] as char);
        s.push(DIGITS[(b & 0x0f) as usize] as char);
    }
    s
}

/// True when `s` is exactly `len` lowercase hex characters.
pub fn is_lower(s: &str, len: usize) -> bool {
    s.len() == len
        && s.bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

/// Decodes exactly `N` bytes of lowercase hex.
pub fn decode<const N: usize>(s: &str) -> Option<[u8; N]> {
    if !is_lower(s, N * 2) {
        return None;
    }
    let nibble = |c: u8| {
        if c.is_ascii_digit() {
            c - b'0'
        } else {
            c - b'a' + 10
        }
    };
    let b = s.as_bytes();
    let mut out = [0u8; N];
    for (i, o) in out.iter_mut().enumerate() {
        *o = (nibble(b[2 * i]) << 4) | nibble(b[2 * i + 1]);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    #[test]
    fn roundtrip() {
        let bytes = [0x00, 0x0f, 0xa5, 0xff];
        assert_eq!(super::encode(&bytes), "000fa5ff");
        assert_eq!(super::decode::<4>("000fa5ff"), Some(bytes));
        assert_eq!(super::decode::<4>("000FA5FF"), None);
        assert_eq!(super::decode::<4>("000fa5f"), None);
    }
}
