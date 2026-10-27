//! BonBagi watcher: the member-side verifier of BonBagi. A BonBagi payment is one shielded Zcash
//! transaction with one output per member of a collective, split as the members' signed charter
//! says. There is no server and no contract that could confirm "everyone got their share", and
//! there should not be: whoever ran it would learn who was paid how much. So each member checks
//! their own share, on their own machine, with their own viewing key; this program is that check.
//!
//! The charter rules (canonical bytes, hash, ZIP 215 signatures, split, memo, pay link) mirror
//! @bonbagi/core, the TypeScript the web app runs; `tests/vectors.rs` replays its outputs.

pub mod address;
pub mod args;
pub mod canonical;
pub mod charter;
pub mod check;
pub mod devtool;
pub mod follow;
pub mod hex;
pub mod memo;
pub mod paylink;
pub mod receipt;
pub mod render;
pub mod source;
pub mod split;
pub mod time;
pub mod zec;

use std::path::Path;

use args::{
    CharterSource, CheckArgs, Command, Env, MemberSel, ReceiptArgs, UfvkFrom, ViewSource, USAGE,
};
use charter::Verified;
use check::{PriceFrom, Status, Verdict};
use render::ReceiptOutcome;

/// Exit status: your share arrived and matches (or: the receipt is valid).
pub const EXIT_OK: i32 = 0;
/// Exit status: not arrived, not found, or does not match (or: the receipt is invalid).
pub const EXIT_FAILED: i32 = 1;
/// Exit status: usage, input or network error.
pub const EXIT_ERROR: i32 = 2;

/// Runs one command line and returns (exit status, stdout, stderr).
pub fn main_with(args: &[String], env: &Env) -> (i32, String, String) {
    let fail = |e: String| (EXIT_ERROR, String::new(), format!("error: {e}\n"));
    match args::parse(args, env) {
        Err(e) => fail(e),
        Ok(Command::Help) => (EXIT_OK, USAGE.to_string(), String::new()),
        Ok(Command::Version) => (
            EXIT_OK,
            format!("bonbagi-watcher {}\n", env!("CARGO_PKG_VERSION")),
            String::new(),
        ),
        Ok(Command::Check(c)) => match check(&c, env) {
            Ok((code, out)) => (code, out, String::new()),
            Err(e) => fail(e),
        },
        Ok(Command::VerifyReceipt(r)) => match verify_receipt(&r) {
            Ok((code, out)) => (code, out, String::new()),
            Err(e) => fail(e),
        },
    }
}

fn read(path: &Path, what: &str) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("cannot read {what} {}: {e}", path.display()))
}

/// The charter, fully verified, and the pay link's price if it came from a link.
fn load_charter(src: &CharterSource) -> Result<(Verified, Option<u64>), String> {
    let (signed, price) = match src {
        CharterSource::File(p) => {
            let text = read(p, "charter")?;
            let signed = charter::parse_charter_file(&text)
                .map_err(|e| format!("charter {}: {e}", p.display()))?;
            (signed, None)
        }
        CharterSource::Link(l) => {
            let link = paylink::decode(l).map_err(|e| format!("pay link: {e}"))?;
            (link.signed, link.price_zat)
        }
    };
    let v = charter::verify(&signed).map_err(|e| format!("charter: {e}"))?;
    if !v.fully_signed() {
        let names: Vec<&str> = v
            .unsigned
            .iter()
            .map(|&i| v.charter.members[i].label.as_str())
            .collect();
        return Err(format!(
            "the charter is not signed by every member (missing or invalid: {}); the pay page \
             refuses such a charter, and so does the watcher",
            names.join(", ")
        ));
    }
    Ok((v, price))
}

fn pick_member(
    v: &Verified,
    sel: Option<&MemberSel>,
    key: Option<&receipt::MemberKey>,
) -> Result<usize, String> {
    let members = &v.charter.members;
    let by_key = key
        .map(|k| {
            members
                .iter()
                .position(|m| m.pubkey == k.pubkey)
                .ok_or_else(|| {
                    "the key file does not belong to any member of this charter".to_string()
                })
        })
        .transpose()?;
    let by_sel = sel
        .map(|s| match s {
            MemberSel::Number(n) if (1..=members.len()).contains(n) => Ok(n - 1),
            MemberSel::Number(n) => Err(format!(
                "--member {n}: this charter has members 1 to {}",
                members.len()
            )),
            MemberSel::Label(l) => {
                let hits: Vec<usize> = (0..members.len())
                    .filter(|&i| members[i].label.eq_ignore_ascii_case(l))
                    .collect();
                match hits.as_slice() {
                    [i] => Ok(*i),
                    [] => Err(format!("--member {l}: no member has that label")),
                    _ => Err(format!(
                        "--member {l}: several members match; use the number"
                    )),
                }
            }
        })
        .transpose()?;
    match (by_key, by_sel) {
        (Some(k), Some(s)) if k != s => Err(format!(
            "--member says member {}, but the key file is member {}",
            s + 1,
            k + 1
        )),
        (Some(i), _) | (None, Some(i)) => Ok(i),
        (None, None) => {
            Err("which member are you? pass --member <N|LABEL> or --key-file <FILE>".into())
        }
    }
}

