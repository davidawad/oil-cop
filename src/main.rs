mod assemble;
mod cli;
mod config;
mod health;
mod model;
mod render;
mod sources;

use anyhow::Result;
use chrono::Utc;
use clap::{CommandFactory, Parser};
use cli::{Cli, Command};
use health::Thresholds;
use sources::adapters::Adapters;
use std::io::Write;

/// Thin wrapper around the render functions that write into a generic
/// `impl Write` (rather than printing directly) so they're unit-testable in
/// isolation: build the output in memory, then flush it to real stdout in
/// one write.
fn write_to_stdout<F>(f: F) -> Result<()>
where
    F: FnOnce(&mut Vec<u8>) -> std::io::Result<()>,
{
    let mut buf = Vec::new();
    f(&mut buf)?;
    std::io::stdout().write_all(&buf)?;
    Ok(())
}

fn main() {
    let cli = Cli::parse();
    render::color::init(cli.no_color);

    if let Err(e) = run(cli) {
        eprintln!("oil-cop: {e:#}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<()> {
    let file_cfg = config::load();

    let stale_after = cli
        .stale_after
        .clone()
        .or_else(|| file_cfg.stale_after.clone())
        .unwrap_or_else(|| "30m".to_string());
    let thresholds = Thresholds {
        stale_after_secs: health::parse_duration_secs(&stale_after)?,
    };
    let city = cli.city.clone().or_else(|| file_cfg.city.clone());
    let city = city.as_deref();
    let adapters = Adapters::default();

    let rig_or_default = |rig: &Option<String>| -> Result<String> {
        rig.clone()
            .or_else(|| file_cfg.default_rig.clone())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "no rig given, and no default_rig set in .oilcop.toml / ~/.config/oil-cop/config.toml"
                )
            })
    };

    match &cli.command {
        Command::Completion { shell } => {
            clap_complete::generate(
                *shell,
                &mut cli::Cli::command(),
                "oil-cop",
                &mut std::io::stdout(),
            );
            Ok(())
        }
        Command::Status => cmd_status(&adapters, city, cli.json),
        Command::Queue { rig, limit } => cmd_queue(
            &adapters,
            city,
            &rig_or_default(rig)?,
            *limit,
            cli.json,
            &thresholds,
        ),
        Command::Agents { rig } => cmd_agents(
            &adapters,
            city,
            &rig_or_default(rig)?,
            cli.json,
            &thresholds,
        ),
        Command::Dag { rig, all } => {
            cmd_dag(&adapters, city, &rig_or_default(rig)?, *all, cli.json)
        }
        Command::Check { rig } => {
            let rig = rig.clone().or_else(|| file_cfg.default_rig.clone());
            cmd_check(&adapters, city, rig.as_deref(), cli.json, &thresholds)
        }
        Command::Sessions { rig } => {
            // Deliberately does NOT fall back to `default_rig` like every
            // other command here -- omitting `rig` means "scan every rig's
            // sessions city-wide," which is the whole point (oilcop-e1c):
            // surfacing this automatically across the city, not just the
            // one rig a human happens to be looking at.
            cmd_sessions(&adapters, city, rig.as_deref(), cli.json, &thresholds)
        }
        Command::Watch { interval, rig } => {
            let rig = rig.clone().or_else(|| file_cfg.default_rig.clone());
            render::watch::run(&adapters, city, rig.as_deref(), *interval, thresholds)
        }
    }
}

fn cmd_status(adapters: &Adapters, city: Option<&str>, json: bool) -> Result<()> {
    let raw = adapters.gc.status(city)?;
    let view = assemble::city_view(adapters, raw);
    if json {
        println!("{}", serde_json::to_string_pretty(&view)?);
    } else {
        write_to_stdout(|w| render::status::render(&view, None, w))?;
    }
    Ok(())
}

