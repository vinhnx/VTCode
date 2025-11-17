# VT Code Agent Guide

## Build / Lint / Test
- `cargo check` – quick compile
- `cargo clippy` – lint (must pass before commit)
- `cargo fmt` – format (4 spaces)
- `cargo test` – run all tests
- `cargo test <test_name>` – single test (use `-- --nocapture` to see output)
- `cargo nextest run -- <test_name>` – fast test runner

## Architecture
- `vtcode-core/` – library (LLM providers, tools, config)
- `src/` – CLI binary (TUI, PTY execution, slash commands)
- `docs/` – all documentation (no docs in root)
- Internal API modules: `llm/`, `tools/`, `config/`
- No external DB; state in memory or local files

## Code Style
- Snake_case for vars/functions, PascalCase for types
- `anyhow::Result<T>` with `.with_context()` for errors
- Imports grouped: std, external crates, local modules
- 4‑space indentation, early returns preferred
- No hard‑coded values – use `vtcode.toml` or `constants.rs`
- No comments unless required

## Rules
- Cursor: `.cursor/rules/`
- Claude: `CLAUDE.md`
- Windsurf: `.windsurfrules`
- Cline: `.clinerules`
- Goose: `.goosehints`
- Copilot: `.github/copilot-instructions.md`
