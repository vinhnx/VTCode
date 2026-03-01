NOTE: use private relay signup codex free

---

Analyze the codebase to identify and improve panic-prone code patterns including force unwraps (!), force try, expect() calls, implicitly unwrapped optionals, and fatalError usages. For each occurrence, assess whether it can be replaced with safer alternatives such as guard statements, optional binding, custom error handling, Result types, or default values. Provide refactored code examples for each improvement, explain the potential runtime failures being prevented, and suggest coding standards to minimize future unsafe patterns. Include analysis of any custom panic handlers or recovery mechanisms that could be implemented.

---

Perform a comprehensive review and optimization of the vtcode agent harness, prioritizing execution speed, computational efficiency, context and token economy. Refactor the tool call architecture to minimize overhead and latency, while implementing robust error handling strategies to significantly reduce the agent's error rate and ensure reliable, effective performance.

---

Conduct a thorough, end-to-end performance audit and systematic optimization of the vtcode agent harness framework with explicit focus on maximizing execution velocity, achieving superior computational efficiency, and implementing aggressive token and context conservation strategies throughout all operational layers. Execute comprehensive refactoring of the tool invocation and agent communication architecture to eliminate redundant processing, minimize inter-process communication latency, and optimize resource utilization at every stage. Design and implement multilayered error handling protocols including predictive failure detection, graceful degradation mechanisms, automatic recovery procedures, and comprehensive logging to drive error occurrence to near-zero levels. Deliver measurable improvements in reliability, throughput, and operational stability while preserving all existing functionality and maintaining backward compatibility with current integration points.

---

extract and open source more components from vtcode-core

---

Review the unified_exec implementation and vtcode's tool ecosystem to identify token efficiency gaps. Analyze which components waste tokens through redundancy, verbosity, or inefficient patterns, and which are already optimized. Develop optimizations for inefficient tools and propose new tools that consolidate multiple operations into single calls to reduce token consumption in recurring workflows.

Specifically examine these known issues: command payloads for non-diff unified_exec still contain duplicated text (output and stdout fields), which wastes tokens across all command-like tool calls. Address this by ensuring unified_exec normalizes all tool calls to eliminate redundant information.

Identify and address these additional token waste patterns: remove duplicated spool guidance that reaches the model both through spool_hint fields and separate system prompts; trim repeated or unused metadata from model-facing tool payloads such as redundant spool_hint fields, spooled_bytes data, duplicate id==session_id entries, and null working_directory values; shorten high-frequency follow-up prompts for PTY and spool-chunk read operations, and implement compact structured continuation arguments for chunked spool reads.

Review each tool's prompt and response structure to ensure conciseness while maintaining effectiveness, eliminating unnecessary verbosity that increases token usage without adding functional value.

---

implement realtime status line config, also add /statusline command to show it

```
Configure Status Line
  Select which items to display in the status lin

  Type to search
  >
› [x] current-dir           Current working dire…
  [x] git-branch            Current Git branch (…
  [ ] model-name            Current model name
  [ ] model-with-reasoning  Current model name w…
  [ ] project-root          Project root directo…
  [ ] context-remaining     Percentage of contex…
  [ ] context-used          Percentage of contex…
  [ ] five-hour-limit       Remaining usage on 5…

  ~/project/path · feat/awesome-feature
  Use ↑↓ to navigate, ←→ to move, space to select,
```

---

Implement a comprehensive solution to resolve the escape key conflict in external editor functionality and expand configurable editor support. First, modify the crossterm event handling system to implement context-aware escape key detection that distinguishes between escape key presses intended for normal mode navigation and those that should trigger rewind functionality. Consider implementing either a configurable double-escape mechanism where a single press exits external editor mode while a double-press triggers rewind, or introduce an alternative keybinding such as Ctrl+Shift+R or F5 for rewind that does not conflict with escape behavior. Ensure VTCode/ANSI escape sequence parsing correctly identifies the source of escape key events. Second, expand the external editor configuration system to include a user-customizable editor preference setting stored in the application configuration. This setting should accept any valid shell command or path to an executable, and the system should parse this command to launch the specified editor when Ctrl+E is triggered. Implement support for launching common editors including Visual Studio Code (code command), Zed (zed command), TextEdit (open command on macOS), Sublime Text (subl command), TextMate (mate command), Emacs (emacsclient or emacs command), Neovim (nvim or vim command), Nano (nano command), and any other editor specified via custom command. The implementation should handle platform-specific editor detection, manage editor process spawning and termination, capture editor output, and properly restore focus to the main application after the external editor session completes. Include error handling for cases where the specified editor is not installed or the command fails to execute.

