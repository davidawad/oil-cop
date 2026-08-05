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

/// Health of an agent: suspended/dead takes priority over work-based signals.
/// `has_fresh_work` is `None` when the agent has no assigned in-progress bead
/// at all (idle, waiting for hooks — not necessarily a problem). A draining
/// agent is mid-shutdown by design, so it reads as idle rather than healthy
/// even if it's still finishing fresh work.
pub fn agent_health(
    running: bool,
    suspended: bool,
    draining: bool,
    has_fresh_work: Option<bool>,
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
    match has_fresh_work {
        Some(true) => Health::Healthy,
        Some(false) => Health::Stale,
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
        assert_eq!(
            agent_health(true, true, false, Some(true)),
            Health::Suspended
        );
        assert_eq!(agent_health(false, false, false, Some(true)), Health::Dead);
        assert_eq!(
            agent_health(true, false, false, Some(true)),
            Health::Healthy
        );
        assert_eq!(agent_health(true, false, false, Some(false)), Health::Stale);
        assert_eq!(agent_health(true, false, false, None), Health::Idle);
        assert_eq!(agent_health(true, false, true, Some(true)), Health::Idle);
    }

    #[test]
    fn worst_of_picks_dead_over_healthy() {
        let h = worst_of([Health::Healthy, Health::Dead, Health::Idle]);
        assert_eq!(h, Health::Dead);
    }
}
