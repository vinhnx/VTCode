# vtcode-* Crate Audit Report

**Date**: 2026-06-14 (updated 2026-06-14)
**Scope**: All vtcode-* workspace crates
**Goal**: Identify mergeable crates, redundant code, and test duplicates

> **Note**: Several merges from this audit have been completed. See "Completed Merges" section below.

---

## Workspace Overview

| Crate | Source Files | Inline Tests | Purpose | Deps on other vtcode-* |
|-------|-------------|--------------|---------|------------------------|
| vtcode-commons | 47 | 22 | Shared primitives (ANSI, diff, errors, paths, tokens, walk, telemetry) | none |
| vtcode-core | 828 | 466 | Main agent runtime (LLM, tools, skills, agents, MCP) | 12 crates |
| vtcode-config | 120 | 32 | Config loading, schema, models, defaults, hooks | commons, auth |
| vtcode-ui | 184 | 61 | Design system, theme registry, TUI framework | commons, config, terminal-detection, vim |
| vtcode-acp | 32 | 13 | Agent Communication Protocol (Zed adapter) | core, config |
| vtcode-auth | 10 | 8 | OAuth, PKCE, credential storage | none (leaf) |
| vtcode-bash-runner | 9 | 5 | Shell execution, process groups | commons, exec-events |
| vtcode-exec-events | 3 | 3 | Structured execution telemetry event schemas | none (leaf) |
| vtcode-file-search | 2 | 1 | Fast fuzzy file search (nucleo) | commons |
| vtcode-indexer | 1 | 1 | Workspace file indexer with markdown persistence | commons |
| vtcode-macros | 1 | 0 | `#[derive(StringNewtype)]` proc macro | none (leaf) |
| vtcode-markdown-store | 1 | 1 | Markdown-backed storage (KV, project, cache) | none (leaf) |
| vtcode-process-hardening | 1 | 0 | Pre-main process security hardening | none (leaf) |
| vtcode-terminal-detection | 1 | 1 | Terminal emulator detection | none (leaf) |
| vtcode-utility-tool-specs | 5 | 4 | JSON schemas for tool parameter definitions | none (leaf) |
| vtcode-vim | 4 | 1 | Vim-style prompt editing engine | none (leaf) |
| vtcode-tool-types | 6 | 0 | Shared tool runtime types (breaks circular deps) | none (leaf) |
| vtcode-llm | ~100 | 0 | LLM provider abstraction, client implementations, streaming | commons, config, tool-types, utility-tool-specs |
| vtcode-skills | 22 | 0 | Skill types, discovery, loading, and validation | commons, config |
| vtcode-safety | 15 | 31 | Command safety detection, execution policies, sandboxing | commons |
| vtcode-pods | 6 | 0 | GPU pod management | commons |
| vtcode-a2a | 10 | 6 | Agent2Agent protocol client and server | none (leaf) |
| vtcode-mcp | 15 | 31 | MCP client, connection pooling, tool discovery | config, commons, utility-tool-specs |

**Dependency graph** (leaf crates at bottom):

```
                          vtcode (binary)
                        /    |    \      \
                   vtcode-acp  |  vtcode-ui  vtcode-process-hardening
                     /    \    |    /    \  \
              vtcode-core  vtcode-config | vtcode-vim  vtcode-terminal-detection
              / | | | \ \  \    /   \      |
    commons auth bash exec file idx md-store
         macros    events search         utility-tool-specs
              \      |      /
               vtcode-safety (command_safety + exec_policy + sandboxing)
               vtcode-llm (LLM providers, client abstraction, streaming)
               vtcode-skills (skill types, discovery, validation)
                    vtcode-pods (GPU management)
                    vtcode-a2a (Agent2Agent protocol)
                    vtcode-tool-types (shared types, breaks circular deps)
                    vtcode-mcp (MCP client, connection pooling, tool discovery)
```

---

## MERGE RECOMMENDATIONS

### MERGE 1: vtcode-indexer + vtcode-file-search -> vtcode-search (HIGH impact)

**Rationale**: Both crates provide file discovery/indexing capabilities with overlapping domain concepts.

**Evidence**:
- Both define a `pub struct FileIndex` with different fields but the same name
- vtcode-file-search provides fuzzy matching (nucleo); vtcode-indexer provides regex-based search + markdown persistence
- vtcode-core integrates both through `tools/file_search_bridge.rs` and `tools/file_search_rpc.rs`
- vtcode-indexer already uses `vtcode-commons::walk` for traversal, same as file-search would
- Combined crate would be ~500 lines, still small enough to be focused

**Merged crate responsibilities**:
- File indexing (hash, metadata, content) from indexer
- Fuzzy search (nucleo) from file-search
- Regex/grep search from indexer
- Markdown-backed index persistence from indexer
- `FileIndexCache` from file-search

**Risk**: Medium. The two `FileIndex` structs need unification. The indexer's `MarkdownIndexStorage` depends on `fs2` for locking which file-search doesn't need. Feature-gate the persistence behind an optional dep.

**Savings**: 1 fewer crate in workspace, 1 fewer bridge layer in vtcode-core, unified `FileIndex` type.

