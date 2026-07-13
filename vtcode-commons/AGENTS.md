# vtcode-commons

[Root AGENTS.md](../AGENTS.md) | Shared traits and utilities. Zero business logic — pure infrastructure.

## Module Groups

| Area | Modules |
|---|---|
| Traits | `paths/` (WorkspacePaths, PathResolver), `errors/` (ErrorFormatter, ErrorReporter), `telemetry/` (TelemetrySink) |
| Display | `ansi/`, `colors/`, `styling/`, `diff_preview/`, `color256_theme/`, `color_policy/` |
| LLM | `llm/` (BackendKind, LLMError, Usage) |
| Filesystem | `fs/`, `diff/`, `diff_paths/`, `vtcodegitignore/`, `workspace_snapshot/` (env-delta fingerprint: `capture`/`diff`/`is_drift`) |
| Text | `tokens/`, `unicode/`, `sanitizer/`, `slug/`, `formatting/` |
| Async | `async_utils/`, `thread_safety/` (RelaxedAtomic) |
| Other | `editor/`, `http/`, `project/`, `validation/`, `serde_helpers/`, `env_lock/` |

## Rules

- Re-export key types from `lib.rs`: `WorkspacePaths`, `TelemetrySink`, `ErrorFormatter`, `BackendKind`, etc.
- `reference.rs` provides in-memory test adapters: `StaticWorkspacePaths`, `MemoryTelemetry`, `MemoryErrorReporter`.
- `ui_protocol/` is a submodule, not a flat module.
- `anstyle_utils` gated behind `tui` feature.

## Gotchas

- `paths` has two containment tiers: lexical `ensure_path_within_workspace` and async symlink-resolving `ensure_path_within_workspace_resolved`. Downstream crates delegate here — do not fork the logic.
- `retry` owns the canonical `RetryPolicy` (delay math, jitter, `RetryDecision`/`RetryStep`, `simple()` constructor). vtcode-core only layers domain adapters on top.
- `error_category/` classifies LLM errors for retry — `is_retryable_llm_error_message()` is the key function; `classify_anyhow_error` → `ErrorCategory` is the single classifier for tool errors.
- `errors/` provides `MultiErrors<E>` — a reusable error collection type implementing the "error parameter" pattern for continuing work while collecting failures. Use it instead of ad-hoc `Vec<String>` or `Vec<ErrorEnum>` for batch/parallel operations where individual items can fail independently.
- `env_lock/` is macOS-specific env mutex — used by `vtcode` binary, not by library crates.
- `utils/` contains `calculate_sha256()` used by `vtcode-indexer`.
