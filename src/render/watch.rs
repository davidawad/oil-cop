use super::{agents, queue, status};
use crate::assemble;
use crate::health::Thresholds;
use crate::sources::adapters::Adapters;
use chrono::Utc;
use colored::Colorize;
use std::collections::HashMap;
use std::io::Write;
use std::time::Duration;

/// How many refresh ticks between `gc session peek` sweeps. Peeking is one
/// subprocess call per running agent -- fine occasionally, too chatty every
/// single tick. The cached activity line from the last sweep is shown on
/// ticks in between, so the display still updates every refresh; only the
/// line's own freshness lags.
const PEEK_EVERY_N_TICKS: u64 = 5;

/// Lines of pane tail to request per peek -- just enough to usually find one
/// non-chrome line (status/spinner/tool-call text) at the bottom.
const PEEK_LINES: usize = 6;

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
    // Session id -> last peeked activity line, carried across ticks so the
    // line shown between peek sweeps (see `PEEK_EVERY_N_TICKS`) is the most
    // recent one actually seen, not blank.
    let mut activity_lines: HashMap<String, String> = HashMap::new();
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
            match render_rig(
                adapters,
                city,
                rig_name,
                now,
                &thresholds,
                tick,
                &mut activity_lines,
                &mut buf,
            ) {
                Ok(()) => {
                    std::io::stdout().write_all(&buf).ok();
                }
                Err(e) => println!("{} rig '{rig_name}': {e}", "error".red().bold()),
            }
        }

        // Erase anything left over from a previous, longer frame without
        // touching what was just drawn.
        print!("\x1B[0J");
        std::io::stdout().flush().ok();
        std::thread::sleep(Duration::from_secs(interval_secs.max(1)));
        tick = tick.wrapping_add(1);
    }
}

/// Assembles and renders one rig's queue/agents sections into `w`. Pure glue
/// over `assemble::*` + the sub-renderers -- kept as its own function
/// (rather than inlined in the `loop` above) specifically so it's directly
/// unit-testable against mock adapters, without needing the infinite loop
/// or a real terminal.
///
/// Deliberately doesn't render the full bead DAG (unlike the standalone
/// `dag` command): a live loop is about "are the workers alive, and what are
/// they doing right now" -- the whole open-bead tree is too much to parse at
/// a glance on every refresh. Run `oil-cop dag <rig>` for that view.
#[allow(clippy::too_many_arguments)]
fn render_rig(
    adapters: &Adapters,
    city: Option<&str>,
    rig: &str,
    now: chrono::DateTime<Utc>,
    thresholds: &Thresholds,
    tick: u64,
    activity_lines: &mut HashMap<String, String>,
    w: &mut impl Write,
) -> anyhow::Result<()> {
    let resolved = adapters.resolve_rig(city, rig)?;

    // bd.status, bd's in-progress list, gc.rig_status, and gc.session_list
    // are all independent of each other -- fetch all four concurrently
    // instead of one after another. Bind the two `Sync` fields used here
    // (not `adapters` as a whole -- its `git` field deliberately isn't
    // Sync, see Adapters' doc comment).
    let bd_adapter = adapters.bd.as_ref();
    let gc_adapter = adapters.gc.as_ref();
    let (bd_status, in_progress, rig_status, session_list) = std::thread::scope(|scope| {
        let path = resolved.path.as_str();
        let status_h = scope.spawn(|| bd_adapter.status(path));
        let list_h = scope.spawn(|| bd_adapter.list(path, "in_progress"));
        let rig_status_h = scope.spawn(|| gc_adapter.rig_status(city, rig));
        let session_list_h = scope.spawn(|| gc_adapter.session_list(city));
        (
            status_h.join().expect("bd.status thread panicked"),
            list_h.join().expect("bd.list thread panicked"),
            rig_status_h.join().expect("gc.rig_status thread panicked"),
            session_list_h
                .join()
                .expect("gc.session_list thread panicked"),
        )
    });
    let bd_status = bd_status?;
    let in_progress = in_progress?;
    let rig_status = rig_status?;
    let session_list = session_list?;

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

    // Peeking is a subprocess call per active session -- throttle it instead
    // of doing it every tick (see `PEEK_EVERY_N_TICKS`), and drop any cached
    // lines for sessions that no longer belong to this rig so the cache
    // doesn't grow unbounded across a long watch as sessions churn.
    let live_session_ids: std::collections::HashSet<&str> = rig_status
        .agents
        .iter()
        .filter_map(|a| a.session_id.as_deref())
        .collect();
    activity_lines.retain(|id, _| live_session_ids.contains(id.as_str()));
    if tick.is_multiple_of(PEEK_EVERY_N_TICKS) {
        // Gate on gc's own per-session `state`, not just rig-status's
        // coarser `running` bool: a session rig-status still calls running
        // can already be asleep/suspended by the time `session list` sees
        // it, and peeking one of those is a subprocess call known to fail.
        let active_session_ids: std::collections::HashSet<&str> = session_list
            .sessions
            .iter()
            .filter(|s| s.state == "active")
            .map(|s| s.id.as_str())
            .collect();
        for agent in rig_status.agents.iter().filter(|a| a.running) {
            let Some(session_id) = agent.session_id.as_deref() else {
                continue;
            };
            if !active_session_ids.contains(session_id) {
                continue;
            }
            if let Ok(peek) = adapters.gc.session_peek(city, session_id, PEEK_LINES) {
                if let Some(line) = last_meaningful_line(&peek.output) {
                    activity_lines.insert(session_id.to_string(), line);
                }
            }
            // A failed peek (e.g. the session went idle/asleep between
            // `session_list` and this call) just leaves the previous cached
            // line in place -- better a slightly stale line than a flash to
            // blank on a transient race.
        }
    }

    let sessions = assemble::session_signals(&session_list, activity_lines, now);
    let views = assemble::agent_views(rig_status.agents, &in_progress, &sessions, now, thresholds);
    agents::render(&resolved.name, &views, suspended, Some(tick), w)?;

    Ok(())
}

