//! A receipt is a member's signed statement "my share of order X arrived": order, amount, txid
//! and block, signed with the same Ed25519 key the member signed the charter with. The member
//! decides who sees it (the collective, the buyer). Anyone holding the charter can check it
//! offline with `verify-receipt`; nobody needs the member's viewing key.
//!
//! Signed bytes: "bonbagi-receipt-v1\n" + canonical JSON of the `receipt` object, the same
//! canonical form as the charter, so a TypeScript check with @bonbagi/core's `canonicalJson`
//! gets the same bytes.

use ed25519_zebra::{SigningKey, VerificationKey};
use serde_json::{json, Map, Value};

use crate::address::Network;
use crate::canonical::canonical_json;
use crate::charter::{verify_signature, Verified};
use crate::check::Row;
use crate::hex;
use crate::split::totals_for_share;
use crate::zec::{with_ticker, MAX_ZAT};

pub const RECEIPT_VERSION: u64 = 1;
const DOMAIN: &str = "bonbagi-receipt-v1\n";

/// A member's charter private key, as the charter page downloads it (64 hex characters).
pub struct MemberKey {
    seed: [u8; 32],
    pub pubkey: String,
}

impl MemberKey {
    pub fn from_text(text: &str) -> Result<Self, String> {
        let t = text.trim().to_ascii_lowercase();
        let seed = hex::decode::<32>(&t)
            .ok_or("the key file must hold your 64-hex-character charter private key")?;
        let vk = VerificationKey::from(&SigningKey::from(seed));
        Ok(MemberKey {
            seed,
            pubkey: hex::encode(vk.as_ref()),
        })
    }

    pub fn sign(&self, msg: &[u8]) -> String {
        let sig = SigningKey::from(self.seed).sign(msg);
        hex::encode(&sig.to_bytes())
    }
}

fn message(body: &Value) -> Result<Vec<u8>, String> {
    Ok(format!("{DOMAIN}{}", canonical_json(body)?).into_bytes())
}

pub struct Context<'a> {
    pub verified: &'a Verified,
    pub me: usize,
    pub source: &'a str,
    pub checked_at: String,
}

/// Builds and signs the receipt for one PAID row. Returns the file text.
pub fn issue(ctx: &Context, row: &Row, key: &MemberKey) -> Result<String, String> {
    let c = &ctx.verified.charter;
    let m = &c.members[ctx.me];
    if key.pubkey != m.pubkey {
        return Err("the key file does not belong to this member of the charter".into());
    }
    let mut body = json!({
        "bonbagi_receipt": RECEIPT_VERSION,
        "network": c.network.code(),
        "charter": ctx.verified.hash,
        "title": c.title,
        "order": row.order,
        "member": ctx.me + 1,
        "of": c.members.len(),
        "label": m.label,
        "pubkey": m.pubkey,
        "amount_zat": row.amount_zat.to_string(),
        "txid": row.txid,
        "output_index": row.output_index,
        "pool": row.pool,
        "height": row.height,
        "confirmations": row.confirmations,
        "source": ctx.source,
        "checked_at": ctx.checked_at,
    });
    if let Some(e) = row.expected_zat {
        body["expected_zat"] = Value::String(e.to_string());
    }
    let sig = key.sign(&message(&body)?);
    let mut text = serde_json::to_string_pretty(&json!({ "receipt": body, "sig": sig }))
        .expect("receipt serializes");
    text.push('\n');
    Ok(text)
}

#[derive(Debug, Clone)]
pub struct Checked {
    pub valid: bool,
    /// One line per check, "PASS …" / "FAIL …" / "INFO …".
    pub lines: Vec<String>,
    pub summary: String,
    pub body: Map<String, Value>,
}

