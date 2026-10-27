//! The deterministic split of @bonbagi/core: each member gets floor(total * bp / 10,000); the
//! leftover zatoshi go one each to the largest remainders, ties to the member listed first.
//! Every client computes the same outputs, so a member can recompute their own share.

pub const TOTAL_BPS: u32 = 10_000;

pub fn split_amount(total_zat: u64, bps: &[u32], min_share_zat: u64) -> Result<Vec<u64>, String> {
    if bps.is_empty() {
        return Err("no members".into());
    }
    if let Some(b) = bps.iter().find(|&&b| b == 0) {
        return Err(format!(
            "share must be a positive whole number of basis points, got {b}"
        ));
    }
    let sum: u64 = bps.iter().map(|&b| u64::from(b)).sum();
    if sum != u64::from(TOTAL_BPS) {
        return Err(format!("shares must add up to 100% (10000 bp), got {sum}"));
    }
    if total_zat == 0 {
        return Err("price must be positive".into());
    }
    let t = u128::from(TOTAL_BPS);
    let mut parts: Vec<(usize, u64, u128)> = bps
        .iter()
        .enumerate()
        .map(|(i, &b)| {
            let num = u128::from(total_zat) * u128::from(b);
            (i, (num / t) as u64, num % t)
        })
        .collect();
    let mut left = total_zat - parts.iter().map(|p| p.1).sum::<u64>();
    let mut order: Vec<usize> = (0..parts.len()).collect();
    order.sort_by(|&a, &b| parts[b].2.cmp(&parts[a].2).then(a.cmp(&b)));
    for i in order {
        if left == 0 {
            break;
        }
        parts[i].1 += 1;
        left -= 1;
    }
    let out: Vec<u64> = parts.iter().map(|p| p.1).collect();
    if let Some((i, z)) = out.iter().enumerate().find(|(_, &z)| z < min_share_zat) {
        return Err(format!(
            "member {} would receive {z} zat, below the minimum of {min_share_zat} zat — raise the price or merge small shares",
            i + 1
        ));
    }
    Ok(out)
}

/// Every total whose split gives member `index` exactly `amount_zat`, as (lowest, highest, count).
/// Used when nobody told the member the price: their output alone pins the total down to a
/// small range, and an amount that fits no total at all cannot be their share.
pub fn totals_for_share(
    amount_zat: u64,
    index: usize,
    bps: &[u32],
    min_share_zat: u64,
    max_total: u64,
) -> Option<(u64, u64, u64)> {
    let b = u128::from(*bps.get(index)?);
    if b == 0 {
        return None;
    }
    // share is floor(P*b/T) or one more, so P*b/T lies in [amount - 1, amount + 1).
    let t = u128::from(TOTAL_BPS);
    let a = u128::from(amount_zat);
    let lo = (a.saturating_sub(1) * t / b).max(1);
    let hi = ((a + 1) * t).div_ceil(b).min(u128::from(max_total));
    let mut found: Option<(u64, u64, u64)> = None;
    let mut p = lo;
    while p <= hi {
        let total = p as u64;
        if let Ok(shares) = split_amount(total, bps, min_share_zat) {
            if shares[index] == amount_zat {
                found = Some(match found {
                    None => (total, total, 1),
                    Some((l, _, n)) => (l, total, n + 1),
                });
            }
        }
        p += 1;
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remainder_goes_to_largest_then_first() {
        assert_eq!(
            split_amount(300_001, &[6000, 4000], 1).unwrap(),
            [180_001, 120_000]
        );
        assert_eq!(
            split_amount(100, &[3333, 3333, 3334], 1).unwrap(),
            [33, 33, 34]
        );
        assert_eq!(
            split_amount(101, &[3333, 3333, 3334], 1).unwrap(),
            [34, 33, 34]
        );
    }

    #[test]
    fn totals_for_a_share() {
        let bps = [6000, 4000];
        assert_eq!(
            totals_for_share(180_000, 0, &bps, 10_000, u64::MAX),
            Some((300_000, 300_000, 1))
        );
        // 120,000 is member 2's 40% share of 299,999 (it gets the leftover zatoshi), of 300,000,
        // and of 300,001 (the leftover goes to member 1).
        assert_eq!(
            totals_for_share(120_000, 1, &bps, 10_000, u64::MAX),
            Some((299_999, 300_001, 3))
        );
        // Every total found really gives that share, and nothing outside the range does.
        let thirds = [3333, 3333, 3334];
        let (lo, hi, n) = totals_for_share(3_000, 0, &thirds, 1, u64::MAX).unwrap();
        assert!(n >= 1 && hi - lo + 1 >= n);
        for p in [lo, hi] {
            assert_eq!(split_amount(p, &thirds, 1).unwrap()[0], 3_000);
        }
        for p in [lo - 1, hi + 1] {
            assert_ne!(split_amount(p, &thirds, 1).unwrap()[0], 3_000);
        }
        // Below the charter's minimum share: no valid total at all.
        assert_eq!(totals_for_share(5_000, 0, &bps, 10_000, u64::MAX), None);
    }
}
