# AGENTS.md

## Core rules

- Start with `docs/ARCHITECTURE.md` when repo orientation matters.
- Run `./scripts/check.sh` before calling work complete.
- Prefer `cargo check`, `cargo nextest run`, `cargo fmt`, and `cargo clippy` for local verification.
- Use Conventional Commits (`type(scope): subject`).

## Repository shape

- Repository: `vtcode`.
- Main code lives in `src/`, `vtcode-core/`, `vtcode-tui/`, and `tests/`.
- Match CI expectations in `.github/workflows/`.

## `vtcode-core`

- Resist adding new code to `vtcode-core`.
- Prefer an existing smaller crate, or introduce one, when reusable logic does not need to live in `vtcode-core`.

## Style

- Rust uses 4-space indentation, snake_case functions, PascalCase types, and `anyhow::Result<T>` with `.with_context()` on fallible paths.
- Keep changes surgical and behavior-preserving.
- Measure before optimizing.
