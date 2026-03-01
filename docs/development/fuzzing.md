# Fuzzing Guide

This guide describes local fuzz testing for VT Code with `cargo-fuzz`.

## Scope

Current fuzz targets focus on security parser surfaces in `vtcode-core`:

- `shell_parser`: `command_safety::shell_parser` parsing paths
- `exec_policy_parser`: `exec_policy::PolicyParser` (simple/TOML/JSON)
- `unified_path_validation`: `tools::validation::unified_path::validate_and_resolve_path`

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
