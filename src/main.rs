mod assemble;
mod cli;
mod config;
mod health;
mod model;
mod render;
mod sources;

use anyhow::Result;
use chrono::Utc;
use clap::{CommandFactory, Parser};
use cli::{Cli, Command};
use health::Thresholds;
use sources::adapters::Adapters;
use std::io::Write;

/// Thin wrapper around the render functions that write into a generic
/// `impl Write` (rather than printing directly) so they're unit-testable in
/// isolation: build the output in memory, then flush it to real stdout in
/// one write.
fn write_to_stdout<F>(f: F) -> Result<()>
where
    F: FnOnce(&mut Vec<u8>) -> std::io::Result<()>,
{
    let mut buf = Vec::new();
    f(&mut buf)?;
    std::io::stdout().write_all(&buf)?;
    Ok(())
}

fn main() {
    let cli = Cli::parse();
    render::color::init(cli.no_color);

    if let Err(e) = run(cli) {
        eprintln!("oil-cop: {e:#}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<()> {
    let file_cfg = config::load();

    let stale_after = cli
        .stale_after
        .clone()
        .or_else(|| file_cfg.stale_after.clone())
        .unwrap_or_else(|| "30m".to_string());
    let thresholds = Thresholds {
        stale_after_secs: health::parse_duration_secs(&stale_after)?,
    };
    let city = cli.city.clone().or_else(|| file_cfg.city.clone());
    let city = city.as_deref();
    let adapters = Adapters::default();

    let rig_or_default = |rig: &Option<String>| -> Result<String> {
        rig.clone()
            .or_else(|| file_cfg.default_rig.clone())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "no rig given, and no default_rig set in .oilcop.toml / ~/.config/oil-cop/config.toml"
                )
            })
    };

    match &cli.command {
        Command::Completion { shell } => {
            clap_complete::generate(
                *shell,
                &mut cli::Cli::command(),
                "oil-cop",
                &mut std::io::stdout(),
            );
            Ok(())
        }
        Command::Status => cmd_status(&adapters, city, cli.json),
        Command::Queue { rig, limit } => cmd_queue(
            &adapters,
            city,
            &rig_or_default(rig)?,
            *limit,
            cli.json,
            &thresholds,
        ),
        Command::Agents { rig } => cmd_agents(
            &adapters,
            city,
            &rig_or_default(rig)?,
            cli.json,
            &thresholds,
        ),
        Command::Dag { rig, all } => {
            cmd_dag(&adapters, city, &rig_or_default(rig)?, *all, cli.json)
        }
        Command::Check { rig } => {
            let rig = rig.clone().or_else(|| file_cfg.default_rig.clone());
            cmd_check(&adapters, city, rig.as_deref(), cli.json, &thresholds)
        }
        Command::Watch { interval, rig } => {
            let rig = rig.clone().or_else(|| file_cfg.default_rig.clone());
            render::watch::run(&adapters, city, rig.as_deref(), *interval, thresholds)
        }
    }
}

fn cmd_status(adapters: &Adapters, city: Option<&str>, json: bool) -> Result<()> {
    let raw = adapters.gc.status(city)?;
    let view = assemble::city_view(adapters, raw);
    if json {
        println!("{}", serde_json::to_string_pretty(&view)?);
    } else {
        render::status::render(&view, None);
    }
    Ok(())
}

fn cmd_queue(
    adapters: &Adapters,
    city: Option<&str>,
    rig: &str,
    limit: usize,
    json: bool,
    thresholds: &Thresholds,
) -> Result<()> {
    let resolved = adapters.resolve_rig(city, rig)?;
    let bd_status = adapters.bd.status(&resolved.path)?;
    let in_progress = adapters.bd.list(&resolved.path, "in_progress")?;
    let now = Utc::now();
    let view = assemble::queue_view(
        &resolved.name,
        &resolved.path,
        resolved.running,
        bd_status,
        in_progress,
        now,
        thresholds,
    );
    if json {
        println!("{}", serde_json::to_string_pretty(&view)?);
    } else {
        write_to_stdout(|w| render::queue::render(&view, None, limit, w))?;
    }
    Ok(())
}

fn cmd_agents(
    adapters: &Adapters,
    city: Option<&str>,
    rig: &str,
    json: bool,
    thresholds: &Thresholds,
) -> Result<()> {
    let resolved = adapters.resolve_rig(city, rig)?;
    let in_progress = adapters.bd.list(&resolved.path, "in_progress")?;
    let rig_status = adapters.gc.rig_status(city, rig)?;
    let now = Utc::now();
    let suspended = rig_status.rig.suspended;
    let views = assemble::agent_views(rig_status.agents, &in_progress, now, thresholds);
    if json {
        println!("{}", serde_json::to_string_pretty(&views)?);
    } else {
        write_to_stdout(|w| render::agents::render(&resolved.name, &views, suspended, None, w))?;
    }
    Ok(())
}

fn cmd_dag(
    adapters: &Adapters,
    city: Option<&str>,
    rig: &str,
    include_closed: bool,
    json: bool,
) -> Result<()> {
    let resolved = adapters.resolve_rig(city, rig)?;
    let now = Utc::now();
    let view = assemble::dag_view(adapters, &resolved, include_closed, now)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&view)?);
    } else {
        write_to_stdout(|w| render::dag::render(&view, None, w))?;
    }
    Ok(())
}

fn cmd_check(
    adapters: &Adapters,
    city: Option<&str>,
    rig: Option<&str>,
    json: bool,
    thresholds: &Thresholds,
) -> Result<()> {
    let report = assemble::check(adapters, city, rig, thresholds)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        render::check::render(&report);
    }
    if !report.ok {
        std::process::exit(1);
    }
    Ok(())
}