fn cmd_queue(
    adapters: &Adapters,
    city: Option<&str>,
    rig: &str,
    limit: usize,
    json: bool,
    thresholds: &Thresholds,
) -> Result<()> {
    let resolved = adapters.resolve_rig(city, rig)?;
    // bd.status and bd.list are independent -- fetch them concurrently
    // instead of one after another. Bind just `bd` (not `adapters` as a
    // whole -- its `git` field deliberately isn't Sync, see Adapters' doc
    // comment) before spawning.
    let bd_adapter = adapters.bd.as_ref();
    let (bd_status, in_progress) = std::thread::scope(|scope| {
        let path = resolved.path.as_str();
        let status_h = scope.spawn(|| bd_adapter.status(path));
        let list_h = scope.spawn(|| bd_adapter.list(path, "in_progress"));
        (
            status_h.join().expect("bd.status thread panicked"),
            list_h.join().expect("bd.list thread panicked"),
        )
    });
    let bd_status = bd_status?;
    let in_progress = in_progress?;
    let now = Utc::now();
    let view = assemble::queue_view(
        &resolved.name,
        &resolved.path,
        resolved.running,
        bd_status,
        in_progress,
        now,
        thresholds,
    );
    if json {
        println!("{}", serde_json::to_string_pretty(&view)?);
    } else {
        write_to_stdout(|w| render::queue::render(&view, None, limit, w))?;
    }
    Ok(())
}

fn cmd_agents(
    adapters: &Adapters,
    city: Option<&str>,
    rig: &str,
    json: bool,
    thresholds: &Thresholds,
) -> Result<()> {
    let resolved = adapters.resolve_rig(city, rig)?;
    // bd.list, gc.rig_status, and gc.session_list hit different services
    // entirely -- independent, so fetch them concurrently. Bind the two
    // `Sync` fields (not `adapters` as a whole -- its `git` field
    // deliberately isn't Sync).
    let bd_adapter = adapters.bd.as_ref();
    let gc_adapter = adapters.gc.as_ref();
    let (in_progress, rig_status, session_list) = std::thread::scope(|scope| {
        let path = resolved.path.as_str();
        let list_h = scope.spawn(|| bd_adapter.list(path, "in_progress"));
        let rig_status_h = scope.spawn(|| gc_adapter.rig_status(city, rig));
        let session_list_h = scope.spawn(|| gc_adapter.session_list(city));
        (
            list_h.join().expect("bd.list thread panicked"),
            rig_status_h.join().expect("gc.rig_status thread panicked"),
            session_list_h
                .join()
                .expect("gc.session_list thread panicked"),
        )
    });
    let in_progress = in_progress?;
    let rig_status = rig_status?;
    let session_list = session_list?;
    let now = Utc::now();
    let suspended = rig_status.rig.suspended;
    let sessions = assemble::session_signals(&session_list, &std::collections::HashMap::new(), now);
    let views = assemble::agent_views(rig_status.agents, &in_progress, &sessions, now, thresholds);
    if json {
        println!("{}", serde_json::to_string_pretty(&views)?);
    } else {
        write_to_stdout(|w| render::agents::render(&resolved.name, &views, suspended, None, w))?;
    }
    Ok(())
}

fn cmd_dag(
    adapters: &Adapters,
    city: Option<&str>,
    rig: &str,
    include_closed: bool,
    json: bool,
) -> Result<()> {
    let resolved = adapters.resolve_rig(city, rig)?;
    let now = Utc::now();
    let view = assemble::dag_view(adapters, &resolved, include_closed, now)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&view)?);
    } else {
        write_to_stdout(|w| render::dag::render(&view, None, w))?;
    }
    Ok(())
}

fn cmd_check(
    adapters: &Adapters,
    city: Option<&str>,
    rig: Option<&str>,
    json: bool,
    thresholds: &Thresholds,
) -> Result<()> {
    let report = assemble::check(adapters, city, rig, thresholds)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        render::check::render(&report);
    }
    if !report.ok {
        std::process::exit(1);
    }
    Ok(())
}

