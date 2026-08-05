//! Wrapper around the `bd` (beads) CLI. Shapes below match real
//! `bd status --json` and `bd list --json` output, verified against a live
//! rig's bead database. `bd` has no `--json-schema` introspection (that's a
//! `gc`-only feature), so these are captured-and-trimmed real payloads --
//! every field oil-cop doesn't use is dropped rather than guessed at.

use super::proc::{run_json, Output};
use anyhow::Result;
use serde::Deserialize;

#[derive(Debug, Default, Clone, Deserialize)]
pub struct StatusResult {
    #[serde(default)]
    pub summary: StatusSummary,
}

#[derive(Debug, Default, Clone, Deserialize)]
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

#[cfg(test)]
mod tests {
    use super::*;

    fn bead_with_metadata(metadata: serde_json::Value) -> BeadRaw {
        BeadRaw {
            id: "test-1".to_string(),
            title: "title".to_string(),
            status: "in_progress".to_string(),
            priority: None,
            assignee: None,
            updated_at: None,
            started_at: None,
            parent: None,
            dependencies: vec![],
            metadata,
        }
    }

    #[test]
    fn branch_and_work_branch_read_flat_dotted_keys() {
        let bead = bead_with_metadata(serde_json::json!({
            "branch": "polecat/foo-1",
            "gc.work_branch": "master",
            "gc.session_id": "cc-abcd"
        }));
        assert_eq!(bead.branch(), Some("polecat/foo-1"));
        assert_eq!(bead.work_branch(), Some("master"));
    }

    #[test]
    fn branch_and_work_branch_are_none_when_metadata_is_missing_or_wrong_shape() {
        let empty = bead_with_metadata(serde_json::json!({}));
        assert_eq!(empty.branch(), None);
        assert_eq!(empty.work_branch(), None);

        // metadata present but the field is the wrong JSON type -- as_str()
        // returns None rather than panicking.
        let wrong_type = bead_with_metadata(serde_json::json!({"branch": 42}));
        assert_eq!(wrong_type.branch(), None);

        // metadata isn't even an object -- get() on a non-object Value
        // returns None, same treatment.
        let not_an_object = bead_with_metadata(serde_json::json!("just a string"));
        assert_eq!(not_an_object.branch(), None);
    }

    #[test]
    fn blocked_by_only_returns_blocks_type_dependencies() {
        let mut bead = bead_with_metadata(serde_json::json!({}));
        bead.dependencies = vec![
            DependencyRaw {
                depends_on_id: "parent-1".to_string(),
                dep_type: "parent-child".to_string(),
            },
            DependencyRaw {
                depends_on_id: "blocker-1".to_string(),
                dep_type: "blocks".to_string(),
            },
            DependencyRaw {
                depends_on_id: "blocker-2".to_string(),
                dep_type: "blocks".to_string(),
            },
            DependencyRaw {
                depends_on_id: "related-1".to_string(),
                dep_type: "related".to_string(),
            },
        ];
        assert_eq!(bead.blocked_by(), vec!["blocker-1", "blocker-2"]);
    }

    #[test]
    fn blocked_by_is_empty_when_there_are_no_dependencies() {
        let bead = bead_with_metadata(serde_json::json!({}));
        assert!(bead.blocked_by().is_empty());
    }
}
