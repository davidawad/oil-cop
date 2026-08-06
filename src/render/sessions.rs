//! Renders `oil-cop sessions`: sessions `gc` itself still calls active
//! whose real activity heartbeat says otherwise -- see
//! `assemble::zombie_sessions` for the detection logic and why `gc session
//! prune` can't reach these on its own.

use super::color::humanize_age;
use crate::model::SessionsReport;
use colored::Colorize;
use std::io::{self, Write};

pub fn render(report: &SessionsReport, w: &mut impl Write) -> io::Result<()> {
    if report.zombies.is_empty() {
        writeln!(
            w,
            "{} all clear -- no zombie sessions among {} total",
            "✓".green().bold(),
            report.total_sessions
        )?;
        return Ok(());
    }
    for z in &report.zombies {
        let rig = z.rig.as_deref().unwrap_or("-");
        writeln!(
            w,
            "{} {} {} rig:{} last seen {}",
            "✕".red().bold(),
            z.id.dimmed(),
            z.name.bold(),
            rig,
            humanize_age(z.last_active_secs)
        )?;
        for cmd in &z.suggested_commands {
            writeln!(w, "    {} {}", "->".dimmed(), cmd.dimmed())?;
        }
    }
    writeln!(w)?;
    writeln!(
        w,
        "{}",
        format!(
            "{} zombie session(s) found out of {} total",
            report.zombies.len(),
            report.total_sessions
        )
        .red()
        .bold()
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ZombieSession;

    fn render_to_string(report: &SessionsReport) -> String {
        colored::control::set_override(false);
        let mut buf = Vec::new();
        render(report, &mut buf).unwrap();
        String::from_utf8(buf).unwrap()
    }

    #[test]
    fn renders_all_clear_when_no_zombies() {
        let out = render_to_string(&SessionsReport {
            total_sessions: 3,
            zombies: vec![],
        });
        assert!(out.contains("all clear"));
        assert!(out.contains("3 total"));
    }

    #[test]
    fn renders_a_zombie_with_its_suggested_fix_commands() {
        let out = render_to_string(&SessionsReport {
            total_sessions: 2,
            zombies: vec![ZombieSession {
                id: "cc-abcd".to_string(),
                name: "luminate/gastown.nux".to_string(),
                rig: Some("luminate".to_string()),
                last_active_secs: Some(9000),
                suggested_commands: vec![
                    "gc session kill cc-abcd".to_string(),
                    "gc session close cc-abcd".to_string(),
                ],
            }],
        });
        assert!(out.contains("cc-abcd"));
        assert!(out.contains("luminate/gastown.nux"));
        assert!(out.contains("rig:luminate"));
        assert!(out.contains("2h30m ago"));
        assert!(out.contains("gc session kill cc-abcd"));
        assert!(out.contains("gc session close cc-abcd"));
        assert!(out.contains("1 zombie session(s) found out of 2 total"));
    }

    #[test]
    fn renders_a_dash_for_zombie_with_no_known_rig() {
        let out = render_to_string(&SessionsReport {
            total_sessions: 1,
            zombies: vec![ZombieSession {
                id: "do-1".to_string(),
                name: "some-session".to_string(),
                rig: None,
                last_active_secs: None,
                suggested_commands: vec!["gc session kill do-1".to_string()],
            }],
        });
        assert!(out.contains("rig:-"));
        assert!(out.contains("—")); // humanize_age(None)
    }
}
