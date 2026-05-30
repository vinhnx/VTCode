# AGENTS.md

## Use This File For

- Repo-wide workflow and placement decisions.
- Open `docs/ARCHITECTURE.md` only when the task spans crates, touches runtime boundaries, or you need repo orientation.
- Prefer module-local docs over broad repo exploration when working in one area.

## Core Rules

- Keep changes surgical and behavior-preserving.
- Use Conventional Commits (`type(scope): subject`).
- Match CI expectations in `.github/workflows/`.
- Rust uses 4-space indentation, snake_case functions, PascalCase types, and `anyhow::Result<T>` with `.with_context()` on fallible paths.
- Measure before optimizing.
- Consult [`docs/development/rust-performance-principles.md`](docs/development/rust-performance-principles.md) for Rust-specific performance guidance (aliasing, destructive moves, iterator elision, overflow checking near-zero cost, `#[cold]` strategy, and safety-enables-aggressive-optimization patterns).
- In hot Rust paths, treat `Cow` as conditional ownership, not a free borrow: if values are always borrowed or stored in dense token/AST-style enums, prefer `&str`/slices and compact variants unless measurement shows `Cow` pays for itself.
- Prefer ownership and borrowing by default; introduce `Rc<T>`/`Arc<T>` only for genuine shared ownership.
- When ownership or lifetimes get tangled, first prefer explicit handles/IDs plus an owning context.
- Use `Rc<T>` only for single-threaded sharing and `Arc<T>` only for cross-thread/task sharing; prefer immutable sharing and narrowly scoped interior mutability.
- Break back-references or task-parent links with `Weak<T>`/`Arc::downgrade()` so cycles do not leak memory or keep state alive unexpectedly.
- Do not reach for raw pointers, custom `Send`/`Sync`, or lifetime-branding patterns unless simpler handle-based designs are insufficient; document the invariant if you do.
- If this repo includes or adds C/C++ surfaces, follow [`docs/development/CPP_CORE_GUIDELINES_ADOPTION.md`](docs/development/CPP_CORE_GUIDELINES_ADOPTION.md).

## Workspace Structure

This is a Cargo workspace with ~20 crates. Key boundaries:

| Crate | Role |
| --- | --- |
| `vtcode` (root `src/`) | Binary entrypoint, CLI, session bootstrap |
| `vtcode-core` | Core runtime: agent loop, tools, prompts, LLM orchestration, UI |
| `vtcode-tui` | Public TUI API surface for downstream consumers |
| `vtcode-llm` | LLM provider abstraction and request shaping |
| `vtcode-tools` | Tool registry and built-in tool implementations |
| `vtcode-config` | Configuration loading and schema |
| `vtcode-bash-runner` | Shell execution sandbox |
| `vtcode-acp` | Agent Client Protocol integration (Zed) |
| `vtcode-auth` | OAuth and credential storage |
| `vtcode-indexer` | Code indexing and search |
| `vtcode-file-search` | File search primitives |
| `vtcode-process-hardening` | OS-level sandboxing (Seatbelt, Landlock) |
| `vtcode-exec-events` | Runtime event contract (`ThreadEvent`) and ATIF export |
| `vtcode-markdown-store` | Markdown persistence layer |
| `vtcode-commons` | Shared utilities across crates |
| `vtcode-ghostty-vt-sys` | Ghostty VT runtime bindings |
| `vtcode-theme` | UI theme definitions |
| `vtcode-vim` | Vim keybinding support |
| `vtcode-lmstudio` | LM Studio local provider |

`default-members` in `Cargo.toml` limits `cargo check`/`clippy`/`build` to the root crate, `vtcode-core`, and `vtcode-tui`. Use `--workspace` to check all crates.

## Build & Toolchain

- Rust stable, MSRV 1.88 (`rust-toolchain.toml`).
- `rustfmt.toml` sets `edition = "2024"`.
- Dev profile: `incremental = false` (for sccache compatibility). Set `CARGO_INCREMENTAL=1` to override for rapid iteration.
- CI sets `RUSTFLAGS: "-D warnings"` — clippy and compiler warnings are hard failures.
- CI uses `--locked` on all cargo commands. Run `cargo check --locked` to match.
- `clippy.toml` allows indexing/slicing/panic/unwrap in tests (`allow-indexing-slicing-in-tests = true`, etc.).

## Verification

Use `./scripts/check-dev.sh` during development. Do not use `./scripts/check.sh` for routine iteration.

