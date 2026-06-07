# VTCode Comprehensive Cleanup Plan

This plan covers every TODO, FIXME, placeholder, dead code, duplicate, redundant, deprecated, stale, and leftover artifact found across the entire codebase. Execute in the order listed -- each phase builds on the previous.

---

## Phase 1: Delete Leftover Artifacts (Low Risk, Immediate)

Remove tracked files that should never have been committed.

### 1.1 Delete empty `.tmp` files (8 files)
```
vtcode-acp/Cargo.toml.tmp
vtcode-bash-runner/Cargo.toml.tmp
vtcode-collaboration-tool-specs/Cargo.toml.tmp
vtcode-indexer/Cargo.toml.tmp
vtcode-llm/Cargo.toml.tmp
vtcode-markdown-store/Cargo.toml.tmp
vtcode-terminal-detection/Cargo.toml.tmp
vtcode-tools/Cargo.toml.tmp
```

### 1.2 Delete backup files (3 files)
```
scripts/release.sh.backup
.vtcode/tool-policy.json.bak
vtcode-core/.vtcode/tool-policy.json.bak
```

### 1.3 Delete stale empty files
```
.vtcode/ide-context.md                  (0 bytes, tracked)
```

### 1.4 Delete stale `__pycache__` directories
```
evals/__pycache__/
scripts/__pycache__/
evals/terminal_bench/__pycache__/
```

### 1.5 Delete stale empty directories
```
docs/diagrams/
.local/bin/
.claude/.state/
.config/vtcode/statusline/
evals/benchmark/evals/
evals/benchmark/agents/
evals/benchmark/results/
.agents/skills/cmd-generate-agent-file/
.agents/skills/vtcode-dev-helper/scripts/
.agents/skills/vtcode-dev-helper/assets/
```

### 1.6 Update `.gitignore`
- Add `*.tmp`, `*.bak`, `*.backup`, `*.orig` if not already present (they are, but files were force-added)
- Verify `AGENTS.md` and `CLAUDE.md` entries under "Ruler Generated Files" section are intentional (they are tracked despite being listed)

---

## Phase 2: Remove Dead and Commented-Out Code

### 2.1 Delete entirely dead modules and functions

| File | What to remove | Reason |
|------|---------------|--------|
| `vtcode-core/src/tools/middleware.rs` | Entire file (11 deprecated items) | Fully superseded by `async_middleware` since 0.1.0 |
| `vtcode-core/src/tools/mod.rs:199-200,293-294` | Remove `pub mod middleware` and re-export | No remaining callers |
| `vtcode-commons/src/anstyle_utils.rs` | Entire file (13 deprecated functions) | All replaced by `vtcode_design::style` since 0.123.2 |
| `vtcode-indexer/src/lib.rs:599-660` | Remove `walk_directory`, `walk_directory_internal`, `is_allowed_path`, `walk_allowed_descendants` | Dead code, never called |
| `src/cli/messages.rs:24-58` | Remove `info()` and `config_hint()` | Dead code, never called |
| `zed-extension/src/metrics.rs:12` | Remove `MetricPoint` struct | Dead code, never used |
| `vtcode-core/src/config/output_styles.rs:1` | Remove "Placeholder module to fix compilation error" comment | Replace with proper module doc or merge into real module |

### 2.2 Remove commented-out code blocks

| File | Lines | What |
|------|-------|------|
| `tests/test_image_functionality.rs:46-48` | Commented-out test assertions making test a no-op | Either implement the test or delete the test function |
| `vtcode-core/src/mcp/rmcp_transport.rs:127-130` | Commented-out notes in always-bailing stub | Clean up the stub or remove it |
| `vtcode-core/src/llm/providers/common.rs:1290-1292` | Commented-out design notes | Remove |

### 2.3 Remove debug `println!` / `eprintln!` in production code

| File | Line | Statement |
|------|------|-----------|
| `vtcode-core/src/tools/ripgrep_installer/mod.rs:159` | `println!("Ripgrep status: {:?}", status)` | Replace with `tracing::debug!` or remove |
| `vtcode-core/src/core/agent/runner/tool_args.rs:30` | `println!("{}", style(&warning).red().bold())` | Replace with proper logging |
| `vtcode-core/src/commands/ask.rs:23` | `eprintln!("Sending prompt to {}: {}", ...)` | Replace with `tracing::debug!` |

### 2.4 Clean up stub functions

