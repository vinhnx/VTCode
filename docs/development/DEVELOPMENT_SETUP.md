# Development Setup

Canonical local setup for contributing to VT Code.

## Prerequisites

- Rust toolchain (stable) via [rustup](https://rustup.rs/)
- Git
- One LLM provider API key for local interactive runs (`OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, `GEMINI_API_KEY`, or `GOOGLE_API_KEY`)

## One-Time Setup

```bash
git clone https://github.com/vinhnx/vtcode.git
cd vtcode
./scripts/setup.sh --with-hooks
```

`./scripts/setup.sh` verifies `rustfmt`/`clippy`, installs `cargo-nextest` when missing, and runs `cargo check`.

## Daily Development Loop

```bash
# Fast compile check
cargo check

# Fast test loop (recommended)
cargo nextest run

# Fallback if nextest is unavailable
cargo test --workspace
```

## Full Quality Gate

```bash
./scripts/check.sh
```

This runs formatting checks, linting, governance checks, build, tests (nextest-first), and docs generation.

## Common Commands

```bash
# Format
cargo fmt --all

# Lint (deny warnings)
cargo clippy --workspace --all-targets --all-features -- -D warnings

# Build
cargo build --workspace

# Docs
cargo doc --workspace --no-deps --document-private-items
```

## Troubleshooting

- `cargo nextest` missing:
  - Run `cargo install cargo-nextest --locked`
- API key missing for interactive runs:
  - Export one provider key, for example `export OPENAI_API_KEY="sk-..."`
- Script permissions:
  - Run `chmod +x scripts/*.sh`
