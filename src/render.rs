//! Text for people, JSON for scripts. The text names the network, the member and the source so
//! a screenshot of it is self-explanatory.

use serde_json::{json, Value};

use crate::charter::Verified;
use crate::check::{pct, show_totals, Check, PriceFrom, Row, Status, Verdict};
use crate::receipt;
use crate::source::{Origin, View};
use crate::zec::{format_zec, with_ticker};

pub enum ReceiptOutcome {
    NotAsked,
    Written { path: String, pubkey: String },
    NotWritten(String),
}

pub struct Report<'a> {
    pub verified: &'a Verified,
    pub me: usize,
    pub view: &'a View,
    pub order: Option<&'a str>,
    pub price: Option<(u64, PriceFrom)>,
    pub min_conf: u32,
    pub check: &'a Check,
    pub receipt: ReceiptOutcome,
}

fn short_key(k: &str) -> String {
    format!("{}…{}", &k[..8], &k[k.len() - 4..])
}

fn source_lines(view: &View) -> String {
    match &view.origin {
        Origin::Snapshot { path, note } => {
            let mut s = format!(
                "Source   snapshot {} · chain tip {}\n",
                path.display(),
                view.tip_height
            );
            if let Some(n) = note {
                s.push_str(&format!("         {n}\n"));
            }
            s
        }
        Origin::Live {
            wallet_dir,
            created,
            server,
            connection,
            transactions,
            elapsed,
        } => format!(
            "Source   live scan, {} · watch-only wallet {} ({})\n         {transactions} transaction(s) · server {server} ({connection}) · {:.1} s · chain tip {}\n",
            view.network.name(),
            wallet_dir.display(),
            if *created { "created now" } else { "existing" },
            elapsed.as_secs_f64(),
            view.tip_height
        ),
    }
}

fn row_line(r: &Row, net: crate::address::Network) -> String {
    let height = r.height.map_or("-".to_string(), |h| h.to_string());
    format!(
        "  {:<14} {:<9} {:>14}  {:>9} {:>5}  {}…\n",
        r.order,
        r.status.word(),
        with_ticker(r.amount_zat, net),
        height,
        r.confirmations,
        &r.txid[..16]
    )
}

pub fn text(r: &Report) -> String {
    let v = r.verified;
    let c = &v.charter;
    let m = &c.members[r.me];
    let net = c.network;
    let n = c.members.len();
    let mut s = format!(
        "BonBagi watcher {} · local check: your viewing key and amounts stay on this machine\n",
        env!("CARGO_PKG_VERSION")
    );
    s.push_str(&format!(
        "Charter  \"{}\" · {} · {} · signed by {n}/{n} members\n",
        c.title,
        net.name(),
        v.fingerprint()
    ));
    s.push_str(&format!(
        "You      member {}/{n} \"{}\" · {}% share · key {}\n",
        r.me + 1,
        m.label,
        pct(m.bps),
        short_key(&m.pubkey)
    ));
    s.push_str(&source_lines(r.view));
    if let Some((price, from)) = r.price {
        let from = match from {
            PriceFrom::Flag => "--price",
            PriceFrom::Link => "pay link",
        };
        let share = r.check.rows.first().and_then(|x| x.expected_zat);
        s.push_str(&format!(
            "Price    {} ({from}){}\n",
            with_ticker(price, net),
            share.map_or(String::new(), |z| format!(
                " → your share {}",
                with_ticker(z, net)
            ))
        ));
    }
    if r.check.rows.is_empty() {
        s.push_str("\n  (no BonBagi output for this charter in this wallet)\n");
    } else {
        s.push_str(&format!(
            "\n  {:<14} {:<9} {:>14}  {:>9} {:>5}  txid\n",
            "order", "status", "amount", "height", "conf"
        ));
        for row in &r.check.rows {
            s.push_str(&row_line(row, net));
            for p in &row.problems {
                s.push_str(&format!("{:17}! {p}\n", ""));
            }
            for note in &row.notes {
                s.push_str(&format!("{:17}· {note}\n", ""));
            }
            if row.problems.is_empty() && row.expected_zat.is_none() {
                if let Some(t) = row.totals {
                    s.push_str(&format!(
                        "{:17}· fits your {}% share of a {} total\n",
                        "",
                        pct(m.bps),
                        show_totals(t, net)
                    ));
                }
            }
        }
    }
    let mut left_out = Vec::new();
    if r.check.other_orders > 0 {
        left_out.push(format!(
            "{} payment(s) for other orders of this charter are not shown (run without --order to list them).",
            r.check.other_orders
        ));
    }
    if r.check.other_charters > 0 {
        left_out.push(format!(
            "{} BonBagi output(s) for a different charter were left out.",
            r.check.other_charters
        ));
    }
    if !left_out.is_empty() {
        s.push('\n');
        for l in left_out {
            s.push_str(&format!("  {l}\n"));
        }
    }
    s.push_str(&format!(
        "\n=> {}  {}\n",
        r.check.verdict.word(),
        r.check.summary
    ));
    if let Some(o) = r.order {
        let rows: Vec<&Row> = r.check.rows.iter().filter(|x| x.order == o).collect();
        if let [one] = rows.as_slice() {
            s.push_str(&format!("   txid {}\n", one.txid));
        }
    }
    match &r.receipt {
        ReceiptOutcome::NotAsked => {}
        ReceiptOutcome::Written { path, pubkey } => s.push_str(&format!(
            "Receipt  {path} · signed with key {}\n",
            short_key(pubkey)
        )),
        ReceiptOutcome::NotWritten(why) => s.push_str(&format!("Receipt  not written: {why}\n")),
    }
    s
}

