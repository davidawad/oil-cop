use super::color::{glyph_live, humanize_age};
use crate::model::{BeadRef, Health, QueueView};
use colored::Colorize;

pub fn render(view: &QueueView, tick: Option<u64>, limit: usize) {
    let running_tag = match view.rig_running {
        Some(true) => " (running)".green().to_string(),
        Some(false) => " (not running)".red().to_string(),
        None => String::new(),
    };
    println!(
        "{} {}{}  {}",
        glyph_live(view.health, tick),
        view.rig_name.bold(),
        running_tag,
        view.rig_path.dimmed()
    );
    println!();

    let s = &view.summary;
    println!(
        "  {}  {}  {}  {}  {}  {}",
        format!("{} total", s.total).normal(),
        format!("{} ready", s.ready).green(),
        format!("{} in_progress", s.in_progress).cyan(),
        format!("{} blocked", s.blocked).yellow(),
        format!("{} deferred", s.deferred).dimmed(),
        format!("{} closed", s.closed).truecolor(110, 110, 110),
    );
    println!();

    if view.in_progress.is_empty() {
        println!("  (nothing in progress)");
        return;
    }

    let mut beads: Vec<&BeadRef> = view.in_progress.iter().collect();
    // Stalest (and least-known) first — that's what needs eyes on it.
    beads.sort_by_key(|b| match b.health {
        Health::Stale => (0, -b.age_secs.unwrap_or(0)),
        Health::Unknown => (1, 0),
        _ => (2, -b.age_secs.unwrap_or(0)),
    });

    let shown = beads.len().min(limit);
    println!(
        "  {:<3} {:<14} {:<9} {:<24} TITLE",
        "P", "ID", "AGE", "ASSIGNEE"
    );
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
        println!(
            "  {} {:<3} {:<14} {:<9} {:<24} {}",
            glyph_live(b.health, tick),
            priority,
            b.id,
            humanize_age(b.age_secs),
            assignee,
            title
        );
    }
    if shown < view.in_progress.len() {
        println!(
            "  {}",
            format!(
                "... {} more in progress not shown (--limit {limit})",
                view.in_progress.len() - shown
            )
            .dimmed()
        );
    }
}