/// Checks a receipt's signature and, given the charter, that it belongs to it.
pub fn verify(text: &str, charter: Option<&Verified>) -> Result<Checked, String> {
    let file: Value = serde_json::from_str(text).map_err(|_| "not valid JSON".to_string())?;
    let shape = || r#"expected {"receipt": {…}, "sig": "…"}"#.to_string();
    let body = file
        .get("receipt")
        .and_then(Value::as_object)
        .ok_or_else(shape)?
        .clone();
    let sig = file.get("sig").and_then(Value::as_str).ok_or_else(shape)?;
    let s = |k: &str| {
        body.get(k)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    let u = |k: &str| body.get(k).and_then(Value::as_u64);
    if u("bonbagi_receipt") != Some(RECEIPT_VERSION) {
        return Err("unsupported receipt version".into());
    }
    let network = Network::parse(&s("network")).ok_or("receipt network must be main or test")?;
    let (pubkey, hash, order, txid) = (s("pubkey"), s("charter"), s("order"), s("txid"));
    if !hex::is_lower(&pubkey, 64) || !hex::is_lower(&hash, 64) || !hex::is_lower(&txid, 64) {
        return Err("receipt pubkey, charter and txid must be lowercase hex".into());
    }
    let amount: u64 = s("amount_zat")
        .parse()
        .map_err(|_| "receipt amount_zat must be a whole number of zatoshi".to_string())?;
    let (member, of) = (
        u("member").unwrap_or(0) as usize,
        u("of").unwrap_or(0) as usize,
    );
    if member == 0 || member > of {
        return Err("receipt member must be between 1 and of".into());
    }

    let mut lines = Vec::new();
    let mut valid = true;
    let mut check = |ok: bool, pass: String, fail: String| {
        valid &= ok;
        lines.push(if ok {
            format!("PASS  {pass}")
        } else {
            format!("FAIL  {fail}")
        });
    };
    let sig_ok = verify_signature(&pubkey, sig, &message(&Value::Object(body.clone()))?);
    check(
        sig_ok,
        format!(
            "signature: signed by key {}…{}",
            &pubkey[..8],
            &pubkey[56..]
        ),
        "signature: does not verify; the receipt was changed or signed with another key".into(),
    );
    match charter {
        None => lines.push(
            "INFO  charter: not given, so the key is not tied to a member (pass --charter or --link)"
                .into(),
        ),
        Some(v) => {
            let c = &v.charter;
            check(
                v.hash == hash,
                format!("charter: receipt is for this charter ({})", v.fingerprint()),
                format!(
                    "charter: receipt is for charter {}, not this one ({})",
                    &hash[..16],
                    v.fingerprint()
                ),
            );
            let slot = c.members.get(member - 1);
            check(
                of == c.members.len() && slot.is_some_and(|m| m.pubkey == pubkey),
                format!(
                    "member: key is member {member}/{of} \"{}\" of the charter",
                    slot.map(|m| m.label.as_str()).unwrap_or_default()
                ),
                format!("member: key is not member {member}/{of} of this charter"),
            );
            check(
                network == c.network,
                format!("network: {}", network.name()),
                format!("network: receipt is {}, charter is {}", network.name(), c.network.name()),
            );
            if let Some(m) = slot {
                let bps: Vec<u32> = c.members.iter().map(|m| m.bps).collect();
                let fits = totals_for_share(amount, member - 1, &bps, c.min_share_zat, MAX_ZAT);
                check(
                    fits.is_some(),
                    format!(
                        "amount: {} fits a {}% share",
                        with_ticker(amount, network),
                        crate::check::pct(m.bps)
                    ),
                    format!(
                        "amount: {} is not a {}% share of any total",
                        with_ticker(amount, network),
                        crate::check::pct(m.bps)
                    ),
                );
            }
        }
    }
    let height = u("height").map_or("unmined".to_string(), |h| format!("block {h}"));
    let summary = if valid {
        format!(
            "VALID: member {member}/{of} \"{}\" confirms {} for order {order} (tx {}…, {height}, checked {}).",
            s("label"),
            with_ticker(amount, network),
            &txid[..16],
            s("checked_at")
        )
    } else {
        "INVALID: do not rely on this receipt.".to_string()
    };
    Ok(Checked {
        valid,
        lines,
        summary,
        body,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_files_are_hex_seeds() {
        // RFC 8032 test 1: secret key 9d61b1…7f60 has public key d75a98…511a.
        let k = MemberKey::from_text(
            "9D61B19DEFFD5A60BA844AF492EC2CC44449C5697B326919703BAC031CAE7F60\n",
        )
        .unwrap();
        assert_eq!(
            k.pubkey,
            "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a"
        );
        assert_eq!(
            k.sign(b""),
            "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b"
        );
        assert!(MemberKey::from_text("1234").is_err());
    }
}