fn check(a: &CheckArgs, env: &Env) -> Result<(i32, String), String> {
    let (verified, link_price) = load_charter(&a.charter)?;
    let key = a
        .key_file
        .as_ref()
        .map(|p| receipt::MemberKey::from_text(&read(p, "key file")?))
        .transpose()?;
    let me = pick_member(&verified, a.member.as_ref(), key.as_ref())?;
    let network = verified.charter.network;

    let live = match &a.view {
        ViewSource::Snapshot(_) => None,
        ViewSource::Live(l) => {
            let ufvk = match &l.ufvk {
                UfvkFrom::File(p) => read(p, "viewing key file")?,
                UfvkFrom::Env => env.ufvk.clone().unwrap_or_default(),
            };
            let ufvk = ufvk.trim().to_string();
            let key_net = address::Network::of_ufvk(&ufvk)?;
            if key_net != network {
                return Err(format!(
                    "the viewing key is for {}, the charter is on {}",
                    key_net.name(),
                    network.name()
                ));
            }
            Some((
                source::LiveArgs {
                    ufvk,
                    birthday: l.birthday,
                    wallet_dir: l.wallet_dir.clone(),
                    devtool: l.devtool.clone(),
                    server: l.server.clone(),
                    connection: l.connection.clone(),
                },
                l,
            ))
        }
    };

    let price = match (a.price_zat, link_price) {
        (Some(p), _) => Some((p, PriceFrom::Flag)),
        (None, Some(p)) => Some((p, PriceFrom::Link)),
        (None, None) => None,
    };
    let params = check::Params {
        verified: &verified,
        me,
        order: a.order.as_deref(),
        price,
        min_conf: a.min_conf,
    };
    let observe = || -> Result<(source::View, check::Check), String> {
        let view = match (&a.view, &live) {
            (ViewSource::Snapshot(p), _) => source::load_snapshot(p)?,
            (_, Some((args, l))) => {
                let view = source::scan(args)?;
                if let Some(path) = &l.save_snapshot {
                    std::fs::write(path, source::to_snapshot(&view, None))
                        .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
                }
                view
            }
            (ViewSource::Live(_), None) => unreachable!("live arguments are prepared above"),
        };
        if view.network != network {
            return Err(format!(
                "the snapshot is from {}, the charter is on {}",
                view.network.name(),
                network.name()
            ));
        }
        let result = check::run(&params, &view)?;
        Ok((view, result))
    };
    let follow = live.as_ref().and_then(|(_, l)| l.follow);
    let (view, result) = match follow {
        None => observe()?,
        Some(f) => follow::until_settled(
            observe,
            std::time::Duration::from_secs(f.interval_secs),
            std::time::Duration::from_secs(60 * f.max_wait_mins),
            |v, c| {
                // Progress goes to stderr straight away; stdout keeps only the final report.
                let detail = if c.verdict == Verdict::Mismatch {
                    format!(" ({})", c.summary)
                } else {
                    String::new()
                };
                eprintln!(
                    "{}  {} at block {}{detail}; scanning again in {} s (Ctrl+C to stop)",
                    time::rfc3339(time::now_secs(None)),
                    c.verdict.word(),
                    v.tip_height,
                    f.interval_secs
                );
            },
            std::thread::sleep,
        )?,
    };

    let receipt = match (&a.receipt, &key, a.order.as_deref()) {
        (Some(path), Some(key), Some(order)) => {
            let row = result
                .rows
                .iter()
                .find(|r| r.order == order && r.status == Status::Paid);
            match row {
                Some(row) if result.verdict == Verdict::Paid => {
                    let ctx = receipt::Context {
                        verified: &verified,
                        me,
                        source: match view.origin {
                            source::Origin::Snapshot { .. } => "snapshot",
                            source::Origin::Live { .. } => "live",
                        },
                        checked_at: time::rfc3339(time::now_secs(env.source_date_epoch.as_deref())),
                    };
                    let text = receipt::issue(&ctx, row, key)?;
                    std::fs::write(path, text)
                        .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
                    ReceiptOutcome::Written {
                        path: path.display().to_string(),
                        pubkey: key.pubkey.clone(),
                    }
                }
                _ => {
                    ReceiptOutcome::NotWritten("there is no arrived payment to confirm yet".into())
                }
            }
        }
        _ => ReceiptOutcome::NotAsked,
    };

    let report = render::Report {
        verified: &verified,
        me,
        view: &view,
        order: a.order.as_deref(),
        price,
        min_conf: a.min_conf,
        check: &result,
        receipt,
    };
    let out = if a.json {
        let mut s =
            serde_json::to_string_pretty(&render::json(&report)).expect("report serializes");
        s.push('\n');
        s
    } else {
        render::text(&report)
    };
    Ok((render::exit_code(result.verdict), out))
}

fn verify_receipt(a: &ReceiptArgs) -> Result<(i32, String), String> {
    let charter = a.charter.as_ref().map(load_charter).transpose()?;
    let text = read(&a.file, "receipt")?;
    let checked = receipt::verify(&text, charter.as_ref().map(|c| &c.0))
        .map_err(|e| format!("receipt {}: {e}", a.file.display()))?;
    let out = if a.json {
        let mut s =
            serde_json::to_string_pretty(&render::receipt_json(&checked)).expect("serializes");
        s.push('\n');
        s
    } else {
        render::receipt_text(&a.file.display().to_string(), &checked)
    };
    Ok((if checked.valid { EXIT_OK } else { EXIT_FAILED }, out))
}
