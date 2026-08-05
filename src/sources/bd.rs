//! Wrapper around the `bd` (beads) CLI. Shapes below match real
//! `bd status --json` and `bd list --json` output, verified against a live
//! rig's bead database. `bd` has no `--json-schema` introspection (that's a
//! `gc`-only feature), so these are captured-and-trimmed real payloads --
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
pub struct DependencyRaw {
    pub depends_on_id: String,
    #[serde(rename = "type")]
    pub dep_type: String,
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
    pub parent: Option<String>,
    #[serde(default)]
    pub dependencies: Vec<DependencyRaw>,
    #[serde(default)]
    pub metadata: serde_json::Value,
}

impl BeadRaw {
    /// This bead's own branch, e.g. "polecat/luminate-2vr.2" -- flat key in
    /// `metadata`, not nested.
    pub fn branch(&self) -> Option<&str> {
        self.metadata.get("branch").and_then(|v| v.as_str())
    }

    /// The branch this bead's work is destined to land on, e.g. "master".
    pub fn work_branch(&self) -> Option<&str> {
        self.metadata.get("gc.work_branch").and_then(|v| v.as_str())
    }

    /// IDs of beads that block this one ("blocks" dependency edges only --
    /// parent/child is covered separately by the `parent` field).
    pub fn blocked_by(&self) -> Vec<&str> {
        self.dependencies
            .iter()
            .filter(|d| d.dep_type == "blocks")
            .map(|d| d.depends_on_id.as_str())
            .collect()
    }
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
