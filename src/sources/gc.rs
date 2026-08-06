//! Wrapper around the `gc` (Gas City) CLI. Shapes below match the real
//! `--json` / `--json-schema=result` output of `gc status`, `gc rig list`,
//! and `gc rig status` — verified against a live city, not guessed. Every
//! field beyond what oil-cop actually uses is `#[serde(default)]`-tolerant
//! so a pack-specific or version-drifted field never breaks parsing.

use super::proc::{run_json, Output};
use anyhow::Result;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct StatusResult {
    #[serde(default)]
    pub city_name: String,
    #[serde(default)]
    pub city_path: String,
    #[serde(default)]
    pub controller: Controller,
    #[serde(default)]
    pub running: bool,
    #[serde(default)]
    pub suspended: bool,
    #[serde(default)]
    pub health: HealthBlock,
    #[serde(default)]
    pub agents: Vec<AgentRaw>,
    #[serde(default)]
    pub rigs: Vec<RigRaw>,
    #[serde(default)]
    pub summary: SummaryBlock,
    #[serde(default)]
    pub partial: bool,
    #[serde(default)]
    pub partial_errors: Vec<String>,
}

#[derive(Debug, Default, Clone, Deserialize)]
pub struct Controller {
    #[serde(default)]
    pub running: bool,
    pub pid: Option<i64>,
    pub mode: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Default, Clone, Deserialize)]
pub struct HealthBlock {
    #[serde(default)]
    pub usable: bool,
    #[serde(default)]
    pub degraded: bool,
    #[serde(default)]
    pub signals: Vec<String>,
}

#[derive(Debug, Default, Clone, Deserialize)]
pub struct SummaryBlock {
    #[serde(default)]
    pub total_agents: i64,
    #[serde(default)]
    pub running_agents: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AgentRaw {
    #[serde(default)]
    pub qualified_name: String,
    #[serde(default)]
    pub scope: String,
    #[serde(default)]
    pub running: bool,
    #[serde(default)]
    pub suspended: bool,
}

#[derive(Debug, Default, Clone, Deserialize)]
pub struct RigRaw {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub path: String,
    pub prefix: Option<String>,
    #[serde(default)]
    pub suspended: bool,
    /// Only present on `gc rig list`, not `gc status`.
    #[serde(default)]
    pub running: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RigListResult {
    #[serde(default)]
    pub rigs: Vec<RigRaw>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RigStatusResult {
    #[serde(default)]
    pub rig: RigRaw,
    #[serde(default)]
    pub agents: Vec<RigAgentRaw>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RigAgentRaw {
    pub name: String,
    #[serde(default)]
    pub qualified_name: String,
    pub runtime_session_name: Option<String>,
    pub session_id: Option<String>,
    #[serde(default)]
    pub running: bool,
    #[serde(default)]
    pub suspended: bool,
    #[serde(default)]
    pub draining: bool,
    pub status: Option<String>,
}

/// One entry from `gc session list --json`. This is a ground-truth liveness
/// source `rig status`'s `running` bool isn't: `last_active` is a real
/// per-session heartbeat timestamp, not an inference from when a bead last
/// got touched. `last_active` comes back as the zero-value RFC3339 sentinel
/// (`0001-01-01T00:00:00Z`) for a session that has never been active --
/// callers should treat that the same as "unknown", not "ancient".
#[derive(Debug, Clone, Deserialize)]
pub struct SessionRaw {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub rig: Option<String>,
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub last_active: Option<String>,
    #[serde(default)]
    pub attached: bool,
    #[serde(default)]
    pub closed: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct SessionListResult {
    #[serde(default)]
    pub sessions: Vec<SessionRaw>,
}

/// Result of `gc session peek`: the tail of the session's actual live pane
/// output -- proof of what the agent is doing right now, not a status flag
/// someone else computed.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PeekResult {
    #[serde(default)]
    pub output: String,
}

fn city_args<'a>(city: Option<&'a str>, base: &mut Vec<&'a str>) {
    if let Some(c) = city {
        base.push("--city");
        base.push(c);
    }
}

pub fn status(city: Option<&str>) -> Result<(StatusResult, Output)> {
    let mut args = vec!["status", "--json"];
    city_args(city, &mut args);
    run_json("gc", &args)
}

pub fn rig_list(city: Option<&str>) -> Result<(RigListResult, Output)> {
    let mut args = vec!["rig", "list", "--json"];
    city_args(city, &mut args);
    run_json("gc", &args)
}

pub fn rig_status(city: Option<&str>, rig: &str) -> Result<(RigStatusResult, Output)> {
    let mut args = vec!["rig", "status", "--rig", rig, "--json"];
    city_args(city, &mut args);
    run_json("gc", &args)
}

/// List all chat sessions known to the city -- the real, per-session
/// liveness source (see `SessionRaw`).
pub fn session_list(city: Option<&str>) -> Result<(SessionListResult, Output)> {
    let mut args = vec!["session", "list", "--json"];
    city_args(city, &mut args);
    run_json("gc", &args)
}

/// Peek at a session's live pane output without attaching -- proof the
/// agent is actually doing something, not just that gc thinks it's running.
/// Fails (returns `Err`) if the session isn't currently active; callers
/// should treat that as "no fresh activity line available" rather than a
/// hard error.
pub fn session_peek(
    city: Option<&str>,
    session_id: &str,
    lines: usize,
) -> Result<(PeekResult, Output)> {
    let lines_str = lines.to_string();
    let mut args = vec![
        "session", "peek", session_id, "--lines", &lines_str, "--json",
    ];
    city_args(city, &mut args);
    run_json("gc", &args)
}
