# oil-cop

Color-coded visibility into a Gas City stack: rigs, agents, and bead queues.
Not specific to any one pack -- it talks to whatever `gc`/`bd` binaries are on
`PATH` and works against their real `--json` output, so it works for any
city/pack combination.

The point: `gc status` will happily tell you an agent is "running" while the
bead it's supposedly working has sat untouched for an hour. oil-cop layers a
staleness signal on top of that -- an agent or bead is only "healthy" if it's
both running *and* has actually updated recently.

## Install

```sh
brew install davidawad/oil-cop/oil-cop
```

Or build from source:

```sh
cargo install --path .
# or: cargo build --release   (binary at target/release/oil-cop)
```

A project-local `.cargo/config.toml` pins the linker to Apple clang
(`/usr/bin/cc`) -- on this machine a Nix-provided `cc` shadows it on `PATH`
and breaks linking against macOS SDK libs (`-liconv`). If your machine
doesn't have that problem, the pin is harmless.

## Commands

- `oil-cop status` -- city-wide overview: controller state, health signals,
  rigs with a live/dead/suspended rollup and per-rig bead counts
  (ready/in_progress/blocked).
- `oil-cop queue <rig>` -- a rig's bead counts by state (ready/in_progress/
  blocked/deferred/closed), with the in-progress list sorted stalest-first.
- `oil-cop agents <rig>` -- each agent in a rig and what bead it's currently
  assigned, joined by matching bead `assignee` to agent
  `runtime_session_name`.
- `oil-cop dag <rig> [--all]` -- a rig's beads as a git-graph-style DAG (tree
  by parent/child, cross-references for "blocks" edges), colored by
  lifecycle stage. Flags beads that bd still shows `in_progress` but whose
  branch has already landed in git -- the "refinery didn't close this"
  signal. `--all` includes closed/merged beads too (excluded by default).
- `oil-cop check [rig]` -- scriptable pass/fail gate for cron/CI: exits 0
  if nothing is stale/dead, 1 otherwise, printing the offending
  city/rig/bead/agent items. City-wide signals are always checked; a rig
  adds its in-progress beads and agents to the check.
- `oil-cop watch [--interval N] [rig]` -- live-refreshing dashboard: city
  status, plus the given rig's queue, agents, and DAG, redrawn in place each
  tick (no screen-clear flicker). A `healthy` glyph spins on each tick;
  anything stale/dead/suspended stays a static dot -- motion *is* the health
  signal.

`<rig>` accepts either a registered rig name or a filesystem path, same as
`gc` itself -- or falls back to `default_rig` from config (see below) if
omitted.

## Shell completions

```sh
# zsh (example -- adjust to wherever your $fpath completion dir is)
oil-cop completion zsh > /opt/homebrew/share/zsh/site-functions/_oil-cop

# bash
oil-cop completion bash > /opt/homebrew/etc/bash_completion.d/oil-cop

# fish
oil-cop completion fish > ~/.config/fish/completions/oil-cop.fish
```

`elvish` and `powershell` are also supported.

## Config file

CLI flags always win. Otherwise oil-cop looks for a project-local
`.oilcop.toml` (walking up from cwd, same discovery style `gc` uses for
`city.toml`), then a global `~/.config/oil-cop/config.toml`. Either is
optional -- a missing file just means no defaults.

```toml
# .oilcop.toml
city = "/Users/david/projects/tools/gas-city-hq"
default_rig = "luminate"
stale_after = "45m"
```

With that in place, `oil-cop queue` / `agents` / `dag` / `watch` all work
with no `--city`/`<rig>` arguments.

## Flags (global)

- `--city <path>` -- city directory (default: config file, then `gc`'s own
  cwd-walking discovery)
- `--json` -- machine-readable JSON instead of colored text, on every command
- `--no-color` -- force color off (auto-detected already via `NO_COLOR` / non-tty)
- `--stale-after <dur>` -- staleness threshold for in-progress work, e.g.
  `30m` (default), `1h`, `2h30m`, `90s`

