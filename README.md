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
  rigs with a live/dead/suspended rollup.
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
- `oil-cop watch [--interval N] [rig]` -- live-refreshing dashboard: city
  status, plus the given rig's queue, agents, and DAG. A `healthy` glyph
  spins on each tick; anything stale/dead/suspended stays a static dot --
  motion *is* the health signal.

`<rig>` accepts either a registered rig name or a filesystem path, same as
`gc` itself.

## Flags (global)

- `--city <path>` -- city directory (default: `gc`'s own cwd-walking discovery)
- `--json` -- machine-readable JSON instead of colored text, on every command
- `--no-color` -- force color off (auto-detected already via `NO_COLOR` / non-tty)
- `--stale-after <dur>` -- staleness threshold for in-progress work, e.g.
  `30m` (default), `1h`, `2h30m`, `90s`

## Health signals (`status`/`queue`/`agents`)

| Glyph | Meaning |
|---|---|
| green (pulses in `watch`) | healthy -- running/in-progress and recently updated |
| cyan | idle -- waiting on something upstream, not itself a problem |
| yellow | stale -- should be moving, hasn't updated within the threshold |
| red | dead -- expected to be running/usable but isn't |
| dim | suspended -- intentionally paused |
| gray | done -- closed |
| magenta | unknown -- not enough data (e.g. a partial status probe) |

## Bead-stage colors (`dag`)

A separate palette axis from the health signals above -- "where is it in the
pipeline," not "is it stuck":

| Color | Stage |
|---|---|
| red | pending -- open / blocked / deferred, not started |
| yellow, flashes each `watch` tick | active -- in_progress |
| green, static | merged -- closed |

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
- `git -C <rig-path> merge-base --is-ancestor <branch> <target>` (for the
  DAG's landed-but-unclosed signal only)

Raw JSON shapes were verified against a live city (`gc --json-schema=result`
where available), not guessed -- see `src/sources/gc.rs` and
`src/sources/bd.rs` for the exact fields relied on. Every struct tolerates
missing/extra fields (`#[serde(default)]`) so a version bump or a different
pack's quirks don't hard-crash the tool.

## Issue tracking

This project tracks its own work with `bd` (beads) -- run `bd ready` or `bd
list` in this directory to see it.
