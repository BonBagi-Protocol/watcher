//! Where the member's received outputs come from: a live scan with their own viewing key, or a
//! snapshot file (offline, deterministic; also what a live scan can save). Either way the data
//! is the member's own and stays on their machine.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::address::Network;
use crate::devtool::{self, Devtool};

pub const SNAPSHOT_VERSION: u32 = 1;
/// Account name of the watch-only wallet.
pub const WATCH_ACCOUNT: &str = "bonbagi-watch";

/// One output this wallet received from someone else (change is never in a snapshot).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Received {
    pub txid: String,
    /// Block height; null while the transaction is not mined.
    pub height: Option<u32>,
    pub output_index: u32,
    /// Shielded pool as the wallet names it (Sapling, Orchard, Ironwood) or Transparent.
    pub pool: String,
    pub value_zat: u64,
    /// Text memo; null when the output has none.
    pub memo: Option<String>,
    /// Not mined and past its expiry height: it never will be.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub expired: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub bonbagi_snapshot: u32,
    /// "main" or "test".
    pub network: String,
    /// Chain tip the wallet had synced to; confirmations are counted from it.
    pub tip_height: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub outputs: Vec<Received>,
}

#[derive(Debug, Clone)]
pub enum Origin {
    Snapshot {
        path: PathBuf,
        note: Option<String>,
    },
    Live {
        wallet_dir: PathBuf,
        created: bool,
        server: String,
        connection: String,
        transactions: usize,
        elapsed: Duration,
    },
}

#[derive(Debug, Clone)]
pub struct View {
    pub network: Network,
    pub tip_height: u32,
    pub outputs: Vec<Received>,
    pub origin: Origin,
}

fn check_outputs(outputs: &[Received]) -> Result<(), String> {
    for o in outputs {
        if !crate::hex::is_lower(&o.txid, 64) {
            return Err(format!(
                "txid {} is not 64 lowercase hex characters",
                o.txid
            ));
        }
    }
    Ok(())
}

pub fn from_snapshot(text: &str) -> Result<Snapshot, String> {
    let s: Snapshot =
        serde_json::from_str(text).map_err(|e| format!("not a watcher snapshot: {e}"))?;
    if s.bonbagi_snapshot != SNAPSHOT_VERSION {
        return Err(format!(
            "unsupported snapshot version {}",
            s.bonbagi_snapshot
        ));
    }
    Network::parse(&s.network).ok_or("snapshot network must be main or test")?;
    check_outputs(&s.outputs)?;
    Ok(s)
}

pub fn load_snapshot(path: &Path) -> Result<View, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let s = from_snapshot(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(View {
        network: Network::parse(&s.network).expect("checked"),
        tip_height: s.tip_height,
        outputs: s.outputs,
        origin: Origin::Snapshot {
            path: path.to_path_buf(),
            note: s.note,
        },
    })
}

/// Only what the check needs is written: received outputs with a BonBagi memo.
pub fn to_snapshot(view: &View, note: Option<String>) -> String {
    let s = Snapshot {
        bonbagi_snapshot: SNAPSHOT_VERSION,
        network: view.network.code().into(),
        tip_height: view.tip_height,
        note,
        outputs: view
            .outputs
            .iter()
            .filter(|o| o.memo.as_deref().is_some_and(|m| m.starts_with("bonbagi:")))
            .cloned()
            .collect(),
    };
    let mut text = serde_json::to_string_pretty(&s).expect("snapshot serializes");
    text.push('\n');
    text
}

#[derive(Debug, Clone)]
pub struct LiveArgs {
    pub ufvk: String,
    pub birthday: Option<u32>,
    pub wallet_dir: Option<PathBuf>,
    pub devtool: PathBuf,
    pub server: Option<String>,
    pub connection: Option<String>,
}

/// Default wallet folder: one per viewing key, named by a short hash so it reveals nothing.
pub fn default_wallet_dir(network: Network, ufvk: &str) -> PathBuf {
    let h = crate::hex::encode(&Sha256::digest(ufvk.trim().as_bytes()));
    PathBuf::from(format!("bonbagi-watch-{}-{}", network.code(), &h[..8]))
}

