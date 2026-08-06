//! Test-only mock implementations of the adapter traits (test-support code,
//! not a real backend) -- used by unit tests in `adapters.rs` and
//! `assemble.rs` to exercise the join logic without shelling out to real
//! `gc`/`bd`/`git`. Compiled only under `#[cfg(test)]` (see `sources/mod.rs`).

use super::adapters::{BdAdapter, GcAdapter, GitAdapter};
use super::{bd, gc};
use anyhow::Result;
use std::collections::HashMap;

#[derive(Default)]
pub struct MockGc {
    pub status: Option<gc::StatusResult>,
    pub rig_list: Option<gc::RigListResult>,
    pub rig_status: HashMap<String, gc::RigStatusResult>,
}

impl GcAdapter for MockGc {
    fn status(&self, _city: Option<&str>) -> Result<gc::StatusResult> {
        self.status
            .clone()
            .ok_or_else(|| anyhow::anyhow!("mock: no status configured"))
    }
    fn rig_list(&self, _city: Option<&str>) -> Result<gc::RigListResult> {
        self.rig_list
            .clone()
            .ok_or_else(|| anyhow::anyhow!("mock: no rig_list configured"))
    }
    fn rig_status(&self, _city: Option<&str>, rig: &str) -> Result<gc::RigStatusResult> {
        self.rig_status
            .get(rig)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("mock: no rig_status configured for '{rig}'"))
    }
}

#[derive(Default)]
pub struct MockBd {
    /// Keyed by rig dir.
    pub status: HashMap<String, bd::StatusResult>,
    /// Keyed by (rig dir, status_filter) exactly as requested.
    pub list: HashMap<(String, String), Vec<bd::BeadRaw>>,
}

impl BdAdapter for MockBd {
    fn status(&self, dir: &str) -> Result<bd::StatusResult> {
        self.status
            .get(dir)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("mock: no bd status configured for '{dir}'"))
    }
    fn list(&self, dir: &str, status_filter: &str) -> Result<Vec<bd::BeadRaw>> {
        self.list
            .get(&(dir.to_string(), status_filter.to_string()))
            .cloned()
            .ok_or_else(|| {
                anyhow::anyhow!("mock: no bd list configured for '{dir}' status '{status_filter}'")
            })
    }
}

#[derive(Default)]
pub struct MockGit {
    /// Keyed by (repo_dir, branch, target_branch) -- returns false (not
    /// merged) for any combination not explicitly configured, same as the
    /// real LocalGit's lenient "unknown means not landed" behavior.
    pub merged: HashMap<(String, String, String), bool>,
    /// Keyed by (repo_dir, prefix) -- returns an empty vec (no matching
    /// branches) for any combination not explicitly configured, same as the
    /// real LocalGit's lenient "unknown means nothing found" behavior.
    pub remote_branches: HashMap<(String, String), Vec<String>>,
    /// Keyed by repo_dir -- returns `None` (base branch undetermined) for
    /// any repo not explicitly configured.
    pub default_branch: HashMap<String, String>,
}

impl GitAdapter for MockGit {
    fn is_merged(&self, repo_dir: &str, branch: &str, target_branch: &str) -> bool {
        self.merged
            .get(&(
                repo_dir.to_string(),
                branch.to_string(),
                target_branch.to_string(),
            ))
            .copied()
            .unwrap_or(false)
    }

    fn remote_branches(&self, repo_dir: &str, prefix: &str) -> Vec<String> {
        self.remote_branches
            .get(&(repo_dir.to_string(), prefix.to_string()))
            .cloned()
            .unwrap_or_default()
    }

    fn default_branch(&self, repo_dir: &str) -> Option<String> {
        self.default_branch.get(repo_dir).cloned()
    }
}
