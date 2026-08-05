# oil-cop task runner. `just ci` is the fast full gate every commit should
# pass; `just heavy` documents what's deliberately NOT automated here and
# why (see its recipe) rather than silently having nothing.

# This machine's `cargo` resolves to a Homebrew Rust install, separate from
# the rustup-managed toolchain llvm-tools-preview installs into -- so
# cargo-llvm-cov can't find llvm-cov/llvm-profdata on its own. Point it at
# the rustup toolchain's copies explicitly. Harmless if your machine's
# cargo/rustup are unified (llvm-cov falls back to its own discovery when
# these happen to already be right).
export LLVM_COV := "/Users/david/.rustup/toolchains/stable-aarch64-apple-darwin/lib/rustlib/aarch64-apple-darwin/bin/llvm-cov"
export LLVM_PROFDATA := "/Users/david/.rustup/toolchains/stable-aarch64-apple-darwin/lib/rustlib/aarch64-apple-darwin/bin/llvm-profdata"

# Full fast gate -- every commit should pass this. Mirrors the plugin
# pack's live hooks (fmt/machete/audit run on every `git commit` already)
# plus the things a commit-time hook can't afford (clippy across all
# targets, the full test suite, license/supply-chain, coverage) in one
# command a human or CI can run explicitly.
ci:
    cargo fmt --all -- --check
    cargo clippy --all-targets -- -D warnings
    cargo test
    cargo machete
    cargo audit
    cargo deny check
    cargo llvm-cov --workspace --fail-under-lines 90

# Deliberately-skipped heavy tier. The canonical Rust standard
# (config/programming-languages/rust/README.md, this repo's reference is
# AI/skills/swe-rust-standards/SKILL.md) calls for Kani, Verus, cargo-mutants,
# cargo-fuzz, loom, and miri here. All five are explicitly out of scope for
# oil-cop, decided by the project owner: it has zero `unsafe` code, zero
# concurrency (no thread::spawn/async anywhere), and no financial or
# safety-critical arithmetic -- the exact risk profile those tools exist
# for. This recipe exists so that decision is a documented one sitting
# where the standard expects it, not silent absence (the standard's own
# "an exclusion without a reason is a review failure" rule, applied to
# itself).
heavy:
    @echo "Deliberately not run for oil-cop (see this recipe's comment in the justfile):"
    @echo "  - Kani (model checking): no unsafe code, no complex arithmetic invariants to prove"
    @echo "  - Verus (deductive proofs): same reason, disproportionate for this codebase"
    @echo "  - cargo-mutants (mutation testing): no critical-path numeric/financial logic"
    @echo "  - cargo-fuzz: no hand-rolled parsers of untrusted binary input (JSON via serde only)"
    @echo "  - loom (concurrency model checking): oil-cop is single-threaded, nothing to check"
    @echo "  - miri (UB detection): no unsafe code for it to find UB in"

# Show real coverage numbers per-file, not just the pass/fail gate.
coverage:
    cargo llvm-cov --workspace --summary-only

# cargo-mutants/cargo-fuzz/kani/verus/loom/miri installs are intentionally
# absent from this justfile -- see `just heavy`.
