use super::{agents, dag, queue, status};
use crate::assemble;
use crate::health::Thresholds;
use crate::sources::adapters::Adapters;
use chrono::Utc;
use colored::Colorize;
use std::io::Write;
use std::time::Duration;

pub fn run(
    adapters: &Adapters,
    city: Option<&str>,
    rig: Option<&str>,
    interval_secs: u64,
    thresholds: Thresholds,
) -> anyhow::Result<()> {
    let mut tick: u64 = 0;
    loop {
        print!("\x1B[2J\x1B[H"); // clear screen, cursor home
        let now = Utc::now();
        println!(
            "{} {}  {}",
            "oil-cop watch".bold(),
            now.format("%Y-%m-%d %H:%M:%S UTC"),
            format!("(every {interval_secs}s, ctrl-c to exit)").dimmed()
        );
        println!();

        match adapters.gc.status(city) {
            Ok(raw) => status::render(&assemble::city_view(raw), Some(tick)),
            Err(e) => println!("{} status: {e}", "error".red().bold()),
        }

        if let Some(rig_name) = rig {
            println!();
            if let Err(e) = render_rig(adapters, city, rig_name, now, &thresholds, tick) {
                println!("{} rig '{rig_name}': {e}", "error".red().bold());
            }
        }

        std::io::stdout().flush().ok();
        std::thread::sleep(Duration::from_secs(interval_secs.max(1)));
        tick = tick.wrapping_add(1);
    }
}

fn render_rig(
    adapters: &Adapters,
    city: Option<&str>,
    rig: &str,
    now: chrono::DateTime<Utc>,
    thresholds: &Thresholds,
    tick: u64,
) -> anyhow::Result<()> {
    let resolved = adapters.resolve_rig(city, rig)?;
    let bd_status = adapters.bd.status(&resolved.path)?;
    let in_progress = adapters.bd.list(&resolved.path, "in_progress")?;
    let qview = assemble::queue_view(
        &resolved.name,
        &resolved.path,
        resolved.running,
        bd_status,
        in_progress.clone(),
        now,
        thresholds,
    );
    queue::render(&qview, Some(tick), 15);

    println!();
    let rig_status = adapters.gc.rig_status(city, rig)?;
    let suspended = rig_status.rig.suspended;
    let views = assemble::agent_views(rig_status.agents, &in_progress, now, thresholds);
    agents::render(&resolved.name, &views, suspended, Some(tick));

    println!();
    let dview = assemble::dag_view(adapters, &resolved, false, now)?;
    dag::render(&dview, Some(tick));

    Ok(())
}
