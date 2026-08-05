mod assemble;
mod cli;
mod health;
mod model;
mod render;
mod sources;

use anyhow::Result;
use chrono::Utc;
use clap::Parser;
use cli::{Cli, Command};
use health::Thresholds;

fn main() {
    let cli = Cli::parse();
    render::color::init(cli.no_color);

    if let Err(e) = run(cli) {
        eprintln!("oil-cop: {e:#}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<()> {
    let thresholds = Thresholds {
        stale_after_secs: health::parse_duration_secs(&cli.stale_after)?,
    };
    let city = cli.city.as_deref();

    match &cli.command {
        Command::Status => cmd_status(city, cli.json),
        Command::Queue { rig, limit } => cmd_queue(city, rig, *limit, cli.json, &thresholds),
        Command::Agents { rig } => cmd_agents(city, rig, cli.json, &thresholds),
        Command::Watch { interval, rig } => {
            render::watch::run(city, rig.as_deref(), *interval, thresholds)
        }
    }
}

fn cmd_status(city: Option<&str>, json: bool) -> Result<()> {
    let (raw, _out) = sources::gc::status(city)?;
    let view = assemble::city_view(raw);
    if json {
        println!("{}", serde_json::to_string_pretty(&view)?);
    } else {
        render::status::render(&view, None);
    }
    Ok(())
}

fn cmd_queue(
    city: Option<&str>,
    rig: &str,
    limit: usize,
    json: bool,
    thresholds: &Thresholds,
) -> Result<()> {
    let resolved = sources::resolve_rig(city, rig)?;
    let (bd_status, _out) = sources::bd::status(&resolved.path)?;
    let (in_progress, _out2) = sources::bd::list(&resolved.path, "in_progress")?;
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
        render::queue::render(&view, None, limit);
    }
    Ok(())
}

fn cmd_agents(city: Option<&str>, rig: &str, json: bool, thresholds: &Thresholds) -> Result<()> {
    let resolved = sources::resolve_rig(city, rig)?;
    let (in_progress, _out) = sources::bd::list(&resolved.path, "in_progress")?;
    let (rig_status, _out2) = sources::gc::rig_status(city, rig)?;
    let now = Utc::now();
    let suspended = rig_status.rig.suspended;
    let views = assemble::agent_views(rig_status.agents, &in_progress, now, thresholds);
    if json {
        println!("{}", serde_json::to_string_pretty(&views)?);
    } else {
        render::agents::render(&resolved.name, &views, suspended, None);
    }
    Ok(())
}
