# vtcode (binary)

[Root AGENTS.md](../AGENTS.md) | CLI entrypoint, session bootstrap, agent runloop wiring.

## Modules

`main.rs` binary entry | `agent/` runloop + subagent dispatch | `cli/` CLI handlers | `startup/` first-run + onboarding | `updater/` self-update | `codex_app_server/` app server bridge | `main_helpers/` tracing + runtime init

## Rules

- Thin binary — all runtime logic in `vtcode-core`. This crate wires CLI args to `Agent::run()`.
- Global allocator is `mimalloc` by default; `allocator-jemalloc` opts into
  `tikv-jemalloc` (background-thread purging, recommended for Linux servers).
  Selected in `src/allocator.rs` (a `mod allocator;` in `src/main.rs`). See
  `docs/development/ALLOCATOR_MEMORY.md`.
- `vtcode bench-allocator` measures RSS under a bursty/sparse Tokio workload to detect
  allocator memory pinning; use it before changing the default allocator (see Gotchas).
- `vtcode_ui::tui::panic_hook` installs custom panic handler — must run before any output.
- `agent/runloop/` contains the single-agent runloop. Multi-agent is in `vtcode-core::subagents`.
- `agent/runloop/unified/turn/compaction/` is a **thin delegator** — all compaction logic (auto/manual orchestration, memory envelope, dedup, thresholds) lives in `vtcode-core::compaction`. Do not re-implement compaction here; call the shared orchestrator.

## Gotchas

- `main_helpers` handles runtime relaunch context — do not duplicate init logic in `main.rs`.
- Allocator memory pinning: vtcode's bursty/sparse Tokio workload (semaphore-capped
  `JoinSet` fans-out, then workers idle) makes both `mimalloc` and `glibc` hold RSS flat
  after a burst (frees stranded on cross-thread lists, only reconciled by future allocation
  activity). `jemalloc` only returns memory on idle when its `background_thread` is active —
  on macOS the jemalloc build prints `background_thread currently supports pthread only` and
  behaves like mimalloc, so switching the allocator does NOT help on macOS dev. On Linux
  containers (the article's target) `background_thread` works and jemalloc reclaims memory.
  Measure with `vtcode bench-allocator` before changing the default; do not switch blindly.
- `load_dotenv()` must run before config load to pick up `.env` API keys.
- Provider noise (e.g. MiniMax `]<]minimax[>[`) is stripped centrally in `turn::provider_noise`. Stream-level sanitization (harmony + minimax) lives in `stream_sanitization::StreamSanitizer`. Do not re-implement noise stripping inline.
- Wall-clock budget exhaustion (`run_loop_context::record_wall_clock_exhaustion_notice`) emits the full policy message once, a compact stub for later calls in the batch, then a single "synthesize now" directive via `flush_wall_clock_directive` (called after the tool batch so it is never interleaved between tool responses). Do not push the directive inline in `validate_tool_call`. The flush also arms `switch_to_tool_free_recovery()` so the next request strips tool definitions at the API level — the directive alone is advisory and models kept emitting rejected tool calls after it (turn_637/turn_647).
- `turn_processing/llm_request/` is split into contract-carrying submodules (`snapshot`, `tool_shaping`, `context_management`, `response_chain`, `prompt_assembly`) with `pub(super)` visibility; go through `mod.rs`'s exports. Prompt section order in `prompt_assembly::build_prompt_output` is prompt-cache-sensitive — reordering sections invalidates provider caches. `tool_shaping.rs` carries the wire invariant: hosted Anthropic/OpenAI payloads keep full deferred tool definitions; only the ClientLocal policy omits them (pinned by tests in `request_builder.rs`).
- Plan-mode recovery contract: during the tool-free recovery pass (`tool_free_recovery`, tools disabled) do NOT inject a `request_user_input` interview call — it trips the recovery contract guard and dead-ends the turn. In `turn_loop.rs`, the interview synthesis/forcing block is gated on `!tool_free_recovery`, and `post_tool_recovery::complete_turn_after_failed_tool_free_recovery` is plan-aware: it marks `plan_session` `interview_pending` and emits `PLANNING_RECOVERY_SYNTHESIS_FALLBACK` so the next turn re-forces the interview instead of losing planning state. EXCEPTION: if `plan_session.is_budget_exhausted()` OR `is_recovery_exhausted()` (post-tool recovery cycle cap reached, or the turn's tool wall-clock budget was exhausted — `wall_clock_exhausted_emitted` is checked in `dispatch_post_tool_failure` and before `normalize_tool_free_recovery_break_outcome` — saturated planning context), it concludes with the USER-facing `PLANNING_BUDGET_EXHAUSTED_USER_NOTICE` / `PLANNING_RECOVERY_EXHAUSTED_USER_NOTICE` plus the `implement`/`keep planning` confirmation hint, and does NOT re-force the interview — re-researching the still-huge context would loop forever across turns. NEVER push a `*_FINALIZE` model directive as the final answer in this path: no LLM call follows it, so the user sees a bare instruction and the turn dead-ends (turn_655). The interview re-forcing guards in `planning_workflow.rs` and `interview_forcing.rs` honor both flags.
- Plan output MUST stay compact/spec-like (root cause of the old "plan cut off mid-flight → re-summarize loop" bug). `PLANNING_WORKFLOW_PLAN_QUALITY_LINE` (prompts/system.rs) mandates a `<proposed_plan>` that fits ~1500 tokens with `Action -> files/symbols -> verify:` steps and file:symbol refs over prose; `docs/guides/planning-workflow.md` is the canonical format. NEVER widen this instruction back to "summary, steps, test cases, assumptions" — a verbose plan exceeds the model's output-token budget and is truncated mid-plan. The turn loop (`turn_loop::planning_workflow_recovery`) detects a truncated planning synthesis (`plan_synthesis_was_truncated`: `finish_reason == Length`, no tool calls, unclosed `<proposed_plan>`) and re-prompts once for a compact spec (`PLANNING_SYNTHESIS_TRUNCATED_CONDENSE_DIRECTIVE`, bounded by `MAX_PLAN_SYNTHESIS_CONDENSE_ATTEMPTS`) instead of looping.
- Plan-approval handoff invariant: when the user approves by typing `approve`/`approved`/`lgtm`/`looks good`/`ship it`/etc., `detect_planning_intent` (`turn/planning_intent.rs`) must classify it as `ExitAndImplement` (it previously missed `approve`, so the model self-approved by mutating the plan file and stayed in plan mode). `maybe_handle_planning_exit_trigger` then runs `finish_planning` via `run_tool_call`; that outcome's `pending_primary_agent` (set by `handle_pending_confirmation` for `SwitchBuild`/`SwitchAuto`) MUST be propagated back to the turn outcome — `maybe_handle_planning_exit_trigger` now takes a `&mut Option<String>` out-param for this. If you ever change that function to return only a bool again, the build/auto agent switch silently breaks and the session stays in plan mode.
- Tool-summary rendering (`agent/runloop/unified/tool_summary*.rs`) takes ambient context (workspace root for relative-path display) via the `ToolSummaryRenderContext` guard-rail struct, not a bare `Option<&Path>` threaded through every `describe_*`/`collect_*` helper. Pass `&ToolSummaryRenderContext` at the `render_*` entry points; the pure helpers stay `Option<&Path>`-driven internally.
