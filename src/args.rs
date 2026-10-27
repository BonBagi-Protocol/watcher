//! Command line parsing, hand-written to keep the dependency tree small and auditable.

use std::path::PathBuf;

use crate::memo::is_order_id;
use crate::zec::parse_zec;

pub const USAGE: &str = "\
bonbagi-watcher: did my share of a BonBagi payment arrive?
Runs on your own machine with your own viewing key. Nothing about you or your amounts is sent
anywhere; a live scan only downloads blocks from a Zcash lightwalletd server.

USAGE:
    bonbagi-watcher check (--charter <FILE> | --link <URL>)
                          (--snapshot <FILE> | --ufvk-file <FILE>) [OPTIONS]
    bonbagi-watcher verify-receipt <RECEIPT> [--charter <FILE> | --link <URL>] [--json]

    check            Find your share of each BonBagi payment, check it against the charter,
                     optionally write a receipt signed with your charter key.
    verify-receipt   Check someone's receipt offline (signature, charter, member, amount).

CHARTER (exactly one):
    --charter <FILE>        Signed charter file (bonbagi-charter.json from the charter page).
    --link <URL>            Pay link; the charter is read from its #c=... fragment, locally.

YOUR RECEIVED OUTPUTS (check, exactly one):
    --snapshot <FILE>       Offline: received outputs as JSON (docs/FORMATS.md). No network.
    --ufvk-file <FILE>      Live: a file holding your unified full viewing key (uview1... or
                            uviewtest1...); or set BONBAGI_UFVK. Never a spending key or seed.

WHO AND WHAT (check):
    --member <N|LABEL>      Which member you are: 1-based number or your label.
                            Not needed with --key-file.
    --key-file <FILE>       Your charter private key (bonbagi-private-key.txt). Picks your
                            member slot and signs the receipt. It never leaves this machine.
    --order <ID>            Only this order: the o=... in the memo, e.g. ord-k7m2qx9p.
    --price <ZEC>           Total the buyer paid; your share must then match exactly. Without
                            it, a pay link's price is used if it has one; otherwise the amount
                            must be your share of some total the charter allows.
    --min-conf <N>          Confirmations before a payment counts as arrived [default: 1].
    --receipt <FILE>        Write a signed receipt for --order (needs --key-file).

LIVE SCAN (check with --ufvk-file or BONBAGI_UFVK):
    --birthday <HEIGHT>     First block to scan. Needed once, when the watch-only wallet is
                            created: when your wallet was made, or before your first payment.
    --wallet-dir <DIR>      Watch-only wallet folder [default: bonbagi-watch-<network>-<hash>].
    --devtool <PATH>        zcash-devtool binary [default: $BONBAGI_DEVTOOL or zcash-devtool].
    --server <SERVER>       lightwalletd server: zecrocks (default) or host:port.
    --connection <MODE>     direct (default), tor, or socks5://host:port. tor hides your IP
                            address from the server.
    --save-snapshot <FILE>  Also write the BonBagi outputs the scan found as a snapshot.

OUTPUT:
    --json                  Machine-readable report.

EXIT STATUS:
    0 your share arrived and matches the charter (verify-receipt: the receipt is valid)
    1 not arrived yet, not found, or does not match (verify-receipt: invalid)
    2 usage, input or network error
