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
