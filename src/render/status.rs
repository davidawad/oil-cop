use super::color::{glyph, glyph_live, label};
use crate::model::CityView;
use colored::Colorize;
use std::io::{self, Write};

/// Renders the `status` view into `w`. See `render/agents.rs` for why this
/// takes a generic `impl Write` instead of printing directly.
pub fn render(view: &CityView, tick: Option<u64>, w: &mut impl Write) -> io::Result<()> {
    writeln!(
        w,
        "{} {}  {}",
        glyph_live(view.health, tick),
        view.city_name.bold(),
        view.city_path.dimmed()
    )?;

    let controller = if view.controller_running {
        "running".green()
    } else {
        "down".red().bold()
    };
    let suspended = if view.suspended {
        " (suspended)".yellow().to_string()
    } else {
        String::new()
    };
    let mut detail = Vec::new();
    if let Some(pid) = view.controller_pid {
        detail.push(format!("pid {pid}"));
    }
    if let Some(mode) = &view.controller_mode {
        detail.push(format!("mode {mode}"));
    }
    if let Some(status) = &view.controller_status {
        detail.push(status.clone());
    }
    let detail = if detail.is_empty() {
        String::new()
    } else {
        format!(" ({})", detail.join(", "))
    };
    writeln!(w, "  controller: {controller}{detail}{suspended}")?;
    if !view.city_running && view.controller_running {
        writeln!(
            w,
            "  {} city reports not running despite controller being up",
            glyph(crate::model::Health::Stale)
        )?;
    }
    writeln!(
        w,
        "  agents: {}/{} running",
        view.running_agents, view.total_agents
    )?;

    if view.partial {
        writeln!(
            w,
            "  {} partial status — some probes timed out",
            glyph(crate::model::Health::Unknown)
        )?;
        for e in &view.partial_errors {
            writeln!(w, "    {}", e.dimmed())?;
        }
    }
    if view.degraded {
        for s in &view.signals {
            writeln!(w, "  {} {}", glyph(crate::model::Health::Dead), s)?;
        }
    }

    writeln!(w)?;
    writeln!(
        w,
        "  {:<2} {:<18} {:<9} {:<6} {:<4} {:<6} HEALTH",
        "", "RIG", "STATE", "READY", "WIP", "BLOCKED"
    )?;
    for rig in &view.rigs {
        let state = if rig.suspended {
            "suspended".dimmed()
        } else if rig.running {
            "active".green()
        } else {
            "inactive".yellow()
        };
        let (ready, wip, blocked) = match &rig.bead_summary {
            Some(s) => (
                s.ready.to_string(),
                s.in_progress.to_string(),
                s.blocked.to_string(),
            ),
            None => ("-".to_string(), "-".to_string(), "-".to_string()),
        };
        writeln!(
            w,
            "  {} {:<18} {:<9} {:<6} {:<4} {:<6} {}",
            glyph_live(rig.health, tick),
            rig.name,
            state,
            ready,
            wip,
            blocked,
            label(rig.health)
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Health, QueueSummary, RigView};

    fn base_view() -> CityView {
        CityView {
            city_name: "testcity".to_string(),
            city_path: "/fake/testcity".to_string(),
            city_running: true,
            controller_running: true,
            controller_pid: Some(42),
            controller_mode: Some("supervisor".to_string()),
            controller_status: None,
            suspended: false,
            usable: true,
            degraded: false,
            signals: vec![],
            partial: false,
            partial_errors: vec![],
            rigs: vec![],
            total_agents: 3,
            running_agents: 2,
            health: Health::Healthy,
        }
    }

    fn render_to_string(view: &CityView, tick: Option<u64>) -> String {
        colored::control::set_override(false);
        let mut buf = Vec::new();
        render(view, tick, &mut buf).expect("render should not fail writing to a Vec<u8>");
        String::from_utf8(buf).unwrap()
    }

    #[test]
    fn renders_city_name_path_and_controller_detail() {
        let out = render_to_string(&base_view(), None);
        assert!(out.contains("testcity"));
        assert!(out.contains("/fake/testcity"));
        assert!(out.contains("pid 42"));
        assert!(out.contains("mode supervisor"));
        assert!(out.contains("agents: 2/3 running"));
    }

    #[test]
    fn shows_suspended_tag_when_city_is_suspended() {
        let mut view = base_view();
        view.suspended = true;
        let out = render_to_string(&view, None);
        assert!(out.contains("(suspended)"));
    }

    #[test]
    fn flags_city_running_false_despite_controller_up_as_a_mismatch() {
        let mut view = base_view();
        view.city_running = false;
        let out = render_to_string(&view, None);
        assert!(out.contains("city reports not running despite controller being up"));
    }

    #[test]
    fn shows_partial_status_and_its_errors() {
        let mut view = base_view();
        view.partial = true;
        view.partial_errors = vec!["probe timed out".to_string()];
        let out = render_to_string(&view, None);
        assert!(out.contains("partial status"));
        assert!(out.contains("probe timed out"));
    }

    #[test]
    fn shows_degraded_signals() {
        let mut view = base_view();
        view.degraded = true;
        view.signals = vec!["dolt is unhappy".to_string()];
        let out = render_to_string(&view, None);
        assert!(out.contains("dolt is unhappy"));
    }

    #[test]
    fn rig_table_shows_bead_counts_when_present_and_dashes_when_absent() {
        let mut view = base_view();
        view.rigs = vec![
            RigView {
                name: "active-rig".to_string(),
                path: "/fake/active-rig".to_string(),
                prefix: None,
                running: true,
                suspended: false,
                health: Health::Healthy,
                bead_summary: Some(QueueSummary {
                    total: 10,
                    ready: 3,
                    in_progress: 2,
                    blocked: 1,
                    deferred: 0,
                    closed: 4,
                }),
            },
            RigView {
                name: "sleepy-rig".to_string(),
                path: "/fake/sleepy-rig".to_string(),
                prefix: None,
                running: false,
                suspended: true,
                health: Health::Suspended,
                bead_summary: None,
            },
            RigView {
                name: "inactive-rig".to_string(),
                path: "/fake/inactive-rig".to_string(),
                prefix: None,
                running: false,
                suspended: false,
                health: Health::Dead,
                bead_summary: None,
            },
        ];
        let out = render_to_string(&view, Some(0));
        assert!(out.contains("active-rig"));
        assert!(out.contains("3")); // ready
        assert!(out.contains("sleepy-rig"));
        assert!(out.contains("suspended"));
        assert!(out.contains("inactive-rig"));
        assert!(out.contains("-")); // dash placeholder for no bead_summary
    }
}
