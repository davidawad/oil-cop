//! Health -> color/glyph mapping. One place to change the palette.

use crate::model::Health;
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

pub fn glyph(h: Health) -> ColoredString {
    let dot = "●";
    match h {
        Health::Healthy => dot.green().bold(),
        Health::Idle => dot.cyan(),
        Health::Stale => dot.yellow().bold(),
        Health::Dead => dot.red().bold(),
        Health::Suspended => dot.dimmed(),
        Health::Done => dot.truecolor(110, 110, 110),
        Health::Unknown => dot.magenta(),
    }
}

/// Same as `glyph`, but in `watch` mode a `Healthy` signal spins on each
/// refresh tick instead of sitting as a static dot — motion reads as "this
/// is actually moving," stillness reads as "this is frozen," which is the
/// whole point of animating at all. Every other health stays a static dot:
/// a stale/dead/suspended thing shouldn't look alive.
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
