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
use sources::adapters::Adapters;

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
    let adapters = Adapters::default();

    match &cli.command {
        Command::Status => cmd_status(&adapters, city, cli.json),
        Command::Queue { rig, limit } => {
            cmd_queue(&adapters, city, rig, *limit, cli.json, &thresholds)
        }
        Command::Agents { rig } => cmd_agents(&adapters, city, rig, cli.json, &thresholds),
        Command::Dag { rig, all } => cmd_dag(&adapters, city, rig, *all, cli.json),
        Command::Watch { interval, rig } => {
            render::watch::run(&adapters, city, rig.as_deref(), *interval, thresholds)
        }
    }
}

fn cmd_status(adapters: &Adapters, city: Option<&str>, json: bool) -> Result<()> {
    let raw = adapters.gc.status(city)?;
    let view = assemble::city_view(raw);
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
        render::queue::render(&view, None, limit);
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
        render::agents::render(&resolved.name, &views, suspended, None);
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
        render::dag::render(&view, None);
    }
    Ok(())
}
