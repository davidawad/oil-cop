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

    // -- is_merged, against a real (local, no-network) git repo -----------
    //
    // This is the exact logic oilcop-t37 fixed (checking origin/* refs
    // instead of local branch names) -- worth verifying against a real
    // repo, not just a fake stub that hardcodes the answer.

    fn git_cmd(dir: &std::path::Path, args: &[&str]) {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .expect("failed to spawn git");
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// A bare "origin" repo plus a working clone with `origin` configured,
    /// an initial commit on `main`, already pushed. Returns the working
    /// clone's path (what `LocalGit` operates against).
    fn setup_repo_with_origin() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().expect("tempdir");
        let bare = tmp.path().join("origin.git");
        let work = tmp.path().join("work");

        git_cmd(tmp.path(), &["init", "--quiet", "--bare", "origin.git"]);
        git_cmd(
            tmp.path(),
            &[
                "clone",
                "--quiet",
                bare.to_str().unwrap(),
                work.to_str().unwrap(),
            ],
        );
        git_cmd(&work, &["config", "user.email", "test@example.com"]);
        git_cmd(&work, &["config", "user.name", "Test"]);
        git_cmd(&work, &["checkout", "--quiet", "-b", "main"]);
        std::fs::write(work.join("README.md"), "initial\n").unwrap();
        git_cmd(&work, &["add", "."]);
        git_cmd(&work, &["commit", "--quiet", "-m", "initial"]);
        git_cmd(&work, &["push", "--quiet", "-u", "origin", "main"]);

        tmp
    }

    #[test]
    fn is_merged_true_for_a_branch_fast_forwarded_into_target_on_origin() {
        let tmp = setup_repo_with_origin();
        let work = tmp.path().join("work");

        // Direct-strategy landing: commit straight onto main (no separate
        // feature branch kept around), same shape as refinery's default
        // merge_strategy=direct fast-forward push.
        std::fs::write(work.join("feature.txt"), "done\n").unwrap();
        git_cmd(&work, &["add", "."]);
        git_cmd(&work, &["commit", "--quiet", "-m", "feature work"]);
        git_cmd(&work, &["branch", "polecat/feature-1"]);
        git_cmd(&work, &["push", "--quiet", "origin", "main"]);
        git_cmd(&work, &["push", "--quiet", "origin", "polecat/feature-1"]);

        let git = LocalGit::default();
        assert!(git.is_merged(work.to_str().unwrap(), "polecat/feature-1", "main"));
    }

    #[test]
    fn is_merged_false_for_a_branch_pushed_but_never_landed_on_target() {
        let tmp = setup_repo_with_origin();
        let work = tmp.path().join("work");

        git_cmd(&work, &["checkout", "--quiet", "-b", "polecat/stuck-1"]);
        std::fs::write(work.join("stuck.txt"), "not merged\n").unwrap();
        git_cmd(&work, &["add", "."]);
        git_cmd(&work, &["commit", "--quiet", "-m", "stuck work"]);
        git_cmd(
            &work,
            &["push", "--quiet", "-u", "origin", "polecat/stuck-1"],
        );

        let git = LocalGit::default();
        assert!(!git.is_merged(work.to_str().unwrap(), "polecat/stuck-1", "main"));
    }

    #[test]
    fn is_merged_false_when_the_branch_was_never_pushed_to_origin_at_all() {
        let tmp = setup_repo_with_origin();
        let work = tmp.path().join("work");

        let git = LocalGit::default();
        assert!(!git.is_merged(work.to_str().unwrap(), "polecat/never-existed", "main"));
    }

    #[test]
    fn is_merged_false_when_repo_dir_is_not_a_git_repo_at_all() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let git = LocalGit::default();
        assert!(!git.is_merged(tmp.path().to_str().unwrap(), "any-branch", "main"));
    }
}