| File | Line | What |
|------|------|------|
| `vtcode-core/src/mcp/rmcp_transport.rs:121` | `create_http_transport()` always bails | Either implement or remove with feature gate |
| `vtcode-core/src/tools/file_search_rpc.rs:286` | `find_references` stub | Mark as `unimplemented!()` or implement |
| `vtcode-core/src/tools/optimized_registry.rs:149` | Placeholder tool execution | Replace with real logic or remove |
| `vtcode-core/src/llm/optimized_client.rs:253` | Placeholder implementation | Replace with real logic or remove |

### 2.5 Clean up placeholder comments

Replace every remaining `// Placeholder` comment with either:
- A proper implementation, OR
- A `// TODO(username): description` with a tracking issue reference, OR
- Remove the comment if the code is intentionally minimal

Files with placeholder comments:
- `vtcode-core/src/memory_integration_tests.rs:29`
- `vtcode-core/src/metrics/execution_metrics.rs:56`
- `vtcode-core/src/terminal_setup/wizard.rs:339`
- `vtcode-core/src/terminal_setup/terminals/mod.rs:20`
- `vtcode-core/src/terminal_setup/features/mod.rs:15`
- `vtcode-core/src/core/telemetry.rs:29`
- `vtcode-core/src/core/prompt_caching.rs:369`
- `vtcode-core/src/skills/document_processor.rs:227-229,243`
- `vtcode-config/src/core/agent.rs:1104`
- `vtcode-config/src/models/model_id/capabilities.rs:559`

---

## Phase 3: Resolve TODO Comments

### 3.1 Cargo.toml lint TODOs (7 items)

Each of these has a `# TODO: Phase in after cleanup pass` comment. For each, either:
- **Phase it in** (change `"allow"` to `"warn"`) and fix all resulting warnings, OR
- **Remove the TODO** if the lint is intentionally allowed long-term

| File:Line | Lint | Action |
|-----------|------|--------|
| `Cargo.toml:175` | `string_slice` | Audit codebase, phase in as `"warn"` |
| `Cargo.toml:176` | `indexing_slicing` | Audit codebase, phase in as `"warn"` |
| `Cargo.toml:189` | `let_underscore_must_use` | Audit intentional drops, phase in |
| `Cargo.toml:219` | `cast_possible_truncation` | Audit overflow risk, phase in |
| `Cargo.toml:220` | `cast_possible_wrap` | Audit overflow risk, phase in |
| `Cargo.toml:232` | `allow_attributes` | Phase in after suppression audit |
| `Cargo.toml:233` | `allow_attributes_without_reason` | Phase in after suppression audit |

### 3.2 Source code TODOs

| File | Line | TODO | Action |
|------|------|------|--------|
| `src/agent/runloop/unified/tool_pipeline/status.rs:30` | Progress variant for streaming | Implement or remove |
| `vtcode-core/src/skills/assets/samples/skill-creator/scripts/init_skill.py:105` | Add actual script logic | Implement or mark as sample template |

---

## Phase 4: Consolidate Duplicate Code

### 4.1 Config struct deduplication (HIGHEST IMPACT)

**Problem**: At least 17 structs and 5 enums are defined in both `vtcode-tui/src/config/` and `vtcode-config/src/`.

**Action**: For each duplicate, keep the canonical definition in `vtcode-config/src/` and replace the `vtcode-tui` version with `pub use vtcode_config::*;` re-exports.

Structs to deduplicate (vtcode-tui -> vtcode-config):
- `VTCodeConfig` -> `vtcode-config/src/loader/config.rs:59`
- `AgentConfig` -> `vtcode-config/src/core/agent.rs:16`
- `UiConfig` -> `vtcode-config/src/root.rs:218`
- `ToolOutputMode` -> `vtcode-config/src/root.rs:11`
- `UiDisplayMode` -> `vtcode-config/src/root.rs:48`
- `NotificationDeliveryMode` -> `vtcode-config/src/root.rs:62`
- `KeyboardProtocolConfig` -> `vtcode-config/src/root.rs:837`
- `UiNotificationsConfig` -> `vtcode-config/src/root.rs:91`
- `PtyConfig` -> `vtcode-config/src/root.rs:638`
- `PromptCacheConfig` -> `vtcode-core/src/core/prompt_caching.rs:29`
- `FullAutoConfig` -> `vtcode-config/src/core/automation.rs:37`
- `AutomationConfig` -> `vtcode-config/src/core/automation.rs:8`
- `ToolsConfig` -> `vtcode-config/src/core/tools.rs`
- `SecurityConfig` -> `vtcode-config/src/core/security.rs:24`
- `ContextConfig` -> `vtcode-config/src/types/mod.rs:650`
- `SyntaxHighlightingConfig` -> `vtcode-config/src/loader/syntax_highlighting.rs:9`

