# Development Setup

Canonical local setup for contributing to VT Code.

## Prerequisites

- Rust toolchain (stable) via [rustup](https://rustup.rs/)
- Git
- An LLM provider credential: either (a) a shell env var like `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, `GEMINI_API_KEY`, `GOOGLE_API_KEY`, `ZAI_API_KEY`, `MOONSHOT_API_KEY`, `STEPFUN_API_KEY`, or `MINIMAX_API_KEY`, or (b) an OAuth session for an auth-managed provider, or (c) a key stored via `vtcode secret add <provider>`.

## One-Time Setup

```bash
git clone https://github.com/vinhnx/vtcode.git
cd vtcode
./scripts/setup.sh --with-hooks
```

`./scripts/setup.sh` verifies `rustfmt`/`clippy`, installs `cargo-nextest` when missing, and runs `cargo check`.

For debug or release launches:

```bash
./scripts/run.sh
./scripts/run-debug.sh
```

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
- No provider credential found:
  - Run `vtcode secret add <provider>` to store a key in your OS keyring (recommended), or
  - `export OPENAI_API_KEY="sk-..."` (or the equivalent env var for your provider) in your shell, or
  - Run `vtcode login <provider>` for OAuth/managed-auth providers (copilot, openai, openrouter).
- Script permissions:
  - Run `chmod +x scripts/*.sh`
