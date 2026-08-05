//! Health/BeadStage -> color *and* glyph mapping. One place to change the
//! palette. Every state gets its own shape, not just its own color -- with
//! `--no-color`, a non-tty pipe, or for a colorblind reader, color alone
//! collapses every state to the same dot. Shape carries the signal color
//! is normally doing; color is the (usual) reinforcement, not the only
//! channel.

use crate::model::{BeadStage, Health};
use colored::{ColoredString, Colorize};
use is_terminal::IsTerminal;

/// Call once at startup: disables ANSI color when stdout isn't a tty, when
/// `NO_COLOR` is set, or when the caller passed `--no-color`.
pub fn init(force_no_color: bool) {
    let should_color = !force_no_color
        && std::env::var_os("NO_COLOR").is_none()
        && std::io::stdout().is_terminal();
    colored::control::set_override(should_color);
}

const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

fn health_shape(h: Health) -> &'static str {
    match h {
        Health::Healthy => "●",
        Health::Idle => "◇",
        Health::Stale => "▲",
        Health::Dead => "✕",
        Health::Suspended => "‖",
        Health::Done => "✓",
        Health::Unknown => "?",
    }
}

pub fn glyph(h: Health) -> ColoredString {
    let s = health_shape(h);
    match h {
        Health::Healthy => s.green().bold(),
        Health::Idle => s.cyan(),
        Health::Stale => s.yellow().bold(),
        Health::Dead => s.red().bold(),
        Health::Suspended => s.dimmed(),
        Health::Done => s.truecolor(110, 110, 110),
        Health::Unknown => s.magenta(),
    }
}

/// Same as `glyph`, but in `watch` mode a `Healthy` signal spins on each
/// refresh tick instead of sitting as a static dot -- motion reads as "this
/// is actually moving," stillness reads as "this is frozen," which is the
/// whole point of animating at all. Every other health stays its static
/// shape: a stale/dead/suspended thing shouldn't look alive.
pub fn glyph_live(h: Health, tick: Option<u64>) -> ColoredString {
    match (h, tick) {
        (Health::Healthy, Some(t)) => SPINNER[(t as usize) % SPINNER.len()].green().bold(),
        _ => glyph(h),
    }
}

pub fn paint(h: Health, s: &str) -> ColoredString {
    match h {
        Health::Healthy => s.green(),
        Health::Idle => s.cyan(),
        Health::Stale => s.yellow(),
        Health::Dead => s.red(),
        Health::Suspended => s.dimmed(),
        Health::Done => s.truecolor(110, 110, 110),
        Health::Unknown => s.magenta(),
    }
}

pub fn label(h: Health) -> ColoredString {
    paint(h, h.label())
}

/// Bead-lifecycle stage shape+color for the DAG view: hollow circle/red
/// (pending) -> half-filled circle/yellow, flashing between bold and plain
/// on alternating watch ticks (active) -> checkmark/green, static (merged).
/// Deliberately reuses bd's own status glyph vocabulary (open=○,
/// in_progress=◐, closed=✓) so it reads as familiar rather than inventing a
/// new alphabet for the same underlying states. This is a *different*
/// palette axis than `Health` -- it's "where in the pipeline," not "is it
/// stuck" -- so its shapes are deliberately distinct from `health_shape`
/// (no overlap: `watch <rig>` shows both at once).
pub fn bead_stage_glyph(stage: BeadStage, tick: Option<u64>) -> ColoredString {
    match stage {
        BeadStage::Pending => "○".red(),
        BeadStage::Active => match tick {
            Some(t) if t % 2 == 0 => "◐".yellow().bold(),
            Some(_) => "◐".yellow(),
            None => "◐".yellow().bold(),
        },
        BeadStage::Merged => "✓".green(),
    }
}

/// Human-readable "3m ago" / "2h14m ago" from seconds. Negative or missing
/// ages render as "—".
pub fn humanize_age(secs: Option<i64>) -> String {
    let Some(mut s) = secs else {
        return "—".to_string();
    };
    if s < 0 {
        s = 0;
    }
    if s < 60 {
        return format!("{s}s ago");
    }
    let mins = s / 60;
    if mins < 60 {
        return format!("{mins}m ago");
    }
    let hours = mins / 60;
    let rem_mins = mins % 60;
    if hours < 24 {
        return format!("{hours}h{rem_mins:02}m ago");
    }
    let days = hours / 24;
    let rem_hours = hours % 24;
    format!("{days}d{rem_hours}h ago")
}