Enums to deduplicate:
- `ToolPolicy` -> `vtcode-config/src/core/tools.rs:225`
- `ReasoningEffortLevel` -> `vtcode-config/src/types/mod.rs:20`
- `SystemPromptMode` -> `vtcode-config/src/types/mod.rs:86`
- `ToolDocumentationMode` -> `vtcode-config/src/types/mod.rs:161`
- `VerbosityLevel` -> `vtcode-config/src/types/mod.rs:229`

### 4.2 Sandbox type deduplication (within vtcode-core)

Remove duplicate definitions from `vtcode-core/src/tools/handlers/sandboxing.rs` and import from `vtcode-core/src/sandboxing/`:
- `SandboxTransformError` (line 462 -> `sandboxing/manager.rs:13`)
- `SandboxType` (line 372 -> `sandboxing/exec_env.rs:180`)
- `ExecEnv` (line 438 -> `sandboxing/exec_env.rs:155`)

### 4.3 MCP config deduplication

Remove from `vtcode-core/src/mcp/enhanced_config.rs` and import from `vtcode-config/src/mcp.rs`:
- `McpRateLimitConfig` (line 45 -> `vtcode-config/src/mcp.rs:116`)
- `McpValidationConfig` (line 67 -> `vtcode-config/src/mcp.rs:138`)

### 4.4 Error type deduplication

| Duplicate | Keep canonical at | Remove from |
|-----------|------------------|-------------|
| `ErrorSeverity` | `vtcode-core/src/tools/unified_error.rs:21` | `improvements_errors.rs:71`, `zed-extension/src/error_handling.rs:66` |
| `ErrorCode` | `vtcode-core/src/error.rs:50` | `vtcode-core/src/mcp/errors.rs:18`, `zed-extension/src/error_handling.rs:24` |
| `SafetyError` | `vtcode-core/src/tools/safety_gateway.rs:117` | `src/agent/runloop/unified/tool_call_safety.rs:25` |
| `AcpError` | `vtcode-acp/src/error.rs:8` | `vtcode-core/src/copilot/error.rs:8` |

### 4.5 Static variable deduplication

| Duplicate | Keep canonical at | Remove from |
|-----------|------------------|-------------|
| `AST_GREP_OVERRIDE` + guard | `vtcode-core/src/tools/ast_grep_binary.rs` | `vtcode-core/src/tools/editing/patch/semantic.rs` (import instead) |

### 4.6 Constant deduplication

| Constant | Action |
|----------|--------|
| `SECONDS_PER_DAY` (4 copies) | Define once in `vtcode-commons/src/lib.rs` or `vtcode-core/src/core/mod.rs`, import everywhere |
| Model string constants (13+ duplicated across providers) | Ensure `mod.rs` re-exports are canonical; remove local definitions in `opencode_go.rs`, `opencode_zen.rs`, `evolink.rs`, `copilot.rs` |

### 4.7 Function deduplication

| Function | Action |
|----------|--------|
| `strip_ansi_codes` (3 copies) | Use `vtcode_commons::ansi::strip_ansi` everywhere; delete local copies in `vtcode-tui/src/core_tui/session/text_utils.rs:15` and `src/agent/runloop/tool_output/streams_helpers.rs:278` |
| `clean_reasoning_text` (2 copies) | Keep in `vtcode-core/src/llm/providers/reasoning.rs:36`, replace `vtcode-tui/src/core_tui/session/header.rs:20` with import |
| Regex duplicates (`NON_WHITESPACE_TOKEN_PATTERN`, `QUOTED_PATH_PATTERN`) | Consolidate into one location, import from the other |

### 4.8 Re-export layer cleanup

Remove duplicate re-export files where two files re-export the same source:
- `vtcode-core/src/utils/colors.rs` AND `vtcode-core/src/utils/color_utils.rs` both re-export `vtcode_commons::colors::*` -- keep one, delete the other
- `vtcode-core/src/utils/anstyle_utils.rs` AND `vtcode-core/src/utils/ratatui_styles.rs` both re-export `vtcode_commons::anstyle_utils::*` -- keep one, delete the other
- `vtcode-core/src/ui/theme.rs` AND `vtcode-tui/src/ui/theme.rs` are exact duplicates -- keep one, have the other re-export

---

## Phase 5: Clean Up Deprecated Code

### 5.1 Remove deprecated modules with no remaining callers

