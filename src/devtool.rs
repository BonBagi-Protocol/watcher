//! Watch-only wallet through the `zcash-devtool` CLI (pinned commit, see the Dockerfile). The
//! wallet is created from the member's unified full viewing key: it can see what arrived, it
//! can never spend. Only paths, the server choice and command output cross this boundary.

use std::fmt;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

#[derive(Debug)]
pub enum Error {
    /// The binary could not be started (usually: not installed).
    NotFound(String),
    /// devtool exited with an error; carries its last error line, viewing keys redacted.
    Devtool(String),
    Parse(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotFound(bin) => write!(
                f,
                "zcash-devtool not found at `{bin}`: use the Docker image (README, \"Run it\"), \
                 install it, or pass --devtool <path>; --snapshot works without it"
            ),
            Error::Devtool(m) => write!(f, "zcash-devtool: {m}"),
            Error::Parse(m) => write!(f, "zcash-devtool output: {m}"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Devtool {
    pub bin: PathBuf,
    pub wallet_dir: PathBuf,
    /// `--server` for devtool: "zecrocks" (its default) or host:port list.
    pub server: Option<String>,
    /// `--connection` for devtool: "direct" (default), "tor" or "socks5://host:port".
    pub connection: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TxOutput {
    pub index: u32,
    pub pool: String,
    pub value_zat: u64,
    pub is_change: bool,
    /// Received by this wallet from outside it (not change, not an internal transfer).
    pub received: bool,
    /// Text memo, if the output has one.
    pub memo: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalletTx {
    pub txid: String,
    pub mined_height: Option<u32>,
    /// Never mined and past its expiry height: it will not be mined any more.
    pub expired: bool,
    pub outputs: Vec<TxOutput>,
}

const RETRIES: u32 = 3;

impl Devtool {
    fn connection_args(&self) -> Vec<String> {
        let mut a = Vec::new();
        if let Some(s) = &self.server {
            a.extend(["--server".to_string(), s.clone()]);
        }
        if let Some(c) = &self.connection {
            a.extend(["--connection".to_string(), c.clone()]);
        }
        a
    }

    fn run(&self, args: &[String]) -> Result<String, Error> {
        let out = Command::new(&self.bin)
            .arg("wallet")
            .arg("-w")
            .arg(&self.wallet_dir)
            .args(args)
            .output()
            .map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound => Error::NotFound(self.bin.display().to_string()),
                _ => Error::Devtool(e.to_string()),
            })?;
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let last = stderr
                .lines()
                .chain(stdout.lines())
                .rfind(|l| l.contains("rror"))
                .unwrap_or("non-zero exit")
                .trim();
            return Err(Error::Devtool(redact(last)));
        }
        Ok(stdout)
    }

    /// Read-only commands are retried after a transient lightwalletd error.
    fn run_retrying(&self, args: &[String]) -> Result<String, Error> {
        let mut attempt = 0;
        loop {
            match self.run(args) {
                Err(Error::Devtool(m)) if is_transient(&m) && attempt < RETRIES => {
                    attempt += 1;
                    std::thread::sleep(Duration::from_secs(2 * u64::from(attempt)));
                }
                other => return other,
            }
        }
    }

    /// Creates the watch-only wallet. `birthday` is the first block to scan.
    pub fn init_fvk(&self, name: &str, ufvk: &str, birthday: u32) -> Result<(), Error> {
        let mut a: Vec<String> = ["init-fvk", "--name", name, "--fvk", ufvk, "--birthday"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        a.push(birthday.to_string());
        a.extend(self.connection_args());
        self.run_retrying(&a).map(|_| ())
    }

    /// Downloads compact blocks and trial-decrypts them locally.
    pub fn sync(&self) -> Result<(), Error> {
        let mut a = vec!["sync".to_string()];
        a.extend(self.connection_args());
        self.run_retrying(&a).map(|_| ())
    }

    /// Fetches the full transactions so memos become readable (compact blocks carry no memos).
    pub fn enhance(&self) -> Result<(), Error> {
        let mut a = vec!["enhance".to_string()];
        a.extend(self.connection_args());
        self.run_retrying(&a).map(|_| ())
    }

    pub fn list_tx(&self) -> Result<Vec<WalletTx>, Error> {
        parse_list_tx(&self.run(&["list-tx".to_string()])?)
    }

    /// Chain tip the wallet last synced to, from `balance --json`.
    pub fn tip_height(&self) -> Result<u32, Error> {
        let out = self.run(&["balance".to_string(), "--json".to_string()])?;
        let v: serde_json::Value = out
            .lines()
            .rev()
            .find_map(|l| serde_json::from_str(l.trim()).ok())
            .ok_or_else(|| Error::Parse("no JSON in `balance --json`".into()))?;
        v.get("chain_tip_height")
            .and_then(serde_json::Value::as_u64)
            .and_then(|h| u32::try_from(h).ok())
            .ok_or_else(|| Error::Parse("no chain_tip_height in `balance --json`".into()))
    }
}

fn is_transient(message: &str) -> bool {
    let m = message.to_ascii_lowercase();
    m.contains("transport error") || m.contains("timed out") || m.contains("unavailable")
}

/// Error lines are shown to the user; never let a viewing key through, wherever it sits in the
/// line (`fvk=uview1…`, quoted, in parentheses).
fn redact(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(i) = rest.find("uview") {
        out.push_str(&rest[..i]);
        let key_len = rest[i..]
            .find(|c: char| !c.is_ascii_alphanumeric())
            .unwrap_or(rest.len() - i);
        out.push_str("<viewing key>");
        rest = &rest[i + key_len..];
    }
    out.push_str(rest);
    out
}

fn is_txid(s: &str) -> bool {
    crate::hex::is_lower(s, 64)
}

/// "0.00120000 TAZ (Change)" → 120000.
fn parse_value_zat(s: &str) -> Result<u64, Error> {
    let num = s.split_whitespace().next().unwrap_or_default();
    crate::zec::parse_zec(num).map_err(|_| Error::Parse(format!("value: {s}")))
}

/// `Memo::Text("…")` → the text. Rust debug escapes are undone; other memo kinds are not text.
fn parse_memo(s: &str) -> Option<String> {
    let inner = s.strip_prefix("Memo::Text(\"")?.strip_suffix("\")")?;
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next()? {
            'n' => out.push('\n'),
            't' => out.push('\t'),
            'r' => out.push('\r'),
            '0' => out.push('\0'),
            'u' => {
                let rest: String = chars.by_ref().take_while(|&c| c != '}').collect();
                let hex = rest.strip_prefix('{')?;
                out.push(char::from_u32(u32::from_str_radix(hex, 16).ok()?)?);
            }
            other => out.push(other),
        }
    }
    Some(out)
}

/// Parses `zcash-devtool wallet list-tx` (text mode) at the pinned commit.
pub fn parse_list_tx(out: &str) -> Result<Vec<WalletTx>, Error> {
    let mut txs: Vec<WalletTx> = Vec::new();
    for raw in out.lines() {
        let line = raw.trim();
        if is_txid(line) {
            txs.push(WalletTx {
                txid: line.to_string(),
                mined_height: None,
                expired: false,
                outputs: Vec::new(),
            });
            continue;
        }
        let Some(tx) = txs.last_mut() else { continue };
        if let Some(rest) = line.strip_prefix("Mined:") {
            tx.mined_height = rest.split_whitespace().next().and_then(|h| h.parse().ok());
        } else if line.starts_with("Expired") {
            tx.expired = true;
        } else if let Some(rest) = line.strip_prefix("Output ") {
            // "Output 3 (Ironwood)"
            let (idx, pool) = rest
                .split_once(' ')
                .ok_or_else(|| Error::Parse(format!("output: {line}")))?;
            tx.outputs.push(TxOutput {
                index: idx
                    .parse()
                    .map_err(|_| Error::Parse(format!("output: {line}")))?,
                pool: pool.trim_matches(|c| c == '(' || c == ')').to_string(),
                value_zat: 0,
                is_change: false,
                received: false,
                memo: None,
            });
        } else if let Some(o) = tx.outputs.last_mut() {
            if let Some(rest) = line.strip_prefix("Value:") {
                o.value_zat = parse_value_zat(rest.trim())?;
                o.is_change = rest.contains("(Change)");
            } else if line.starts_with("Received by account:") {
                o.received = true;
            } else if let Some(rest) = line.strip_prefix("Memo:") {
                o.memo = parse_memo(rest.trim());
            }
        }
    }
    Ok(txs)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shape of devtool's text output at the pinned commit (format from its list_tx.rs; the
    /// first transaction is the layout seen on testnet with Ironwood outputs).
    const SAMPLE: &str = "Transactions:
64795b2ba61d7998940277b246180a4bf03206d78b81086bcec7c0e28be54932
     Mined: 4508848 (2026-10-16 16:28:36.0 +00:00:00)
    Amount:   0.00180000 TAZ
  Fee paid: Unknown
  Output 0 (Ironwood)
    Value:   0.00180000 TAZ
    Received by account: 0b5f1e6a-0000-4000-8000-000000000001 (bonbagi-watch)
    Memo: Memo::Text(\"bonbagi:v1 o=ord-k7m2qx9p c=a18c7faf6f559412 m=1/2\")
d759bbf2605a9cc9a220dbc0383645133c01bb722a38fc22a0f13b36355d2ac0
   Unmined (expiry height: 4509000)
    Amount:   0.00060000 TAZ
  Fee paid: Unknown
  Output 1 (Orchard)
    Value:   0.00060000 TAZ
    Received by account: 0b5f1e6a-0000-4000-8000-000000000001 (bonbagi-watch)
    Memo: Memo::Text(\"caf\\u{301} \\\"quoted\\\"\")
  Output 2 (Ironwood)
    Value:   0.24931250 TAZ (Change)
    Memo: Memo::Empty
aa00000000000000000000000000000000000000000000000000000000000000
   Expired (expiry height: 4500000)
    Amount:   0.00010000 TAZ
  Output 0 (Sapling)
    Value:   0.00010000 TAZ
    Received by account: 0b5f1e6a-0000-4000-8000-000000000001
    Memo: Memo::Arbitrary(4a:54:53:31:01)
";

    #[test]
    fn parses_list_tx() {
        let txs = parse_list_tx(SAMPLE).unwrap();
        assert_eq!(txs.len(), 3);
        assert_eq!(txs[0].mined_height, Some(4_508_848));
        let o = &txs[0].outputs[0];
        assert!(o.received && !o.is_change);
        assert_eq!(
            (o.index, o.pool.as_str(), o.value_zat),
            (0, "Ironwood", 180_000)
        );
        assert_eq!(
            o.memo.as_deref(),
            Some("bonbagi:v1 o=ord-k7m2qx9p c=a18c7faf6f559412 m=1/2")
        );
        assert_eq!(txs[1].mined_height, None);
        assert!(!txs[1].expired);
        assert_eq!(
            txs[1].outputs[0].memo.as_deref(),
            Some("caf\u{301} \"quoted\"")
        );
        assert!(txs[1].outputs[1].is_change && !txs[1].outputs[1].received);
        assert!(txs[2].expired);
        assert_eq!(txs[2].outputs[0].memo, None);
    }

    #[test]
    fn viewing_keys_never_reach_error_messages() {
        assert_eq!(
            redact("Error: bad key 'uviewtest1abcdef' given"),
            "Error: bad key '<viewing key>' given"
        );
        assert_eq!(
            redact("fvk=uview1qqq,x(uviewtest1zz)"),
            "fvk=<viewing key>,x(<viewing key>)"
        );
        assert!(is_transient("Error: transport error"));
        assert!(!is_transient("Error: insufficient balance"));
    }

    #[test]
    fn a_missing_binary_is_reported_as_such() {
        let d = Devtool {
            bin: "zcash-devtool-that-does-not-exist".into(),
            wallet_dir: std::env::temp_dir().join("bonbagi-watcher-no-devtool"),
            server: None,
            connection: None,
        };
        assert!(matches!(d.sync(), Err(Error::NotFound(_))));
    }
}
