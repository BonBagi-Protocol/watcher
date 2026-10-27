//! Canonical JSON, byte for byte the `canonicalJson` of @bonbagi/core: object keys sorted at
//! every level (UTF-16 order, as JavaScript sorts), no whitespace, strings NFC-normalised and
//! escaped as `JSON.stringify` does, numbers only as safe integers. These are the bytes every
//! member signs, so any difference here would make good signatures look bad.

use serde_json::{Number, Value};
use unicode_normalization::UnicodeNormalization;

/// 2^53 - 1, `Number.MAX_SAFE_INTEGER`.
const MAX_SAFE: u64 = 9_007_199_254_740_991;

pub fn canonical_json(value: &Value) -> Result<String, String> {
    let mut out = String::new();
    write(value, &mut out)?;
    Ok(out)
}

fn write(value: &Value, out: &mut String) -> Result<(), String> {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => out.push_str(&number(n)?),
        Value::String(s) => string(&s.nfc().collect::<String>(), out),
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write(item, out)?;
            }
            out.push(']');
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
            out.push('{');
            for (i, k) in keys.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                string(k, out);
                out.push(':');
                write(&map[k], out)?;
            }
            out.push('}');
        }
    }
    Ok(())
}

/// `JSON.stringify` escaping: `"` `\` and control characters; everything else is written as is.
fn string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

fn number(n: &Number) -> Result<String, String> {
    let bad = || Err("charter numbers must be safe integers".to_string());
    if let Some(u) = n.as_u64() {
        return if u <= MAX_SAFE {
            Ok(u.to_string())
        } else {
            bad()
        };
    }
    if let Some(i) = n.as_i64() {
        return if i.unsigned_abs() <= MAX_SAFE {
            Ok(i.to_string())
        } else {
            bad()
        };
    }
    match n.as_f64() {
        // `JSON.parse("6000.0")` is the number 6000, written back as "6000" (and -0 as "0").
        Some(f) if f.fract() == 0.0 && f.abs() <= MAX_SAFE as f64 => Ok((f as i64).to_string()),
        _ => bad(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn sorts_keys_and_drops_whitespace() {
        let v = json!({"b": [1, {"z": true, "a": null}], "a": "x"});
        assert_eq!(
            canonical_json(&v).unwrap(),
            r#"{"a":"x","b":[1,{"a":null,"z":true}]}"#
        );
    }

    #[test]
    fn strings_are_nfc_and_escaped_like_javascript() {
        let v = json!({"t": "Cafe\u{301} \"q\" \\ \n\u{1}\u{7f}\u{2028}"});
        assert_eq!(
            canonical_json(&v).unwrap(),
            "{\"t\":\"Caf\u{e9} \\\"q\\\" \\\\ \\n\\u0001\u{7f}\u{2028}\"}"
        );
    }

    #[test]
    fn numbers_must_be_safe_integers() {
        let parse = |s: &str| serde_json::from_str::<Value>(s).unwrap();
        assert_eq!(canonical_json(&parse("6000.0")).unwrap(), "6000");
        assert_eq!(canonical_json(&parse("-0.0")).unwrap(), "0");
        assert_eq!(canonical_json(&parse("-12")).unwrap(), "-12");
        assert!(canonical_json(&parse("1.5")).is_err());
        assert!(canonical_json(&parse("9007199254740992")).is_err());
    }
}
