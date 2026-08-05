use super::color::{glyph, glyph_live, label};
use crate::model::CityView;
use colored::Colorize;

pub fn render(view: &CityView, tick: Option<u64>) {
    println!(
        "{} {}  {}",
        glyph_live(view.health, tick),
        view.city_name.bold(),
        view.city_path.dimmed()
    );

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
    println!("  controller: {controller}{detail}{suspended}");
    if !view.city_running && view.controller_running {
        println!(
            "  {} city reports not running despite controller being up",
            glyph(crate::model::Health::Stale)
        );
    }
    println!(
        "  agents: {}/{} running",
        view.running_agents, view.total_agents
    );

    if view.partial {
        println!(
            "  {} partial status — some probes timed out",
            glyph(crate::model::Health::Unknown)
        );
        for e in &view.partial_errors {
            println!("    {}", e.dimmed());
        }
    }
    if view.degraded {
        for s in &view.signals {
            println!("  {} {}", glyph(crate::model::Health::Dead), s);
        }
    }

    println!();
    println!("  {:<2} {:<18} {:<8} HEALTH", "", "RIG", "STATE");
    for rig in &view.rigs {
        let state = if rig.suspended {
            "suspended".dimmed()
        } else if rig.running {
            "active".green()
        } else {
            "inactive".yellow()
        };
        println!(
            "  {} {:<18} {:<8} {}",
            glyph_live(rig.health, tick),
            rig.name,
            state,
            label(rig.health)
        );
    }
}
