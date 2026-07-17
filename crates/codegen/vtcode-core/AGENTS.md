# vtcode-core

[Root AGENTS.md](../AGENTS.md) | Largest crate (~70 modules). Agent loop, tools, LLM, config, safety, UI.

## Key Modules

`core/agent/` runtime (+ `progress_monitor.rs`: durable goal-progress via injected `ProgressLedgerSink`; + `context_reset.rs`: context reset decision logic distinct from compaction) | `llm/` **thin re-export layer** + `models_manager/` + `factory.rs` + `cgp.rs` + `rl/` (adaptive action selection: `signal`/`ledger`/`engine`/`eval`) | `tools/` + `tool_policy.rs` registry | `safety/` + `sandboxing/` + `exec_policy/` + `command_safety/` policies | `config/` + `constants.rs` | `context/` + `memory/` conversation | `prompts/` | `exec/events.rs` (re-exports `vtcode-exec-events::ThreadEvent`) | `git/` worktree management | `loop_memory.rs` + `loop_state.rs` loop persistence | `tools/web_search/` | `tools/defuddle/` | `tools/outline_search/` | `compaction/` unified auto+manual orchestrator + shared memory envelope (single source of truth for both runloops) | `core/agent/harness_artifacts.rs` (+ feature list artifact for evaluator-driven replanning)

## Rules

- Re-export from `lib.rs`. Consumers must not reach into submodules.
- `ThreadEvent` lives in `vtcode-exec-events` — never duplicate.
- `exec_policy` (Codex policy) != `command_safety` (tree-sitter validation) — do not merge.
- Constants in `config::constants`, not inline.
- Feature gates at module level, not scattered `#[cfg]`.

## Adding a Tool

Implement in `tools/` (web_search, defuddle, outline_search are reference patterns) → register in `tools::registry` → name in `tools::names` → classify in `ToolPolicy` → wire in `core/agent/`.

## Adding an LLM Provider

Implement in `crates/codegen/vtcode-llm/src/providers/` (the canonical home). Use `adding-llm-providers` skill. Update `ModelId::all_models()` + `builtin_model_presets()`. Then add a re-export in `crates/codegen/vtcode-core/src/llm/providers/mod.rs`.

## Gotchas

- `retry.rs` re-exports `vtcode_commons::retry::RetryPolicy`; domain methods (typed downcasts, `run_with_retry`) live on the `RetryPolicyCoreExt` extension trait — import it for method syntax.
- Error classification is `vtcode_commons::classify_anyhow_error` → `ErrorCategory`; `UnifiedErrorKind`/`ToolErrorType` are derived views and `ToolExecutionError.error_type` always derives from `category`.
- `lib.rs` is 500+ lines — append re-exports, don't restructure.
- `#[cfg_attr(not(test), allow(...))]` clippy suppressions — do not remove.
- Provider implementations live in `crates/codegen/vtcode-llm/src/providers/`, not in core. Core's `llm/providers/` is a re-export facade.
- `llm/usage_cost.rs` is the canonical session-cost normalization: `raw_usd` for budget enforcement, `effective_usd` (cache-discounted) for display. Do not compute costs inline from `Usage`. `BudgetStatus::classify` is the single budget decision (used by both the runner `execute.rs` and binary `turn_loop.rs`) — do not re-derive `> max` / `>= threshold*max` inline.
- `llm/request_gap.rs::RequestGapTracker` (+ `format_gap`) is the single home for cache-gap timing, embedded in both runloop `SessionStats` and headless `AgentSessionState` — do not re-add per-site `last_request_at` timers.
- `context_reset.rs` is distinct from compaction: compaction preserves conversational continuity; context reset discards history. `should_reset()` is pure logic; `maybe_write_reset_*` writes the manifest. The runloop wires it via `summarize.rs` (on_compaction) and `continuation.rs` (on_stall).
- Token efficiency is a correctness concern. Tool-catalog deferral lives in `tools/handlers/session_tool_catalog.rs`: MCP tools defer whenever present (any count), a schema-token budget backstops large single-server schemas, and subagents get a lightweight profile (`system_prompt_mode=minimal`, `tool_documentation_mode=minimal`, no inherited MCP servers) in `subagents/config.rs`. First-request budget is guard-railed by tests in `tools/registry/builtins.rs` (Progressive schema ≤ 3k tokens, builtin count ≤ 14), `tools/handlers/session_tool_catalog.rs` (MCP deferral keeps the wire payload within 25% of the no-MCP baseline; `catalog_would_benefit_from_deferral` is the pure warning condition), and `subagents/config.rs` (Minimal-mode composed bootstrap prompt ≤ 80% of parent Default). Advisory warnings: system-prompt-over-budget is in `prompts/system.rs::apply_token_budget` (gated on `system_prompt_budget_warning`); disabled-deferral-but-beneficial is a one-shot `tracing::warn!` in `model_tools` (gated by a `OnceLock`), covering the tool side. Do NOT add per-request warnings for the count/schema thresholds *triggering* deferral when enabled — that is correct behavior, not a warning condition.