| Module | Since | Replacement |
|--------|-------|-------------|
| `vtcode-core/src/tools/middleware.rs` (entire file) | 0.1.0 | `async_middleware` |
| `vtcode-commons/src/anstyle_utils.rs` (entire file) | 0.123.2 | `vtcode_design::style` |
| `vtcode-acp/src/lib.rs` deprecated types (`AcpClient`, `AcpClientBuilder`, `AcpMessage`, `AcpRequest`, `AcpResponse`) | 0.60.0 | `AcpClientV2`, jsonrpc types |

### 5.2 Remove deprecated config fields

| Field | Since | Action |
|-------|-------|--------|
| `vtcode-config/src/core/provider.rs:422` `skip_model_validation` | 0.75.0 | Remove field, keep serde alias for backward compat if needed |

### 5.3 Remove `#[allow(deprecated)]` suppression wrappers

After removing deprecated items, also remove:
- `vtcode-core/src/utils/mod.rs:141`
- `vtcode-core/src/utils/ratatui_styles.rs:5`
- `vtcode-core/src/utils/anstyle_utils.rs:5`
- `vtcode-config/src/core/provider.rs:499`
- `vtcode-commons/src/anstyle_utils.rs:9`

---

## Phase 6: Fix Dead Code Suppressions

### 6.1 Remove `#[allow(dead_code)]` / `#[expect(dead_code)]` on genuinely dead code

For each item below, either **delete the dead code** or **add a doc comment explaining why it's kept** (e.g., "Public API for downstream consumers"):

| File | What |
|------|------|
| `vtcode-core/src/tools/pty/scrollback.rs:292` | `snapshot()` -- implement or remove |
| `vtcode-core/src/llm/providers/poolside.rs:30` | `model_behavior` field -- use or remove |
| `vtcode-config/src/core/prompt_cache.rs:430,435,452` | 3 serde default functions -- use or remove |
| `vtcode-config/src/models/model_id/capabilities.rs:6,12` | `capability_generated` module -- use or remove |
| `vtcode-acp/src/client_v2.rs:52,59` | `client_id`, `timeout` fields -- use or remove |
| `vtcode-acp/src/client.rs:23` | `timeout` field (deprecated client) -- remove with client |
| `vtcode-config/build_codegen.rs:239,425` | `ENTRIES` constants -- use or remove |

### 6.2 Audit `#[expect(dead_code)]` in vtcode-tui (184 instances)

This is the largest cluster. For each file in `vtcode-tui/src/core_tui/`:
- If the dead item is truly unused scaffolding for a planned feature: add a `// SAFETY: reason` comment
- If the dead item is leftover from refactoring: delete it
- If the dead item is a public API surface that might be used externally: keep with documented reason

Priority files to audit:
1. `session/reflow/formatting.rs` (7 instances)
2. `session/config.rs` (7 instances)
3. `session/reflow/blocks.rs` (5 instances)
4. `session/modal/state.rs` (5 instances)
5. `session/message_renderer.rs` (5 instances)

---

## Phase 7: Fix Skipped Tests

### 7.1 Add reasons to `#[ignore]` tests (14 tests missing reasons)

| File | Test | Action |
|------|------|--------|
| `vtcode-core/tests/mcp_context7_manual.rs:9` | `context7_list_tools_smoke` | Add reason or remove |
| `vtcode-core/src/memory_integration_tests.rs:318` | `bench_cache_optimization_impact` | Add reason or remove |
| `vtcode-core/src/memory_tests.rs:141` | `bench_cache_operations` | Add reason or remove |
| `tests/models_sync.rs:9` | `constants_cover_models_json` | Add reason or remove |
| `tests/llm_providers_test.rs:268` | `test_provider_supported_models` | Add reason or remove |
| `tests/llm_providers_test.rs:328` | `test_request_validation` | Add reason or remove |
| `tests/prompt_extraction_test.rs:4,15,28` | 3 tests | Add reason or remove |
| `tests/llm_provider_integration.rs:91` | `test_provider_supported_models` | Add reason or remove |
| `tests/tools_anthropic_alignment.rs:53` | `grep_file_default_concise_and_cap` | Add reason or remove |
| `tests/integration_tests.rs:22,136` | 2 tests | Add reason or remove |

---

## Phase 8: Fix Broken Documentation

### 8.1 Fix broken links in README.md

| Line | Broken link | Action |
|------|-------------|--------|
| 387 | `./docs/guides/vscode.md` | Create file or remove link |
| 387 | `./docs/guides/claude-code.md` | Create file or remove link |
| 400 | `./docs/safety/SAFETY_ARCHITECTURE.md` | Create file or remove link |
| 400 | `./docs/safety/SECURITY_HARDENING.md` | Create file or remove link |
| 400 | `./docs/safety/THREAT_MODEL.md` | Create file or remove link |

