//! The charter: who is paid, to which shielded address, what share. Every member signs the same
//! canonical bytes with their own Ed25519 key. Same rules and error messages as @bonbagi/core;
//! `tests/vectors.rs` replays charters, hashes and signatures produced by that TypeScript.

use ed25519_zebra::{Signature, VerificationKey};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::address::{assert_shielded, Network};
use crate::canonical::canonical_json;
use crate::hex;
use crate::split::TOTAL_BPS;

pub const CHARTER_VERSION: u64 = 1;
const DOMAIN: &str = "bonbagi-charter-v1\n";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub label: String,
    pub address: String,
    /// Share in basis points (100 = 1%).
    pub bps: u32,
    /// Ed25519 public key, 64 lowercase hex characters.
    pub pubkey: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Charter {
    pub network: Network,
    pub title: String,
    pub min_share_zat: u64,
    pub members: Vec<Member>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureEntry {
    pub pubkey: String,
    pub sig: String,
}

/// A charter file as members exchange it. `raw` is the charter object exactly as written: its
/// canonical form is what was signed, so it is hashed as is, unknown fields included.
#[derive(Debug, Clone)]
pub struct SignedCharter {
    pub raw: Value,
    pub signatures: Vec<SignatureEntry>,
}

/// A charter whose structure checked out, with its hash and who has not signed (yet).
#[derive(Debug, Clone)]
pub struct Verified {
    pub charter: Charter,
    pub hash: String,
    /// Members (0-based) whose signature is missing or invalid. Empty = fully signed.
    pub unsigned: Vec<usize>,
}

impl Verified {
    pub fn fingerprint(&self) -> &str {
        &self.hash[..crate::memo::FINGERPRINT_LEN]
    }

    pub fn fully_signed(&self) -> bool {
        self.unsigned.is_empty()
    }
}

/// Reads a charter file. Checks shape only, like `parseCharterFile`; `verify` does the rest.
pub fn parse_charter_file(text: &str) -> Result<SignedCharter, String> {
    let file: Value = serde_json::from_str(text).map_err(|_| "not valid JSON".to_string())?;
    from_value(file)
}

pub fn from_value(file: Value) -> Result<SignedCharter, String> {
    let shape = || r#"expected {"charter": …, "signatures": […]}"#.to_string();
    let Value::Object(mut file) = file else {
        return Err(shape());
    };
    let raw = file.remove("charter").ok_or_else(shape)?;
    let Value::Object(c) = &raw else {
        return Err(shape());
    };
    let Some(Value::Array(members)) = c.get("members") else {
        return Err("charter.members must be a list".into());
    };
    for m in members {
        let ok = m.get("label").is_some_and(Value::is_string)
            && m.get("address").is_some_and(Value::is_string)
            && m.get("bps").is_some_and(Value::is_number)
            && m.get("pubkey").is_some_and(Value::is_string);
        if !ok {
            return Err("each member needs label, address, bps and pubkey".into());
        }
    }
    if !["title", "minShareZat", "network"]
        .iter()
        .all(|k| c.get(*k).is_some_and(Value::is_string))
    {
        return Err("charter needs title, network and minShareZat".into());
    }
    let sigs_error = || "signatures must be a list of {pubkey, sig}".to_string();
    let signatures = match file.remove("signatures") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) => items
            .iter()
            .map(|s| match (s.get("pubkey"), s.get("sig")) {
                (Some(Value::String(pubkey)), Some(Value::String(sig))) => Ok(SignatureEntry {
                    pubkey: pubkey.clone(),
                    sig: sig.clone(),
                }),
                _ => Err(sigs_error()),
            })
            .collect::<Result<_, _>>()?,
        Some(_) => return Err(sigs_error()),
    };
    Ok(SignedCharter { raw, signatures })
}

