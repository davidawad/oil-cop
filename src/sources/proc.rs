//! Subprocess execution helper shared by the `gc` and `bd` wrappers.
//!
//! Both CLIs print their JSON cleanly to stdout and route warnings
//! (e.g. "runtime status probe timed out") to stderr, so we capture them
//! separately and surface stderr as context rather than mixing it into the
//! JSON we try to parse. On failure, both also emit a structured JSON error
//! envelope on stderr (`{"level":"error","code":"...","message":"..."}`) --
//! we parse that and surface just the message (plus a hint for known
//! codes) instead of dumping the raw blob.

use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use std::process::{Command, ExitStatus};

pub struct Output {
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Deserialize)]
struct ToolErrorEnvelope {
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

/// Turn a failed invocation into a readable error: if stderr is the JSON
/// error envelope both `gc` and `bd` emit on failure, surface its
/// `message` (plus a hint for error codes we recognize) instead of the raw
/// blob; otherwise fall back to the exit status and trimmed stderr as-is.
fn friendly_error(bin: &str, args: &[&str], status: ExitStatus, stderr: &str) -> String {
    let trimmed = stderr.trim();
    let invocation = format!("`{bin} {}`", args.join(" "));

    if let Ok(envelope) = serde_json::from_str::<ToolErrorEnvelope>(trimmed) {
        if let Some(message) = envelope.message {
            let hint = match envelope.code.as_deref() {
                Some(code) if code.contains("city_resolve") => {
                    "\nhint: pass --city <path>, or set `city` in .oilcop.toml \
                     (project-local) / ~/.config/oil-cop/config.toml (global)"
                }
                _ => "",
            };
            return format!("{invocation}: {message}{hint}");
        }
    }

    format!("{invocation} exited with {status}: {trimmed}")
}

pub fn run(bin: &str, args: &[&str]) -> Result<Output> {
    let out = Command::new(bin)
        .args(args)
        .output()
        .with_context(|| format!("failed to spawn `{bin}` (is it installed and on PATH?)"))?;

    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();

    if !out.status.success() {
        anyhow::bail!(friendly_error(bin, args, out.status, &stderr));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn friendly_error_extracts_message_and_hint_for_city_resolve() {
        let stderr = r#"{"schema_version":"1","level":"error","code":"city_resolve_failed","message":"gc rig list: not in a city directory (no city.toml or .gc/ found)","exit_code":1}"#;
        let status = std::process::Command::new("false")
            .status()
            .expect("spawn false");
        let msg = friendly_error("gc", &["rig", "list", "--json"], status, stderr);
        assert!(msg.contains("not in a city directory"));
        assert!(msg.contains("--city"));
        assert!(msg.contains(".oilcop.toml"));
        assert!(!msg.contains("schema_version"));
    }

    #[test]
    fn friendly_error_falls_back_for_non_json_stderr() {
        let status = std::process::Command::new("false")
            .status()
            .expect("spawn false");
        let msg = friendly_error("gc", &["status"], status, "some plain text error");
        assert!(msg.contains("some plain text error"));
        assert!(msg.contains("exited with"));
    }
}
