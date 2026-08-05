//! Wrapper around the `gc` (Gas City) CLI. Shapes below match the real
//! `--json` / `--json-schema=result` output of `gc status`, `gc rig list`,
//! and `gc rig status` — verified against a live city, not guessed. Every
//! field beyond what oil-cop actually uses is `#[serde(default)]`-tolerant
//! so a pack-specific or version-drifted field never breaks parsing.

use super::proc::{run_json, Output};
use anyhow::Result;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct StatusResult {
    #[serde(default)]
    pub city_name: String,
    #[serde(default)]
    pub city_path: String,
    #[serde(default)]
    pub controller: Controller,
    #[serde(default)]
    pub running: bool,
    #[serde(default)]
    pub suspended: bool,
    #[serde(default)]
    pub health: HealthBlock,
    #[serde(default)]
    pub agents: Vec<AgentRaw>,
    #[serde(default)]
    pub rigs: Vec<RigRaw>,
    #[serde(default)]
    pub summary: SummaryBlock,
    #[serde(default)]
    pub partial: bool,
    #[serde(default)]
    pub partial_errors: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct Controller {
    #[serde(default)]
    pub running: bool,
    pub pid: Option<i64>,
    pub mode: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct HealthBlock {
    #[serde(default)]
    pub usable: bool,
    #[serde(default)]
    pub degraded: bool,
    #[serde(default)]
    pub signals: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct SummaryBlock {
    #[serde(default)]
    pub total_agents: i64,
    #[serde(default)]
    pub running_agents: i64,
}

#[derive(Debug, Deserialize)]
pub struct AgentRaw {
    #[serde(default)]
    pub qualified_name: String,
    #[serde(default)]
    pub scope: String,
    #[serde(default)]
    pub running: bool,
    #[serde(default)]
    pub suspended: bool,
}

#[derive(Debug, Default, Deserialize)]
pub struct RigRaw {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub path: String,
    pub prefix: Option<String>,
    #[serde(default)]
    pub suspended: bool,
    /// Only present on `gc rig list`, not `gc status`.
    #[serde(default)]
    pub running: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct RigListResult {
    #[serde(default)]
    pub rigs: Vec<RigRaw>,
}

#[derive(Debug, Deserialize)]
pub struct RigStatusResult {
    #[serde(default)]
    pub rig: RigRaw,
    #[serde(default)]
    pub agents: Vec<RigAgentRaw>,
}

#[derive(Debug, Deserialize)]
pub struct RigAgentRaw {
    pub name: String,
    #[serde(default)]
    pub qualified_name: String,
    pub runtime_session_name: Option<String>,
    pub session_id: Option<String>,
    #[serde(default)]
    pub running: bool,
    #[serde(default)]
    pub suspended: bool,
    #[serde(default)]
    pub draining: bool,
    pub status: Option<String>,
}

fn city_args<'a>(city: Option<&'a str>, base: &mut Vec<&'a str>) {
    if let Some(c) = city {
        base.push("--city");
        base.push(c);
    }
}

pub fn status(city: Option<&str>) -> Result<(StatusResult, Output)> {
    let mut args = vec!["status", "--json"];
    city_args(city, &mut args);
    run_json("gc", &args)
}

pub fn rig_list(city: Option<&str>) -> Result<(RigListResult, Output)> {
    let mut args = vec!["rig", "list", "--json"];
    city_args(city, &mut args);
    run_json("gc", &args)
}

pub fn rig_status(city: Option<&str>, rig: &str) -> Result<(RigStatusResult, Output)> {
    let mut args = vec!["rig", "status", "--rig", rig, "--json"];
    city_args(city, &mut args);
    run_json("gc", &args)
}
