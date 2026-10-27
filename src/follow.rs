//! `--follow`: keep scanning until the order is paid, so a member can leave the watcher running
//! while the buyer pays. Only PAID ends it early: a MISMATCH may be followed by the right payment
//! (and anyone who knows the order could send a stray output to stop the watch), so it is shown
//! as progress and the scan goes on until `--max-wait`.

use std::time::Duration;

use crate::check::{Check, Verdict};
use crate::source::View;

pub fn settled(v: Verdict) -> bool {
    v == Verdict::Paid
}

/// Runs `observe` until the verdict settles or `max_wait` of waiting between scans has passed,
/// then returns the last observation. `progress` is told about each unsettled one.
pub fn until_settled<S, P, Z>(
    mut observe: S,
    interval: Duration,
    max_wait: Duration,
    mut progress: P,
    mut sleep: Z,
) -> Result<(View, Check), String>
where
    S: FnMut() -> Result<(View, Check), String>,
    P: FnMut(&View, &Check),
    Z: FnMut(Duration),
{
    let mut waited = Duration::ZERO;
    loop {
        let (view, check) = observe()?;
        if settled(check.verdict) || waited + interval > max_wait {
            return Ok((view, check));
        }
        progress(&view, &check);
        sleep(interval);
        waited += interval;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::address::Network;
    use crate::source::Origin;

    fn obs(verdict: Verdict, tip: u32) -> (View, Check) {
        (
            View {
                network: Network::Test,
                tip_height: tip,
                outputs: vec![],
                origin: Origin::Snapshot {
                    path: "t".into(),
                    note: None,
                },
            },
            Check {
                rows: vec![],
                other_charters: 0,
                other_orders: 0,
                verdict,
                summary: String::new(),
            },
        )
    }

    #[test]
    fn scans_until_paid() {
        let mut script = vec![Verdict::NotFound, Verdict::Pending, Verdict::Paid].into_iter();
        let mut tip = 0;
        let (mut seen, mut slept) = (0, Duration::ZERO);
        let (view, check) = until_settled(
            || {
                tip += 1;
                Ok(obs(script.next().unwrap(), tip))
            },
            Duration::from_secs(30),
            Duration::from_secs(3600),
            |_, _| seen += 1,
            |d| slept += d,
        )
        .unwrap();
        assert_eq!((check.verdict, view.tip_height), (Verdict::Paid, 3));
        assert_eq!((seen, slept), (2, Duration::from_secs(60)));
    }

    #[test]
    fn gives_up_after_max_wait_with_the_last_answer() {
        let mut scans = 0;
        let (_, check) = until_settled(
            || {
                scans += 1;
                Ok(obs(Verdict::NotFound, 1))
            },
            Duration::from_secs(30),
            Duration::from_secs(90),
            |_, _| {},
            |_| {},
        )
        .unwrap();
        assert_eq!((check.verdict, scans), (Verdict::NotFound, 4));
    }

    #[test]
    fn a_mismatch_is_reported_and_the_watch_goes_on() {
        let mut script = vec![Verdict::Mismatch, Verdict::Paid].into_iter();
        let mut reported = Vec::new();
        let (_, check) = until_settled(
            || Ok(obs(script.next().unwrap(), 1)),
            Duration::from_secs(30),
            Duration::from_secs(90),
            |_, c| reported.push(c.verdict),
            |_| {},
        )
        .unwrap();
        assert_eq!(
            (check.verdict, reported),
            (Verdict::Paid, vec![Verdict::Mismatch])
        );
    }

    #[test]
    fn paid_stops_at_once() {
        let mut scans = 0;
        until_settled(
            || {
                scans += 1;
                Ok(obs(Verdict::Paid, 1))
            },
            Duration::from_secs(30),
            Duration::from_secs(90),
            |_, _| {},
            |_| panic!("no sleep after a settled answer"),
        )
        .unwrap();
        assert_eq!(scans, 1);
    }
}
