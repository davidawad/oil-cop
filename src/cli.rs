use clap::{Parser, Subcommand};

/// oil-cop -- color-coded visibility into a Gas City stack: rigs, agents,
/// and bead queues. Works against any city/pack combination through the
/// standard `gc`/`bd` CLIs; nothing here is specific to one pack.
#[derive(Parser)]
#[command(name = "oil-cop", version, about, long_about = None)]
pub struct Cli {
    /// Path to the city directory (default: `gc`'s own discovery -- walk up from cwd)
    #[arg(long, global = true)]
    pub city: Option<String>,

    /// Disable ANSI color even on a tty
    #[arg(long, global = true)]
    pub no_color: bool,

    /// Emit machine-readable JSON instead of colored text
    #[arg(long, global = true)]
    pub json: bool,

    /// How long an in-progress bead can go without an update before it's "stale"
    #[arg(long, global = true, default_value = "30m")]
    pub stale_after: String,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// City-wide overview: controller, health signals, rigs, agent counts
    Status,

    /// Visualize a rig's bead queue: counts per state, with stale
    /// in-progress work highlighted
    Queue {
        /// Rig name (as registered with the city) or a filesystem path
        rig: String,
        /// Max in-progress beads to list (sorted stalest-first)
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },

    /// Show a rig's agents and what bead each is currently working on
    Agents {
        /// Rig name (as registered with the city) or a filesystem path
        rig: String,
    },

    /// Render a rig's beads as a git-graph-style DAG: red (pending) ->
    /// yellow (active) -> green (merged), flagging beads whose branch has
    /// already landed in git but bd still shows in_progress (the
    /// refinery-stuck signal).
    Dag {
        /// Rig name (as registered with the city) or a filesystem path
        rig: String,
        /// Include closed (merged) beads too, not just the open pipeline
        #[arg(long)]
        all: bool,
    },

    /// Live-refreshing dashboard: city status, plus one rig's queue/agents
    /// if given. Healthy items visibly pulse each refresh; stale/dead ones
    /// stay frozen -- motion is the health signal.
    Watch {
        /// Refresh interval in seconds
        #[arg(long, default_value_t = 3)]
        interval: u64,
        /// Rig to also show queue+agents for (name or path)
        rig: Option<String>,
    },
}