fn cmd_sessions(
    adapters: &Adapters,
    city: Option<&str>,
    rig: Option<&str>,
    json: bool,
    thresholds: &Thresholds,
) -> Result<()> {
    let mut session_list = adapters.gc.session_list(city)?;
    if let Some(rig_name) = rig {
        session_list
            .sessions
            .retain(|s| s.rig.as_deref() == Some(rig_name));
    }
    let now = Utc::now();
    let report = assemble::zombie_sessions(&session_list, now, thresholds);
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        write_to_stdout(|w| render::sessions::render(&report, w))?;
    }
    if !report.zombies.is_empty() {
        std::process::exit(1);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sources::mocks::{MockBd, MockGc, MockGit};
    use sources::{bd, gc};
    use std::collections::HashMap;

    fn thresholds() -> Thresholds {
        Thresholds {
            stale_after_secs: 1800,
        }
    }

    fn bead(id: &str, status: &str, assignee: Option<&str>) -> bd::BeadRaw {
        bd::BeadRaw {
            id: id.to_string(),
            title: format!("title for {id}"),
            status: status.to_string(),
            priority: Some(1),
            assignee: assignee.map(str::to_string),
            updated_at: Some("2020-01-01T00:00:00Z".to_string()),
            started_at: None,
            parent: None,
            dependencies: vec![],
            metadata: serde_json::Value::Null,
        }
    }

    fn adapters_for_rig(rig_path: &str) -> Adapters {
        let rig = gc::RigRaw {
            name: "luminate".to_string(),
            path: rig_path.to_string(),
            prefix: None,
            suspended: false,
            running: Some(true),
        };

        let mut status_by_dir = HashMap::new();
        status_by_dir.insert(
            rig_path.to_string(),
            bd::StatusResult {
                summary: bd::StatusSummary {
                    total_issues: 2,
                    ready_issues: 0,
                    in_progress_issues: 1,
                    blocked_issues: 0,
                    deferred_issues: 0,
                    closed_issues: 1,
                },
            },
        );

        let mut list_by_key = HashMap::new();
        list_by_key.insert(
            (rig_path.to_string(), "in_progress".to_string()),
            vec![bead("luminate-1", "in_progress", Some("session-a"))],
        );
        list_by_key.insert(
            (
                rig_path.to_string(),
                "open,in_progress,blocked,deferred".to_string(),
            ),
            vec![bead("luminate-1", "in_progress", Some("session-a"))],
        );
        list_by_key.insert(
            (
                rig_path.to_string(),
                "open,in_progress,blocked,deferred,closed".to_string(),
            ),
            vec![bead("luminate-1", "in_progress", Some("session-a"))],
        );

        let mut rig_status = HashMap::new();
        rig_status.insert(
            "luminate".to_string(),
            gc::RigStatusResult {
                rig: rig.clone(),
                agents: vec![gc::RigAgentRaw {
                    name: "nux".to_string(),
                    qualified_name: "luminate/nux".to_string(),
                    runtime_session_name: Some("session-a".to_string()),
                    session_id: None,
                    running: true,
                    suspended: false,
                    draining: false,
                    status: Some("running".to_string()),
                }],
            },
        );

        let city_status = gc::StatusResult {
            city_name: "testcity".to_string(),
            city_path: "/fake/testcity".to_string(),
            controller: gc::Controller {
                running: true,
                pid: Some(1),
                mode: Some("supervisor".to_string()),
                status: None,
            },
            running: true,
            suspended: false,
            health: gc::HealthBlock {
                usable: true,
                degraded: false,
                signals: vec![],
            },
            agents: vec![],
            rigs: vec![rig.clone()],
            summary: gc::SummaryBlock {
                total_agents: 1,
                running_agents: 1,
            },
            partial: false,
            partial_errors: vec![],
        };

        Adapters {
            gc: Box::new(MockGc {
                status: Some(city_status),
                rig_list: Some(gc::RigListResult { rigs: vec![rig] }),
                rig_status,
                session_list: Some(gc::SessionListResult::default()),
                session_peek: HashMap::new(),
            }),
            bd: Box::new(MockBd {
                status: status_by_dir,
                list: list_by_key,
            }),
            git: Box::new(MockGit::default()),
        }
    }

    #[test]
    fn cmd_status_succeeds_in_both_json_and_text_mode() {
        colored::control::set_override(false);
        let adapters = adapters_for_rig("/fake/luminate");
        assert!(cmd_status(&adapters, None, true).is_ok());
        assert!(cmd_status(&adapters, None, false).is_ok());
    }

    #[test]
    fn cmd_queue_succeeds_for_a_known_rig_and_errors_for_an_unknown_one() {
        colored::control::set_override(false);
        let adapters = adapters_for_rig("/fake/luminate");
        assert!(cmd_queue(&adapters, None, "luminate", 20, true, &thresholds()).is_ok());
        assert!(cmd_queue(&adapters, None, "luminate", 20, false, &thresholds()).is_ok());
        assert!(cmd_queue(&adapters, None, "not-a-real-rig", 20, true, &thresholds()).is_err());
    }

    #[test]
    fn cmd_agents_succeeds_for_a_known_rig_and_errors_for_an_unknown_one() {
        colored::control::set_override(false);
        let adapters = adapters_for_rig("/fake/luminate");
        assert!(cmd_agents(&adapters, None, "luminate", true, &thresholds()).is_ok());
        assert!(cmd_agents(&adapters, None, "luminate", false, &thresholds()).is_ok());
        assert!(cmd_agents(&adapters, None, "not-a-real-rig", true, &thresholds()).is_err());
    }

    #[test]
    fn cmd_dag_succeeds_with_and_without_include_closed() {
        colored::control::set_override(false);
        let adapters = adapters_for_rig("/fake/luminate");
        assert!(cmd_dag(&adapters, None, "luminate", false, true).is_ok());
        assert!(cmd_dag(&adapters, None, "luminate", true, false).is_ok());
    }

    #[test]
    fn cmd_check_succeeds_when_everything_is_healthy() {
        // Only the "everything's fine" path is exercised here -- the
        // failure path calls std::process::exit(1) directly, which would
        // kill the whole test binary if invoked in-process. That path is
        // covered by the e2e tests instead (a real subprocess whose exit
        // code is safe to observe from outside). Deliberately a minimal,
        // rig-free fixture (not `adapters_for_rig`, whose "luminate" rig
        // has no live agent at the city level and would itself register
        // as a problem, tripping the exit(1) this test must avoid).
        colored::control::set_override(false);
        let adapters = Adapters {
            gc: Box::new(MockGc {
                status: Some(gc::StatusResult {
                    city_name: "testcity".to_string(),
                    city_path: "/fake/testcity".to_string(),
                    controller: gc::Controller {
                        running: true,
                        pid: Some(1),
                        mode: Some("supervisor".to_string()),
                        status: None,
                    },
                    running: true,
                    suspended: false,
                    health: gc::HealthBlock {
                        usable: true,
                        degraded: false,
                        signals: vec![],
                    },
                    agents: vec![],
                    rigs: vec![],
                    summary: gc::SummaryBlock {
                        total_agents: 0,
                        running_agents: 0,
                    },
                    partial: false,
                    partial_errors: vec![],
                }),
                rig_list: None,
                rig_status: HashMap::new(),
                session_list: None,
                session_peek: HashMap::new(),
            }),
            bd: Box::new(MockBd::default()),
            git: Box::new(MockGit::default()),
        };
        assert!(cmd_check(&adapters, None, None, true, &thresholds()).is_ok());
    }

    #[test]
    fn write_to_stdout_propagates_the_inner_closures_error() {
        let result = write_to_stdout(|_w| Err(std::io::Error::other("boom")));
        assert!(result.is_err());
    }
}