/// Last non-blank, non-chrome line of a `session peek` pane tail --
/// deliberately unopinionated about *content* (a tool-call line, a spinner
/// status, a reply paragraph) rather than trying to pattern-match a
/// specific agent UI's output: any provider's pane renders differently, and
/// the last content line changing between refreshes is itself the liveness
/// proof this exists for. It does skip a handful of near-universal footer
/// *chrome* markers (separator bars, the input prompt, a permission-mode
/// toggle bar) that are static regardless of whether the agent is frozen or
/// working -- surfacing those every tick would prove nothing.
fn last_meaningful_line(output: &str) -> Option<String> {
    output
        .lines()
        .rev()
        .map(str::trim)
        .find(|l| {
            !l.is_empty()
                && !l.chars().all(|c| matches!(c, '─' | '-' | '=' | '━'))
                && !l.starts_with('❯') // input prompt
                && !l.starts_with('⏵') // permission-mode toggle bar
        })
        .map(|l| {
            const MAX_CHARS: usize = 80;
            if l.chars().count() > MAX_CHARS {
                let truncated: String = l.chars().take(MAX_CHARS).collect();
                format!("{truncated}…")
            } else {
                l.to_string()
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::bd;
    use crate::sources::gc;
    use crate::sources::mocks::{MockBd, MockGc, MockGit};

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

    /// Builds the `MockGc` half of `adapters_for_rig`'s fixture -- factored
    /// out so tests that need a variant (e.g. a `session_peek` that always
    /// fails) can start from the same base via struct-update syntax instead
    /// of duplicating the whole fixture.
    fn mock_gc_for_rig(
        rig: &gc::RigRaw,
        rig_status: HashMap<String, gc::RigStatusResult>,
    ) -> MockGc {
        MockGc {
            status: None,
            rig_list: Some(gc::RigListResult {
                rigs: vec![rig.clone()],
            }),
            rig_status,
            session_list: Some(gc::SessionListResult {
                sessions: vec![gc::SessionRaw {
                    id: "cc-abcd".to_string(),
                    name: "luminate/nux".to_string(),
                    rig: Some("luminate".to_string()),
                    state: "active".to_string(),
                    last_active: Some("2020-01-01T00:09:30Z".to_string()),
                    attached: false,
                    closed: false,
                }],
            }),
            session_peek: HashMap::from([(
                "cc-abcd".to_string(),
                gc::PeekResult {
                    output: "\n  Running 1 shell command…\n".to_string(),
                },
            )]),
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

        let mut rig_status = HashMap::new();
        rig_status.insert(
            "luminate".to_string(),
            gc::RigStatusResult {
                rig: rig.clone(),
                agents: vec![gc::RigAgentRaw {
                    name: "nux".to_string(),
                    qualified_name: "luminate/nux".to_string(),
                    runtime_session_name: Some("session-a".to_string()),
                    session_id: Some("cc-abcd".to_string()),
                    running: true,
                    suspended: false,
                    draining: false,
                    status: Some("running".to_string()),
                }],
            },
        );

        Adapters {
            gc: Box::new(mock_gc_for_rig(&rig, rig_status)),
            bd: Box::new(MockBd {
                status: status_by_dir,
                list: list_by_key,
            }),
            git: Box::new(MockGit::default()),
        }
    }

    #[test]
    fn render_rig_assembles_queue_and_truthful_agents_sections() {
        colored::control::set_override(false);
        let adapters = adapters_for_rig("/fake/luminate");
        let now = chrono::DateTime::parse_from_rfc3339("2020-01-01T00:10:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut activity_lines = HashMap::new();
        let mut buf = Vec::new();
        render_rig(
            &adapters,
            None,
            "luminate",
            now,
            &thresholds(),
            0,
            &mut activity_lines,
            &mut buf,
        )
        .expect("render_rig should succeed against fully-configured mocks");
        let out = String::from_utf8(buf).unwrap();

        // Queue section.
        assert!(out.contains("luminate"));
        assert!(out.contains("2 total"));
        // Agents section: bead it's working on, and the ground-truth peek
        // line (proof it's actually doing something, not just self-reported
        // as running).
        assert!(out.contains("nux"));
        assert!(out.contains("luminate-1"));
        assert!(out.contains("title for luminate-1"));
        assert!(out.contains("Running 1 shell command"));
        // The DAG section is gone from watch entirely -- `oil-cop dag` is
        // the standalone command for it now.
        assert!(!out.contains("active (flashes in watch)"));
        // Peeked line is cached for the caller (throttled across ticks).
        assert_eq!(
            activity_lines.get("cc-abcd").map(String::as_str),
            Some("Running 1 shell command…")
        );
    }

    #[test]
    fn render_rig_surfaces_an_unknown_rig_as_an_error_not_a_panic() {
        let adapters = adapters_for_rig("/fake/luminate");
        let now = Utc::now();
        let mut activity_lines = HashMap::new();
        let mut buf = Vec::new();
        let result = render_rig(
            &adapters,
            None,
            "not-a-real-rig",
            now,
            &thresholds(),
            0,
            &mut activity_lines,
            &mut buf,
        );
        assert!(result.is_err());
    }

    #[test]
    fn render_rig_skips_the_peek_sweep_off_ticks_but_keeps_the_cached_line() {
        colored::control::set_override(false);
        let adapters = adapters_for_rig("/fake/luminate");
        let now = chrono::DateTime::parse_from_rfc3339("2020-01-01T00:10:00Z")
            .unwrap()
            .with_timezone(&Utc);
        // Seed the cache as if a previous (throttled) tick already peeked --
        // an off-tick call must still show it without calling peek again.
        let mut activity_lines = HashMap::from([("cc-abcd".to_string(), "stale line".to_string())]);
        let mut buf = Vec::new();
        render_rig(
            &adapters,
            None,
            "luminate",
            now,
            &thresholds(),
            1, // not a multiple of PEEK_EVERY_N_TICKS
            &mut activity_lines,
            &mut buf,
        )
        .unwrap();
        let out = String::from_utf8(buf).unwrap();
        assert!(out.contains("stale line"));
        assert_eq!(
            activity_lines.get("cc-abcd").map(String::as_str),
            Some("stale line"),
            "off-tick render must not overwrite the cached line"
        );
    }

    #[test]
    fn render_rig_prunes_cached_lines_for_sessions_no_longer_on_the_rig() {
        let adapters = adapters_for_rig("/fake/luminate");
        let now = chrono::DateTime::parse_from_rfc3339("2020-01-01T00:10:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut activity_lines = HashMap::from([(
            "some-old-session-id".to_string(),
            "orphaned line".to_string(),
        )]);
        let mut buf = Vec::new();
        render_rig(
            &adapters,
            None,
            "luminate",
            now,
            &thresholds(),
            1, // off-tick, so this only exercises pruning, not a fresh peek
            &mut activity_lines,
            &mut buf,
        )
        .unwrap();
        assert!(!activity_lines.contains_key("some-old-session-id"));
    }

    #[test]
    fn render_rig_leaves_the_cache_untouched_when_peek_fails() {
        colored::control::set_override(false);
        let rig_path = "/fake/luminate";
        let mut adapters = adapters_for_rig(rig_path);
        let rig = gc::RigRaw {
            name: "luminate".to_string(),
            path: rig_path.to_string(),
            prefix: None,
            suspended: false,
            running: Some(true),
        };
        let mut rig_status = HashMap::new();
        rig_status.insert(
            "luminate".to_string(),
            gc::RigStatusResult {
                rig: rig.clone(),
                agents: vec![gc::RigAgentRaw {
                    name: "nux".to_string(),
                    qualified_name: "luminate/nux".to_string(),
                    runtime_session_name: Some("session-a".to_string()),
                    session_id: Some("cc-abcd".to_string()),
                    running: true,
                    suspended: false,
                    draining: false,
                    status: Some("running".to_string()),
                }],
            },
        );
        // Every peek attempt fails, same as a real session that went
        // idle/asleep between `session list` and the peek call.
        adapters.gc = Box::new(MockGc {
            session_peek: HashMap::new(),
            ..mock_gc_for_rig(&rig, rig_status)
        });
        let now = chrono::DateTime::parse_from_rfc3339("2020-01-01T00:10:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut activity_lines = HashMap::new();
        let mut buf = Vec::new();
        render_rig(
            &adapters,
            None,
            "luminate",
            now,
            &thresholds(),
            0, // on-tick, so a peek IS attempted (and fails)
            &mut activity_lines,
            &mut buf,
        )
        .unwrap();
        assert!(!activity_lines.contains_key("cc-abcd"));
        let out = String::from_utf8(buf).unwrap();
        assert!(!out.contains("└"));
    }

    #[test]
    fn last_meaningful_line_finds_the_last_non_blank_non_separator_line() {
        // Deliberately unopinionated about content (see the function's doc
        // comment) -- it's whatever the true last non-blank, non-separator
        // line is, trailing blank lines and separator bars skipped.
        let output = "─────────────\n  Ran 1 shell command\n\n   \n";
        assert_eq!(
            last_meaningful_line(output).as_deref(),
            Some("Ran 1 shell command")
        );
    }

    #[test]
    fn last_meaningful_line_skips_the_prompt_and_permission_toggle_chrome() {
        // Reproduces a real Claude Code pane tail: the actual content
        // ("Crunched for 24s") sits above a fixed footer block (separator,
        // empty prompt, separator, hint line, permission-mode toggle) that's
        // identical whether the agent is frozen or actively working --
        // surfacing that every tick would prove nothing.
        let output = "\n✻ Crunched for 24s\n\n────────────────────\n❯ \n────────────────────\n  ● Run gc hook --claim --json now  ctx 0%\n  ⏵⏵ bypass permissions on (shift+tab to cycle)\n";
        assert_eq!(
            last_meaningful_line(output).as_deref(),
            Some("● Run gc hook --claim --json now  ctx 0%")
        );
    }

    #[test]
    fn last_meaningful_line_is_none_for_blank_or_separator_only_output() {
        assert_eq!(last_meaningful_line(""), None);
        assert_eq!(last_meaningful_line("\n\n   \n"), None);
        assert_eq!(last_meaningful_line("────────\n────\n"), None);
    }

    #[test]
    fn last_meaningful_line_truncates_a_long_line_with_an_ellipsis() {
        let long_line = "x".repeat(200);
        let got = last_meaningful_line(&long_line).unwrap();
        assert_eq!(got.chars().count(), 81); // 80 chars + the ellipsis mark
        assert!(got.ends_with('…'));
    }
}
