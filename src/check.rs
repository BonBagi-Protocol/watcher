//! The member's question: "did my share of this order arrive, and is it what the charter says?"
//! Only outputs this wallet received are looked at, and only those whose memo names this
//! charter's fingerprint. Each one is checked against the member's place and share.

use std::collections::HashMap;

use crate::address::Network;
use crate::charter::Verified;
use crate::memo;
use crate::source::View;
use crate::split::{split_amount, totals_for_share};
use crate::zec::{with_ticker, MAX_ZAT};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PriceFrom {
    /// `--price`: the member knows what the buyer paid for this order.
    Flag,
    /// The pay link's `p=`: what the buyer was asked to pay; they could have changed it.
    Link,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Arrived, mined with enough confirmations, and matches the charter.
    Paid,
    /// Seen, but not mined yet or not enough confirmations.
    Pending,
    /// Arrived but does not match the charter (amount, member slot, pool).
    Mismatch,
    /// Never mined and past expiry: it will not arrive.
    Expired,
}

impl Status {
    pub fn word(self) -> &'static str {
        match self {
            Status::Paid => "PAID",
            Status::Pending => "PENDING",
            Status::Mismatch => "MISMATCH",
            Status::Expired => "EXPIRED",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Paid,
    Pending,
    NotFound,
    Mismatch,
}

impl Verdict {
    pub fn word(self) -> &'static str {
        match self {
            Verdict::Paid => "PAID",
            Verdict::Pending => "PENDING",
            Verdict::NotFound => "NOT FOUND",
            Verdict::Mismatch => "MISMATCH",
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Verdict::Paid => "paid",
            Verdict::Pending => "pending",
            Verdict::NotFound => "not_found",
            Verdict::Mismatch => "mismatch",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Row {
    pub order: String,
    pub txid: String,
    pub height: Option<u32>,
    pub confirmations: u32,
    pub output_index: u32,
    pub pool: String,
    pub amount_zat: u64,
    pub expected_zat: Option<u64>,
    /// Totals consistent with this amount, when no price was given: (lowest, highest, count).
    pub totals: Option<(u64, u64, u64)>,
    pub status: Status,
    /// Why it does not match (only for Mismatch).
    pub problems: Vec<String>,
    /// Facts worth knowing that do not change the status.
    pub notes: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Params<'a> {
    pub verified: &'a Verified,
    /// This member, 0-based.
    pub me: usize,
    pub order: Option<&'a str>,
    pub price: Option<(u64, PriceFrom)>,
    pub min_conf: u32,
}

#[derive(Debug, Clone)]
pub struct Check {
    pub rows: Vec<Row>,
    /// Outputs with a BonBagi memo for a different charter (another collective, or an edited one).
    pub other_charters: usize,
    /// Outputs for this charter but another order, when one order was asked for.
    pub other_orders: usize,
    pub verdict: Verdict,
    pub summary: String,
}

pub fn run(p: &Params, view: &View) -> Result<Check, String> {
    let charter = &p.verified.charter;
    let me = &charter.members[p.me];
    let bps: Vec<u32> = charter.members.iter().map(|m| m.bps).collect();
    let n = charter.members.len() as u32;
    let net = charter.network;
    let expected = match p.price {
        Some((price, from)) => Some((
            split_amount(price, &bps, charter.min_share_zat).map_err(|e| {
                format!(
                    "price {} cannot be split by this charter: {e}",
                    with_ticker(price, net)
                )
            })?[p.me],
            price,
            from,
        )),
        None => None,
    };

    let mut rows = Vec::new();
    let mut other_charters = 0;
    let mut other_orders = 0;
    for o in &view.outputs {
        let Some(m) = o.memo.as_deref().and_then(memo::parse) else {
            continue;
        };
        if m.fingerprint != p.verified.fingerprint() {
            other_charters += 1;
            continue;
        }
        if p.order.is_some_and(|want| want != m.order) {
            other_orders += 1;
            continue;
        }
        let mut problems = Vec::new();
        let mut notes = Vec::new();
        if m.member != p.me as u32 + 1 || m.of != n {
            problems.push(format!(
                "the memo is for member {}/{}, you are member {}/{n}",
                m.member,
                m.of,
                p.me + 1
            ));
        }
        if o.pool.eq_ignore_ascii_case("transparent") {
            problems
                .push("it arrived in the transparent pool: the amount is public on chain".into());
        }
        let mut totals = None;
        match expected {
            Some((want, price, from)) if o.value_zat != want => {
                let fits =
                    totals_for_share(o.value_zat, p.me, &bps, charter.min_share_zat, MAX_ZAT);
                match (from, fits) {
                    (PriceFrom::Link, Some(t)) => {
                        totals = Some(t);
                        notes.push(format!(
                            "the buyer paid a total of {} instead of the pay link's {}; your share of that total is right",
                            show_totals(t, net),
                            with_ticker(price, net)
                        ));
                    }
                    _ => problems.push(format!(
                        "expected {} ({}% of {}), got {}",
                        with_ticker(want, net),
                        pct(me.bps),
                        with_ticker(price, net),
                        with_ticker(o.value_zat, net)
                    )),
                }
            }
            Some(_) => {}
            None => match totals_for_share(o.value_zat, p.me, &bps, charter.min_share_zat, MAX_ZAT)
            {
                Some(t) => totals = Some(t),
                None => problems.push(format!(
                    "{} is not your {}% share of any total this charter allows",
                    with_ticker(o.value_zat, net),
                    pct(me.bps)
                )),
            },
        }
        let confirmations = match o.height {
            Some(h) if view.tip_height >= h => view.tip_height - h + 1,
            _ => 0,
        };
        let status = if !problems.is_empty() {
            Status::Mismatch
        } else if o.height.is_none() && o.expired {
            Status::Expired
        } else if o.height.is_none() || confirmations < p.min_conf {
            Status::Pending
        } else {
            Status::Paid
        };
        rows.push(Row {
            order: m.order,
            txid: o.txid.clone(),
            height: o.height,
            confirmations,
            output_index: o.output_index,
            pool: o.pool.clone(),
            amount_zat: o.value_zat,
            expected_zat: expected.map(|e| e.0),
            totals,
            status,
            problems,
            notes,
        });
    }

    let mut per_order: HashMap<String, usize> = HashMap::new();
    for r in &rows {
        *per_order.entry(r.order.clone()).or_default() += 1;
    }
    for r in &mut rows {
        let k = per_order[&r.order];
        if k > 1 {
            r.notes
                .push(format!("this order reached you in {k} outputs"));
        }
    }
    rows.sort_by(|a, b| {
        (a.height.unwrap_or(u32::MAX), &a.txid, a.output_index).cmp(&(
            b.height.unwrap_or(u32::MAX),
            &b.txid,
            b.output_index,
        ))
    });

    let relevant: Vec<&Row> = rows.iter().collect();
    let any = |s: Status| relevant.iter().any(|r| r.status == s);
    let verdict = match p.order {
        Some(_) if any(Status::Paid) => Verdict::Paid,
        Some(_) if any(Status::Mismatch) => Verdict::Mismatch,
        Some(_) if any(Status::Pending) => Verdict::Pending,
        Some(_) => Verdict::NotFound,
        None if relevant.iter().all(|r| r.status == Status::Expired) => Verdict::NotFound,
        None if any(Status::Mismatch) => Verdict::Mismatch,
        None if any(Status::Pending) => Verdict::Pending,
        None => Verdict::Paid,
    };
    let summary = summary(p, view, &relevant, verdict, net);
    Ok(Check {
        rows,
        other_charters,
        other_orders,
        verdict,
        summary,
    })
}

/// "60" for 6000 bp, "33.33" for 3333 bp.
pub fn pct(bps: u32) -> String {
    let (whole, frac) = (bps / 100, bps % 100);
    if frac == 0 {
        whole.to_string()
    } else {
        format!("{whole}.{frac:02}")
            .trim_end_matches('0')
            .to_string()
    }
}

pub fn show_totals((lo, hi, _): (u64, u64, u64), net: Network) -> String {
    if lo == hi {
        with_ticker(lo, net)
    } else {
        format!("{}–{}", crate::zec::format_zec(lo), with_ticker(hi, net))
    }
}

fn summary(p: &Params, view: &View, rows: &[&Row], verdict: Verdict, net: Network) -> String {
    let block = |r: &Row| match r.height {
        Some(h) => format!(
            "in block {h} ({} confirmation{})",
            r.confirmations,
            if r.confirmations == 1 { "" } else { "s" }
        ),
        None => "not mined yet".into(),
    };
    let first = |s: Status| rows.iter().find(|r| r.status == s).copied();
    match (p.order, verdict) {
        (Some(o), Verdict::Paid) => {
            let r = first(Status::Paid).expect("paid row");
            format!(
                "Your share of order {o} arrived: {} {}.",
                with_ticker(r.amount_zat, net),
                block(r)
            )
        }
        (Some(o), Verdict::Mismatch) => {
            let r = first(Status::Mismatch).expect("mismatch row");
            format!(
                "Order {o} reached you but does not match the charter: {}.",
                r.problems.join("; ")
            )
        }
        (Some(o), Verdict::Pending) => {
            let r = first(Status::Pending).expect("pending row");
            let wait = match r.height {
                None => "waiting to be mined".to_string(),
                Some(_) => format!("{} of {} confirmations", r.confirmations, p.min_conf),
            };
            format!(
                "Order {o} is on its way: {} seen, {wait}. Run again in a few minutes.",
                with_ticker(r.amount_zat, net)
            )
        }
        (Some(o), _) => format!(
            "No output for order {o} has reached this wallet (checked up to block {}). If the buyer \
             paid just now, run again in a few minutes; otherwise ask them for the transaction ID.",
            view.tip_height
        ),
        (None, Verdict::NotFound) => format!(
            "No BonBagi payment under this charter has reached this wallet (checked up to block {}).",
            view.tip_height
        ),
        (None, v) => {
            let count = |s: Status| rows.iter().filter(|r| r.status == s).count();
            let paid: u64 = rows
                .iter()
                .filter(|r| r.status == Status::Paid)
                .map(|r| r.amount_zat)
                .sum();
            let mut s = format!(
                "{} payment(s) to you under this charter: {} paid ({}), {} pending, {} mismatched.",
                rows.iter().filter(|r| r.status != Status::Expired).count(),
                count(Status::Paid),
                with_ticker(paid, net),
                count(Status::Pending),
                count(Status::Mismatch)
            );
            if v == Verdict::Mismatch {
                s.push_str(" Look at the MISMATCH rows before you trust them.");
            }
            s
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentages() {
        assert_eq!(pct(6000), "60");
        assert_eq!(pct(3333), "33.33");
        assert_eq!(pct(3350), "33.5");
        assert_eq!(pct(5), "0.05");
    }
}