### 8.2 Fix stale `src/tui.rs` references (15 references across 4 files)

Update all references from `src/tui.rs` to `vtcode-tui/src/` (or whatever the current path is):

| File | Lines to update |
|------|----------------|
| `docs/FAQ.md` | 11, 40, 173, 174, 183, 198 |
| `docs/guides/async-architecture.md` | 24, 116, 138, 177, 455 |
| `docs/guides/terminal-rendering-best-practices.md` | 357 |
| `docs/guides/tui-event-handling.md` | 17, 43, 58 |

### 8.3 Fix other broken doc links

| Reference | Action |
|-----------|--------|
| `docs/tools/EDITOR_CONFIG.md` (referenced from `docs/README.md:79`, `docs/config/config.md:317`) | Create file or remove links |

### 8.4 Remove hardcoded example user path

| File | Line | What |
|------|------|------|
| `vtcode-core/src/tools/registry/executors.rs:862` | `/Users/example/.vtcode/sessions/agent-1.json` | Use a placeholder like `<workspace>/.vtcode/sessions/agent-1.json` |

---

## Phase 9: Reduce Clippy Suppression Surface

### 9.1 Remove per-crate `[lints.clippy]` duplication

Six Cargo.toml files repeat the same allow list. Migrate to workspace-level `[workspace.lints.clippy]` only, removing per-crate overrides:

- `Cargo.toml:31-37`
- `vtcode-auth/Cargo.toml:56-63`
- `vtcode-core/Cargo.toml:225-231`
- `vtcode-config/Cargo.toml:66-73`
- `vtcode-bash-runner/Cargo.toml:52-59`
- `vtcode-file-search/Cargo.toml:41-48`

### 9.2 Address `clippy::result_large_err` (17 LLM provider files)

This systemic suppression indicates the error enum is too large. Fix by:
- Boxing the large variant in the error enum, OR
- Using `Box<dyn Error>` for the large variant, OR
- Splitting the error enum into provider-specific errors

### 9.3 Address `clippy::too_many_arguments` (8 locations)

Refactor functions with too many parameters into builder pattern or config struct:
- `vtcode-core/src/retry.rs:334`
- `vtcode-core/src/core/agent/runner.rs:152`
- `vtcode-core/src/core/agent/runner/tool_exec.rs:1082`
- `vtcode-acp/src/tooling/catalog.rs:82`
- `vtcode-bash-runner/src/process.rs:75`
- `src/agent/runloop/tool_output/streams.rs`
- `src/agent/runloop/unified/tool_pipeline.rs`
- `src/agent/runloop/unified/turn/finalization.rs`

### 9.4 Remove `unused_imports` suppression

| File | Line | Action |
|------|------|--------|
| `vtcode-tools/src/lib.rs:115` | `#[allow(unused_imports)]` on re-export | Fix the import or remove the re-export |

---

## Phase 10: Remove Unused Feature Flags

### 10.1 `vtcode-tools/Cargo.toml` features

| Feature | Action |
|---------|--------|
| `net` | Remove -- no `#[cfg(feature = "net")]` usage anywhere |
| `examples` | Audit -- only used once with `#[allow(unused_imports)]` |

---

## Execution Order Summary

| Phase | Risk | Effort | Description |
|-------|------|--------|-------------|
| 1 | Minimal | Small | Delete leftover artifacts (.tmp, .bak, empty dirs) |
| 2 | Low | Medium | Remove dead/commented-out code, debug prints, stubs |
| 3 | Low | Small | Resolve TODO comments |
| 4 | Medium | Large | Consolidate duplicate code (configs, types, functions) |
| 5 | Medium | Medium | Remove deprecated code |
| 6 | Medium | Large | Audit and fix dead code suppressions (184+ instances) |
| 7 | Low | Medium | Fix skipped tests |
| 8 | Low | Small | Fix broken documentation |
| 9 | Medium | Large | Reduce clippy suppression surface |
| 10 | Low | Small | Remove unused feature flags |

**Total cleanup items: 300+**

After all phases, the codebase should have:
- Zero `TODO` comments (resolved or converted to tracked issues)
- Zero `Placeholder` comments (implemented or documented)
- Zero dead `#[allow(dead_code)]` / `#[expect(dead_code)]` (code deleted or reason documented)
- Zero `.tmp` / `.bak` / `.backup` files
- Zero broken documentation links
- Zero duplicate struct/enum/type definitions
- Zero duplicate function implementations
- Minimal `#[deprecated]` surface (removed or gated behind feature flags)
- Minimal per-crate lint overrides (consolidated to workspace level)
