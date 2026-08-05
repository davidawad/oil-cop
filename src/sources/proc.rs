//! Subprocess execution helper shared by the `gc` and `bd` wrappers.
//!
//! Both CLIs print their JSON cleanly to stdout and route warnings
//! (e.g. "runtime status probe timed out") to stderr, so we capture them
//! separately and surface stderr as context rather than mixing it into the
//! JSON we try to parse.

use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use std::process::Command;

pub struct Output {
    pub stdout: String,
    pub stderr: String,
}

pub fn run(bin: &str, args: &[&str]) -> Result<Output> {
    let out = Command::new(bin)
        .args(args)
        .output()
        .with_context(|| format!("failed to spawn `{bin}` (is it installed and on PATH?)"))?;

    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();

    if !out.status.success() {
        anyhow::bail!(
            "`{bin} {}` exited with {}: {}",
            args.join(" "),
            out.status,
            stderr.trim()
        );
    }

    Ok(Output { stdout, stderr })
}

/// Run a command and parse its stdout as JSON, with stderr attached as
/// context on any failure (parse or exit-status).
pub fn run_json<T: DeserializeOwned>(bin: &str, args: &[&str]) -> Result<(T, Output)> {
    let out = run(bin, args)?;
    let parsed: T = serde_json::from_str(&out.stdout).with_context(|| {
        format!(
            "failed to parse JSON from `{bin} {}`\nstdout: {}\nstderr: {}",
            args.join(" "),
            truncate(&out.stdout, 500),
            out.stderr.trim()
        )
    })?;
    Ok((parsed, out))
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        &s[..max]
    }
}
