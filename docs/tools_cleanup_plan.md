# Tools Cleanup Plan

## Problem Statement

We are consolidating all search functionality around a single tool to prevent redundancy and inconsistent behaviour.

**Active component**

-   `GrepSearchManager` in `grep_file.rs` – registered via `ToolRegistry::grep_file_executor` and used by the CLI, ACP bridge, and tests

**Retired components**

-   `AdvancedSearchTool` in `advanced_search.rs` – no longer exported
-   `FileSearcher` in `file_search.rs` – removed from module exports
-   `SearchTool` in `search.rs` – previously deleted legacy implementation
-   `SimpleSearchTool` in `simple_search.rs` – previously deleted bash wrapper

## Analysis

### GrepSearchManager (`grep_file.rs`)

-   **Purpose**: Primary content-search implementation used across the runtime
-   **Features**:
    -   Debounce and cancellation logic for responsive searches
    -   Ripgrep-first execution with perg fallback when ripgrep is unavailable
    -   Supports pattern matching, glob filters, context lines, and similarity modes
    -   Serves as the backend for `ToolRegistry::grep_file_executor` and higher-level helpers
-   **Status**: Active and well-maintained

### Retired Helpers

-   `advanced_search.rs` and `file_search.rs` have been removed from exports; no runtime code should reference them
-   Remaining code paths must delegate to `grep_file.rs` for content search

### Legacy SearchTool (`search.rs`, removed)

-   **Purpose**: Previously provided enhanced search modes (exact, fuzzy, multi-pattern, similarity)
-   **Status**: Deleted to eliminate redundancy with `GrepSearchManager`
-   **Action**: Remove remaining references (e.g., docs, configs) and rely on grep-backed pathways

### Legacy SimpleSearchTool (`simple_search.rs`, removed)

-   **Purpose**: Shell-based search wrapper intended for quick grep-like operations
-   **Status**: Deleted to consolidate on the Rust-based search stack
-   **Action**: Confirm older automation scripts no longer reference the bash runner

## Redundancy Issues

1. **Legacy SearchTool duplication**: Fully removed; ensure lingering references are eliminated (docs/config/tests)
2. **Retired helper drift**: Confirm `advanced_search` and `file_search` never reappear via re-exports or docs
3. **FileOpsTool ad-hoc content search**:
    - `list_files` `find_content` mode manually scans files, duplicating functionality
    - Delegate to `GrepSearchManager` or remove the mode to avoid parallel implementations

## Cleanup Plan

### Phase 1: Immediate Actions

1. ✅ Remove `SearchTool` implementation (already done - file deleted from the tree)
2. ✅ Remove references to `SearchTool` in mod.rs and other files
3. ✅ Update tool registry to remove `search_tool()` method

### Phase 2: Finalize Grep-Only Surface

1. ✅ Decommission `SimpleSearchTool`; confirm no runtime paths invoke it
2. ✅ Remove module exports for `advanced_search` and `file_search`
3. ✅ Update `list_files` (`find_content`) to route through `grep_file`
4. ✅ Publish ownership guidance (file discovery vs. content search) for contributors
    - `docs/ARCHITECTURE.md` now clarifies that `list_files` covers discovery while `grep_file` handles content search

### Phase 3: Documentation Updates

1. ✅ Remove references to retired helpers across docs and configuration (architecture guide, migration guide, full auto guide, research notes)
2. ✅ State clearly that `grep_file` is the sole search interface
3. ✅ Refresh summaries (`docs/tools_cleanup_summary.md`, cleanup plan) after doc edits

### Phase 3b: Contributor Communication

1. Coordinate with maintainer to announce grep-only surface (Slack/CHANGELOG entry)
2. Update onboarding snippets (`docs/ADVANCED_FEATURES_IMPLEMENTATION.md`, `docs/context_engineering_best_practices.md`) to reflect new guidance
3. ✅ Update system prompts to reinforce `grep_file` as the sole content-search tool (`prompts/system.md`, `vtcode-core/src/prompts/system.rs`)

### Phase 4: Testing

1. Run targeted unit tests and integration paths that rely on `grep_file`
    - Execute `cargo test tests::test_consolidated_search` and `vtcode-core::tests::file_ops_security`
2. Validate tool registry execution paths (`ToolRegistry::grep_file_executor`, downstream consumers)
    - Smoke-test TUI/CLI flows once registry diff stabilizes
3. Ensure legacy references in tests/fixtures are removed (`tools::search`, `simple_search`, `advanced_search`, `file_search`)
    - Sweep `tests/`, `scripts/`, and config scaffolding for stale tool names

### Phase 5: Registry & Policy Alignment

1. ✅ Review `vtcode-core/src/tools/registry/*` to confirm only intended search tools are exported
2. ✅ Remove `advanced_search` / `file_search` exports from `vtcode-core` and `vtcode-tools`
3. ✅ Update `vtcode-config` constants and tool policies to match the consolidated search stack
    - `vtcode-config/src/constants.rs`, `.../loader/mod.rs` now highlight `grep_file` as the sole search surface
4. ✅ Validate `vtcode.toml` / `vtcode.toml.example` defaults so that capability levels align with available tools
    - Audited defaults/automation profiles; comments now call out `grep_file` as the only search entry
5. ✅ Confirm UI layers (`src/agent/runloop/*`, `vtcode-core/src/ui`) route search requests through the consolidated pathways
    - Verified `text_tools`, `tool_summary`, `tool_routing` use `tools::GREP_FILE` and contain no legacy search aliases

### Phase 6: Summarization Controls

1. ✅ Verify automatic summarization defaults to off (`ConversationSummarizer::new()` reads env, defaults false)
2. ✅ Ensure no config templates (`vtcode.toml`, `vtcode.toml.example`) enable summarization by default
3. ✅ Document follow-up: summarization remains opt-in via `VTCODE_AUTO_SUMMARY_ENABLED`

## Recommended Approach

Standardize on `GrepSearchManager` as the sole search backend. Remove or refactor any code paths
that bypass it to maintain consistent, maintainable behaviour.