fn row_json(r: &Row) -> Value {
    json!({
        "order": r.order,
        "status": match r.status {
            Status::Paid => "paid",
            Status::Pending => "pending",
            Status::Mismatch => "mismatch",
            Status::Expired => "expired",
        },
        "amount_zat": r.amount_zat.to_string(),
        "amount": format_zec(r.amount_zat),
        "expected_zat": r.expected_zat.map(|z| z.to_string()),
        "implied_total_zat": r.totals.map(|(lo, hi, _)| json!({"min": lo.to_string(), "max": hi.to_string()})),
        "txid": r.txid,
        "output_index": r.output_index,
        "pool": r.pool,
        "height": r.height,
        "confirmations": r.confirmations,
        "problems": r.problems,
        "notes": r.notes,
    })
}

pub fn json(r: &Report) -> Value {
    let v = r.verified;
    let c = &v.charter;
    let m = &c.members[r.me];
    let source = match &r.view.origin {
        Origin::Snapshot { path, note } => json!({
            "kind": "snapshot",
            "path": path.display().to_string(),
            "note": note,
            "tip_height": r.view.tip_height,
        }),
        Origin::Live {
            wallet_dir,
            created,
            server,
            connection,
            transactions,
            elapsed,
        } => json!({
            "kind": "live",
            "wallet_dir": wallet_dir.display().to_string(),
            "wallet_created": created,
            "server": server,
            "connection": connection,
            "transactions": transactions,
            "elapsed_ms": elapsed.as_millis() as u64,
            "tip_height": r.view.tip_height,
        }),
    };
    json!({
        "watcher": env!("CARGO_PKG_VERSION"),
        "charter": {
            "title": c.title,
            "network": c.network.code(),
            "hash": v.hash,
            "fingerprint": v.fingerprint(),
            "members": c.members.len(),
            "fully_signed": v.fully_signed(),
        },
        "member": {
            "index": r.me + 1,
            "of": c.members.len(),
            "label": m.label,
            "bps": m.bps,
            "pubkey": m.pubkey,
        },
        "source": source,
        "order": r.order,
        "price_zat": r.price.map(|p| p.0.to_string()),
        "price_from": r.price.map(|p| match p.1 { PriceFrom::Flag => "flag", PriceFrom::Link => "pay_link" }),
        "min_conf": r.min_conf,
        "payments": r.check.rows.iter().map(row_json).collect::<Vec<_>>(),
        "other_charters": r.check.other_charters,
        "other_orders": r.check.other_orders,
        "verdict": r.check.verdict.code(),
        "summary": r.check.summary,
        "receipt": match &r.receipt {
            ReceiptOutcome::NotAsked => Value::Null,
            ReceiptOutcome::Written { path, .. } => json!({"written": path}),
            ReceiptOutcome::NotWritten(why) => json!({"not_written": why}),
        },
    })
}

pub fn receipt_text(path: &str, c: &receipt::Checked) -> String {
    let mut s = format!("Receipt  {path}\n");
    for l in &c.lines {
        s.push_str(&format!("  {l}\n"));
    }
    s.push_str(&format!("\n=> {}\n", c.summary));
    s
}

pub fn receipt_json(c: &receipt::Checked) -> Value {
    json!({
        "valid": c.valid,
        "checks": c.lines,
        "summary": c.summary,
        "receipt": c.body,
    })
}

/// Exit status of `check`.
pub fn exit_code(v: Verdict) -> i32 {
    if v == Verdict::Paid {
        crate::EXIT_OK
    } else {
        crate::EXIT_FAILED
    }
}
