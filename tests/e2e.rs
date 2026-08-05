//! Black-box end-to-end tests: invoke the compiled `oil-cop` binary as a
//! real subprocess, exactly as a user would, against fake `gc`/`bd`/`git`
//! executables (tests/fixtures/bin/) that return canned real-schema JSON
//! (tests/fixtures/data/) instead of talking to an actual Gas City stack.
//! This is the layer the adapter architecture (src/sources/adapters.rs)
//! exists to make possible -- nothing here reaches into oil-cop's internals,
//! it only checks what a real invocation prints and exits with.

use serde_json::Value;
use std::path::PathBuf;
use std::process::{Command, Output};

fn fixtures_bin_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bin")
}

/// Run the real compiled binary with the fake gc/bd/git prepended to PATH.
fn run(args: &[&str]) -> Output {
    let real_path = std::env::var("PATH").unwrap_or_default();
    let fake_path_first = format!("{}:{}", fixtures_bin_dir().display(), real_path);
    Command::new(env!("CARGO_BIN_EXE_oil-cop"))
        .args(args)
        .env("PATH", fake_path_first)
        .env("NO_COLOR", "1")
        .output()
        .expect("failed to spawn oil-cop binary")
}

fn run_json(args: &[&str]) -> Value {
    let out = run(args);
    assert!(
        out.status.success(),
        "expected success, got {:?}\nstderr: {}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        panic!(
            "stdout wasn't valid JSON: {e}\nstdout: {}",
            String::from_utf8_lossy(&out.stdout)
        )
    })
}

#[test]
fn status_reports_city_and_rig_rollup() {
    let v = run_json(&["--json", "status"]);
    assert_eq!(v["city_name"], "testcity");
    assert_eq!(v["controller_running"], true);
    assert_eq!(v["total_agents"], 2);
    assert_eq!(v["running_agents"], 1);

    let rigs = v["rigs"].as_array().expect("rigs array");
    let testrig = rigs
        .iter()
        .find(|r| r["name"] == "testrig")
        .expect("testrig present");
    assert_eq!(testrig["health"], "healthy"); // has a live, non-suspended rig-scoped agent
    assert_eq!(testrig["bead_summary"]["ready"], 1);
    assert_eq!(testrig["bead_summary"]["in_progress"], 2);
    assert_eq!(testrig["bead_summary"]["blocked"], 1);

    let sleepyrig = rigs
        .iter()
        .find(|r| r["name"] == "sleepyrig")
        .expect("sleepyrig present");
    assert_eq!(sleepyrig["health"], "suspended");
    assert!(
        sleepyrig["bead_summary"].is_null(),
        "suspended rigs should skip the extra bd status call"
    );
}

#[test]
fn status_text_mode_does_not_crash() {
    let out = run(&["status"]);
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("testcity"));
}

#[test]
fn queue_sorts_stale_bead_first_and_reports_counts() {
    let v = run_json(&["--json", "queue", "testrig"]);
    assert_eq!(v["rig_name"], "testrig");
    assert_eq!(v["summary"]["total"], 5);
    assert_eq!(v["summary"]["ready"], 1);
    assert_eq!(v["summary"]["in_progress"], 2);

    let in_progress = v["in_progress"].as_array().expect("in_progress array");
    assert_eq!(in_progress.len(), 2);
    // Both fixture in-progress beads are pinned to 2020 -- far past any
    // stale_after threshold -- so both must read as stale.
    for bead in in_progress {
        assert_eq!(bead["health"], "stale");
    }
}

#[test]
fn agents_joins_bead_by_runtime_session_name() {
    let v = run_json(&["--json", "agents", "testrig"]);
    let agents = v.as_array().expect("agents array");

    let polecat = agents
        .iter()
        .find(|a| a["qualified_name"] == "testrig/gastown.polecat")
        .expect("polecat present");
    assert_eq!(polecat["running"], true);
    assert_eq!(polecat["current_bead"]["id"], "test-1");
    assert_eq!(polecat["health"], "stale"); // running, but its bead is stale

    let refinery = agents
        .iter()
        .find(|a| a["qualified_name"] == "testrig/gastown.refinery")
        .expect("refinery present");
    assert_eq!(refinery["running"], false);
    assert_eq!(refinery["health"], "dead");
    assert!(refinery["current_bead"].is_null());
}

#[test]
fn dag_reports_stage_and_landed_unmerged_per_bead() {
    let v = run_json(&["--json", "dag", "testrig", "--all"]);
    let nodes = v["nodes"].as_array().expect("nodes array");
    assert_eq!(nodes.len(), 5);

    let by_id = |id: &str| {
        nodes
            .iter()
            .find(|n| n["id"] == id)
            .unwrap_or_else(|| panic!("node {id} present"))
    };

    assert_eq!(by_id("test-1")["stage"], "active");
    assert_eq!(by_id("test-1")["landed_unmerged"], false);

    assert_eq!(by_id("test-2")["stage"], "pending");

    assert_eq!(by_id("test-3")["stage"], "pending");
    assert_eq!(by_id("test-3")["blocked_by"][0], "test-2");

    assert_eq!(by_id("test-4")["stage"], "merged");

    // test-5's branch ("polecat/test-5-landed") is the one the fake git
    // reports as already merged -- this is the whole point of the DAG's
    // landed-but-unclosed signal (bead oilcop-t37).
    assert_eq!(by_id("test-5")["stage"], "active");
    assert_eq!(by_id("test-5")["landed_unmerged"], true);
}

#[test]
fn dag_excludes_closed_by_default() {
    let v = run_json(&["--json", "dag", "testrig"]);
    let nodes = v["nodes"].as_array().expect("nodes array");
    assert!(nodes.iter().all(|n| n["id"] != "test-4"));
}

#[test]
fn check_exits_nonzero_and_lists_issues_when_stale_work_exists() {
    let out = run(&["check", "testrig"]);
    assert!(!out.status.success(), "expected a nonzero exit code");
    assert_eq!(out.status.code(), Some(1));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("issue(s) found"));
    assert!(text.contains("test-1") || text.contains("bead:test-1"));
}

#[test]
fn check_stale_threshold_only_affects_bead_staleness() {
    // A 100-year threshold means the 2020-pinned fixture bead timestamps
    // never count as stale -- but the fixture's refinery agent
    // (running: false) is a genuine problem regardless of any staleness
    // threshold, so the check still fails overall; the point of this test
    // is that the *stale-bead* issues specifically disappear.
    let out = Command::new(env!("CARGO_BIN_EXE_oil-cop"))
        .args(["--stale-after", "876000h", "check", "testrig"])
        .env(
            "PATH",
            format!(
                "{}:{}",
                fixtures_bin_dir().display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        )
        .env("NO_COLOR", "1")
        .output()
        .expect("failed to spawn oil-cop binary");
    assert_eq!(out.status.code(), Some(1));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(!text.contains("in_progress but stale"));
    assert!(text.contains("agent is not running"));
}

#[test]
fn check_city_wide_only_is_clean_with_no_rig_given() {
    // No rig given at all -- only city-wide signals are checked, and the
    // fixture city is healthy, so this must exit 0 regardless of the
    // testrig fixture's stale beads / dead agent.
    let out = run(&["check"]);
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("all clear"));
}

#[test]
fn unknown_rig_name_fails_clearly() {
    let out = run(&["queue", "not-a-real-rig"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("no rig named"));
}
