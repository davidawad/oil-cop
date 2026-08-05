//! Minimal local `git` introspection -- not one of the orchestrator's own
//! tools, but the actual authority on "has this bead's branch landed."
//! `bd`/`gc` model a bead reaching `closed` as the merge signal, but a bead
//! can sit `in_progress` with finished work whose branch just hasn't been
//! fast-forwarded into its target yet (the refinery-stuck case). Checking
//! ancestry directly in the rig's own working tree is the only way to see
//! that distinction.

/// Is `branch` already merged into (an ancestor of) `target_branch`,
/// checked in the git repo at `repo_dir`? `Ok(false)` covers both "not yet
/// merged" and "branch doesn't exist here" -- callers treat both as "not
/// landed yet," which is the useful signal for a DAG's refinery stage.
pub fn is_merged(repo_dir: &str, branch: &str, target_branch: &str) -> anyhow::Result<bool> {
    let out = std::process::Command::new("git")
        .args([
            "-C",
            repo_dir,
            "merge-base",
            "--is-ancestor",
            branch,
            target_branch,
        ])
        .output();
    match out {
        Ok(o) => Ok(o.status.success()),
        Err(e) => anyhow::bail!("failed to spawn git in {repo_dir}: {e}"),
    }
}

/// Same check, but folds "not a git repo here" / "branch not found" into
/// `false` instead of erroring -- callers only want a yes/no landed signal.
pub fn is_merged_lenient(repo_dir: &str, branch: &str, target_branch: &str) -> bool {
    is_merged(repo_dir, branch, target_branch).unwrap_or(false)
}
