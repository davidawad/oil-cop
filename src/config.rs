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

/// Walk up from `start` looking for `.oilcop.toml`, the same discovery
/// style `gc` uses for `city.toml`.
fn find_project_local_from(start: &Path) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
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

/// The actual merge logic, taking explicit paths rather than reading
/// `$HOME`/cwd itself -- split out from `load()` so it's testable without
/// mutating process-global state (env vars, cwd) that parallel test
/// execution can't safely share.
fn load_from(global_config_path: Option<&Path>, project_search_start: Option<&Path>) -> FileConfig {
    let global = global_config_path.and_then(read).unwrap_or_default();
    let local = project_search_start
        .and_then(find_project_local_from)
        .as_deref()
        .and_then(read)
        .unwrap_or_default();
    global.merge_over(local)
}

pub fn load() -> FileConfig {
    load_from(
        global_path().as_deref(),
        std::env::current_dir().ok().as_deref(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_toml(dir: &Path, filename: &str, contents: &str) -> PathBuf {
        let path = dir.join(filename);
        fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn merge_over_prefers_the_more_specific_value_per_field_independently() {
        let global = FileConfig {
            city: Some("global-city".to_string()),
            stale_after: Some("30m".to_string()),
            default_rig: None,
        };
        let local = FileConfig {
            city: Some("local-city".to_string()),
            stale_after: None,
            default_rig: Some("luminate".to_string()),
        };
        let merged = global.merge_over(local);
        assert_eq!(merged.city.as_deref(), Some("local-city"));
        assert_eq!(merged.stale_after.as_deref(), Some("30m"));
        assert_eq!(merged.default_rig.as_deref(), Some("luminate"));
    }

    #[test]
    fn read_returns_none_for_a_missing_file() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(read(&tmp.path().join("does-not-exist.toml")).is_none());
    }

    #[test]
    fn read_returns_none_for_malformed_toml_instead_of_panicking() {
        let tmp = tempfile::tempdir().unwrap();
        let path = write_toml(tmp.path(), "bad.toml", "this is not [ valid toml");
        assert!(read(&path).is_none());
    }

    #[test]
    fn read_parses_a_valid_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = write_toml(
            tmp.path(),
            "config.toml",
            "city = \"/fake/city\"\ndefault_rig = \"luminate\"\n",
        );
        let cfg = read(&path).expect("should parse");
        assert_eq!(cfg.city.as_deref(), Some("/fake/city"));
        assert_eq!(cfg.default_rig.as_deref(), Some("luminate"));
        assert_eq!(cfg.stale_after, None);
    }

    #[test]
    fn load_from_merges_global_and_project_local_with_neither_file_present() {
        let tmp = tempfile::tempdir().unwrap();
        let missing_global = tmp.path().join("no-global.toml");
        let cfg = load_from(Some(&missing_global), Some(tmp.path()));
        assert_eq!(cfg.city, None);
        assert_eq!(cfg.default_rig, None);
    }

    #[test]
    fn load_from_lets_project_local_override_global_per_field() {
        let tmp = tempfile::tempdir().unwrap();
        let global_path = write_toml(
            tmp.path(),
            "global.toml",
            "city = \"/global/city\"\nstale_after = \"1h\"\n",
        );
        let project_dir = tmp.path().join("project");
        fs::create_dir(&project_dir).unwrap();
        write_toml(&project_dir, ".oilcop.toml", "city = \"/project/city\"\n");

        let cfg = load_from(Some(&global_path), Some(&project_dir));
        assert_eq!(cfg.city.as_deref(), Some("/project/city"));
        assert_eq!(cfg.stale_after.as_deref(), Some("1h")); // not overridden locally
    }

    #[test]
    fn load_from_walks_up_from_a_nested_directory_to_find_oilcop_toml() {
        let tmp = tempfile::tempdir().unwrap();
        write_toml(tmp.path(), ".oilcop.toml", "default_rig = \"luminate\"\n");
        let nested = tmp.path().join("a/b/c");
        fs::create_dir_all(&nested).unwrap();

        let cfg = load_from(None, Some(&nested));
        assert_eq!(cfg.default_rig.as_deref(), Some("luminate"));
    }

    #[test]
    fn load_from_with_no_paths_at_all_returns_all_defaults() {
        let cfg = load_from(None, None);
        assert_eq!(cfg.city, None);
        assert_eq!(cfg.stale_after, None);
        assert_eq!(cfg.default_rig, None);
    }
}
