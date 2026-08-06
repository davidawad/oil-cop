//! Domain types shared between data sources and renderers.
//! These are the normalized, tool-agnostic views oil-cop actually displays --
//! kept separate from the raw `gc`/`bd` JSON shapes in `sources::*`.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Health {
    /// Actively progressing: running agent with recently-updated work, or a
    /// fresh in-progress bead.
    Healthy,
    /// Waiting on something upstream (open/blocked/no assigned work) -- not
    /// itself a problem.
    Idle,
    /// Should be moving but hasn't updated within the staleness threshold.
    Stale,
    /// Expected to be running/usable but isn't.
    Dead,
    /// Intentionally paused.
    Suspended,
    /// Finished.
    Done,
    /// Not enough data to judge.
    Unknown,
}

impl Health {
    pub fn label(self) -> &'static str {
        match self {
            Health::Healthy => "healthy",
            Health::Idle => "idle",
            Health::Stale => "stale",
            Health::Dead => "dead",
            Health::Suspended => "suspended",
            Health::Done => "done",
            Health::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct BeadRef {
    pub id: String,
    pub title: String,
    pub status: String,
    pub priority: Option<i64>,
    pub assignee: Option<String>,
    pub started_at: Option<String>,
    pub updated_at: Option<String>,
    /// Seconds since `updated_at`, at assembly time.
    pub age_secs: Option<i64>,
    pub health: Health,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentView {
    pub name: String,
    pub qualified_name: String,
    pub scope: String,
    pub running: bool,
    pub suspended: bool,
    pub draining: bool,
    /// Raw status text from `gc rig status` (e.g. "running", "stopped"),
    /// shown verbatim when it carries more nuance than the booleans do.
    pub raw_status: Option<String>,
    pub session_id: Option<String>,
    pub runtime_session_name: Option<String>,
    pub current_bead: Option<BeadRef>,
    /// Seconds since this agent's session last showed real activity (`gc
    /// session list`'s `last_active`) -- the ground-truth "last sign it did
    /// anything," independent of `running`/bead staleness. `None` when no
    /// matching session (or heartbeat) was found.
    pub last_active_secs: Option<i64>,
    /// Last meaningful line of the agent's live pane (`gc session peek`),
    /// when one was fetched -- proof of what it's actually doing right now,
    /// not just that something calls it "running." `None` when no peek was
    /// taken this render (peeking is throttled/best-effort) or none succeeded.
    pub activity_line: Option<String>,
    pub health: Health,
}

#[derive(Debug, Clone, Serialize)]
pub struct RigView {
    pub name: String,
    pub path: String,
    pub prefix: Option<String>,
    pub running: bool,
    pub suspended: bool,
    pub health: Health,
    /// Bead counts for this rig (ready/in_progress/blocked/etc). `None` for
    /// suspended rigs (skipped -- nothing useful to show) or if the `bd
    /// status` call for this rig's path failed.
    pub bead_summary: Option<QueueSummary>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CityView {
    pub city_name: String,
    pub city_path: String,
    pub city_running: bool,
    pub controller_running: bool,
    pub controller_pid: Option<i64>,
    pub controller_mode: Option<String>,
    pub controller_status: Option<String>,
    pub suspended: bool,
    pub usable: bool,
    pub degraded: bool,
    pub signals: Vec<String>,
    pub partial: bool,
    pub partial_errors: Vec<String>,
    pub rigs: Vec<RigView>,
    pub total_agents: usize,
    pub running_agents: usize,
    pub health: Health,
}

#[derive(Debug, Clone, Serialize)]
pub struct QueueSummary {
    pub total: i64,
    pub ready: i64,
    pub in_progress: i64,
    pub blocked: i64,
    pub deferred: i64,
    pub closed: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct QueueView {
    pub rig_name: String,
    pub rig_path: String,
    pub rig_running: Option<bool>,
    pub summary: QueueSummary,
    pub in_progress: Vec<BeadRef>,
    pub health: Health,
}

/// Bead-lifecycle stage for the DAG view's red -> yellow -> green coloring.
/// Deliberately separate from `Health` (staleness) -- this is "where is it
/// in the pipeline," not "is it stuck."
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BeadStage {
    /// open / blocked / deferred -- not started yet.
    Pending,
    /// in_progress -- being worked on right now.
    Active,
    /// closed -- landed.
    Merged,
}

#[derive(Debug, Clone, Serialize)]
pub struct DagNode {
    pub id: String,
    pub title: String,
    pub status: String,
    pub priority: Option<i64>,
    pub assignee: Option<String>,
    pub parent: Option<String>,
    pub blocked_by: Vec<String>,
    pub age_secs: Option<i64>,
    pub stage: BeadStage,
    /// True when bd still reports `in_progress` but the bead's branch has
    /// already landed on its target in git -- the "refinery didn't close
    /// this" signal this view exists to surface.
    pub landed_unmerged: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DagView {
    pub rig_name: String,
    pub rig_path: String,
    pub nodes: Vec<DagNode>,
}

/// One concrete problem found by `oil-cop check` -- only `Dead`/`Stale`
/// signals are surfaced here (see `health::is_problem`); `Idle`/`Unknown`/
/// `Suspended`/`Done`/`Healthy` are not failures.
#[derive(Debug, Clone, Serialize)]
pub struct CheckIssue {
    /// e.g. "city", "rig:luminate", "agent:luminate/gastown.nux",
    /// "bead:luminate-2vr.2"
    pub scope: String,
    pub health: Health,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CheckReport {
    pub ok: bool,
    pub issues: Vec<CheckIssue>,
}

/// A session `gc` itself still calls active, but whose real activity
/// heartbeat (`last_active`) is stale (or has never fired at all) --
/// exactly the case `gc session prune` can't reach, since prune only ever
/// ages out sessions gc already labels suspended/asleep/drained.
#[derive(Debug, Clone, Serialize)]
pub struct ZombieSession {
    pub id: String,
    pub name: String,
    pub rig: Option<String>,
    /// `None` means the session has never shown any activity at all despite
    /// gc calling it active -- arguably worse than merely stale.
    pub last_active_secs: Option<i64>,
    /// `gc session kill`/`gc session close` -- printed for a human to run,
    /// never executed automatically.
    pub suggested_commands: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionsReport {
    pub total_sessions: usize,
    pub zombies: Vec<ZombieSession>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_covers_every_health_variant_with_a_distinct_string() {
        let labels = [
            Health::Healthy.label(),
            Health::Idle.label(),
            Health::Stale.label(),
            Health::Dead.label(),
            Health::Suspended.label(),
            Health::Done.label(),
            Health::Unknown.label(),
        ];
        let unique: std::collections::HashSet<_> = labels.iter().collect();
        assert_eq!(
            unique.len(),
            labels.len(),
            "every Health variant's label must be distinct"
        );
        assert_eq!(Health::Healthy.label(), "healthy");
        assert_eq!(Health::Unknown.label(), "unknown");
    }
}
