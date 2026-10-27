//! Conformance with @bonbagi/core, the TypeScript the BonBagi web app runs. The vectors were
//! produced by `conformance/generate.ts` from that code; if the two implementations ever
//! disagree about a charter's bytes, hash, signatures, split, memo or pay link, this fails.

use bonbagi_watcher::{charter, memo, paylink, receipt, split};
use serde_json::Value;

fn vectors() -> Value {
    let text = include_str!("../conformance/vectors.json");
    serde_json::from_str(text).expect("vectors.json is JSON")
}

fn s(v: &Value) -> &str {
    v.as_str().expect("string")
}

fn zat(v: &Value) -> u64 {
    s(v).parse().expect("zatoshi")
}

fn charter_named(v: &Value, name: &str) -> Value {
    v["charters"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == name)
        .unwrap_or_else(|| panic!("charter {name}"))
        .clone()
}

#[test]
fn keys_derive_the_same_public_keys() {
    for k in vectors()["keys"].as_array().unwrap() {
        let key = receipt::MemberKey::from_text(s(&k["priv"])).unwrap();
        assert_eq!(key.pubkey, s(&k["pub"]));
    }
}

#[test]
fn charters_hash_and_verify_like_the_web_app() {
    let v = vectors();
    let keys = v["keys"].as_array().unwrap();
    for c in v["charters"].as_array().unwrap() {
        let name = s(&c["name"]);
        let signed = charter::from_value(c["file"].clone()).unwrap();
        let canonical = bonbagi_watcher::canonical::canonical_json(&signed.raw).unwrap();
        assert_eq!(canonical, s(&c["canonical"]), "{name}: canonical bytes");
        let verified = charter::verify(&signed).unwrap();
        assert_eq!(verified.hash, s(&c["hash"]), "{name}: hash");
        let unsigned: Vec<usize> = c["unsigned"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i.as_u64().unwrap() as usize)
            .collect();
        assert_eq!(verified.unsigned, unsigned, "{name}: unsigned members");

        // Ed25519 is deterministic: signing here gives the very signature the browser made.
        if unsigned.is_empty() {
            let msg = charter::charter_message(&signed.raw).unwrap();
            for sig in &signed.signatures {
                let k = keys
                    .iter()
                    .find(|k| k["pub"] == sig.pubkey.as_str())
                    .unwrap();
                let key = receipt::MemberKey::from_text(s(&k["priv"])).unwrap();
                assert_eq!(key.sign(&msg), sig.sig, "{name}: signature");
            }
        }
    }
}

#[test]
fn the_unicode_charter_is_signed_over_nfc_bytes() {
    let c = charter_named(&vectors(), "unicode3");
    assert!(s(&c["canonical"]).contains("Caf\u{e9} Bersama"));
    assert!(s(&c["file"]["charter"]["title"]).contains("Cafe\u{301}"));
}

#[test]
fn splits_match_including_error_messages() {
    for case in vectors()["splits"].as_array().unwrap() {
        let bps: Vec<u32> = case["bps"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| b.as_u64().unwrap() as u32)
            .collect();
        let got = split::split_amount(zat(&case["total"]), &bps, zat(&case["min"]));
        match (&case.get("out"), got) {
            (Some(out), Ok(got)) => {
                let want: Vec<u64> = out.as_array().unwrap().iter().map(zat).collect();
                assert_eq!(got, want, "{case}");
            }
            (None, Err(e)) => assert_eq!(e, s(&case["error"]), "{case}"),
            (want, got) => panic!("{case}: want {want:?}, got {got:?}"),
        }
    }
}

#[test]
fn payment_requests_give_each_member_the_share_and_memo_we_expect() {
    let v = vectors();
    for r in v["requests"].as_array().unwrap() {
        let c = charter_named(&v, s(&r["charter"]));
        let verified = charter::verify(&charter::from_value(c["file"].clone()).unwrap()).unwrap();
        let bps: Vec<u32> = verified.charter.members.iter().map(|m| m.bps).collect();
        let shares =
            split::split_amount(zat(&r["price_zat"]), &bps, verified.charter.min_share_zat)
                .unwrap();
        let n = bps.len() as u32;
        for (i, line) in r["lines"].as_array().unwrap().iter().enumerate() {
            assert_eq!(shares[i], zat(&line["amount_zat"]));
            let m = memo::bonbagi_memo(s(&r["order"]), &verified.hash, i as u32 + 1, n).unwrap();
            assert_eq!(m, s(&line["memo"]));
            let parsed = memo::parse(&m).unwrap();
            assert_eq!(parsed.fingerprint, verified.fingerprint());
            assert_eq!((parsed.member, parsed.of), (i as u32 + 1, n));
            // The memo travels base64url-encoded in the ZIP-321 request.
            let enc = memo::base64url(m.as_bytes());
            let sfx = if i == 0 {
                String::new()
            } else {
                format!(".{i}")
            };
            assert!(s(&r["uri"]).contains(&format!("memo{sfx}={enc}")));
        }
    }
}

#[test]
fn pay_links_decode_to_the_signed_charter_and_price() {
    let v = vectors();
    for l in v["paylinks"].as_array().unwrap() {
        let c = charter_named(&v, s(&l["charter"]));
        let link = paylink::decode(s(&l["url"])).unwrap();
        let verified = charter::verify(&link.signed).unwrap();
        assert_eq!(verified.hash, s(&c["hash"]));
        assert!(verified.fully_signed());
        assert_eq!(
            link.price_zat,
            l["price_zat"].as_str().map(|p| p.parse().unwrap())
        );
    }
}

#[test]
fn memos_match() {
    for m in vectors()["memos"].as_array().unwrap() {
        let got = memo::bonbagi_memo(
            s(&m["order"]),
            s(&m["hash"]),
            m["member"].as_u64().unwrap() as u32,
            m["of"].as_u64().unwrap() as u32,
        )
        .unwrap();
        assert_eq!(got, s(&m["memo"]));
    }
}