---

### MERGE 2: vtcode-exec-events into vtcode-commons (MEDIUM impact)

**Rationale**: vtcode-exec-events is a tiny leaf crate (3 files, 898 lines) that only defines event schema types. vtcode-commons already contains foundational types (LLM types, telemetry traits, error categories).

**Evidence**:
- vtcode-exec-events depends on nothing -- pure data types
- vtcode-commons already defines `TelemetrySink<Event>` trait and `Usage` struct
- Both define a `Usage` struct (vtcode-commons uses u32, exec-events uses u64) -- these should be unified
- vtcode-bash-runner optionally depends on exec-events; it already depends on commons
- Moving exec-events into `vtcode-commons::exec_events` module would eliminate a crate and resolve the dual-Usage conflict

**Merged location**: `crates/common/vtcode-commons/src/exec_events/` module

**Risk**: Low. Pure schema types with no complex logic. The u32->u64 Usage migration needs careful type width audit.

**Savings**: 1 fewer crate, unified `Usage` type, cleaner dependency graph.

---

### MERGE 3: vtcode-markdown-store into vtcode-commons (MEDIUM impact)

**Rationale**: vtcode-markdown-store is a single-file crate (653 lines) providing markdown-backed persistence. vtcode-commons already provides file system primitives (`fs`, `paths`, `walk`).

**Evidence**:
- Only 1 source file, no inter-crate deps
- Provides `MarkdownStorage`, `SimpleKVStorage`, `ProjectStorage`, `SimpleCache`
- vtcode-indexer implements its own `MarkdownIndexStorage` instead of reusing this -- the consolidation in MERGE 1 would bring this into the combined search crate anyway
- `SimpleCache` (feature-gated) overlaps conceptually with vtcode-core's `UnifiedCache`

**Merged location**: `crates/common/vtcode-commons/src/markdown_store/` module

**Risk**: Low. Self-contained, no complex dependencies.

**Savings**: 1 fewer crate, consistent storage primitives.

---

### MERGE 4: vtcode-process-hardening into vtcode (binary crate) (LOW impact)

**Rationale**: vtcode-process-hardening is a single-file crate (421 lines) with a single `#[ctor]` function that runs before `main`. It's only consumed by the top-level binary crate.

**Evidence**:
- Only used by `vtcode` binary (the top-level crate)
- No other vtcode-* crate depends on it
- It's a `#[ctor]` that runs pre-main -- there's no reason it needs to be a separate library crate
- Moving the code into `src/main.rs` or a `src/hardening.rs` module in the binary crate is simpler

**Risk**: Very low. The `#[ctor]` attribute works identically in binary crate modules.

**Savings**: 1 fewer crate, simpler build graph.

---

### NO MERGE: Crates that should stay separate

| Crate | Why keep separate |
|-------|-------------------|
| vtcode-commons | Foundational leaf crate, everything depends on it. Keep focused. |
| vtcode-core | The hub. 828 files, 466 test modules. Too large to merge anything into. |
| vtcode-config | Large (120 files), clean separation of config from runtime. |
| vtcode-ui | Large (184 files), complex TUI framework. Clean boundary. |
| vtcode-acp | Protocol-specific, depends on core. Clean adapter pattern. |
| vtcode-auth | OAuth flows are self-contained and security-sensitive. |
| vtcode-bash-runner | Shell execution is a distinct concern with its own safety model. |
| vtcode-macros | Proc-macro crates must be separate by Rust rules. |
| vtcode-terminal-detection | Clean, focused, no overlap. |
| vtcode-utility-tool-specs | Passive JSON schemas, zero deps, no overlap. |
| vtcode-vim | Self-contained editor engine, clean boundary. |

---

## RE-EXPORT CLEANUP (in vtcode-core)

vtcode-core re-exports types from other crates through 3+ import paths. These should be consolidated.

### Token estimation functions (3 paths -> 1)

Current state:
- `vtcode_commons::tokens::{estimate_tokens, truncate_to_tokens}` -- canonical
- `vtcode_core::llm::utils::{estimate_token_count, truncate_to_token_limit}` -- re-export with alias
- `vtcode_core::utils::tokens::*` -- wildcard re-export

**Action**: Keep `vtcode_commons::tokens` as canonical. Remove `vtcode_core::utils::tokens` re-export. Rename `vtcode_core::llm::utils` re-exports to use original names.

### HTTP client (3 paths -> 1)

Current state:
- `vtcode_commons::http::*` -- canonical
- `vtcode_core::http_client::*` -- re-export
- `vtcode_core::utils::http_client::*` -- re-export

**Action**: Keep `vtcode_commons::http` as canonical. Remove both re-export files in vtcode-core.

### Error types (3 paths -> 2)

Current state:
- `vtcode_commons::errors::*` and `vtcode_commons::error_category::*` -- canonical
- `vtcode_core::error::{VtCodeError, ErrorCode}` -- vtcode-core-specific types
- `vtcode_core::utils::error_messages::*` -- re-export of commons

**Action**: Remove `vtcode_core::utils::error_messages` re-export. Keep vtcode-core's own error types separate from commons.

