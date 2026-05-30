# AGENTS.md

## Rules

- Conventional Commits (`type(scope): subject`).
- 4-space indentation, `snake_case` fns, `PascalCase` types, `anyhow::Result<T>` + `.with_context()`.
- CI sets `RUSTFLAGS: "-D warnings"` and uses `--locked`. Match locally with `cargo check --locked`.
- Keep changes surgical. Preserve existing APIs unless the task requires a change.
- `vtcode-exec-events::ThreadEvent` is the authoritative runtime event contract — do not invent parallel types.
- Harness config is split across `agent.harness`, `automation.full_auto`, `context.dynamic` — do not add a new top-level harness subsystem.

## Workspace

Cargo workspace, ~20 crates. `default-members` = root, `vtcode-core`, `vtcode-tui` only.

| Crate                      | Role                                              |
| -------------------------- | ------------------------------------------------- |
| `vtcode` (root `src/`)     | Binary, CLI, session bootstrap                    |
| `vtcode-core`              | Agent loop, tools, prompts, LLM orchestration, UI |
| `vtcode-tui`               | Public TUI API surface                            |
| `vtcode-llm`               | LLM provider abstraction                          |
| `vtcode-tools`             | Tool registry and implementations                 |
| `vtcode-config`            | Config loading and schema                         |
| `vtcode-bash-runner`       | Shell execution sandbox                           |
| `vtcode-acp`               | Agent Client Protocol (Zed)                       |
| `vtcode-auth`              | OAuth and credential storage                      |
| `vtcode-indexer`           | Code indexing and search                          |
| `vtcode-process-hardening` | OS sandboxing (Seatbelt, Landlock)                |
| `vtcode-exec-events`       | `ThreadEvent` contract and ATIF export            |
| `vtcode-commons`           | Shared utilities                                  |
| `vtcode-ghostty-vt-sys`    | Ghostty VT runtime bindings                       |

New reusable logic: put it in an existing small crate or a new one. Keep it out of `vtcode-core` by default unless tightly coupled to the core runtime.

## Build

- Rust stable, MSRV 1.88, edition 2024.
- Dev profile has `incremental = false` (sccache). Set `CARGO_INCREMENTAL=1` to override.
- `clippy.toml` allows `unwrap`/`panic`/indexing in tests.

## Verification

Prefer `./scripts/check-dev.sh` (10-30s) over `./scripts/check.sh` (2-5m) for iteration.

| Change          | Command                              |
| --------------- | ------------------------------------ |
| Fast gate       | `./scripts/check-dev.sh`             |
| + tests         | `./scripts/check-dev.sh --test`      |
| + workspace     | `./scripts/check-dev.sh --workspace` |
| + lints         | `./scripts/check-dev.sh --lints`     |
| Harness PTY/TUI | `./scripts/check.sh harness`         |
| Release/PR      | `./scripts/check.sh`                 |
| Ast-grep rules  | `vtcode check ast-grep`              |

Narrow commands: `cargo check`, `cargo nextest run`, `cargo fmt`, `cargo clippy`.

## Testing

- **Runner**: `cargo nextest run` (parallel, fast). Fallback: `cargo test --workspace`.
- **Single test**: `cargo nextest run test_name` or `cargo test test_name`.
- **Single crate**: `cargo nextest run -p vtcode-core`.
- **Profiles**: `default` (local), `ci` (no fail-fast, 2 retries, 60s timeout), `quick` (TDD, skips integration/e2e).
- **Harness regressions** (run separately):
    ```
    cargo test -p vtcode-core --test pty_tests
    cargo test -p vtcode-bash-runner --test pipe_tests
    cargo test -p vtcode --bin vtcode inline_events::tests
    ```
- Integration tests: `tests/` at workspace root. Unit tests: in-module.

## LLM Provider

Use the `adding-llm-providers` skill. The `/model` picker uses `ModelId::all_models()` — `builtin_model_presets()` is a separate system used by `ModelsManager`. Both must be updated.

## Dev Run

```bash
./scripts/run-debug.sh   # debug build + launch
./scripts/run.sh         # release build + launch
```

Both auto-bootstrap Ghostty VT runtime. Without it, PTY snapshots fall back to `legacy_vt100`.

## Output

Cap large command output: `COMMAND 2>&1 | head -c 4000`
