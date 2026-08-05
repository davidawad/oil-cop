//! Adapter traits: one per underlying tool (gc, bd, git), so a different
//! backend -- a gastown-pack-specific data source, a mocked one for tests,
//! whatever -- can be swapped in without touching `assemble`/`render`/
//! `main`. Mirrors Gas City's own pack-based extensibility: oil-cop
//! shouldn't be any more hardwired to "shell out to gc/bd" than the city
//! itself is hardwired to one pack.
//!
//! `Adapters` bundles the three and is the one thing callers hold; the
//! default bundle shells out to the real `gc`/`bd`/`git` binaries.

use super::{bd, gc, git};
use anyhow::Result;

pub trait GcAdapter {
    fn status(&self, city: Option<&str>) -> Result<gc::StatusResult>;
    fn rig_list(&self, city: Option<&str>) -> Result<gc::RigListResult>;
    fn rig_status(&self, city: Option<&str>, rig: &str) -> Result<gc::RigStatusResult>;
}

pub trait BdAdapter {
    fn status(&self, dir: &str) -> Result<bd::StatusResult>;
    fn list(&self, dir: &str, status_filter: &str) -> Result<Vec<bd::BeadRaw>>;
}

pub trait GitAdapter {
    /// Has `branch` already landed (merged into) `target_branch`, in the
    /// working tree at `repo_dir`?
    fn is_merged(&self, repo_dir: &str, branch: &str, target_branch: &str) -> bool;
}

pub struct CliGc;

impl GcAdapter for CliGc {
    fn status(&self, city: Option<&str>) -> Result<gc::StatusResult> {
        Ok(gc::status(city)?.0)
    }
    fn rig_list(&self, city: Option<&str>) -> Result<gc::RigListResult> {
        Ok(gc::rig_list(city)?.0)
    }
    fn rig_status(&self, city: Option<&str>, rig: &str) -> Result<gc::RigStatusResult> {
        Ok(gc::rig_status(city, rig)?.0)
    }
}

pub struct CliBd;

impl BdAdapter for CliBd {
    fn status(&self, dir: &str) -> Result<bd::StatusResult> {
        Ok(bd::status(dir)?.0)
    }
    fn list(&self, dir: &str, status_filter: &str) -> Result<Vec<bd::BeadRaw>> {
        Ok(bd::list(dir, status_filter)?.0)
    }
}

impl GitAdapter for git::LocalGit {
    fn is_merged(&self, repo_dir: &str, branch: &str, target_branch: &str) -> bool {
        git::LocalGit::is_merged(self, repo_dir, branch, target_branch)
    }
}

/// The set of adapters oil-cop's commands depend on. Construct once per run
/// (`Adapters::default()` for the real CLI-backed set) and pass it down —
/// nothing downstream calls `gc::`/`bd::`/`git::` functions directly.
pub struct Adapters {
    pub gc: Box<dyn GcAdapter>,
    pub bd: Box<dyn BdAdapter>,
    pub git: Box<dyn GitAdapter>,
}

impl Default for Adapters {
    fn default() -> Self {
        Self {
            gc: Box::new(CliGc),
            bd: Box::new(CliBd),
            git: Box::new(git::LocalGit::default()),
        }
    }
}

impl Adapters {
    /// Resolve a `--rig` argument the same way `gc` itself does: a
    /// filesystem path if one exists, otherwise a registered rig name
    /// looked up via `gc rig list`.
    pub fn resolve_rig(&self, city: Option<&str>, rig: &str) -> Result<gc::RigRaw> {
        if std::path::Path::new(rig).is_dir() {
            return Ok(gc::RigRaw {
                name: rig.to_string(),
                path: rig.to_string(),
                prefix: None,
                suspended: false,
                running: None,
            });
        }
        let list = self.gc.rig_list(city)?;
        list.rigs
            .into_iter()
            .find(|r| r.name == rig)
            .ok_or_else(|| {
                anyhow::anyhow!(
                "no rig named '{rig}' registered with this city, and it isn't a directory either"
            )
            })
    }
}