## Health signals (`status`/`queue`/`agents`)

Each state has its own shape as well as its own color -- distinguishable
even with `--no-color` or a colorblind reader:

| Glyph | Color | Meaning |
|---|---|---|
| `●` (pulses in `watch`) | green | healthy -- running/in-progress and recently updated |
| `◇` | cyan | idle -- waiting on something upstream, not itself a problem |
| `▲` | yellow | stale -- should be moving, hasn't updated within the threshold |
| `✕` | red | dead -- expected to be running/usable but isn't |
| `‖` | dim | suspended -- intentionally paused |
| `✓` | gray | done -- closed |
| `?` | magenta | unknown -- not enough data (e.g. a partial status probe) |

## Bead-stage colors (`dag`)

A separate palette axis from the health signals above -- "where is it in the
pipeline," not "is it stuck." Shapes are deliberately different from the
health glyphs above (both appear together in `watch <rig>`), and deliberately
borrow bd's own status glyphs (open=`○`, in_progress=`◐`, closed=`✓`) so
they read as familiar:

| Glyph | Color | Stage |
|---|---|---|
| `○` | red | pending -- open / blocked / deferred, not started |
| `◐`, flashes each `watch` tick | yellow | active -- in_progress |
| `✓`, static | green | merged -- closed |

An active node also gets a `[landed, not closed]` flag when its branch has
already merged into its target branch in git but bd still shows
`in_progress` -- the visual signal for a stuck refinery.

## Architecture: adapters

`src/sources/adapters.rs` defines one trait per underlying tool --
`GcAdapter`, `BdAdapter`, `GitAdapter` -- with the real CLI-backed
implementations (`CliGc`, `CliBd`, `LocalGit`) bundled in `Adapters`.
`assemble`/`render`/`main` depend only on the traits, never on
`sources::gc`/`sources::bd`/`sources::git` directly. This mirrors Gas City's
own pack-based extensibility: a different backend (a gastown-pack-specific
data source, a mock for tests) plugs in by implementing the trait, with
nothing outside `sources/` needing to change.

No Dolt (SQL) adapter -- investigated and decided against (see bead
`oilcop-bb9.4`): `bd`'s own bead status already models the pipeline, and the
one thing raw Dolt access might have added (per-bead merge state) is better
answered by checking git ancestry directly, which is what `GitAdapter` does.

## Data sources

Everything is read via subprocess calls to the real CLIs, never a direct
Dolt/DB connection:

- `gc status --json`, `gc rig list --json`, `gc rig status --rig <r> --json`
- `bd status --json`, `bd list --json --status <s>` (run with `-C <rig-path>`)
- `git -C <rig-path> merge-base --is-ancestor origin/<branch> origin/<target>`
  (for the DAG's landed-but-unclosed signal only), preceded by a `git fetch
  origin` throttled to once per rig path per ~30s

Ancestry is checked against `origin/*` remote-tracking refs, not local
branch names -- a rig checkout's local branches can be missing or stale
(never fetched, or only ever pushed from a different worktree) while the
remote is authoritative for "did this actually land." See
`8-26-oil-crisis/07-luminate-bd-close-detour-and-the-almost-shipped-hook.txt`
for the real incident this fixes: `git log --merges`/`gh pr list` are both
blind to a plain fast-forward push (no merge commit, no PR), which is
refinery's default merge strategy.

Raw JSON shapes were verified against a live city (`gc --json-schema=result`
where available), not guessed -- see `src/sources/gc.rs` and
`src/sources/bd.rs` for the exact fields relied on. Every struct tolerates
missing/extra fields (`#[serde(default)]`) so a version bump or a different
pack's quirks don't hard-crash the tool.

## Issue tracking

This project tracks its own work with `bd` (beads) -- run `bd ready` or `bd
list` in this directory to see it.
