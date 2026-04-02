# AGENTS.md

## Quick start

- Default verification command: `./scripts/check.sh` before calling work complete.
- Build with `cargo check` (preferred) or `cargo build --release`.
- Format via `cargo fmt` and lint with `cargo clippy` before committing.
- Run tests with `cargo nextest run` or `cargo test <name> -- --nocapture`.
- Install JavaScript dependencies with `npm install`.

## Architecture & layout

- Start with `docs/ARCHITECTURE.md` when you need repo orientation or architectural context.
- Repository: vtcode.
- Primary languages: Rust, JavaScript/TypeScript.
- Key source directories: `src/`, `tests/`.
- Application entrypoints live under the primary source directories.
- CI workflows detected under `.github/workflows/`; match those expectations locally.
- Docker assets are present; some integration flows may depend on container setup.

## Important instructions

- Use Conventional Commits (`type(scope): subject`).

## Code style

- Rust code uses 4-space indentation, snake_case functions, PascalCase types, and `anyhow::Result<T>` with `.with_context()` for fallible paths.
- Run `cargo fmt` before committing and avoid hardcoded configuration.
- Use the repository formatter and linter settings; match existing component and module patterns.

## Testing

- Default verification command: `./scripts/check.sh`.
- Rust suite: `cargo nextest run` for speed, or `cargo test` for targeted fallback.
- Run `cargo clippy --workspace --all-targets -- -D warnings` for lint coverage.
- Run JavaScript/TypeScript checks with `npm test` or the repo's `check` script when present.
- Keep CI green by mirroring workflow steps locally before pushing.

## Performance & simplicity

- Do not guess at bottlenecks; measure before optimizing.
- Prefer simple algorithms and data structures until workload data proves otherwise.
- Keep performance changes surgical and behavior-preserving.

## PR guidelines

- Use Conventional Commits (`type(scope): subject`) with short, descriptive summaries.
- Reference issues with `Fixes #123` or `Closes #123` when applicable.
- Keep pull requests focused and include test evidence for non-trivial changes.

## Additional guidance

- Preferred orientation doc: `docs/ARCHITECTURE.md`.
- Repository docs spotted: AGENTS.md, CHANGELOG.md, LICENSE, README.md, docs/ARCHITECTURE.md, docs/README.md, docs/modules/vtcode_docs_map.md.

