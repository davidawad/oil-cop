use super::{agents, dag, queue, status};
use crate::assemble;
use crate::health::Thresholds;
use crate::sources::adapters::Adapters;
use crate::sources::bd;
use chrono::Utc;
use colored::Colorize;
use std::io::Write;
use std::time::Duration;

pub fn run(
    adapters: &Adapters,
    city: Option<&str>,
    rig: Option<&str>,
    interval_secs: u64,
    thresholds: Thresholds,
) -> anyhow::Result<()> {
    // Clear once up front so the first frame starts from a blank screen;
    // every frame after that re-renders in place (cursor home, redraw,
    // erase-to-end-of-screen for anything the new frame is shorter than)
    // instead of blanking the whole screen every tick, which reads as a
    // visible flicker on real terminals.
    print!("\x1B[2J");
    let mut tick: u64 = 0;
    loop {
        print!("\x1B[H"); // cursor to top-left, no erase -- draw over the previous frame
        let now = Utc::now();
        println!(
            "{} {}  {}",
            "oil-cop watch".bold(),
            now.format("%Y-%m-%d %H:%M:%S UTC"),
            format!("(every {interval_secs}s, ctrl-c to exit)").dimmed()
        );
        println!();

        match adapters.gc.status(city) {
            Ok(raw) => {
                let mut buf: Vec<u8> = Vec::new();
                if status::render(&assemble::city_view(adapters, raw), Some(tick), &mut buf).is_ok()
                {
                    std::io::stdout().write_all(&buf).ok();
                }
            }
            Err(e) => println!("{} status: {e}", "error".red().bold()),
        }

        if let Some(rig_name) = rig {
            println!();
            // Build the whole rig section into a buffer, then flush it to
            // real stdout in one write -- this both keeps the render logic
            // itself (`render_rig`) directly unit-testable (it just writes
            // into any `impl Write`) and, as a side effect, cuts down
            // flicker further versus writing each sub-section straight to
            // the terminal as it's produced.
            let mut buf: Vec<u8> = Vec::new();
            match render_rig(adapters, city, rig_name, now, &thresholds, tick, &mut buf) {
                Ok(()) => {
                    std::io::stdout().write_all(&buf).ok();
                }
                Err(e) => println!("{} rig '{rig_name}': {e}", "error".red().bold()),
            }
        }

        // Erase anything left over from a previous, longer frame (e.g. the
        // DAG shrinking as beads close) without touching what was just drawn.
        print!("\x1B[0J");
        std::io::stdout().flush().ok();
        std::thread::sleep(Duration::from_secs(interval_secs.max(1)));
        tick = tick.wrapping_add(1);
    }
}

/// Assembles and renders one rig's queue/agents/dag sections into `w`. Pure
/// glue over `assemble::*` + the sub-renderers -- kept as its own function
/// (rather than inlined in the `loop` above) specifically so it's directly
/// unit-testable against mock adapters, without needing the infinite loop
/// or a real terminal.
#[allow(clippy::too_many_arguments)]
fn render_rig(
    adapters: &Adapters,
    city: Option<&str>,
    rig: &str,
    now: chrono::DateTime<Utc>,
    thresholds: &Thresholds,
    tick: u64,
    w: &mut impl Write,
) -> anyhow::Result<()> {
    let resolved = adapters.resolve_rig(city, rig)?;

    // bd.status, the full (non-closed) bead list, and gc.rig_status are all
    // independent of each other -- fetch all three concurrently instead of
    // one after another. The full bead list is a superset of "in_progress"
    // (queue/agents only need that subset, dag wants the whole thing), so
    // fetching it once and filtering in-process also removes what used to
    // be a second, redundant `bd list` call just for dag's sake. Bind the
    // two fields used here (not `adapters` as a whole -- its `git` field
    // deliberately isn't Sync, see Adapters' doc comment); `git` itself is
    // still used below, sequentially, via `adapters` directly.
    let bd_adapter = adapters.bd.as_ref();
    let gc_adapter = adapters.gc.as_ref();
    let (bd_status, all_beads, rig_status) = std::thread::scope(|scope| {
        let path = resolved.path.as_str();
        let status_h = scope.spawn(|| bd_adapter.status(path));
        let list_h = scope.spawn(|| bd_adapter.list(path, "open,in_progress,blocked,deferred"));
        let rig_status_h = scope.spawn(|| gc_adapter.rig_status(city, rig));
        (
            status_h.join().expect("bd.status thread panicked"),
            list_h.join().expect("bd.list thread panicked"),
            rig_status_h.join().expect("gc.rig_status thread panicked"),
        )
    });
    let bd_status = bd_status?;
    let all_beads = all_beads?;
    let rig_status = rig_status?;

    let in_progress: Vec<bd::BeadRaw> = all_beads
        .iter()
        .filter(|b| b.status == "in_progress")
        .cloned()
        .collect();

    let qview = assemble::queue_view(
        &resolved.name,
        &resolved.path,
        resolved.running,
        bd_status,
        in_progress.clone(),
        now,
        thresholds,
    );
    queue::render(&qview, Some(tick), 15, w)?;

    writeln!(w)?;
    let suspended = rig_status.rig.suspended;
    let views = assemble::agent_views(rig_status.agents, &in_progress, now, thresholds);
    agents::render(&resolved.name, &views, suspended, Some(tick), w)?;

    writeln!(w)?;
    let dview = assemble::dag_view_from_beads(adapters, &resolved, &all_beads, now);
    dag::render(&dview, Some(tick), w)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::gc;
    use crate::sources::mocks::{MockBd, MockGc, MockGit};
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

        Adapters {
            gc: Box::new(MockGc {
                status: None,
                rig_list: Some(gc::RigListResult { rigs: vec![rig] }),
                rig_status,
            }),
            bd: Box::new(MockBd {
                status: status_by_dir,
                list: list_by_key,
            }),
            git: Box::new(MockGit::default()),
        }
    }

    #[test]
    fn render_rig_assembles_queue_agents_and_dag_sections() {
        colored::control::set_override(false);
        let adapters = adapters_for_rig("/fake/luminate");
        let now = chrono::DateTime::parse_from_rfc3339("2020-01-01T00:10:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut buf = Vec::new();
        render_rig(&adapters, None, "luminate", now, &thresholds(), 0, &mut buf)
            .expect("render_rig should succeed against fully-configured mocks");
        let out = String::from_utf8(buf).unwrap();

        // Queue section.
        assert!(out.contains("luminate"));
        assert!(out.contains("2 total"));
        // Agents section.
        assert!(out.contains("nux"));
        assert!(out.contains("luminate-1"));
        // DAG section.
        assert!(out.contains("title for luminate-1"));
    }

    #[test]
    fn render_rig_surfaces_an_unknown_rig_as_an_error_not_a_panic() {
        let adapters = adapters_for_rig("/fake/luminate");
        let now = Utc::now();
        let mut buf = Vec::new();
        let result = render_rig(
            &adapters,
            None,
            "not-a-real-rig",
            now,
            &thresholds(),
            0,
            &mut buf,
        );
        assert!(result.is_err());
    }
}