pub fn scan(a: &LiveArgs) -> Result<View, String> {
    let network = Network::of_ufvk(&a.ufvk)?;
    let wallet_dir = a
        .wallet_dir
        .clone()
        .unwrap_or_else(|| default_wallet_dir(network, &a.ufvk));
    let d = Devtool {
        bin: a.devtool.clone(),
        wallet_dir: wallet_dir.clone(),
        server: a.server.clone(),
        connection: a.connection.clone(),
    };
    let started = Instant::now();
    // devtool writes keys.toml when it creates a wallet.
    let created = !wallet_dir.join("keys.toml").is_file();
    let err = |e: devtool::Error| e.to_string();
    if created {
        let birthday = a.birthday.ok_or_else(|| {
            format!(
                "{} has no wallet yet: pass --birthday <block height> (when your wallet was \
                 created, or any block before your first BonBagi payment) so the scan does not \
                 start at the chain tip and miss it",
                wallet_dir.display()
            )
        })?;
        d.init_fvk(WATCH_ACCOUNT, a.ufvk.trim(), birthday)
            .map_err(err)?;
    }
    d.sync().map_err(err)?;
    d.enhance().map_err(err)?;
    let txs = d.list_tx().map_err(err)?;
    let tip_height = match d.tip_height() {
        Ok(h) => h,
        Err(_) => txs.iter().filter_map(|t| t.mined_height).max().unwrap_or(0),
    };
    let outputs = txs
        .iter()
        .flat_map(|t| {
            t.outputs
                .iter()
                .filter(|o| o.received && !o.is_change)
                .map(|o| Received {
                    txid: t.txid.clone(),
                    height: t.mined_height,
                    output_index: o.index,
                    pool: o.pool.clone(),
                    value_zat: o.value_zat,
                    memo: o.memo.clone(),
                    expired: t.expired,
                })
        })
        .collect();
    Ok(View {
        network,
        tip_height,
        outputs,
        origin: Origin::Live {
            wallet_dir,
            created,
            server: a.server.clone().unwrap_or_else(|| "zecrocks".into()),
            connection: a.connection.clone().unwrap_or_else(|| "direct".into()),
            transactions: txs.len(),
            elapsed: started.elapsed(),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_roundtrip_keeps_only_bonbagi_outputs() {
        let r = |memo: Option<&str>| Received {
            txid: "ab".repeat(32),
            height: Some(10),
            output_index: 0,
            pool: "Orchard".into(),
            value_zat: 1,
            memo: memo.map(String::from),
            expired: false,
        };
        let view = View {
            network: Network::Test,
            tip_height: 12,
            outputs: vec![
                r(Some("bonbagi:v1 o=a c=0123456789abcdef m=1/2")),
                r(Some("hi")),
                r(None),
            ],
            origin: Origin::Snapshot {
                path: "x".into(),
                note: None,
            },
        };
        let s = from_snapshot(&to_snapshot(&view, None)).unwrap();
        assert_eq!(s.outputs.len(), 1);
        assert_eq!(s.tip_height, 12);
        assert!(!to_snapshot(&view, None).contains("expired"));
    }

    #[test]
    fn bad_snapshots_are_errors() {
        assert!(from_snapshot("[]")
            .unwrap_err()
            .contains("not a watcher snapshot"));
        let bad = r#"{"bonbagi_snapshot":1,"network":"test","tip_height":1,"outputs":[{"txid":"XY","height":1,"output_index":0,"pool":"Orchard","value_zat":1,"memo":null}]}"#;
        assert!(from_snapshot(bad).unwrap_err().contains("txid"));
        let typo = r#"{"bonbagi_snapshot":1,"network":"test","tip_height":1,"outputs":[],"tip":2}"#;
        assert!(from_snapshot(typo).is_err());
    }

    #[test]
    fn a_new_live_wallet_needs_a_birthday() {
        let a = LiveArgs {
            ufvk: "uviewtest1qqqq".into(),
            birthday: None,
            wallet_dir: Some(std::env::temp_dir().join("bonbagi-watcher-test-no-wallet")),
            devtool: "zcash-devtool-that-does-not-exist".into(),
            server: None,
            connection: None,
        };
        assert!(scan(&a).unwrap_err().contains("--birthday"));
        let a = LiveArgs {
            birthday: Some(1),
            ..a
        };
        assert!(scan(&a).unwrap_err().contains("zcash-devtool not found"));
        assert!(default_wallet_dir(Network::Test, "uviewtest1qqqq")
            .display()
            .to_string()
            .starts_with("bonbagi-watch-test-"));
    }
}
