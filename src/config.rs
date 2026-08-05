//! Optional file-based defaults, so `--city <long-path>` doesn't need
//! retyping on every invocation. Precedence: CLI flag > project-local
//! `.oilcop.toml` (walking up from cwd, like `gc` discovers `city.toml`) >
//! global `~/.config/oil-cop/config.toml` > built-in default. A missing
//! file at either location is not an error -- config is purely additive.

use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Deserialize)]
pub struct FileConfig {
    pub city: Option<String>,
    pub stale_after: Option<String>,
    pub default_rig: Option<String>,
}

impl FileConfig {
    fn merge_over(mut self, more_specific: FileConfig) -> Self {
        if more_specific.city.is_some() {
            self.city = more_specific.city;
        }
        if more_specific.stale_after.is_some() {
            self.stale_after = more_specific.stale_after;
        }
        if more_specific.default_rig.is_some() {
            self.default_rig = more_specific.default_rig;
        }
        self
    }
}

fn read(path: &Path) -> Option<FileConfig> {
    let text = std::fs::read_to_string(path).ok()?;
    toml::from_str(&text).ok()
}

fn global_path() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    Some(PathBuf::from(home).join(".config/oil-cop/config.toml"))
}

/// Walk up from the current directory looking for `.oilcop.toml`, the same
/// discovery style `gc` uses for `city.toml`.
fn find_project_local() -> Option<PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        let candidate = dir.join(".oilcop.toml");
        if candidate.is_file() {
            return Some(candidate);
        }
        if !dir.pop() {
            return None;
        }
    }
}

pub fn load() -> FileConfig {
    let global = global_path().as_deref().and_then(read).unwrap_or_default();
    let local = find_project_local()
        .as_deref()
        .and_then(read)
        .unwrap_or_default();
    global.merge_over(local)
}
