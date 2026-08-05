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

`Adapters.gc`/`Adapters.bd` are `+ Sync` (`git` deliberately isn't); callers
that need more than one independent gc/bd fetch (`assemble::city_view`'s
per-rig `bd.status` rollup, `cmd_queue`, `cmd_agents`,
`render::watch::render_rig`'s `bd.status` + full bead list + `gc.rig_status`)
run them concurrently via `std::thread::scope`, joining owned results back
on the calling thread rather than blocking on each subprocess in turn. Each
call site binds the specific `&dyn GcAdapter`/`&dyn BdAdapter` field it
needs *before* the `scope` block and captures only that reference -- a
closure that captured `adapters` as a whole would require the entire
`Adapters` struct to be `Sync`, which it isn't (`git`'s fetch-throttle
cache is a `RefCell`). `assemble::dag_view` was split into a fetching
wrapper and a pure `dag_view_from_beads(adapters, rig, beads, now)` so
`render_rig` can pass in the same full bead list it already fetched for
queue/agents instead of issuing a second, redundant `bd.list` call.
Verified with a controlled harness (fake `gc`/`bd` stubs sleeping 0.5s
each): `queue` dropped from ~1.96s (sequential) to ~0.54s, `agents` from
~1.06s to ~0.53s.

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

## Testing

Run `just ci` for the full gate, or `cargo test` for just the test suite
(102 tests: 91 unit + 11 e2e). `just coverage` prints the real per-file
numbers. Three layers:

- Unit tests, in-module (`#[cfg(test)]`) next to the code they cover:
  `health.rs` (staleness scoring), `sources/git.rs` (fetch throttling, plus
  `LocalGit::is_merged` exercised against real temporary git repos --
  fast-forward-merged, pushed-but-unmerged, never-pushed, and not-a-repo-
  at-all cases), `sources/bd.rs` (metadata helpers), `sources/adapters.rs`
  (`resolve_rig`'s path-vs-name branches), `sources/proc.rs` (error-message
  extraction), `render/color.rs` (`humanize_age` boundaries), `config.rs`
  (merge precedence, TOML parsing), and `assemble.rs` (the core join
  logic -- `city_view`'s live-agent rollup, `queue_view`, `agent_views`,
  `dag_view`'s stage/blocked-by/landed-unmerged classification, and
  `check`'s issue collection). `assemble.rs`'s tests use mock
  `GcAdapter`/`BdAdapter`/`GitAdapter` implementations
  (`sources/mocks.rs`, test-only) rather than real subprocesses -- exactly
  what the adapter architecture exists to make possible.
- Property-based tests (`proptest`) in `health.rs` and `render/color.rs`:
  `parse_duration_secs`/`humanize_age` never panic on arbitrary input
  (including `i64::MIN`/`MAX`); `bead_health`/`agent_health` are total
  functions and their priority rules (e.g. `suspended` always wins) hold
  for every generated input, not just hand-picked examples; `worst_of` is
  order-independent.
- Black-box e2e tests (`tests/e2e.rs`) that invoke the actual compiled
  binary as a subprocess against fake `gc`/`bd`/`git` executables
  (`tests/fixtures/bin/`) returning canned real-schema JSON
  (`tests/fixtures/data/`) — covering `status`/`queue`/`agents`/`dag`/
  `check`'s JSON output shapes, exit codes, and error messages. Nothing in
  `tests/` touches a real Gas City stack; `HOME` is pointed at an empty
  fixture directory so a config file on the machine running the tests
  can't change their behavior.

`render/{agents,dag,queue,watch}.rs` write to an injectable `impl
std::io::Write` rather than calling `println!` directly, specifically so
their exact output is unit-testable in-process (the e2e tests exercise the
same code, but through a subprocess, which coverage tooling can't
attribute back to source -- direct tests were the actual fix, not a
tooling workaround).

## QA gates on this repo

This repo has `swe-project-plugin-pack` + `swe-project-plugin-pack-rust`
(awad-marketplace) enabled at project scope: conventional-commit subjects
and a primary-checkout mutation guard are always enforced; `cargo fmt`,
`cargo machete`, `cargo audit`, and Semgrep/OSV-Scanner SAST/dependency
scanning gate every commit *when their tool is installed* (each hook
soft-skips if its binary is missing, rather than silently trusting an
absent tool). Confirmed live on this machine: cargo-fmt, cargo-machete,
cargo-audit, cargo-semver-checks, Semgrep, and OSV-Scanner all installed
and passing clean. `snyk-gate` remains an accepted soft-skip (needs a
snyk.io account/login this repo doesn't have configured).

Beyond the live commit-time hooks, `just ci` runs the full gate: fmt
check, clippy (`-D warnings`), the test suite, `cargo machete`, `cargo
audit`, `cargo deny check` (license/supply-chain -- `deny.toml`), and a
**coverage gate** (`cargo llvm-cov --fail-under-lines 94`; real measured
total is 95.6% as of this writing, `just coverage` shows the current
per-file breakdown). `just heavy` documents what's *deliberately* not
automated -- Kani, Verus, cargo-mutants, cargo-fuzz, and loom/miri are all
out of scope for oil-cop specifically (no `unsafe` code, no financial or
safety-critical arithmetic, and its one use of concurrency --
`std::thread::scope` in `assemble`/`main`/`render/watch.rs`, see
"Architecture: adapters" above -- has no shared mutable state for loom to
model-check), not silently missing.

## Issue tracking

This project tracks its own work with `bd` (beads) -- run `bd ready` or `bd
list` in this directory to see it.
