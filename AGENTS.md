# Agent Guide

1. Build & Run
   - Use `cargo check` (preferred) or `cargo build --release` for builds.
   - Format with `cargo fmt`; lint with `cargo clippy` before committing.
   - Run all tests via `cargo nextest run` (preferred) or `cargo test`.
   - Run a single test with `cargo nextest run <test>` or `cargo test <test>`.
   - Entry scripts: `./run.sh` (release) and `./run-debug.sh` (debug).
   - Headless query: `cargo run -- ask "<prompt>"`.

2. Architecture
   - `vtcode-core/`: reusable library (LLM providers, tools, config, MCP integration).
   - `src/`: CLI/TUI binary with Ratatui UI, PTY execution, slash commands.
   - Config flows through `vtcode.toml`; constants in `vtcode-core/src/config/constants.rs`; models in `docs/models.json`.
   - Integrates tree-sitter parsers, MCP tools, multi-provider LLM adapters, PTY command execution.

3. Code Style
   - Rust naming: snake_case for vars/functions, PascalCase for types; prefer early returns and descriptive names.
   - Use `anyhow::Result<T>` with `.with_context()` for fallible operations; avoid hardcoded configuration or model IDs.
   - Respect tool traits, async tokio workflows, rustfmt formatting, and keep Markdown docs confined to ./docs/.