### Config types (2 paths -> 1)

Current state:
- `vtcode_config::*` -- canonical
- `vtcode_core::config::*` -- 11+ re-export files mirroring vtcode-config

**Action**: This re-export chain exists for backward compatibility. It's intentional and low-risk, but should be documented as deprecated import paths. Add `#[deprecated]` attributes to guide consumers toward `vtcode_config::*`.

---

## TEST DUPLICATES TO ELIMINATE

### HIGH priority

1. **Delete `vtcode-core/tests/mcp_basic_test.rs`** (160 lines)
   - Every test is a duplicate of tests in `mcp_integration_test.rs`
   - `test_mcp_config_defaults`, `test_mcp_client_creation`, `test_mcp_ui_modes`, `test_provider_environment_variables`, `test_mcp_client_initialization` are all near-identical

2. **Remove duplicated tests from `vtcode-core/tests/mcp_integration_e2e.rs`**
   - `test_mcp_ui_modes` -- duplicates `mcp_basic_test.rs` line 79 (which itself duplicates `mcp_integration_test.rs`)
   - `test_provider_environment_variables` -- duplicates `mcp_basic_test.rs` line 103
   - `test_multiple_providers_config` -- duplicates `mcp_integration_test.rs` line 165
   - `test_mcp_client_status` -- duplicates `mcp_integration_test.rs` line 294
   - E2E file should focus only on real-server lifecycle tests

### MEDIUM priority

3. **Merge `vtcode-core/tests/unicode_handling_test.rs` into `pty_unicode_test.rs`**
   - Both test the same unicode edge cases (emojis, CJK, split sequences)
   - `unicode_handling_test.rs` never instantiates a PTY -- it's pure UTF-8 exercises

4. **Consolidate test helpers into `vtcode-core/tests/support/`**
   - `is_python_available()` -- duplicated in `mcp_integration_e2e.rs:37` and `mcp_startup_timeout_test.rs:183`
   - `mock_mcp_server_path()` -- duplicated in `mcp_integration_e2e.rs:44` and `mcp_startup_timeout_test.rs:190`
   - `setup_registry()` -- duplicated in `apply_patch_comprehensive.rs:6` and `apply_patch_semantic.rs:9`

5. **Create shared `FileOpsTool` test helper**
   - The 2-line `GrepSearchManager::new` + `FileOpsTool::new` pattern appears 15+ times across test files
   - Move to `vtcode-core/tests/support/mod.rs`

### LOW priority

6. **Rename PTY test files for clarity**
   - `pty_test.rs` -> `pty_registry_integration_tests.rs` (tests through ToolRegistry)
   - `pty_tests.rs` -> `pty_manager_unit_tests.rs` (tests PtyManager directly)

7. **Consolidate optimization tests**
   - `real_optimization_integration_test.rs` and `real_execute_tool_ref_optimization_test.rs` overlap significantly

---

## IMPLEMENTATION ORDER

If proceeding with merges, the recommended order is:

1. **MERGE 4** (process-hardening into binary) -- zero risk, smallest change
2. **MERGE 2** (exec-events into commons) -- resolve Usage type conflict
3. **MERGE 3** (markdown-store into commons) -- clean up storage primitives
4. **MERGE 1** (indexer + file-search into search) -- largest change, depends on 2+3 being done first
5. **Test cleanup** -- can be done in parallel with merges
6. **Re-export cleanup** -- do last, after all merges are stable

Each merge should be a separate PR with:
- `cargo check --workspace` passing
- `cargo test --workspace` passing
- Updated AGENTS.md for affected crates
- Deprecation annotations on old import paths

---

## COMPLETED MERGES (2026-06-14)

The following merges from this audit have been implemented:

| Merge | From | To | Status |
|-------|------|----|--------|
| MERGE 4 | vtcode-process-hardening | vtcode (root binary) | Completed |
| MERGE 5 | vtcode-terminal-detection | vtcode-commons | Completed |
| MERGE 6 | vtcode-vim | vtcode-ui | Completed |
| MERGE 7 | vtcode-pods | vtcode-core | Completed |
| MERGE 8 | vtcode-tool-types | vtcode-commons | Completed |
| MERGE 9 | vtcode-file-search + vtcode-markdown-store | vtcode-indexer | Completed (prior session) |

### What changed:
- **vtcode-process-hardening**: Moved to `src/process_hardening.rs` in root binary crate
- **vtcode-terminal-detection**: Moved to `crates/common/vtcode-commons/src/terminal_detection.rs`
- **vtcode-vim**: Moved to `crates/codegen/vtcode-ui/src/vim/` module
- **vtcode-pods**: Moved to `crates/codegen/vtcode-core/src/pods/` module
- **vtcode-tool-types**: Merged into `vtcode-commons` (tool_types, model_family modules)
- **vtcode-file-search + vtcode-markdown-store**: Merged into vtcode-indexer (prior session)

### Benefits realized:
- 6 fewer crates in workspace (from 23 to 17 member crates)
- Reduced cross-crate dependency complexity
- Simpler import paths for shared types
- Fewer Cargo.toml files to maintain
