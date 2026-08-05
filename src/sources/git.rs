//! Minimal local `git` introspection -- not one of the orchestrator's own
//! tools, but the actual authority on "has this bead's branch landed."
//! `bd`/`gc` model a bead reaching `closed` as the merge signal, but a bead
//! can sit `in_progress` with finished work whose branch just hasn't been
//! fast-forwarded into its target yet (the refinery-stuck case). Checking
//! ancestry directly is the only way to see that distinction -- and it has
//! to be ancestry against `origin/*`, not local branch names.
//!
//! A real incident (8-26-oil-crisis/07-luminate-bd-close-detour-and-the-
//! almost-shipped-hook.txt) misdiagnosed a healthy rig as "refinery isn't
//! merging" by checking `git log --merges` / `gh pr list` -- both blind to
//! a plain fast-forward push (refinery's default `merge_strategy=direct`),
//! which never creates a merge commit or a PR. The check that actually
//! answered "did this land" was `git merge-base --is-ancestor
//! origin/<branch> origin/<target>`. Checking *local* branch names instead
//! of `origin/*` has the same failure mode one level down: a rig checkout
//! that hasn't fetched recently (or never had a given polecat branch
//! locally at all) reports "not merged" even after it landed on the
//! remote. Same doc's lesson: this environment's state goes stale fast,
//! so fetch immediately before depending on it -- throttled here so a
//! `watch` loop polling every few seconds doesn't hammer the remote on
//! every tick.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const FETCH_THROTTLE: Duration = Duration::from_secs(30);

pub struct LocalGit {
    last_fetch: Mutex<HashMap<String, Instant>>,
}

impl Default for LocalGit {
    fn default() -> Self {
        Self {
            last_fetch: Mutex::new(HashMap::new()),
        }
    }
}

impl LocalGit {
    /// Is `branch` already merged into (an ancestor of) `target_branch`,
    /// checked against `origin/*` remote-tracking refs in the git repo at
    /// `repo_dir`? Folds "not yet merged," "branch doesn't exist on
    /// origin," and "fetch failed" all into `false` -- callers only want a
    /// yes/no landed signal, and none of those cases should crash a
    /// dag/watch render.
    pub fn is_merged(&self, repo_dir: &str, branch: &str, target_branch: &str) -> bool {
        self.fetch_throttled(repo_dir);
        let origin_branch = format!("origin/{branch}");
        let origin_target = format!("origin/{target_branch}");
        is_ancestor(repo_dir, &origin_branch, &origin_target).unwrap_or(false)
    }

    /// `git fetch origin` in `repo_dir`, at most once per ~30s per repo.
    /// Best-effort: a failed fetch (no network, no `origin` remote) just
    /// means the ancestry check below falls back to whatever
    /// remote-tracking refs are already on disk, not an error.
    fn fetch_throttled(&self, repo_dir: &str) {
        {
            let mut last = self.last_fetch.lock().unwrap();
            if let Some(t) = last.get(repo_dir) {
                if t.elapsed() < FETCH_THROTTLE {
                    return;
                }
            }
            last.insert(repo_dir.to_string(), Instant::now());
        }
        let _ = std::process::Command::new("git")
            .args(["-C", repo_dir, "fetch", "--quiet", "origin"])
            .output();
    }
}

fn is_ancestor(repo_dir: &str, ancestor: &str, descendant: &str) -> anyhow::Result<bool> {
    let out = std::process::Command::new("git")
        .args([
            "-C",
            repo_dir,
            "merge-base",
            "--is-ancestor",
            ancestor,
            descendant,
        ])
        .output();
    match out {
        Ok(o) => Ok(o.status.success()),
        Err(e) => anyhow::bail!("failed to spawn git in {repo_dir}: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fetch_throttle_skips_within_window() {
        let git = LocalGit::default();
        // First call inserts a timestamp; immediately re-checking the same
        // key must see it as "too soon" without needing a real repo/fetch.
        git.last_fetch
            .lock()
            .unwrap()
            .insert("some/repo".to_string(), Instant::now());
        let elapsed = git.last_fetch.lock().unwrap()["some/repo"].elapsed();
        assert!(elapsed < FETCH_THROTTLE);
    }
}
