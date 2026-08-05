//! Wrapper around the `bd` (beads) CLI. Shapes below match real
//! `bd status --json` and `bd list --json` output, verified against a live
//! rig's bead database. `bd` has no `--json-schema` introspection (that's a
//! `gc`-only feature), so these are captured-and-trimmed real payloads —
//! every field oil-cop doesn't use is dropped rather than guessed at.

use super::proc::{run_json, Output};
use anyhow::Result;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct StatusResult {
    #[serde(default)]
    pub summary: StatusSummary,
}

#[derive(Debug, Default, Deserialize)]
pub struct StatusSummary {
    #[serde(default)]
    pub total_issues: i64,
    #[serde(default)]
    pub ready_issues: i64,
    #[serde(default)]
    pub in_progress_issues: i64,
    #[serde(default)]
    pub blocked_issues: i64,
    #[serde(default)]
    pub deferred_issues: i64,
    #[serde(default)]
    pub closed_issues: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BeadRaw {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub status: String,
    pub priority: Option<i64>,
    pub assignee: Option<String>,
    pub updated_at: Option<String>,
    pub started_at: Option<String>,
}

fn dir_args<'a>(dir: &'a str, base: &mut Vec<&'a str>) {
    base.push("-C");
    base.push(dir);
}

pub fn status(dir: &str) -> Result<(StatusResult, Output)> {
    let mut args = vec![];
    dir_args(dir, &mut args);
    args.extend(["status", "--json", "--no-activity"]);
    run_json("bd", &args)
}

/// List beads in a given status. `status_filter` is a bare `bd` status value
/// (open, in_progress, blocked, deferred, closed) or comma-separated list.
pub fn list(dir: &str, status_filter: &str) -> Result<(Vec<BeadRaw>, Output)> {
    let mut args = vec![];
    dir_args(dir, &mut args);
    args.extend(["list", "--json", "--flat", "--status", status_filter]);
    run_json("bd", &args)
}
