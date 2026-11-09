<!-- markdownlint-disable MD013 -->
# vtcode-commons

[Root AGENTS.md](../../../AGENTS.md) | Shared traits and utilities with zero business logic.

## Modules

- Paths/fs: `paths/`, `fs/`, `vtcode_paths/`, `vtcode_paths_migration.rs`, `vtcodegitignore.rs`, `walk.rs` | errors: `errors.rs`, `error_category/`, `misconfiguration.rs`, `retry.rs`.
- Text: `formatting/`, `sanitizer.rs`, `validation.rs`, `search.rs`, `tokens.rs`, `unicode.rs`, `utf8_stream.rs`, `tracker_title.rs` | display: `ansi.rs`, `colors.rs`, `styling.rs`, `diff_theme.rs`.
- Runtime: `http.rs`, `task_guard.rs`, `async_utils.rs`, `runtime_diagnostics.rs`, `line_framing.rs` | UI/LLM: `ui_protocol/`, `program_status/`, `llm/`, `history_scope.rs` | test adapters: `reference.rs`.

## Rules

- `paths` owns workspace containment (lexical and symlink-resolving tiers) and `safe_archive_relative_path` owns archive-entry validation, so every downstream crate delegates here and never forks the logic.
- Origin-bound HTTP consumers use fallible `http::create_client_without_redirects` and never fall back to a redirect-following client on failure.
- `http::HttpTimeouts` is the single process-global HTTP timeout set, where `None` means disabled (never a zero deadline), so do not add per-module timeout constants.
- `classify_anyhow_error` is the single tool-error classifier and `RetryPolicy` in `retry` is the canonical retry policy. Background fetch loops use `http::send_with_retry` (`_transport_only` for non-idempotent token POSTs) instead of hand-rolled loops; interactive LLM requests never use it (the runloop owns that budget).
- `.vtcodegitignore` registered by `walk::apply_defaults` is the single ignore source of truth, so never fork a UI-side skip list.
- `VtCodePaths::write_private_file_atomic` owns synced atomic private writes without pre-deleting the destination, and legacy migration copies only regular files and never follows links.
- Canonical helpers (`formatting/`, `search/`, `validation/`, `task_guard`, `tracker_title::resolve_tracker_title`, `utils::env_flag_enabled`, `llm::Usage::billable_totals`, `tty::TtyExt`) are the single homes, so downstream crates delegate rather than re-implement.
- `Utf8StreamDecoder` (`utf8_stream`) is the single chunked UTF-8 decoder: never `from_utf8_lossy` one read chunk, since a character split across reads becomes U+FFFD; use `push_cow` in hot loops and `finish()` at EOF.
- Keep `ui_protocol::tool_summary` and `ui_protocol::progress` out of `ThreadEvent` and independent of TUI/runtime types.
- `HistoryScope` (`history_scope`) is the single prompt-history scope model (labels, Ctrl+S cycle order, wire values) for the CLI picker, desktop palette, and desktop service; never fork per-surface copies.

## Gotchas

- `ArcSwap` globals (`vtcodegitignore`, `HttpTimeouts`) use whole-value swaps, and clients built before an install keep their old timeouts.
- `ui_protocol::SessionSurface` defaults to `Inline`, so alternate-screen detection must request `Auto` or `Alternate` explicitly.
- `runtime_diagnostics` exposes only stable Tokio metrics, since the rest need `--cfg tokio_unstable`.
- `diff/` and `diff_preview/` are compatibility re-exports, so new diff consumers use `vtcode-diff` directly.

Details: [maintainer notes](../../../docs/development/maintainer-notes/vtcode-commons.md) — read the section for the subsystem you change before editing it.
