# Fuzzing Guide

This guide describes local fuzz testing for VT Code with `cargo-fuzz`.

## Scope

Current fuzz targets focus on security parser surfaces in `vtcode-core`:

- `shell_parser`: `command_safety::shell_parser` parsing paths
- `dangerous_commands`: `command_safety::dangerous_commands` classification invariants
- `exec_policy_parser`: `exec_policy::PolicyParser` (simple/TOML/JSON)
- `exec_policy_command_validation`: `exec_policy::command_validation::validate_command`
- `unified_path_validation`: `vtcode_commons::paths::ensure_path_within_workspace_resolved` (the symlink-aware
  containment primitive composed by `tools/file_ops/path_policy.rs`)

Stable-toolchain generative tests (no nightly required) live next to the code:

- `vtcode-diff`: `generative_small_docs_round_trip_across_algorithms_and_unified` cross-checks `Myers` vs `Patience` vs
  `Histogram` plus a unified format/parse round-trip on tiny swarmed inputs.
- `vtcode-commons`: `formatting/generative_tests.rs` checks the truncation and wrapping helpers against char-based
  oracles on swarmed ASCII + multi-byte alphabets (found a byte-vs-char overshoot in `truncate_path_middle`).

## Oracles

`no-panic` alone rarely finds logic bugs. Prefer differential oracles:

- `shell_parser`: lenient `parse_shell_commands` must equal strict `parse_shell_commands_tree_sitter` whenever the
  strict parse is non-empty; `parse_bash_lc_commands(["bash", "-lc", script])` must equal `parse_shell_commands(script)`
  (`Ok -> Some`, `Err -> None`).
- `dangerous_commands`: classification is deterministic; encoded PowerShell is both dangerous and approval-gated; a
  dangerous parsed sub-command taints its `bash -c/-lc/-ilc` wrapper and unparseable inline scripts fail closed.
- `unified_path_validation`: an `Ok` resolved path must stay inside the canonicalized workspace root.
- `vtcode-diff`: applying hunks must reconstruct both sides across all algorithms, and `format -> from_unified` must
  preserve the applied result.

## Fuzzer-first fixes

When a pest dodges the fuzzers, treat it as a fuzzer bug first: extend the oracle or generator to catch it (plus a
minimized seed in `fuzz/corpus/<target>/`), and only then land the fix and unit test.

## Prerequisites

VT Code defaults to stable Rust. Fuzzing uses nightly explicitly.

```bash
# Install cargo-fuzz once
cargo install cargo-fuzz --locked

# Install nightly toolchain (keeps stable as default)
rustup toolchain install nightly
```

## Basic Commands

Run from repository root:

```bash
# List available fuzz targets
cargo +nightly fuzz list

# Build a target
cargo +nightly fuzz build shell_parser

# Run for 60 seconds
cargo +nightly fuzz run shell_parser -- -max_total_time=60
```

Other targets:

```bash
cargo +nightly fuzz run exec_policy_parser -- -max_total_time=60
cargo +nightly fuzz run exec_policy_command_validation -- -max_total_time=60
cargo +nightly fuzz run unified_path_validation -- -max_total_time=60
```

## Corpus and Artifacts

- Seed corpus: `fuzz/corpus/<target>/`
- Crash artifacts: `fuzz/artifacts/<target>/`
- Coverage outputs: `fuzz/coverage/<target>/`

## Reproducing a Crash

Given an artifact like `fuzz/artifacts/shell_parser/crash-...`:

```bash
cargo +nightly fuzz run shell_parser fuzz/artifacts/shell_parser/crash-...
```

## Coverage (Optional)

```bash
cargo +nightly fuzz coverage shell_parser
```

Then inspect `fuzz/coverage/shell_parser/coverage.profdata` with your preferred LLVM coverage tooling.