| If you changed...                                                        | Run                                    |
| ------------------------------------------------------------------------ | -------------------------------------- |
| A focused code path and you want the default fast gate                   | `./scripts/check-dev.sh`               |
| Logic covered by tests or you added tests                                | `./scripts/check-dev.sh --test`        |
| Multiple crates or shared code                                           | `./scripts/check-dev.sh --workspace`   |
| Extra lint-sensitive code paths                                          | `./scripts/check-dev.sh --lints`       |
| GitHub workflows or workflow-security-sensitive scripts                  | `./scripts/check.sh workflow-security` |
| Ast-grep rules or scan scaffolding (`sgconfig.yml`, `rules/`)            | `vtcode check ast-grep`                |
| PTY/TUI harness paths called out in `docs/harness/QUALITY_SCORE.md`      | `./scripts/check.sh harness`           |
| Release validation, final PR validation, or reviewer/CI explicitly asked | `./scripts/check.sh`                   |

`./scripts/check-dev.sh` runs: rustfmt -> clippy (default-members) -> cargo check. Add `--test` for nextest, `--workspace` for all crates, `--lints` for structured logging + agent legibility lints.

Use `cargo check`, `cargo nextest run`, `cargo fmt`, and `cargo clippy` when you need a narrower command for a specific crate or faster debugging loop.

## Testing

- **Preferred test runner**: `cargo nextest run` (faster, parallel). Fallback: `cargo test --workspace`.
- **Nextest profiles** (`.config/nextest.toml`):
  - `default`: balanced local dev, `fail-fast = true`, 30s slow timeout.
  - `ci`: no fail-fast, 2 retries, 60s slow timeout. CI uses `cargo nextest run --locked --profile ci`.
  - `quick`: TDD, skips integration/e2e tests, 10s timeout.
- **Run a single test**: `cargo nextest run test_name` or `cargo test test_name`.
- **Run tests for one crate**: `cargo nextest run -p vtcode-core` or `cargo test -p vtcode-core`.
- **Harness regression tests** (PTY/TUI): run separately via `./scripts/check.sh harness` or manually:
  ```
  cargo test -p vtcode-core --test pty_tests
  cargo test -p vtcode-bash-runner --test pipe_tests
  cargo test -p vtcode --bin vtcode inline_events::tests
  ```
- Clippy allows `unwrap`, `panic`, indexing, and `expect` in test code (see `clippy.toml`).
- Integration tests live in `tests/` at the workspace root. Unit tests are in-module.

## Code Placement

| Situation                                                               | Preferred location                              |
| ----------------------------------------------------------------------- | ----------------------------------------------- |
| Reusable logic that does not need to live in the core runtime           | An existing smaller crate, or a new small crate |
| Code tightly coupled to existing `vtcode-core` runtime responsibilities | `vtcode-core`                                   |
| Unsure whether new reusable logic belongs in `vtcode-core`              | Keep it out of `vtcode-core` by default         |

## Implementation Notes

- Prefer simple algorithms and control flow until the workload justifies extra complexity.
- Keep new abstractions proportional to current use; do not generalize single-use code.
- Preserve existing APIs and behavior unless the task requires a change.
- `vtcode-exec-events::ThreadEvent` is the authoritative runtime event contract. Do not invent parallel event types.
- The harness is split across `agent.harness`, `automation.full_auto`, and `context.dynamic` — do not create a new top-level harness subsystem.

## Adding a New LLM Provider

Use the `adding-llm-providers` skill for new provider integrations, adding models to existing providers, provider defaults, model picker support, and `docs/models.json` capability metadata.

**Key insight**: The `/model` splash picker uses `ModelId::all_models()` — `builtin_model_presets()` is a separate system used by `ModelsManager`. Both must be updated for a provider/models to appear in the picker.

## Local Development Run

```bash
# Debug build + launch (fast compile)
./scripts/run-debug.sh

# Release build + launch
./scripts/run.sh
```

Both scripts auto-bootstrap Ghostty VT runtime libraries when missing. Without them, PTY snapshots fall back to `legacy_vt100`.

## Command Output

Protect context usage. **Any command with unknown or potentially large output must be byte-capped.**

Default pattern:

```bash
COMMAND 2>&1 | head -c 4000
```

## Codemod Skill Discovery

This section is managed by `codemod` CLI.

- Core skill: `.agents/skills/codemod/SKILL.md`
- Package skills: `.agents/skills/<package-skill>/SKILL.md`
- Codemod MCP: use it for JSSG authoring guidance, CLI/workflow guidance, import-helper guidance, and semantic-analysis-aware codemod work.
- List installed Codemod skills: `npx codemod ai list --harness codex --format json`
