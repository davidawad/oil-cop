use super::color::{glyph_live, humanize_age};
use crate::model::{BeadRef, Health, QueueView};
use colored::Colorize;
use std::io::{self, Write};

/// Renders the `queue` view into `w`. See `render/agents.rs` for why this
/// takes a generic `impl Write` instead of printing directly.
pub fn render(
    view: &QueueView,
    tick: Option<u64>,
    limit: usize,
    w: &mut impl Write,
) -> io::Result<()> {
    let running_tag = match view.rig_running {
        Some(true) => " (running)".green().to_string(),
        Some(false) => " (not running)".red().to_string(),
        None => String::new(),
    };
    writeln!(
        w,
        "{} {}{}  {}",
        glyph_live(view.health, tick),
        view.rig_name.bold(),
        running_tag,
        view.rig_path.dimmed()
    )?;
    writeln!(w)?;

    let s = &view.summary;
    writeln!(
        w,
        "  {}  {}  {}  {}  {}  {}",
        format!("{} total", s.total).normal(),
        format!("{} ready", s.ready).green(),
        format!("{} in_progress", s.in_progress).cyan(),
        format!("{} blocked", s.blocked).yellow(),
        format!("{} deferred", s.deferred).dimmed(),
        format!("{} closed", s.closed).truecolor(110, 110, 110),
    )?;
    writeln!(w)?;

    if view.in_progress.is_empty() {
        writeln!(w, "  (nothing in progress)")?;
        return Ok(());
    }

    let mut beads: Vec<&BeadRef> = view.in_progress.iter().collect();
    // Stalest (and least-known) first — that's what needs eyes on it.
    beads.sort_by_key(|b| match b.health {
        Health::Stale => (0, -b.age_secs.unwrap_or(0)),
        Health::Unknown => (1, 0),
        _ => (2, -b.age_secs.unwrap_or(0)),
    });

    let shown = beads.len().min(limit);
    writeln!(
        w,
        "  {:<3} {:<14} {:<9} {:<24} TITLE",
        "P", "ID", "AGE", "ASSIGNEE"
    )?;
    for b in beads.into_iter().take(limit) {
        let title = if b.title.chars().count() > 60 {
            let truncated: String = b.title.chars().take(57).collect();
            format!("{truncated}...")
        } else {
            b.title.clone()
        };
        let assignee = b.assignee.as_deref().unwrap_or("(unassigned)");
        let priority = b
            .priority
            .map(|p| format!("P{p}"))
            .unwrap_or_else(|| "-".to_string());
        writeln!(
            w,
            "  {} {:<3} {:<14} {:<9} {:<24} {}",
            glyph_live(b.health, tick),
            priority,
            b.id,
            humanize_age(b.age_secs),
            assignee,
            title
        )?;
    }
    if shown < view.in_progress.len() {
        writeln!(
            w,
            "  {}",
            format!(
                "... {} more in progress not shown (--limit {limit})",
                view.in_progress.len() - shown
            )
            .dimmed()
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::QueueSummary;

    fn render_to_string(view: &QueueView, limit: usize) -> String {
        colored::control::set_override(false);
        let mut buf = Vec::new();
        render(view, None, limit, &mut buf).unwrap();
        String::from_utf8(buf).unwrap()
    }

    fn bead(id: &str, title: &str, health: Health, age_secs: Option<i64>) -> BeadRef {
        BeadRef {
            id: id.to_string(),
            title: title.to_string(),
            status: "in_progress".to_string(),
            priority: Some(1),
            assignee: None,
            started_at: None,
            updated_at: None,
            age_secs,
            health,
        }
    }

    fn empty_summary() -> QueueSummary {
        QueueSummary {
            total: 0,
            ready: 0,
            in_progress: 0,
            blocked: 0,
            deferred: 0,
            closed: 0,
        }
    }

    #[test]
    fn renders_rig_header_and_running_tag() {
        let view = QueueView {
            rig_name: "luminate".to_string(),
            rig_path: "/repo/luminate".to_string(),
            rig_running: Some(true),
            summary: empty_summary(),
            in_progress: vec![],
            health: Health::Idle,
        };
        let out = render_to_string(&view, 15);
        assert!(out.contains("luminate"));
        assert!(out.contains("(running)"));
        assert!(out.contains("/repo/luminate"));
    }

    #[test]
    fn not_running_tag_when_rig_running_is_false() {
        let view = QueueView {
            rig_name: "luminate".to_string(),
            rig_path: "/repo/luminate".to_string(),
            rig_running: Some(false),
            summary: empty_summary(),
            in_progress: vec![],
            health: Health::Idle,
        };
        let out = render_to_string(&view, 15);
        assert!(out.contains("(not running)"));
    }

    #[test]
    fn no_running_tag_when_rig_running_is_unknown() {
        let view = QueueView {
            rig_name: "luminate".to_string(),
            rig_path: "/repo/luminate".to_string(),
            rig_running: None,
            summary: empty_summary(),
            in_progress: vec![],
            health: Health::Idle,
        };
        let out = render_to_string(&view, 15);
        assert!(!out.contains("(running)"));
        assert!(!out.contains("(not running)"));
    }

    #[test]
    fn empty_in_progress_shows_nothing_in_progress() {
        let view = QueueView {
            rig_name: "luminate".to_string(),
            rig_path: "/repo/luminate".to_string(),
            rig_running: Some(true),
            summary: QueueSummary {
                total: 5,
                ready: 5,
                in_progress: 0,
                blocked: 0,
                deferred: 0,
                closed: 0,
            },
            in_progress: vec![],
            health: Health::Idle,
        };
        let out = render_to_string(&view, 15);
        assert!(out.contains("5 total"));
        assert!(out.contains("(nothing in progress)"));
    }

    #[test]
    fn stale_beads_sort_before_healthy_ones() {
        let view = QueueView {
            rig_name: "luminate".to_string(),
            rig_path: "/repo/luminate".to_string(),
            rig_running: Some(true),
            summary: QueueSummary {
                total: 2,
                ready: 0,
                in_progress: 2,
                blocked: 0,
                deferred: 0,
                closed: 0,
            },
            in_progress: vec![
                bead("b-healthy", "healthy one", Health::Healthy, Some(10)),
                bead("b-stale", "stale one", Health::Stale, Some(9999)),
            ],
            health: Health::Stale,
        };
        let out = render_to_string(&view, 15);
        let stale_pos = out.find("b-stale").unwrap();
        let healthy_pos = out.find("b-healthy").unwrap();
        assert!(stale_pos < healthy_pos);
    }

    #[test]
    fn limit_truncates_and_reports_how_many_more() {
        let beads: Vec<BeadRef> = (0..5)
            .map(|i| bead(&format!("b-{i}"), "a bead", Health::Healthy, Some(i)))
            .collect();
        let view = QueueView {
            rig_name: "luminate".to_string(),
            rig_path: "/repo/luminate".to_string(),
            rig_running: Some(true),
            summary: QueueSummary {
                total: 5,
                ready: 0,
                in_progress: 5,
                blocked: 0,
                deferred: 0,
                closed: 0,
            },
            in_progress: beads,
            health: Health::Healthy,
        };
        let out = render_to_string(&view, 2);
        assert!(out.contains("... 3 more in progress not shown (--limit 2)"));
    }

    #[test]
    fn long_titles_are_truncated() {
        let view = QueueView {
            rig_name: "luminate".to_string(),
            rig_path: "/repo/luminate".to_string(),
            rig_running: Some(true),
            summary: QueueSummary {
                total: 1,
                ready: 0,
                in_progress: 1,
                blocked: 0,
                deferred: 0,
                closed: 0,
            },
            in_progress: vec![bead("b-1", &"x".repeat(80), Health::Healthy, Some(1))],
            health: Health::Healthy,
        };
        let out = render_to_string(&view, 15);
        assert!(out.contains(&format!("{}...", "x".repeat(57))));
        assert!(!out.contains(&"x".repeat(58)));
    }

    #[test]
    fn unassigned_bead_shows_placeholder() {
        let view = QueueView {
            rig_name: "luminate".to_string(),
            rig_path: "/repo/luminate".to_string(),
            rig_running: Some(true),
            summary: QueueSummary {
                total: 1,
                ready: 0,
                in_progress: 1,
                blocked: 0,
                deferred: 0,
                closed: 0,
            },
            in_progress: vec![bead("b-1", "a bead", Health::Healthy, Some(1))],
            health: Health::Healthy,
        };
        let out = render_to_string(&view, 15);
        assert!(out.contains("(unassigned)"));
    }
}
