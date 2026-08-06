use clap::{Parser, Subcommand};
use clap_complete::Shell;

/// oil-cop -- color-coded visibility into a Gas City stack: rigs, agents,
/// and bead queues. Works against any city/pack combination through the
/// standard `gc`/`bd` CLIs; nothing here is specific to one pack.
#[derive(Parser)]
#[command(name = "oil-cop", version, about, long_about = None)]
pub struct Cli {
    /// Path to the city directory (default: config file, then `gc`'s own
    /// discovery -- walk up from cwd)
    #[arg(long, global = true)]
    pub city: Option<String>,

    /// Disable ANSI color even on a tty
    #[arg(long, global = true)]
    pub no_color: bool,

    /// Emit machine-readable JSON instead of colored text
    #[arg(long, global = true)]
    pub json: bool,

    /// How long an in-progress bead can go without an update before it's
    /// "stale" (default: config file, then "30m")
    #[arg(long, global = true)]
    pub stale_after: Option<String>,

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
        /// (default: config file's default_rig)
        rig: Option<String>,
        /// Max in-progress beads to list (sorted stalest-first)
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },

    /// Show a rig's agents and what bead each is currently working on
    Agents {
        /// Rig name (as registered with the city) or a filesystem path
        /// (default: config file's default_rig)
        rig: Option<String>,
    },

    /// Render a rig's beads as a git-graph-style DAG: red (pending) ->
    /// yellow (active) -> green (merged), flagging beads whose branch has
    /// already landed in git but bd still shows in_progress (the
    /// refinery-stuck signal).
    Dag {
        /// Rig name (as registered with the city) or a filesystem path
        /// (default: config file's default_rig)
        rig: Option<String>,
        /// Include closed (merged) beads too, not just the open pipeline
        #[arg(long)]
        all: bool,
    },

    /// Generate shell completions (bash/zsh/fish/elvish/powershell) and
    /// print them to stdout.
    Completion { shell: Shell },

    /// Scriptable health check: exits 0 if nothing is stale/dead, 1
    /// otherwise. Checks city-wide signals always, plus one rig's
    /// in-progress beads and agents if given (name or path; default:
    /// config file's default_rig).
    Check { rig: Option<String> },

    /// Scriptable zombie-session check: exits 0 if every session gc calls
    /// active has shown real activity recently, 1 otherwise. Flags a
    /// session gc labels active whose real heartbeat (`last_active`) is
    /// stale or missing entirely -- the gap `gc session prune` can't reach,
    /// since prune only ever ages out sessions already labeled
    /// suspended/asleep/drained. Never kills or closes anything itself;
    /// prints the `gc session kill`/`close` command for a human to run.
    Sessions {
        /// Only show zombies belonging to this rig (name as registered with
        /// the city); omit to scan every rig's sessions city-wide.
        rig: Option<String>,
    },

    /// Live-refreshing dashboard: city status, plus one rig's queue/agents
    /// if given. Healthy items visibly pulse each refresh; stale/dead ones
    /// stay frozen -- motion is the health signal.
    Watch {
        /// Refresh interval in seconds
        #[arg(long, default_value_t = 3)]
        interval: u64,
        /// Rig to also show queue+agents for (name or path; default:
        /// config file's default_rig)
        rig: Option<String>,
    },
}
