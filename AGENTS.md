# AGENTS.md

## Core rules

- Start with `docs/ARCHITECTURE.md` when repo orientation matters.
- Use Conventional Commits (`type(scope): subject`).
- Prefer `cargo check`, `cargo nextest run`, `cargo fmt`, and `cargo clippy` for local verification.
- **During development: ALWAYS use `./scripts/check-dev.sh`** — never `./scripts/check.sh` (too slow for daily coding).

## Development Workflow

### Active Development (use this 95% of the time)

**Use `./scripts/check-dev.sh` for all daily coding tasks.** This is fast (~10-30s) and covers what you need:

```bash
# Quick check: fmt + clippy + compilation (default-members only)
./scripts/check-dev.sh

# Add tests to the mix
./scripts/check-dev.sh --test

# Full workspace scope when touching multiple crates
./scripts/check-dev.sh --workspace

# Add extra lints (structured logging, etc)
./scripts/check-dev.sh --lints
```

### Release/PR Validation (rare, ~2-5m)

**ONLY run `./scripts/check.sh` when:**
- Preparing a release tag
- Final PR review before merge
- Explicitly requested by reviewer or CI

```bash
./scripts/check.sh
```

**Why?** `check.sh` runs the full comprehensive quality gate across the entire workspace. CI already runs these checks in parallel, so local `check.sh` is only for pre-release validation.

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
- If this repo includes or adds C/C++ surfaces, follow [`docs/development/CPP_CORE_GUIDELINES_ADOPTION.md`](docs/development/CPP_CORE_GUIDELINES_ADOPTION.md).
