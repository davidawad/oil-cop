//! Text render for `oil-cop handoff-gaps` (oilcop-kef): grouped by rig, a
//! flagged bead per line -- same `glyph` vocabulary `check.rs` uses (this
//! is a health check, just a rig-wide one instead of a per-bead one), not a
//! new palette axis.

use super::color::glyph;
use crate::model::{HandoffGapReport, Health};
use colored::Colorize;
use std::io::{self, Write};

/// Renders the `handoff-gaps` view into `w`. See `render/agents.rs` for why
/// this takes a generic `impl Write` instead of printing directly.
pub fn render(report: &HandoffGapReport, w: &mut impl Write) -> io::Result<()> {
    if report.rigs.is_empty() {
        writeln!(w, "(no rigs to check)")?;
        return Ok(());
    }

    let mut total_gaps = 0usize;
    for rig in &report.rigs {
        write!(w, "{}", rig.rig_name.bold())?;

        if let Some(err) = &rig.error {
            writeln!(w, "  {}", format!("skipped: {err}").red())?;
            writeln!(w)?;
            continue;
        }

        let Some(base) = &rig.base_branch else {
            writeln!(
                w,
                "  {}",
                "skipped: couldn't determine origin's default branch".yellow()
            )?;
            writeln!(w)?;
            continue;
        };
        writeln!(w, " {}", format!("(base: {base})").dimmed())?;

        if rig.gaps.is_empty() {
            writeln!(w, "  {} no handoff gaps", glyph(Health::Healthy))?;
        } else {
            writeln!(
                w,
                "  {:<18} {:<12} {:<28} BRANCH",
                "BEAD", "STATUS", "ASSIGNEE"
            )?;
            for gap in &rig.gaps {
                let status = gap.bead_status.as_deref().unwrap_or("(no bd record)");
                let assignee = gap.bead_assignee.as_deref().unwrap_or("(unassigned)");
                writeln!(
                    w,
                    "  {} {:<18} {:<12} {:<28} {}",
                    glyph(Health::Dead),
                    gap.bead_id,
                    status,
                    assignee,
                    gap.branch.dimmed()
                )?;
            }
            total_gaps += rig.gaps.len();
        }
        writeln!(w)?;
    }

    if total_gaps > 0 {
        writeln!(
            w,
            "{}",
            format!(
                "{total_gaps} handoff gap(s) found -- real pushed work with no matching bd assignment"
            )
            .red()
            .bold()
        )?;
    } else {
        writeln!(
            w,
            "{} all clear -- no orphaned polecat branches",
            glyph(Health::Healthy)
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{HandoffGap, RigHandoffGaps};

    fn render_to_string(report: &HandoffGapReport) -> String {
        colored::control::set_override(false);
        let mut buf = Vec::new();
        render(report, &mut buf).unwrap();
        String::from_utf8(buf).unwrap()
    }

    #[test]
    fn renders_no_rigs_placeholder() {
        let report = HandoffGapReport {
            rigs: vec![],
            ok: true,
        };
        let out = render_to_string(&report);
        assert!(out.contains("(no rigs to check)"));
    }

    #[test]
    fn renders_all_clear_when_no_rig_has_gaps() {
        let report = HandoffGapReport {
            rigs: vec![RigHandoffGaps {
                rig_name: "luminate".to_string(),
                rig_path: "/repo/luminate".to_string(),
                base_branch: Some("master".to_string()),
                gaps: vec![],
                error: None,
            }],
            ok: true,
        };
        let out = render_to_string(&report);
        assert!(out.contains("luminate"));
        assert!(out.contains("(base: master)"));
        assert!(out.contains("no handoff gaps"));
        assert!(out.contains("all clear"));
    }

    #[test]
    fn renders_a_gap_with_bead_status_and_assignee() {
        let report = HandoffGapReport {
            rigs: vec![RigHandoffGaps {
                rig_name: "luminate".to_string(),
                rig_path: "/repo/luminate".to_string(),
                base_branch: Some("master".to_string()),
                gaps: vec![HandoffGap {
                    bead_id: "luminate-bgb.2".to_string(),
                    branch: "polecat/luminate-bgb.2".to_string(),
                    bead_status: Some("open".to_string()),
                    bead_assignee: None,
                }],
                error: None,
            }],
            ok: false,
        };
        let out = render_to_string(&report);
        assert!(out.contains("luminate-bgb.2"));
        assert!(out.contains("open"));
        assert!(out.contains("(unassigned)"));
        assert!(out.contains("polecat/luminate-bgb.2"));
        assert!(out.contains("1 handoff gap(s) found"));
    }

    #[test]
    fn renders_missing_bd_record_placeholder() {
        let report = HandoffGapReport {
            rigs: vec![RigHandoffGaps {
                rig_name: "luminate".to_string(),
                rig_path: "/repo/luminate".to_string(),
                base_branch: Some("master".to_string()),
                gaps: vec![HandoffGap {
                    bead_id: "luminate-ghost".to_string(),
                    branch: "polecat/luminate-ghost".to_string(),
                    bead_status: None,
                    bead_assignee: None,
                }],
                error: None,
            }],
            ok: false,
        };
        let out = render_to_string(&report);
        assert!(out.contains("(no bd record)"));
    }

    #[test]
    fn renders_skip_reason_when_base_branch_is_unknown() {
        let report = HandoffGapReport {
            rigs: vec![RigHandoffGaps {
                rig_name: "luminate".to_string(),
                rig_path: "/repo/luminate".to_string(),
                base_branch: None,
                gaps: vec![],
                error: None,
            }],
            ok: true,
        };
        let out = render_to_string(&report);
        assert!(out.contains("couldn't determine origin's default branch"));
        assert!(out.contains("all clear"));
    }

    #[test]
    fn renders_a_per_rig_fetch_error() {
        let report = HandoffGapReport {
            rigs: vec![RigHandoffGaps {
                rig_name: "broken".to_string(),
                rig_path: "/repo/broken".to_string(),
                base_branch: None,
                gaps: vec![],
                error: Some("mock: no bd list configured".to_string()),
            }],
            ok: true,
        };
        let out = render_to_string(&report);
        assert!(out.contains("skipped:"));
        assert!(out.contains("mock: no bd list configured"));
    }
}
