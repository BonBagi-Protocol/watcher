//! Light address classification by prefix, as in @bonbagi/core: a guard against obvious mistakes
//! (a transparent address, mixed networks), not full validation. Wallets verify checksums.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Network {
    Main,
    Test,
}

impl Network {
    /// The charter's spelling: "main" or "test".
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "main" => Some(Network::Main),
            "test" => Some(Network::Test),
            _ => None,
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Network::Main => "main",
            Network::Test => "test",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Network::Main => "mainnet",
            Network::Test => "testnet",
        }
    }

    /// Currency code wallets show: ZEC on mainnet, TAZ on testnet.
    pub fn ticker(self) -> &'static str {
        match self {
            Network::Main => "ZEC",
            Network::Test => "TAZ",
        }
    }

    /// Network of a unified full viewing key, from its human-readable part.
    pub fn of_ufvk(ufvk: &str) -> Result<Self, String> {
        if ufvk.starts_with("uviewtest1") {
            Ok(Network::Test)
        } else if ufvk.starts_with("uview1") {
            Ok(Network::Main)
        } else {
            Err(
                "the viewing key must be a unified full viewing key (uview1… or uviewtest1…); \
                 a spending key or seed phrase is never needed"
                    .into(),
            )
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Unified,
    Sapling,
    Transparent,
}

const BECH32: &str = "qpzry9x8gf2tvdw0s3jn54khce6mua7l";
const BASE58: &str = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

pub fn classify(addr: &str) -> Result<(Kind, Network), String> {
    if addr.trim() != addr || addr.is_empty() {
        return Err("address has surrounding whitespace or is empty".into());
    }
    let lower = addr.to_lowercase();
    for (hrp, kind, network) in [
        ("utest", Kind::Unified, Network::Test),
        ("u", Kind::Unified, Network::Main),
        ("ztestsapling", Kind::Sapling, Network::Test),
        ("zs", Kind::Sapling, Network::Main),
    ] {
        let Some(data) = lower.strip_prefix(&format!("{hrp}1")) else {
            continue;
        };
        if addr != lower && addr != addr.to_uppercase() {
            return Err("mixed-case Bech32 address".into());
        }
        if data.is_empty() || !data.chars().all(|c| BECH32.contains(c)) {
            return Err("address contains characters outside the Bech32 alphabet".into());
        }
        return Ok((kind, network));
    }
    let base58 =
        |rest: &str| rest.chars().count() == 33 && rest.chars().all(|c| BASE58.contains(c));
    let mut chars = addr.chars();
    if chars.next() == Some('t') {
        let second = chars.next();
        let rest = chars.as_str();
        if matches!(second, Some('1' | '3')) && base58(rest) {
            return Ok((Kind::Transparent, Network::Main));
        }
        if matches!(second, Some('m' | '2')) && base58(rest) {
            return Ok((Kind::Transparent, Network::Test));
        }
    }
    Err("not a recognised Zcash address".into())
}

/// Members are paid to shielded addresses only: a transparent output would publish who got how much.
pub fn assert_shielded(addr: &str) -> Result<Network, String> {
    match classify(addr)? {
        (Kind::Transparent, _) => {
            Err("transparent addresses are not allowed: every share must be shielded".into())
        }
        (_, network) => Ok(network),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_by_prefix() {
        assert_eq!(
            classify("utest1qpzry").unwrap(),
            (Kind::Unified, Network::Test)
        );
        assert_eq!(classify("u1qpzry").unwrap(), (Kind::Unified, Network::Main));
        assert_eq!(classify("U1QPZRY").unwrap(), (Kind::Unified, Network::Main));
        assert_eq!(
            classify("zs1qpzry").unwrap(),
            (Kind::Sapling, Network::Main)
        );
        assert_eq!(
            classify("ztestsapling1qpzry").unwrap(),
            (Kind::Sapling, Network::Test)
        );
        let t = format!("t1{}", "a".repeat(33));
        assert_eq!(classify(&t).unwrap(), (Kind::Transparent, Network::Main));
        assert!(assert_shielded(&t).is_err());
    }

    #[test]
    fn rejects_garbage() {
        assert!(classify(" u1qpzry").is_err());
        assert!(classify("u1Qpzry").is_err());
        assert!(classify("u1qpzrb").is_err());
        assert!(classify("bc1qpzry").is_err());
        assert!(classify("").is_err());
    }

    #[test]
    fn viewing_key_network() {
        assert_eq!(Network::of_ufvk("uviewtest1abc").unwrap(), Network::Test);
        assert_eq!(Network::of_ufvk("uview1abc").unwrap(), Network::Main);
        assert!(Network::of_ufvk("secret-extended-key-main1").is_err());
    }
}
