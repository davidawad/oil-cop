//! Health heuristics: turning raw timestamps/booleans into the `Health`
//! signal oil-cop color-codes. This is the "is this actually healthy" logic
//! the whole tool exists to surface.

use crate::model::Health;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Copy)]
pub struct Thresholds {
    /// A bead in `in_progress` with no update in this many seconds is stale:
    /// claimed but not actually moving.
    pub stale_after_secs: i64,
}

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            stale_after_secs: 30 * 60,
        }
    }
}

/// Parse a duration like "30m", "1h", "45s", "2h30m", "90" (bare seconds).
pub fn parse_duration_secs(input: &str) -> anyhow::Result<i64> {
    let s = input.trim();
    if s.is_empty() {
        anyhow::bail!("empty duration");
    }
    if let Ok(bare) = s.parse::<i64>() {
        return Ok(bare);
    }
    let mut total: i64 = 0;
    let mut num = String::new();
    for ch in s.chars() {
        if ch.is_ascii_digit() {
            num.push(ch);
            continue;
        }
        let unit_secs = match ch {
            's' => 1,
            'm' => 60,
            'h' => 3600,
            'd' => 86400,
            other => anyhow::bail!("unrecognized duration unit '{other}' in '{input}'"),
        };
        if num.is_empty() {
            anyhow::bail!("unit '{ch}' with no preceding number in '{input}'");
        }
        total += num.parse::<i64>()? * unit_secs;
        num.clear();
    }
    if !num.is_empty() {
        anyhow::bail!("trailing number '{num}' with no unit in '{input}'");
    }
    Ok(total)
}

/// Seconds elapsed between an RFC3339 timestamp and now, if it parses.
pub fn age_secs(timestamp: Option<&str>, now: DateTime<Utc>) -> Option<i64> {
    let ts = timestamp?;
    let parsed = DateTime::parse_from_rfc3339(ts).ok()?.with_timezone(&Utc);
    Some((now - parsed).num_seconds())
}

/// Health of a single bead given its status and how long ago it last updated.
pub fn bead_health(status: &str, age_secs: Option<i64>, thresholds: &Thresholds) -> Health {
    match status {
        "in_progress" => match age_secs {
            Some(s) if s > thresholds.stale_after_secs => Health::Stale,
            Some(_) => Health::Healthy,
            None => Health::Unknown,
        },
        "blocked" | "deferred" => Health::Idle,
        "open" => Health::Idle,
        "closed" => Health::Done,
        _ => Health::Unknown,
    }
}

/// Health of an agent: suspended/dead takes priority over activity-based
/// signals. `last_active_secs` is the real "last sign of life" from the
/// agent's own session (see `gc::SessionRaw::last_active`) -- how long ago
/// it actually did something, not whether its claimed bead's timestamp looks
/// fresh. `None` means no session heartbeat is available at all (idle,
/// waiting for hooks -- not necessarily a problem). A draining agent is
/// mid-shutdown by design, so it reads as idle rather than healthy even if
/// it was recently active.
pub fn agent_health(
    running: bool,
    suspended: bool,
    draining: bool,
    last_active_secs: Option<i64>,
    thresholds: &Thresholds,
) -> Health {
    if suspended {
        return Health::Suspended;
    }
    if !running {
        return Health::Dead;
    }
    if draining {
        return Health::Idle;
    }
    match last_active_secs {
        Some(s) if s > thresholds.stale_after_secs => Health::Stale,
        Some(_) => Health::Healthy,
        None => Health::Idle,
    }
}

/// Roll up a set of child health signals into one overall signal, worst wins.
pub fn worst_of(signals: impl IntoIterator<Item = Health>) -> Health {
    let mut worst = Health::Done;
    let rank = |h: Health| match h {
        Health::Dead => 6,
        Health::Stale => 5,
        Health::Unknown => 4,
        Health::Idle => 3,
        Health::Healthy => 2,
        Health::Suspended => 1,
        Health::Done => 0,
    };
    for h in signals {
        if rank(h) > rank(worst) {
            worst = h;
        }
    }
    worst
}

