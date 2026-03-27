# DRY/KISS Duplication Audit

## Scope

This pass targeted high-value duplication with low behavioral risk:

- exact duplicate modules and constants
- repeated preview/head-tail formatting logic
- repeated diff display semantics
- repeated sync file/image helpers
- repeated local timestamp helpers
- a narrow slice of duplicated UI text/default constants

The goal was to remove duplicated logic while preserving the existing output policies and public behavior.

## Implemented Refactors

### 1. Shared preview helpers

Owner: `vtcode-commons/src/preview.rs`

Added:

- `TextLineExcerpt`
- `split_head_tail_preview_with_limit`
- `excerpt_text_lines`
- `excerpt_text_lines_with_limit`
- `format_hidden_bytes_summary`
- `condense_text_bytes`
- `tail_preview_text`

Replaced local preview logic in:

- `src/agent/runloop/tool_output/large_output.rs`
- `src/agent/runloop/tool_output/streams.rs`
- `src/agent/runloop/unified/tool_pipeline/pty_stream/state.rs`
- `vtcode-core/src/tools/output_spooler.rs`
- `vtcode-core/src/tools/registry/executors.rs`

Benefits:

- one canonical implementation for head/tail previews
- one canonical implementation for byte condensation with UTF-8 boundary safety
- fewer drift risks across PTY, tool-output, and executor previews

Risk level: low. The policy knobs stayed local; only the excerpt/condensation logic moved.

### 2. Shared diff display model

Owner: `vtcode-commons/src/diff_preview.rs`

Added:

- `DiffDisplayKind`
- `DiffDisplayLine`
- `display_lines_from_hunks`
- `display_lines_from_unified_diff`
- `diff_display_line_number_width`

Kept `format_numbered_unified_diff()` as a compatibility wrapper over the shared model.

Replaced duplicate diff semantics in:

- `src/agent/runloop/tool_output/streams.rs`
- `vtcode-tui/src/core_tui/app/session/diff_preview.rs`

Benefits:

- ANSI and Ratatui renderers now consume the same line-kind and numbering rules
- start-only hunk headers are generated once
- diff line-number width calculation is centralized

Risk level: low to medium. Rendering adapters still own styling, but parsing/numbering now comes from one source.

### 3. Exact duplicate prompt-budget constants

Removed duplicate files:

- `vtcode-config/src/constants/instructions.rs`
- `vtcode-config/src/constants/project_doc.rs`

Added canonical owner:

- `vtcode-config/src/constants/prompt_budget.rs`

Updated callers in:

- `src/agent/runloop/welcome.rs`
- `vtcode-core/src/prompts/system.rs`
- `vtcode-config/src/core/agent.rs`

Benefits:

- one source of truth for the default byte budget
- no silent drift between instruction and project-doc defaults

Risk level: low.

### 4. Exact duplicate `FileColorizer`

Removed duplicate implementation:

- deleted `vtcode-core/src/ui/file_colorizer.rs`

Canonical owner remains:

- `vtcode-tui/src/ui/file_colorizer.rs`

Compatibility preserved via re-export from:

- `vtcode-core/src/ui/mod.rs`

Benefits:

- one LS_COLORS parser implementation
- no duplicate tests or bug-fix surface

Risk level: low.

### 5. Shared file helpers and image detection

Added canonical image helper:

- `vtcode-commons/src/fs.rs::is_image_path`

Removed duplicate logic from:

- `vtcode-core/src/tools/file_ops/mod.rs` now re-exports `vtcode_commons::fs::is_image_path`
- deleted `vtcode-tui/src/utils/file_utils.rs`

Updated TUI call sites to use `vtcode-commons::fs` directly:

- `vtcode-tui/src/core_tui/session/config.rs`
- `vtcode-tui/src/core_tui/session/input.rs`
- `vtcode-tui/src/utils/mod.rs`

Benefits:

- one owner for sync file helpers and image-path classification
- less wrapper churn inside `vtcode-tui`

Risk level: low.

### 6. Partial UI constant consolidation

Canonical owner for shared UI text/defaults remains:

- `vtcode-config/src/constants/ui.rs`

`vtcode-tui/src/config/constants/ui.rs` now aliases a small shared subset instead of redefining it:

- tool output mode strings
- default reasoning visibility
- common placeholder/header strings
- welcome-section labels and limits

TUI-only layout/runtime knobs were intentionally kept local.

Benefits:

- reduced drift in stable cross-surface copy/defaults
- avoided forcing TUI layout behavior through config constants that are intentionally local

Risk level: low.

### 7. Repeated timestamp helpers

Switched local timestamp helpers to the shared owner:

- `vtcode-core/src/context/conversation_memory.rs`
- `vtcode-core/src/context/entity_resolver.rs`
- `vtcode-core/src/core/prompt_caching.rs`
- `vtcode-core/src/memory/mod.rs`

Canonical owner:

- `vtcode-commons/src/utils.rs::current_timestamp`

Benefits:

- one timestamp implementation
- fewer tiny utility copies across core modules

Risk level: low.

## Duplication Intentionally Left Alone

### `vtcode-core/src/utils` wrapper sprawl

This workspace still contains many thin `vtcode-core::utils::*` compatibility wrappers over `vtcode-commons`.

Reason not removed in this pass:

- the import surface is broad across the workspace
- deleting them all at once would create high-churn edits with low immediate payoff
- the narrow wrapper removal in `vtcode-tui/src/utils/file_utils.rs` delivered a safer first reduction

Recommended follow-up:

1. rewrite imports module-by-module toward `vtcode-commons`
2. keep only wrappers that are intentional compatibility surface
3. delete dead wrappers after each compile-verified batch

### Spool storage policies

The storage-location policy split was preserved:

- `vtcode-core/src/tools/output_spooler.rs` keeps workspace-relative spool behavior
- `src/agent/runloop/tool_output/large_output.rs` keeps temp-file UI spool behavior

Only shared condensation/excerpt logic was deduplicated.

## Expected Benefits

- less duplicated parsing/rendering code in the two diff-preview paths
- less duplicated preview code across command output, PTY live preview, spool previews, and inspect output
- fewer independent utility copies to maintain
- lower drift risk for shared UI copy/defaults
- simpler future bug fixes because the shared helpers now live in `vtcode-commons`

## Verification

Baseline before changes:

- `cargo test --workspace --no-run` passed

Post-change verification:

- `cargo check --workspace` passed
- `cargo clippy --workspace --all-targets -- -D warnings` passed

`cargo nextest run --workspace` did not complete because of pre-existing missing snapshot baselines in:

- `advanced_tui_scenario_tests__message_combo_user_agent_exchange`
- `advanced_tui_scenario_tests__header_context_basic_context`
- `advanced_tui_scenario_tests__tui_with_conversation_history`
- `advanced_tui_scenario_tests__styled_segment_plain_text`

Those failures produced `.snap.new` files only; no accepted `.snap` baseline exists for those cases in `tests/snapshots/`. The generated `.snap.new` files were removed after verification to keep the worktree clean.
