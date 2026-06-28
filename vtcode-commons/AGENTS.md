# vtcode-commons

[Root AGENTS.md](../AGENTS.md) | Shared traits and utilities. Zero business logic — pure infrastructure.

## Module Groups

| Area | Modules |
|---|---|
| Traits | `paths/` (WorkspacePaths, PathResolver), `errors/` (ErrorFormatter, ErrorReporter), `telemetry/` (TelemetrySink) |
| Display | `ansi/`, `colors/`, `styling/`, `diff_preview/`, `color256_theme/`, `color_policy/` |
| LLM | `llm/` (BackendKind, LLMError, Usage) |
| Filesystem | `fs/`, `diff/`, `diff_paths/`, `vtcodegitignore/` |
| Text | `tokens/`, `unicode/`, `sanitizer/`, `slug/`, `formatting/` |
| Async | `async_utils/`, `thread_safety/` (RelaxedAtomic) |
| Other | `editor/`, `http/`, `project/`, `validation/`, `serde_helpers/`, `env_lock/` |

## Rules

- Re-export key types from `lib.rs`: `WorkspacePaths`, `TelemetrySink`, `ErrorFormatter`, `BackendKind`, etc.
- `reference.rs` provides in-memory test adapters: `StaticWorkspacePaths`, `MemoryTelemetry`, `MemoryErrorReporter`.
- `ui_protocol/` is a submodule, not a flat module.
- `anstyle_utils` gated behind `tui` feature.

## Gotchas

- `error_category/` classifies LLM errors for retry — `is_retryable_llm_error_message()` is the key function.
- `env_lock/` is macOS-specific env mutex — used by `vtcode` binary, not by library crates.
- `utils/` contains `calculate_sha256()` used by `vtcode-indexer`.