---

Perform a comprehensive analysis of the codebase to identify and eliminate all instances of duplicated code, following the DRY (Don't Repeat Yourself) and KISS (Keep It Simple, Stupid) principles. Conduct a systematic search across all modules, classes, and files to find similar code patterns, duplicate logic, redundant implementations, and opportunities for abstraction. Specifically examine rendering-related code such as diff previews and command output previews to determine if they can share unified rendering logic, styling, and common components. Audit all utility functions scattered throughout different modules and extract them into a centralized shared utility module with proper organization and documentation. Create a detailed report identifying each duplication found, the proposed refactoring strategy, and the expected benefits in terms of maintainability, reduced code complexity, and improved consistency. Ensure all refactored code maintains existing functionality while simplifying the overall architecture. Prioritize changes that provide the greatest reduction in duplication with minimal risk to existing functionality.

---

review any duplicated code in the codebase and refactor to remove duplication. For example, the logic for rendering the diff preview and the command output preview can be unified to use the same rendering logic and styling. This will make the codebase cleaner and easier to maintain. Additionally, any common utility functions that are duplicated across different modules can be extracted into a shared utility module. search across modules for similar code patterns and identify opportunities for refactoring to reduce duplication and improve code reuse.

DRY and KISS

---

improve agent's postamlbe summarization. reference and also suggest next actions based on the summary. This will help users quickly understand the outcome of the agent's actions and what they can do next. And for the agent itself.

```
• Implemented. I tightened theme color distribution and readability logic across
  both vtcode-tui and vtcode-core.

  - Added luminance balancing for rendered text/accent styles (foreground,
    primary, secondary, user, response, reasoning, alert) relative to background:
      - vtcode-tui/src/ui/theme.rs:118:118
      - vtcode-core/src/ui/theme.rs:118:118
  - Added explicit luminance adjustment helpers:
      - vtcode-tui/src/ui/theme.rs:1414:1414
      - vtcode-core/src/ui/theme.rs:1373:1373
  - Strengthened ensure_contrast with final black/white high-contrast fallback so
    accessibility floor is always met:
      - vtcode-tui/src/ui/theme.rs:1450:1450
      - vtcode-core/src/ui/theme.rs:1409:1409
  - Added full-theme regression tests to enforce contrast + readability bounds
    across all registered themes:
      - vtcode-tui/src/ui/theme.rs:1781:1781
      - vtcode-core/src/ui/theme.rs:1726:1726

  Validation run:
  - cargo check -p vtcode-core passed
  - cargo test -p vtcode-tui
    ui::theme::tests::test_all_themes_have_readable_foreground_and_accents --
    --nocapture passed

  Note:

  - I also noticed an unrelated local config change in vtcode.toml (theme changed
    to ciapre). I left it untouched.
    ``
```

the goal is to provide users with a clear and concise summary of the agent's actions, including any relevant metrics such as API usage, time spent, and code changes. Additionally, the summary should suggest next steps for the user to take based on the outcome of the agent's actions. This could include options to resume the session, view detailed logs, or take specific actions based on the results of the agent's work. The postamble summarization should be designed to help users quickly understand the results of their interactions with the agent and guide them towards meaningful next steps. The tone should be informative and actionable, providing users with the information they need to make informed decisions about how to proceed after the agent has completed its tasks. Not too verbose, but comprehensive enough to cover the key outcomes and next steps. Not too blunt, but clear and concise.

---

Conduct a comprehensive review and enhancement of error handling and recovery mechanisms within the agent loop, with particular emphasis on tool call operations. Implement a multi-layered error handling strategy that includes retry logic with exponential backoff for transient failures such as network timeouts, rate limiting, and temporary service unavailability while implementing fail-fast behavior for non-recoverable errors including authentication failures, invalid parameters, and permission denied scenarios. Develop and integrate a robust state management system that ensures the agent can maintain consistent internal state during and after error occurrences, including proper rollback mechanisms for partial operations and transaction-like semantics where appropriate. Create a comprehensive error categorization system that distinguishes between retryable and non-retryable errors and implements appropriate handling strategies for each category. Enhance user-facing error messages to be clear, actionable, and informative while avoiding technical jargon that may confuse end users. Implement proper logging at multiple levels including debug, info, warning, and error levels to facilitate troubleshooting and monitoring. Conduct a thorough audit of existing error handling implementations to identify gaps, inconsistencies, and potential failure points. Refactor the error handling code to improve modularity, testability, and maintainability while ensuring comprehensive test coverage for error scenarios including edge cases and unexpected inputs. Add appropriate circuit breaker patterns for external service calls to prevent cascading failures and enable graceful degradation when dependent services are unavailable. Implement proper resource cleanup and resource leak prevention throughout the agent loop.

---

check src/agent/runloop/unified/turn module Analyze the agent harness codebase focusing on the runloop, unified, turn, and tool_outcomes components to identify performance bottlenecks, inefficiencies, and optimization opportunities. Perform a comprehensive review of data flow and control flow through these components, examining how tool calls are executed, how outcomes are processed, and how turn execution manages state and sequencing. Evaluate whether the current implementation maximizes parallelism where possible, minimizes blocking operations, and maintains efficient memory usage patterns. Identify any redundant computational steps, unnecessary data transformations, or algorithmic inefficiencies that degrade performance. Assess the current error handling mechanisms for robustness, examining exception propagation paths, retry logic, and failure recovery procedures to ensure they do not introduce excessive latency or create cascading failure scenarios. Examine the design of core data structures used throughout these components for optimal access patterns, memory efficiency, and scalability characteristics. Provide specific, actionable recommendations for refactoring code to reduce complexity, implementing caching where appropriate to avoid redundant computation, optimizing hot path execution, and improving the overall responsiveness and throughput of the agent harness. Your analysis should include concrete code-level suggestions with estimated impact on performance metrics and potential tradeoffs to consider when implementing optimizations.

--

https://zed.dev/blog/split-diffs

---

## Applied: Default Status Line Items Pattern (Codex PR #12015)

The Codex PR demonstrates enabling default status line items when config is unset:

**Pattern Summary**:

1. Define `DEFAULT_STATUS_LINE_ITEMS` const (e.g., `["model-with-reasoning", "context-remaining", "current-dir"]`)
2. Add `configured_status_line_items()` helper that returns defaults when config is `None`
3. Update schema/docs to explain the fallback behavior
4. Fix telemetry gating so defaults are properly counted

**Application to vtcode**:

- vtcode uses `StatusLineMode` (Auto/Command/Hidden) rather than items
- Pattern already applied implicitly: `Auto` mode provides sensible defaults
- Consider adding explicit `DEFAULT_STATUS_LINE_MODE` constant in `vtcode-config/src/status_line.rs`
- Document in config schema that unset values use defaults

---

## Plan: Apply Data-Oriented Design to vtcode

The article by Ivan Enderlin (Matrix Rust SDK) demonstrates three optimization patterns that yielded a **98.7% execution time reduction**: (1) reduce memory pressure by returning only needed data instead of cloning full structs, (2) eliminate lock contention by pre-fetching into compact cache structs, and (3) apply Data-oriented Design by grouping co-accessed fields. All three patterns have direct applicability to vtcode's MCP subsystem, tool execution pipeline, and turn processing contexts.

The work is organized into 4 phases, prioritized by impact and risk.

---

**Steps**

### Phase 1: Lock Consolidation (Highest Impact, Lowest Risk)

1. **Consolidate `McpClient` 5 RwLocks → 1** in client.rs: Create `McpClientInner { providers, allowlist, tool_provider_index, resource_provider_index, prompt_provider_index }` behind a single `parking_lot::RwLock`. Every method currently acquiring 2–5 locks (e.g., `update_allowlist` at client.rs, `collect_tools` at client.rs) becomes a single lock acquisition. Additionally, replace repetitive `self.allowlist.read().clone()` (11+ call sites) with `Arc`-shared snapshot.

2. **Merge `AdaptiveRateLimiter` double-Mutex → 1** in adaptive_rate_limiter.rs: Create `RateLimiterInner { buckets: HashMap<String, TokenBucket>, priorities: HashMap<String, Priority> }` under a single `Mutex`. `try_acquire()` at adaptive_rate_limiter.rs currently locks `buckets` then `priorities` on **every tool call** via the global static `GLOBAL_ADAPTIVE_RATE_LIMITER`. This halves lock overhead and eliminates a potential deadlock vector.

3. **Consolidate `ToolInventory` 7 locks → 2** in inventory.rs: Group `tools`, `aliases`, `sorted_names`, `frequently_used` into `RwLock<ToolRegistryData>`. Keep `alias_metrics` as separate `Mutex` (write-heavy, infrequent). Keep `command_tool` separate (different access pattern). `get_registration` and `has_tool` currently acquire 2 read locks each — consolidated to 1.

4. **Consolidate `ToolDiscoveryCache` 4 Arc<RwLock> → 1** in tool_discovery_cache.rs: Merge `bloom_filter`, `detailed_cache`, `all_tools_cache`, `last_refresh` into `RwLock<DiscoveryCacheInner>`. Methods like `cache_all_tools` (tool_discovery_cache.rs) and `clear()` (tool_discovery_cache.rs) drop from 3–4 lock acquisitions to 1.

5. **Replace `McpProvider` async RwLock with `ArcSwap`** in provider.rs: The `client: tokio::sync::RwLock<Arc<RmcpClient>>` is read-locked on every MCP operation (`list_tools`, `call_tool`, etc.) but only write-locked on reconnect. Switch to `arc_swap::ArcSwap<RmcpClient>` — readers become wait-free (`load()`), only `reconnect` uses `store()`.

6. **Merge `CommandCache` double-Mutex → 1** in command_cache.rs: Combine `config: Mutex` + `cache: Mutex` into single `Mutex<CommandCacheInner>`. Every `get()`/`put()` in the shell execution path currently pays double-lock cost.

### Phase 2: Memory Pressure Reduction (High Impact)

7. **Fix `ToolDiscoveryCache` defeating its own `Arc`** at tool_discovery_cache.rs: Change `(*cached.results).clone()` to `Arc::clone(&cached.results)` and update callers to accept `Arc<Vec<ToolDiscoveryResult>>`. The current code dereferences the `Arc` then deep-clones the inner `Vec`, completely defeating the purpose of `Arc` wrapping.

8. **Wrap `McpProvider` caches in `Arc`** in provider.rs: Change `tools_cache: Mutex<Option<Vec<McpToolInfo>>>` to `Mutex<Option<Arc<Vec<McpToolInfo>>>>`. Cache hit path in `list_tools` (provider.rs) returns `Arc::clone()` instead of deep-cloning the entire vec of tool info structs (each containing `String` name, description, JSON schema).

9. **Eliminate clone-to-sort in `ToolCatalogState::sorted_snapshot`** at tool_catalog.rs: `defs_guard.clone()` clones the entire `Vec<ToolDefinition>` every cache miss. Instead, sort in-place on a `Vec` obtained via `Vec::from(defs_guard.iter().cloned())` only when dirty, and store the sorted result as `Arc<Vec<ToolDefinition>>` — the existing `cached_sorted` field already does this on cache hit; the improvement is to avoid the full clone when the input `tools` vec hasn't changed.

10. **Use `Cow` pattern for `compact_tool_messages_for_retry`** in llm*request.rs: Messages that don't need modification should be referenced, not cloned. Use `Cow<'*, uni::Message>` or collect indices of messages to modify rather than cloning every non-tool message.

### Phase 3: Data-Oriented Design (Structural)

11. **Group `SessionState` fields into domain sub-structs** in types.rs: The current 20+ flat fields with 8 `Arc<RwLock<T>>` create cache-line pollution. Create:
    - `ToolExecutionContext { tool_result_cache, tool_permission_cache, circuit_breaker, rate_limiter, tool_health_tracker, validation_cache, autonomous_executor }` — co-accessed during tool execution
    - `SessionMetadata { decision_ledger, trajectory, telemetry, error_recovery }` — co-accessed during logging/diagnostics
    - Keep `provider_client`, `tools`, `tool_catalog`, `conversation_history` at the top level since they're accessed individually

12. **Refactor `TurnProcessingContext` into domain sub-contexts** in context.rs: The 30+ fields with the manual `as_turn_loop_context()` copy at context.rs is the exact "Array of Structures" anti-pattern from the article. Create:
    - `ToolContext<'a>` — tool_registry, tools, tool_catalog, tool_result_cache, circuit_breaker, rate_limiter, tool_health_tracker, approval_recorder, autonomous_executor
    - `LLMContext<'a>` — provider_client, config, context_manager, vt_cfg
    - `UIContext<'a>` — renderer, handle, session, input_status_state, last_forced_redraw, default_placeholder
    - `as_turn_loop_context()` becomes trivial field copies of 3–4 sub-struct references instead of 20+

### Phase 4: Secondary Optimizations (Lower Priority)

13. **Skills `ContextManager`: `Mutex` → `RwLock`** in context_manager.rs: Read-only methods (`get_stats`, `get_token_usage`, `get_memory_usage`) currently serialize behind a single Mutex. Split to `RwLock` so readers don't block each other during skill-heavy turns.

14. **`CircuitBreaker` snapshot method** in circuit_breaker.rs: Add `snapshot() -> Arc<CircuitBreakerSnapshot>` that acquires the lock once and returns all diagnostics. The existing `get_open_circuits`, `get_diagnostics`, `get_all_diagnostics`, `should_pause_for_recovery`, `open_circuit_count` each acquire a separate `.read()` on the same `RwLock<HashMap>`.

15. **Replace `Arc<RwLock<String>>` with `ArcSwap`** for `HarnessContext` session/task IDs in harness.rs: These are read on every tool call but rarely written. `ArcSwap::load()` is wait-free.

---

**Verification**

- `cargo check` after each step to catch compile errors incrementally
- `cargo nextest run` after each phase to verify no regressions
- `cargo clippy` to ensure no new warnings
- After Phase 1 & 2 are complete, add a micro-benchmark for `AdaptiveRateLimiter::try_acquire` and `ToolDiscoveryCache::get_cached_discovery` to measure lock/clone reduction
- After Phase 3, verify `sizeof::<ToolExecutionContext>()` and `sizeof::<UIContext>()` fit within a typical L1 cache line (128 bytes on Apple Silicon)

**Decisions**

- Chose `parking_lot::RwLock` over `std::sync::RwLock` for consolidated locks: already used in `McpClient`, provides `try_read`/`try_write` without poisoning
- Chose `ArcSwap` over `RwLock` for McpProvider client: the read-heavy/write-rare pattern is textbook `ArcSwap` — readers are wait-free, only reconnect swaps. Requires adding `arc-swap` as a dependency.
- Chose sub-struct grouping (AoS with co-access locality) over full SoA for `SessionState`/`TurnProcessingContext`: SoA doesn't apply here because different fields of the same "row" are accessed together in context, unlike the pure iterate-one-field-at-a-time SoA pattern
- Deferred `ProgressState` consolidation (Phase 4) — updates are infrequent enough that the 4-mutex overhead is negligible

---

review the overrall plan and start implement carefully. can you do better? KISS and DRY. then start implement
