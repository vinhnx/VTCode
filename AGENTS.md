# AGENTS.md

## Quick start

- Default verification command: `./scripts/check.sh` before calling work complete.
- Build with `cargo check` (preferred) or `cargo build --release`.
- Format via `cargo fmt` and lint with `cargo clippy` before committing.
- Run tests with `cargo nextest run` or `cargo test <name> -- --nocapture`.
- When running Rust commands, be patient with them and do not kill them by PID; Cargo lock contention can make them slow, and that is expected.
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

## The `vtcode-core` crate

Over time, the `vtcode-core` crate (defined in `vtcode-core/`) can become bloated because it is the largest crate, so it is often easier to add something new to `vtcode-core` rather than refactor out the library code you need so your new code neither takes a dependency on, nor contributes to the size of, `vtcode-core`.

To that end: **resist adding code to `vtcode-core`**.

Particularly when introducing a new concept, feature, or API, before adding to `vtcode-core`, consider whether:

- There is an existing crate other than `vtcode-core` that is an appropriate place for the new code to live.
- It is time to introduce a new crate to the workspace for the new functionality. Refactor existing code as necessary to make this possible.

Likewise, when reviewing code, push back on changes that unnecessarily add code to `vtcode-core`.

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