/// JavaScript `String.length`: UTF-16 code units.
fn js_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// Structural checks of `validateCharter`, then the typed charter.
pub fn validate(raw: &Value) -> Result<Charter, String> {
    let get = |k: &str| raw.get(k).cloned().unwrap_or(Value::Null);
    let version = get("bonbagi");
    if version.as_f64() != Some(CHARTER_VERSION as f64) {
        let shown = match &version {
            Value::String(s) => s.clone(),
            Value::Null => "undefined".into(),
            v => v.to_string(),
        };
        return Err(format!("unsupported charter version {shown}"));
    }
    let network = get("network")
        .as_str()
        .and_then(Network::parse)
        .ok_or("network must be main or test")?;
    let title = get("title").as_str().unwrap_or_default().to_string();
    if title.trim().is_empty() || js_len(&title) > 80 {
        return Err("title must be 1–80 characters".into());
    }
    let min = get("minShareZat").as_str().unwrap_or_default().to_string();
    let min_share_zat = (min.starts_with(|c: char| ('1'..='9').contains(&c))
        && min.bytes().all(|c| c.is_ascii_digit()))
    .then(|| min.parse::<u64>().ok())
    .flatten()
    .ok_or("minShareZat must be a positive whole number")?;
    let members = get("members");
    let members = members.as_array().map(Vec::as_slice).unwrap_or_default();
    if members.len() < 2 {
        return Err("a collective needs at least two members".into());
    }
    if members.len() > 50 {
        return Err("at most 50 members".into());
    }
    let mut out: Vec<Member> = Vec::with_capacity(members.len());
    let mut sum: u64 = 0;
    for (i, m) in members.iter().enumerate() {
        let s = |k: &str| {
            m.get(k)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string()
        };
        let (label, address, pubkey) = (s("label"), s("address"), s("pubkey"));
        let who = format!("member {} ({label})", i + 1);
        if label.trim().is_empty() || js_len(&label) > 40 {
            return Err(format!("{who}: label must be 1–40 characters"));
        }
        let net = assert_shielded(&address).map_err(|e| format!("{who}: {e}"))?;
        if net != network {
            return Err(format!(
                "{who}: address is on {}net, charter is {}net",
                net.code(),
                network.code()
            ));
        }
        if out.iter().any(|o| o.address == address) {
            return Err(format!("{who}: address used twice"));
        }
        if !hex::is_lower(&pubkey, 64) {
            return Err(format!(
                "{who}: public key must be 64 lowercase hex characters"
            ));
        }
        if out.iter().any(|o| o.pubkey == pubkey) {
            return Err(format!("{who}: public key used twice"));
        }
        let bps = m
            .get("bps")
            .and_then(Value::as_f64)
            .filter(|b| b.fract() == 0.0 && *b > 0.0 && *b <= f64::from(u32::MAX))
            .ok_or_else(|| {
                format!("{who}: share must be a positive whole number of basis points")
            })? as u32;
        sum += u64::from(bps);
        out.push(Member {
            label,
            address,
            bps,
            pubkey,
        });
    }
    if sum != u64::from(TOTAL_BPS) {
        let pct = sum as f64 / 100.0;
        return Err(format!("shares add up to {pct}%, must be exactly 100%"));
    }
    Ok(Charter {
        network,
        title,
        min_share_zat,
        members: out,
    })
}

/// Bytes every member signs.
pub fn charter_message(raw: &Value) -> Result<Vec<u8>, String> {
    Ok(format!("{DOMAIN}{}", canonical_json(raw)?).into_bytes())
}

pub fn charter_hash(raw: &Value) -> Result<String, String> {
    Ok(hex::encode(&Sha256::digest(charter_message(raw)?)))
}

/// ZIP 215 verification of one 64-byte signature by one 32-byte key, both lowercase hex.
pub fn verify_signature(pubkey: &str, sig: &str, msg: &[u8]) -> bool {
    let (Some(pk), Some(sig)) = (hex::decode::<32>(pubkey), hex::decode::<64>(sig)) else {
        return false;
    };
    VerificationKey::try_from(pk)
        .and_then(|vk| vk.verify(&Signature::from_bytes(&sig), msg))
        .is_ok()
}

/// Structure and every signature, like `verifyCharter`. A bad signature is reported, not thrown.
pub fn verify(signed: &SignedCharter) -> Result<Verified, String> {
    let charter = validate(&signed.raw)?;
    let msg = charter_message(&signed.raw)?;
    if signed
        .signatures
        .iter()
        .any(|s| !charter.members.iter().any(|m| m.pubkey == s.pubkey))
    {
        return Err("signature from a key that is not a member".into());
    }
    let mut unsigned = Vec::new();
    for (i, m) in charter.members.iter().enumerate() {
        let own: Vec<&SignatureEntry> = signed
            .signatures
            .iter()
            .filter(|s| s.pubkey == m.pubkey)
            .collect();
        if own.len() > 1 {
            return Err(format!("member {} has more than one signature", i + 1));
        }
        let ok = own
            .first()
            .is_some_and(|s| verify_signature(&m.pubkey, &s.sig, &msg));
        if !ok {
            unsigned.push(i);
        }
    }
    Ok(Verified {
        hash: hex::encode(&Sha256::digest(&msg)),
        charter,
        unsigned,
    })
}
