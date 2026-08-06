use super::color::{glyph_live, humanize_age, label};
use crate::model::AgentView;
use colored::Colorize;
use std::io::{self, Write};

/// Renders the `agents` view into `w`. Takes a generic `impl Write` (rather
/// than printing directly) so unit tests can assert on exact output by
/// rendering into a `Vec<u8>` -- the e2e tests exercise this via a
/// subprocess, which coverage instrumentation can't see across, so this is
/// also what makes the logic itself directly coverable.
pub fn render(
    rig_name: &str,
    agents: &[AgentView],
    suspended: bool,
    tick: Option<u64>,
    w: &mut impl Write,
) -> io::Result<()> {
    let tag = if suspended {
        " (suspended)".dimmed().to_string()
    } else {
        String::new()
    };
    writeln!(w, "{}{} agents", rig_name.bold(), tag)?;
    writeln!(
        w,
        "  {:<9} {:<28} {:<11} {:<14} {:<9} WORKING ON",
        "", "AGENT", "STATE", "BEAD", "LAST SEEN"
    )?;
    for a in agents {
        let state = if a.draining {
            "draining".yellow()
        } else if a.suspended {
            "suspended".dimmed()
        } else {
            match a.raw_status.as_deref() {
                Some(s) if a.running => s.green(),
                Some(s) => s.red(),
                None if a.running => "running".green(),
                None => "stopped".red(),
            }
        };
        let (bead_id, title): (&str, String) = match &a.current_bead {
            Some(b) => (b.id.as_str(), b.title.clone()),
            None => ("-", String::new()),
        };
        let title = if title.chars().count() > 50 {
            let truncated: String = title.chars().take(47).collect();
            format!("{truncated}...")
        } else {
            title
        };
        // "Last seen" is the agent's own real activity heartbeat (`gc
        // session list`'s `last_active`), not the bead's `updated_at` --
        // deliberately decoupled from what it's claiming to work on, so a
        // frozen agent still reads as frozen even if the bead metadata looks
        // fresh.
        writeln!(
            w,
            "  {} {:<28} {:<11} {:<14} {:<9} {}",
            glyph_live(a.health, tick),
            a.qualified_name,
            state,
            bead_id,
            humanize_age(a.last_active_secs),
            title
        )?;
        // Proof-of-life line from `gc session peek`, when one was fetched --
        // what the agent is actually doing right now, not an inferred status.
        if let Some(line) = &a.activity_line {
            writeln!(w, "  {:<9} {} {}", "", "└".dimmed(), line.dimmed())?;
        }
    }
    writeln!(w)?;
    write!(w, "  legend:")?;
    for h in [
        crate::model::Health::Healthy,
        crate::model::Health::Idle,
        crate::model::Health::Stale,
        crate::model::Health::Dead,
        crate::model::Health::Suspended,
    ] {
        write!(w, "  {} {}", glyph_live(h, None), label(h))?;
    }
    writeln!(w)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{BeadRef, Health};

    fn render_to_string(rig_name: &str, agents: &[AgentView], suspended: bool) -> String {
        colored::control::set_override(false);
        let mut buf = Vec::new();
        render(rig_name, agents, suspended, None, &mut buf).unwrap();
        String::from_utf8(buf).unwrap()
    }

    fn agent(
        name: &str,
        health: Health,
        running: bool,
        current_bead: Option<BeadRef>,
    ) -> AgentView {
        AgentView {
            name: name.to_string(),
            qualified_name: name.to_string(),
            scope: "rig".to_string(),
            running,
            suspended: false,
            draining: false,
            raw_status: if running {
                Some("running".to_string())
            } else {
                None
            },
            session_id: None,
            runtime_session_name: None,
            current_bead,
            last_active_secs: None,
            activity_line: None,
            health,
        }
    }

    #[test]
    fn renders_header_and_rig_name() {
        let out = render_to_string("luminate", &[], false);
        assert!(out.contains("luminate agents"));
        assert!(out.contains("AGENT"));
        assert!(out.contains("STATE"));
        assert!(out.contains("WORKING ON"));
    }

    #[test]
    fn renders_suspended_tag_when_rig_is_suspended() {
        let out = render_to_string("luminate", &[], true);
        assert!(out.contains("(suspended)"));
    }

    #[test]
    fn omits_suspended_tag_when_not_suspended() {
        let out = render_to_string("luminate", &[], false);
        assert!(!out.contains("(suspended)"));
    }

    #[test]
    fn renders_a_healthy_running_agent_with_its_bead() {
        let bead = BeadRef {
            id: "luminate-1".to_string(),
            title: "fix the thing".to_string(),
            status: "in_progress".to_string(),
            priority: Some(1),
            assignee: None,
            started_at: None,
            updated_at: None,
            age_secs: Some(9999), // bead's own age -- deliberately NOT what "last seen" shows
            health: Health::Healthy,
        };
        let mut a = agent("nux", Health::Healthy, true, Some(bead));
        a.last_active_secs = Some(30);
        let out = render_to_string("luminate", &[a], false);
        assert!(out.contains("nux"));
        assert!(out.contains("luminate-1"));
        assert!(out.contains("fix the thing"));
        // "Last seen" comes from the agent's own real activity, not the
        // bead's (much staler) updated_at.
        assert!(out.contains("30s ago"));
        assert!(!out.contains("9999"));
    }

    #[test]
    fn renders_the_activity_line_under_an_agent_when_one_was_peeked() {
        let mut a = agent("nux", Health::Healthy, true, None);
        a.activity_line = Some("Running 1 shell command…".to_string());
        let out = render_to_string("luminate", &[a], false);
        assert!(out.contains("Running 1 shell command…"));
    }

    #[test]
    fn omits_the_activity_line_when_none_was_peeked() {
        let agents = [agent("nux", Health::Healthy, true, None)];
        let out = render_to_string("luminate", &agents, false);
        assert!(!out.contains("└"));
    }

    #[test]
    fn renders_a_dead_agent_with_no_current_bead_as_a_dash() {
        let agents = [agent("nux", Health::Dead, false, None)];
        let out = render_to_string("luminate", &agents, false);
        assert!(out.contains("nux"));
        assert!(out.contains(" - "));
        assert!(out.contains("—"));
    }

    #[test]
    fn draining_agent_shows_draining_state_regardless_of_running() {
        let mut a = agent("nux", Health::Idle, true, None);
        a.draining = true;
        let out = render_to_string("luminate", &[a], false);
        assert!(out.contains("draining"));
    }

    #[test]
    fn suspended_agent_shows_suspended_state() {
        let mut a = agent("nux", Health::Suspended, true, None);
        a.suspended = true;
        let out = render_to_string("luminate", &[a], false);
        assert!(out.contains("suspended"));
    }

    #[test]
    fn long_titles_are_truncated_with_an_ellipsis() {
        let bead = BeadRef {
            id: "luminate-1".to_string(),
            title: "a".repeat(80),
            status: "in_progress".to_string(),
            priority: None,
            assignee: None,
            started_at: None,
            updated_at: None,
            age_secs: Some(5),
            health: Health::Healthy,
        };
        let agents = [agent("nux", Health::Healthy, true, Some(bead))];
        let out = render_to_string("luminate", &agents, false);
        assert!(out.contains(&format!("{}...", "a".repeat(47))));
        assert!(!out.contains(&"a".repeat(48)));
    }

    #[test]
    fn legend_lists_all_five_non_terminal_health_labels() {
        let out = render_to_string("luminate", &[], false);
        assert!(out.contains("legend:"));
        assert!(out.contains("healthy"));
        assert!(out.contains("idle"));
        assert!(out.contains("stale"));
        assert!(out.contains("dead"));
        assert!(out.contains("suspended"));
    }
}