";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CharterSource {
    File(PathBuf),
    Link(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UfvkFrom {
    File(PathBuf),
    Env,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Live {
    pub ufvk: UfvkFrom,
    pub birthday: Option<u32>,
    pub wallet_dir: Option<PathBuf>,
    pub devtool: PathBuf,
    pub server: Option<String>,
    pub connection: Option<String>,
    pub save_snapshot: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViewSource {
    Snapshot(PathBuf),
    Live(Live),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemberSel {
    Number(usize),
    Label(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckArgs {
    pub charter: CharterSource,
    pub view: ViewSource,
    pub member: Option<MemberSel>,
    pub key_file: Option<PathBuf>,
    pub order: Option<String>,
    pub price_zat: Option<u64>,
    pub min_conf: u32,
    pub receipt: Option<PathBuf>,
    pub json: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiptArgs {
    pub file: PathBuf,
    pub charter: Option<CharterSource>,
    pub json: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Help,
    Version,
    Check(Box<CheckArgs>),
    VerifyReceipt(ReceiptArgs),
}

/// Environment the command line may fall back to.
#[derive(Debug, Clone, Default)]
pub struct Env {
    pub ufvk: Option<String>,
    pub devtool: Option<String>,
    pub source_date_epoch: Option<String>,
}

#[derive(Default)]
struct Flags {
    charter: Option<String>,
    link: Option<String>,
    snapshot: Option<String>,
    ufvk_file: Option<String>,
    member: Option<String>,
    key_file: Option<String>,
    order: Option<String>,
    price: Option<String>,
    min_conf: Option<String>,
    receipt: Option<String>,
    birthday: Option<String>,
    wallet_dir: Option<String>,
    devtool: Option<String>,
    server: Option<String>,
    connection: Option<String>,
    save_snapshot: Option<String>,
    json: bool,
    positional: Vec<String>,
}

fn read_flags(args: &[String]) -> Result<Flags, String> {
    let mut f = Flags::default();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--json" {
            f.json = true;
            continue;
        }
        if !a.starts_with("--") {
            f.positional.push(a.clone());
            continue;
        }
        let (name, inline) = match a.split_once('=') {
            Some((n, v)) => (n, Some(v.to_string())),
            None => (a.as_str(), None),
        };
        let slot = match name {
            "--charter" => &mut f.charter,
            "--link" => &mut f.link,
            "--snapshot" => &mut f.snapshot,
            "--ufvk-file" => &mut f.ufvk_file,
            "--member" => &mut f.member,
            "--key-file" => &mut f.key_file,
            "--order" => &mut f.order,
            "--price" => &mut f.price,
            "--min-conf" => &mut f.min_conf,
            "--receipt" => &mut f.receipt,
            "--birthday" => &mut f.birthday,
            "--wallet-dir" => &mut f.wallet_dir,
            "--devtool" => &mut f.devtool,
            "--server" => &mut f.server,
            "--connection" => &mut f.connection,
            "--save-snapshot" => &mut f.save_snapshot,
            "--ufvk" => {
                return Err(
                    "pass the viewing key with --ufvk-file <FILE> or BONBAGI_UFVK, not on \
                            the command line, where shell history and other users can see it"
                        .into(),
                )
            }
            _ => return Err(format!("unknown option {name} (see --help)")),
        };
        if slot.is_some() {
            return Err(format!("{name} given twice"));
        }
        let value = match inline {
            Some(v) => v,
            None => it
                .next()
                .cloned()
                .ok_or_else(|| format!("{name} needs a value"))?,
        };
        *slot = Some(value);
    }
    Ok(f)
}

fn charter_source(f: &mut Flags, required: bool) -> Result<Option<CharterSource>, String> {
    match (f.charter.take(), f.link.take()) {
        (Some(_), Some(_)) => Err("give the charter once: --charter <FILE> or --link <URL>".into()),
        (Some(p), None) => Ok(Some(CharterSource::File(p.into()))),
        (None, Some(l)) => Ok(Some(CharterSource::Link(l))),
        (None, None) if required => {
            Err("which charter? pass --charter <FILE> or --link <URL> (see --help)".into())
        }
        (None, None) => Ok(None),
    }
}

fn number<T: std::str::FromStr>(name: &str, v: Option<String>) -> Result<Option<T>, String> {
    v.map(|s| {
        s.parse()
            .map_err(|_| format!("{name} must be a whole number, got \"{s}\""))
    })
    .transpose()
}

pub fn parse(args: &[String], env: &Env) -> Result<Command, String> {
    let Some(cmd) = args.first() else {
        return Ok(Command::Help);
    };
    let rest = &args[1..];
    match cmd.as_str() {
        "-h" | "--help" | "help" => Ok(Command::Help),
        "-V" | "--version" => Ok(Command::Version),
        "check" => parse_check(rest, env).map(|c| Command::Check(Box::new(c))),
        "verify-receipt" => {
            let mut f = read_flags(rest)?;
            let charter = charter_source(&mut f, false)?;
            let only = |s: &Option<String>| s.is_none();
            if ![
                &f.snapshot,
                &f.ufvk_file,
                &f.member,
                &f.key_file,
                &f.order,
                &f.price,
            ]
            .iter()
            .all(|s| only(s))
            {
                return Err(
                    "verify-receipt takes a receipt file and optionally --charter or --link".into(),
                );
            }
            let [file] = f.positional.as_slice() else {
                return Err("verify-receipt needs exactly one receipt file".into());
            };
            Ok(Command::VerifyReceipt(ReceiptArgs {
                file: file.into(),
                charter,
                json: f.json,
            }))
        }
        other => Err(format!(
            "unknown command {other} (try check, verify-receipt or --help)"
        )),
    }
}

fn parse_check(args: &[String], env: &Env) -> Result<CheckArgs, String> {
    let mut f = read_flags(args)?;
    if let Some(p) = f.positional.first() {
        return Err(format!("unexpected argument {p} (see --help)"));
    }
    let charter = charter_source(&mut f, true)?.expect("required");
    let live_only = [
        ("--birthday", f.birthday.is_some()),
        ("--wallet-dir", f.wallet_dir.is_some()),
        ("--devtool", f.devtool.is_some()),
        ("--server", f.server.is_some()),
        ("--connection", f.connection.is_some()),
        ("--save-snapshot", f.save_snapshot.is_some()),
    ];
    let view = match (f.snapshot.take(), f.ufvk_file.take()) {
        (Some(_), Some(_)) => {
            return Err(
                "give one source: --snapshot <FILE> (offline) or --ufvk-file <FILE> (live)".into(),
            )
        }
        (Some(path), None) => {
            if let Some((name, _)) = live_only.iter().find(|(_, set)| *set) {
                return Err(format!("{name} is for live scans, not --snapshot"));
            }
            ViewSource::Snapshot(path.into())
        }
        (None, ufvk_file) => {
            let ufvk = match ufvk_file {
                Some(p) => UfvkFrom::File(p.into()),
                None if env.ufvk.as_deref().is_some_and(|u| !u.trim().is_empty()) => UfvkFrom::Env,
                None => {
                    return Err(
                        "where are your received outputs? pass --snapshot <FILE> (offline), \
                                or your viewing key with --ufvk-file <FILE> or BONBAGI_UFVK (live)"
                            .into(),
                    )
                }
            };
            ViewSource::Live(Live {
                ufvk,
                birthday: number("--birthday", f.birthday.take())?,
                wallet_dir: f.wallet_dir.take().map(PathBuf::from),
                devtool: f
                    .devtool
                    .take()
                    .or_else(|| env.devtool.clone())
                    .unwrap_or_else(|| "zcash-devtool".into())
                    .into(),
                server: f.server.take(),
                connection: f.connection.take(),
                save_snapshot: f.save_snapshot.take().map(PathBuf::from),
            })
        }
    };
    let member = f.member.take().map(|m| match m.parse::<usize>() {
        Ok(n) => MemberSel::Number(n),
        Err(_) => MemberSel::Label(m),
    });
    if let Some(o) = &f.order {
        if !is_order_id(o) {
            return Err(
                "--order must be 1–32 letters, digits or dashes (e.g. ord-k7m2qx9p)".into(),
            );
        }
    }
    let price_zat = f
        .price
        .take()
        .map(|p| parse_zec(&p).map_err(|e| format!("--price: {e}")))
        .transpose()?;
    if price_zat == Some(0) {
        return Err("--price must be more than zero".into());
    }
    if price_zat.is_some() && f.order.is_none() {
        return Err("--price is the total of one order: pass --order <ID> with it".into());
    }
    let min_conf = number("--min-conf", f.min_conf.take())?.unwrap_or(1);
    if min_conf == 0 {
        return Err("--min-conf must be at least 1".into());
    }
    let receipt = f.receipt.take().map(PathBuf::from);
    if receipt.is_some() && (f.order.is_none() || f.key_file.is_none()) {
        return Err("--receipt needs --order <ID> and --key-file <FILE> (the receipt is signed with your charter key)".into());
    }
    Ok(CheckArgs {
        charter,
        view,
        member,
        key_file: f.key_file.take().map(PathBuf::from),
        order: f.order.take(),
        price_zat,
        min_conf,
        receipt,
        json: f.json,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn snapshot_check() {
        let c = parse(
            &args("check --charter c.json --snapshot s.json --member 1 --order ord-1 --price 0.003 --json"),
            &Env::default(),
        )
        .unwrap();
        let Command::Check(c) = c else { panic!() };
        assert_eq!(c.charter, CharterSource::File("c.json".into()));
        assert_eq!(c.view, ViewSource::Snapshot("s.json".into()));
        assert_eq!(c.member, Some(MemberSel::Number(1)));
        assert_eq!(c.price_zat, Some(300_000));
        assert!(c.json);
    }

    #[test]
    fn live_check_takes_the_key_from_a_file_or_the_environment() {
        let env = Env {
            ufvk: Some("uviewtest1x".into()),
            devtool: Some("/opt/devtool".into()),
            ..Env::default()
        };
        let Command::Check(c) = parse(
            &args("check --link https://x/#c=a --member Dex --birthday 5"),
            &env,
        )
        .unwrap() else {
            panic!()
        };
        let ViewSource::Live(l) = c.view else {
            panic!()
        };
        assert_eq!(l.ufvk, UfvkFrom::Env);
        assert_eq!(l.devtool, PathBuf::from("/opt/devtool"));
        assert_eq!(l.birthday, Some(5));
        assert_eq!(c.member, Some(MemberSel::Label("Dex".into())));
    }

    #[test]
    fn refuses_unsafe_or_incomplete_input() {
        let e = Env::default();
        let err = |s: &str| parse(&args(s), &e).unwrap_err();
        assert!(err("check --charter c --ufvk uview1abc").contains("--ufvk-file"));
        assert!(err("check --charter c").contains("--snapshot"));
        assert!(err("check --snapshot s").contains("which charter"));
        assert!(err("check --charter c --link l --snapshot s").contains("once"));
        assert!(err("check --charter c --snapshot s --birthday 3").contains("live"));
        assert!(err("check --charter c --snapshot s --price 1").contains("--order"));
        assert!(
            err("check --charter c --snapshot s --order ord-1 --receipt r").contains("--key-file")
        );
        assert!(err("check --charter c --snapshot s --order bad_id").contains("--order"));
        assert!(err("check --charter c --snapshot s --min-conf 0").contains("at least 1"));
        assert!(err("check --charter c --snapshot s --member 1 --member 2").contains("twice"));
        assert!(err("frobnicate").contains("unknown command"));
        assert_eq!(parse(&[], &e).unwrap(), Command::Help);
    }

    #[test]
    fn verify_receipt_args() {
        let Command::VerifyReceipt(r) = parse(
            &args("verify-receipt r.json --charter=c.json"),
            &Env::default(),
        )
        .unwrap() else {
            panic!()
        };
        assert_eq!(r.file, PathBuf::from("r.json"));
        assert_eq!(r.charter, Some(CharterSource::File("c.json".into())));
        assert!(parse(&args("verify-receipt"), &Env::default()).is_err());
    }
}
