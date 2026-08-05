use super::color::{glyph_live, humanize_age, label};
use crate::model::AgentView;
use colored::Colorize;

pub fn render(rig_name: &str, agents: &[AgentView], suspended: bool, tick: Option<u64>) {
    let tag = if suspended {
        " (suspended)".dimmed().to_string()
    } else {
        String::new()
    };
    println!("{}{} agents", rig_name.bold(), tag);
    println!(
        "  {:<9} {:<28} {:<11} {:<14} {:<9} WORKING ON",
        "", "AGENT", "STATE", "BEAD", "AGE"
    );
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
        let (bead_id, age, title): (&str, String, String) = match &a.current_bead {
            Some(b) => (b.id.as_str(), humanize_age(b.age_secs), b.title.clone()),
            None => ("-", "—".to_string(), String::new()),
        };
        let title = if title.chars().count() > 50 {
            let truncated: String = title.chars().take(47).collect();
            format!("{truncated}...")
        } else {
            title
        };
        println!(
            "  {} {:<28} {:<11} {:<14} {:<9} {}",
            glyph_live(a.health, tick),
            a.qualified_name,
            state,
            bead_id,
            age,
            title
        );
    }
    println!();
    print!("  legend:");
    for h in [
        crate::model::Health::Healthy,
        crate::model::Health::Idle,
        crate::model::Health::Stale,
        crate::model::Health::Dead,
        crate::model::Health::Suspended,
    ] {
        print!("  {} {}", glyph_live(h, None), label(h));
    }
    println!();
}