/// Is this health signal a concrete problem `oil-cop check` should fail
/// on? `Unknown` (e.g. a partial status probe) is uncertainty, not a proven
/// failure, so it doesn't fail a check on its own.
pub fn is_problem(h: Health) -> bool {
    matches!(h, Health::Dead | Health::Stale)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_durations() {
        assert_eq!(parse_duration_secs("30m").unwrap(), 1800);
        assert_eq!(parse_duration_secs("1h").unwrap(), 3600);
        assert_eq!(parse_duration_secs("2h30m").unwrap(), 9000);
        assert_eq!(parse_duration_secs("45s").unwrap(), 45);
        assert_eq!(parse_duration_secs("90").unwrap(), 90);
        assert!(parse_duration_secs("").is_err());
        assert!(parse_duration_secs("30x").is_err());
    }

    #[test]
    fn bead_health_stale_vs_healthy() {
        let t = Thresholds {
            stale_after_secs: 100,
        };
        assert_eq!(bead_health("in_progress", Some(50), &t), Health::Healthy);
        assert_eq!(bead_health("in_progress", Some(150), &t), Health::Stale);
        assert_eq!(bead_health("in_progress", None, &t), Health::Unknown);
        assert_eq!(bead_health("closed", Some(999999), &t), Health::Done);
    }

    #[test]
    fn agent_health_priority() {
        let t = Thresholds {
            stale_after_secs: 100,
        };
        assert_eq!(
            agent_health(true, true, false, Some(10), &t),
            Health::Suspended
        );
        assert_eq!(
            agent_health(false, false, false, Some(10), &t),
            Health::Dead
        );
        assert_eq!(
            agent_health(true, false, false, Some(10), &t),
            Health::Healthy
        );
        assert_eq!(
            agent_health(true, false, false, Some(150), &t),
            Health::Stale
        );
        assert_eq!(agent_health(true, false, false, None, &t), Health::Idle);
        assert_eq!(agent_health(true, false, true, Some(10), &t), Health::Idle);
    }

    #[test]
    fn worst_of_picks_dead_over_healthy() {
        let h = worst_of([Health::Healthy, Health::Dead, Health::Idle]);
        assert_eq!(h, Health::Dead);
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    /// A single `(digits, unit)` pair, e.g. `"30"` + `'m'` -> `"30m"`.
    fn duration_part() -> impl Strategy<Value = (u32, char)> {
        (
            0u32..100_000,
            prop::sample::select(vec!['s', 'm', 'h', 'd']),
        )
    }

    fn unit_secs(c: char) -> i64 {
        match c {
            's' => 1,
            'm' => 60,
            'h' => 3600,
            'd' => 86400,
            _ => unreachable!(),
        }
    }

    proptest! {
        /// Any string built from a sequence of `(digits, unit)` pairs parses
        /// to the exact expected sum in seconds.
        #[test]
        fn parse_duration_secs_sums_arbitrary_sequences(
            parts in prop::collection::vec(duration_part(), 1..8)
        ) {
            let input: String = parts
                .iter()
                .map(|(n, u)| format!("{n}{u}"))
                .collect();
            let expected: i64 = parts
                .iter()
                .map(|(n, u)| *n as i64 * unit_secs(*u))
                .sum();
            prop_assert_eq!(parse_duration_secs(&input).unwrap(), expected);
        }

        /// No arbitrary input string, however malformed, should ever panic --
        /// only `Ok` or `Err`.
        #[test]
        fn parse_duration_secs_never_panics_on_arbitrary_strings(s in ".*") {
            let _ = parse_duration_secs(&s);
        }

        /// `bead_health` never panics for any status string / age / threshold.
        #[test]
        fn bead_health_never_panics(
            status in ".*",
            age in prop::option::of(any::<i64>()),
            stale_after_secs in any::<i64>(),
        ) {
            let t = Thresholds { stale_after_secs };
            let _ = bead_health(&status, age, &t);
        }

        /// `agent_health` never panics for any combination of inputs, and the
        /// documented priority rules hold: `suspended` always wins.
        #[test]
        fn agent_health_never_panics_and_suspended_always_wins(
            running in any::<bool>(),
            suspended in any::<bool>(),
            draining in any::<bool>(),
            last_active_secs in prop::option::of(any::<i64>()),
            stale_after_secs in any::<i64>(),
        ) {
            let t = Thresholds { stale_after_secs };
            let h = agent_health(running, suspended, draining, last_active_secs, &t);
            if suspended {
                prop_assert_eq!(h, Health::Suspended);
            }
        }

        /// Documented priority: when not suspended, `!running` always wins
        /// (Dead), regardless of draining/last_active_secs.
        #[test]
        fn agent_health_dead_wins_over_draining_and_work_state(
            draining in any::<bool>(),
            last_active_secs in prop::option::of(any::<i64>()),
            stale_after_secs in any::<i64>(),
        ) {
            let t = Thresholds { stale_after_secs };
            let h = agent_health(false, false, draining, last_active_secs, &t);
            prop_assert_eq!(h, Health::Dead);
        }

        /// `worst_of` always returns one of the input elements (or the
        /// default `Done` for an empty vec), and is order-independent.
        #[test]
        fn worst_of_returns_an_input_element_and_is_order_independent(
            signals in prop::collection::vec(health_strategy(), 0..12),
            rotate_by in 0usize..12,
        ) {
            let original = worst_of(signals.clone());
            if signals.is_empty() {
                prop_assert_eq!(original, Health::Done);
            } else {
                prop_assert!(signals.contains(&original));
            }

            // Re-ordering the input must never change the result: check
            // full reversal plus an arbitrary rotation, two cheap ways to
            // permute without pulling in a shuffle dependency, still enough
            // to catch order-dependence bugs.
            let mut reversed = signals.clone();
            reversed.reverse();
            prop_assert_eq!(worst_of(reversed), original);

            if !signals.is_empty() {
                let mut rotated = signals.clone();
                let n = rotated.len();
                rotated.rotate_left(rotate_by % n);
                prop_assert_eq!(worst_of(rotated), original);
            }
        }
    }

    fn health_strategy() -> impl Strategy<Value = Health> {
        prop::sample::select(vec![
            Health::Healthy,
            Health::Idle,
            Health::Stale,
            Health::Dead,
            Health::Suspended,
            Health::Done,
            Health::Unknown,
        ])
    }
}
