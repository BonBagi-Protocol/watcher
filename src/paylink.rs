//! Pay links carry the signed charter (and optionally a price) in the URL fragment:
//! `https://…/pay.html#c=<base64url JSON>&p=0.003`. The fragment never reaches a web server;
//! here it never leaves this machine either.

use crate::charter::{parse_charter_file, SignedCharter};
use crate::memo::from_base64url;
use crate::zec::parse_zec;

#[derive(Debug, Clone)]
pub struct PayLink {
    pub signed: SignedCharter,
    /// Price that pre-fills the pay form. Not signed: the buyer could change it.
    pub price_zat: Option<u64>,
}

/// Accepts a whole pay link or just its fragment (with or without "#").
pub fn decode(link: &str) -> Result<PayLink, String> {
    let link = link.trim();
    let fragment = link.split_once('#').map_or(link, |(_, f)| f);
    let params = search_params(fragment);
    let get = |name: &str| {
        params
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    };
    let c = get("c").ok_or("this link has no charter in it (expected …#c=…)")?;
    let damaged = || {
        "the charter in this link is damaged (not valid base64url text) — ask the seller for the link again"
            .to_string()
    };
    let bytes = from_base64url(c).map_err(|_| damaged())?;
    let text = String::from_utf8(bytes).map_err(|_| damaged())?;
    let signed = parse_charter_file(&text)?;
    let price_zat = match get("p") {
        Some(p) if !p.is_empty() => Some(parse_zec(p)?),
        _ => None,
    };
    Ok(PayLink { signed, price_zat })
}

/// `URLSearchParams` parsing: `&`-separated, `+` is a space, percent escapes are UTF-8.
fn search_params(query: &str) -> Vec<(String, String)> {
    query
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (percent_decode(k), percent_decode(v))
        })
        .collect()
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => match (hex(b[i + 1]), hex(b[i + 2])) {
                (Some(h), Some(l)) => {
                    out.push(h << 4 | l);
                    i += 2;
                }
                _ => out.push(b'%'),
            },
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_parsing_matches_urlsearchparams() {
        assert_eq!(
            search_params("c=abc&p=0.003&x=a+b%20c%"),
            [
                ("c".to_string(), "abc".to_string()),
                ("p".into(), "0.003".into()),
                ("x".into(), "a b c%".into())
            ]
        );
    }

    #[test]
    fn links_without_a_charter_or_with_a_broken_one() {
        assert!(decode("https://bonbagi.vercel.app/pay.html")
            .unwrap_err()
            .contains("no charter"));
        assert!(decode("#c=***").unwrap_err().contains("damaged"));
        // Valid base64url, but the bytes are not UTF-8.
        assert!(decode("c=_w").unwrap_err().contains("damaged"));
        assert_eq!(
            decode("c=e30").unwrap_err(),
            r#"expected {"charter": …, "signatures": […]}"#
        );
    }
}
