---
name: crate-ops
description: Perform and verify module/crate operations with mandatory verification gates. Use when changing Cargo.toml features or dependencies, gating modules behind features, moving code between crates, or fixing failures where a crate compiles in the workspace but fails alone, in CI, or during `cargo publish` verification.
---

# Crate Ops

Any harness (Muse, Claude Code, Codex, VT Code, human) can run this workflow.
It is the procedure, not the runner: a Muse executable version lives at `.agents/workflows/crate-ops.js`,
but the steps below are the source of truth.

## The core trap: feature-unification masking

A workspace-wide build (`cargo check`, `./scripts/check-dev.sh`) unifies features across all default members.
A crate can compile there while its own manifest is broken: a missing `features = ["..."]` declaration
only surfaces when the crate builds alone (`cargo check -p <crate>`), in a restricted CI command, or during
`cargo publish` verification (which rebuilds the packaged tarball).
Real case: `vtcode` depended on `vtcode-core` with `default-features = false` and no explicit `tui`, so the
binary failed with 145 errors everywhere except the default workspace build.

## Workflow

### 1. Orient

- Read root `AGENTS.md` and each touched crate's `AGENTS.md`.
- Read the touched manifests plus every dependent's declaration of the changed crate
  (`grep -rn '<crate-name>' --include=Cargo.toml crates/ Cargo.toml`, excluding `target/`).
- Check `git log` on the manifests for intent behind `default-features = false` or version pins.

### 2. Reproduce in isolation

Reproduce with the crate alone, never with a workspace-wide build:

```sh
cargo check --locked -p <crate>
```

Match the failure against the reported one (same crate, same error class/count) before changing anything.

### 3. Fix

- Keep changes surgical: only touch files the change owns.
- When a crate needs an optional feature of a dependency, declare it explicitly
  on that dependency edge. Precedent (`fuzz/Cargo.toml`, root `Cargo.toml`):
  `vtcode-core = { path = "...", version = "...", default-features = false, features = ["tui"] }`
- Keep path+version on all workspace path dependencies; never rely on unification from other members.
- Follow repo rules: `anyhow::Result` with context, no `unwrap`/`expect`/`panic` in
  production code, workspace lints apply. Do not commit unless asked.

### 4. Verify (mandatory, in this order)

Run each verifier standalone. Piping through `head`/`tail` or joining with `;`/`||` hides
the exit status; a gated pass requires an unpiped exit 0.

| Gate | Command | Pass when |
| --- | --- | --- |
| Isolated check (per affected crate) | `cargo check --locked -p <crate>` | exit 0 |
| Lints (per affected crate) | `cargo clippy --locked -p <crate> --all-targets -- -D warnings` | exit 0 |
| Headless contract | `cargo check --locked -p vtcode-acp`, then `cargo tree --locked -p vtcode-acp --prefix none \| grep -cE '^(vtcode-ui\|ratatui) '` | check passes and count is `0` (grep exits 1 on zero matches: that is the pass case, report the count) |
| Packaging (per affected crate) | `cargo publish --dry-run -p <crate>` (add `--allow-dirty` only when the tree holds the uncommitted fix) | verification build succeeds |
| Desktop gate (slow, optional locally; CI runs it) | `cargo check --locked -p vtcode-desktop -p vtcode-desktop-protocol -p vtcode` | exit 0 |

Skip a gate only with a stated reason (e.g. desktop gate when no TUI/desktop surface changed
and CI covers it). A skipped gate is reported as skipped, never as passed.

### 5. Report

Report per gate: what ran, exit status, and error count. Name every modified file with one
line on what changed. If the implementation did not complete, say verification was skipped so no
gate can give a false pass on unchanged code.
