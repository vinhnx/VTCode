implement claude advisor tool

--

when switching a different model mid-turn of session, trigger compaction of the current context, so that the new model does not see the old model's context. This is important because different models have different capabilities and may interpret the same context differently. By compacting the context, we ensure that the new model starts with a clean slate and can focus on the relevant information for its own reasoning.

plan:

````
# Plan: Compact context on mid-session model switch

## Context

Switching models mid-session currently **only warns** that performance may degrade; the new model still receives the full prior conversation (tool traces, assistant turns, provider-specific structure):

```144:148:src/agent/runloop/unified/model_selection.rs
    if conversation_history_len > 0 {
        renderer.line(
            MessageStyle::Warning,
            "Changing model mid-conversation may degrade performance due to context loss and token inefficiency. For best results, start a new conversation with /clear.",
        )?;
````

That matches the open item in `docs/project/TODO.md`. Compaction already exists end-to-end (`manual_compact_history_in_place`, memory envelope, `thread.compact_boundary` events). We should **automatically compact** when the **main session model/provider actually changes** and history is non-empty, so the next turn starts from a summary + retained high-signal context rather than the old model’s raw trace.

**Scope note:** Model selection runs in the **idle interaction loop** (between turns: `/model`, palette, text picker), not inside an in-flight tool batch. “Mid-turn of session” here means **mid-session / mid-conversation**, not mid–LLM-stream.

## Recommended approach

### Behavior

1. On successful main-model apply via `finalize_model_selection`:
    - If **provider+model** are unchanged relative to the pre-switch session (e.g. only reasoning/service tier changed): **do not** compact; keep existing messages.
    - If provider and/or model **changed** and `conversation_history` is non-empty: run **forced** compaction (same strategy dispatch as `/compact`: native standalone / native inline / local), with **`always_summarize: true`** so short histories are not skipped.
2. After switch (whether compact succeeds or not): clear **all** previous-response chains and rotate prompt-cache lineage so the new model/provider is not chained to the old Responses/cache identity.
3. Replace the mid-conversation **warning + “use /clear”** copy with an **info** line that compaction ran (or was skipped / failed with keep-history).
4. Compaction failure must **not** roll back the model switch; surface error and keep uncompacted history (same posture as failed `/compact`).

### Which model performs the summary call

Use the **new** provider/client after it is installed (same as manual `/compact`). The compact API call may still _read_ prior history once; the important guarantee is that **subsequent session turns** only see the compacted history. Optionally later: compact with the outgoing client before swap for better continuity—out of scope unless we hit quality issues.

### Trigger taxonomy

Add `CompactionTrigger::ModelSwitch` in `vtcode-exec-events` (and wire `as_str`, hooks, harness `compact_boundary_event`) so telemetry distinguishes model-switch compaction from Manual/Auto/Recovery.

### API shape

Introduce a small optional compaction bundle so call sites that already own history can pass it without bloating every intermediate struct:

```rust
// conceptual — place next to finalize_model_selection
pub(crate) struct ModelSwitchCompactionTargets<'a> {
    history: &'a mut Vec<Message>,
    session_stats: &'a mut SessionStats,
    context_manager: &'a mut ContextManager,
    session_id: &'a str,
    thread_id: &'a str,
    lifecycle_hooks: Option<&'a LifecycleHookEngine>,
    harness_emitter: Option<&'a HarnessEventEmitter>,
}
```

`finalize_model_selection(...)` gains:

- Pre-switch snapshot: `previous_provider: &str`, `previous_model: &str` (from `config` **before** mutation).
- `compaction: Option<ModelSwitchCompactionTargets<'_>>`.

When `compaction` is `None`, keep today’s apply-only behavior (defensive for tests / incomplete callers). Production call sites must pass `Some`.

Internal helper (same crate as turn compaction):

```rust
pub(crate) async fn compact_history_on_model_switch_in_place(
    context: CompactionContext<'_>,
    state: CompactionState<'_>,
) -> Result<Option<CompactionOutcome>>
```

- Reuse `manual_compact_history_in_place` path but force `always_summarize` via config (or a thin wrapper around `compact_history_manual` + `apply_compacted_history` with `trigger: ModelSwitch` and envelope placement `Start` / persist-to-disk, matching manual).
- Skip no-op when compacted == input.

### Call-site plumbing

| Call site                                           | Change                                                                                                                                                                                                                                              |
| --------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `interaction_loop_runner.rs` (text picker complete) | Pass full compaction targets (history, stats, context_manager, session/thread ids, hooks, emitter).                                                                                                                                                 |
| `slash_commands/ui.rs` (`/model`)                   | Same via `SlashCommandContext` (already has all fields).                                                                                                                                                                                            |
| `inline_events/modal.rs` (list picker complete)     | Today only has `conversation_history_len`. Extend `InlineEventLoopResources` / modal coordinator with **mutable** history + stats + context_manager + ids + hooks/emitter from `InteractionLoopContext`, then pass into `finalize_model_selection`. |

Avoid duplicating compact logic in three places—only `finalize_model_selection` after successful client apply.

### Skip conditions

- History empty.
- `previous_model` equals `selection.model` **and** provider key equals (case-insensitive provider string compare matching existing config).
- Lightweight model picker (`finalize_lightweight_model_selection`) — **out of scope** (does not change main session model).
- Primary-agent Tab switch — **out of scope** unless that path also mutates `config.model` (today it does not via `finalize_model_selection`).

### UX messages

- Success: e.g. `Compacted conversation for model switch (N → M messages, local|provider compaction).`
- No-op compact: brief note that history was already compact / too small after policy.
- Failure: `Model switched, but context compaction failed: … Continuing with full history.`

Remove or rewrite the old “start a new conversation with /clear” warning so it does not contradict auto-compact.

### Docs

- User-facing: short note on `/model` / model palette in `docs/user-guide/` (commands or interactive mode): mid-session model change auto-compacts context.
- Optional row in agent-loop / compaction docs for `CompactionTrigger::ModelSwitch`.
- Remove or rewrite the matching bullet in `docs/project/TODO.md` once done.

### Config

No new config flag in v1 (always on when model/provider changes and history non-empty). Optional follow-up: `agent.harness.compact_on_model_switch` if users need opt-out.

## Critical files

| File                                                                | Role                                                  |
| ------------------------------------------------------------------- | ----------------------------------------------------- |
| `src/agent/runloop/unified/model_selection.rs`                      | Hook after apply; previous model/provider compare; UX |
| `src/agent/runloop/unified/turn/compaction/mod.rs`                  | `compact_history_on_model_switch_in_place`            |
| `vtcode-exec-events/src/lib.rs`                                     | `CompactionTrigger::ModelSwitch`                      |
| `src/agent/runloop/unified/inline_events/{driver,context,modal}.rs` | Plumb mutable history/stats into picker completion    |
| `src/agent/runloop/unified/turn/session/interaction_loop_runner.rs` | Text-picker completion path                           |
| `src/agent/runloop/unified/turn/session/slash_commands/ui.rs`       | `/model` completion path                              |
| Hooks/harness that match on `CompactionTrigger`                     | Exhaustive match updates                              |
| Docs under `docs/user-guide/`, `docs/guides/agent-loop-contract.md` | Document trigger + behavior                           |

## Reuse (do not reinvent)

- `manual_compact_history_in_place` / `compact_history_manual` / `manual_compaction_strategy` — strategy dispatch.
- `local_compaction_config(vt_cfg, always_summarize)` — force summary on short histories.
- `apply_compacted_history` — memory envelope, response-chain clear for model, harness `compact_boundary_event`, token cap.
- `SessionStats::clear_previous_response_chain` — full clear on provider/model change (in addition to per-model clear inside apply).
- Existing `/compact` UI/stats patterns in `slash_commands/compact.rs`.

## Implementation steps

1. Add `CompactionTrigger::ModelSwitch` + tests for serde/`as_str`.
2. Add `compact_history_on_model_switch_in_place` in turn compaction module; unit-test with mock/dummy provider if patterns exist in `turn/compaction/tests.rs`.
3. Extend `finalize_model_selection` with pre-switch compare + optional compaction targets + messaging; clear full response chain + lineage on real switch even when history empty.
4. Plumb targets through interaction runner, slash `/model`, and inline modal resources.
5. Update docs and TODO.
6. Verify with `./scripts/check-dev.sh` and focused nextest on compaction + model selection if present.

## Verification

- **Unit:** model-switch compact applies summary when history > 0 and model differs; no compact when same model; empty history no-op; failure leaves history and still updates `config.model`.
- **Unit/integration:** `CompactionTrigger::ModelSwitch` serializes and appears in compact boundary event builders.
- **Manual / harness:** interactive session with multi-turn history → `/model` switch → observe compact info line and shorter next-turn context; same-model reselect does not compact.
- **Gate:** `./scripts/check-dev.sh` (and `--test` or nextest on `vtcode` / compaction modules as needed).

## Non-goals (v1)

- Compacting during an active streaming turn or tool batch interrupt.
- Auto-compact on primary-agent-only switches without model change.
- Lightweight-route model changes.
- ACP remote model switch (unless it already shares `finalize_model_selection`—it does not today).

```

--

context engineering: managing the context the model sees at each inference step, so that multi-turn tool use is not drowned by history, noise, tool definitions, and intermediate results.

===

as tasks evolved into long-horizon tasks, agent capability became a system capability composed of the model, harness, context, tools, evals, sandbox, and state management together: within a finite context, external tools, and a changing environment, the agent must keep pushing toward a goal and get closer to completion over time.

===

A concrete context engineering example is: when there are too many MCP tools and too much data, we should not let every tool definition and intermediate result flow through context. Instead, the agent should write code to call tools on demand, filter the data, and bring only high-signal results back to the model. Context engineering also includes compaction, structured note-taking, and even multi-agent patterns, but these mostly serve the question of “how should context be managed?” The long-running agent harness discussed later goes one step further: it does not only manage context, but separates planning, implementation, evaluation, revision, and final acceptance into different processes.

---

Context engineering

People used to focus on prompt engineering, namely how to write the system prompt. But as agents become more multi-turn and longer-running, the focus becomes: at each inference step, what did the model see? For example, an agent’s context may contain the system prompt, developer instructions, tool definitions, MCP server descriptions, user messages, conversation history, file contents, command output, browser observations, screenshots, errors, progress notes, retrieved documents, evaluator feedback. These things cannot all be blindly stuffed in. Even if the context window becomes larger, we still need to choose selectively, because noise in a long context distracts the model and increases cost and latency. So a good context is the minimal high-signal context that lets the model complete the next step at the current step. In general, we use just-in-time context: instead of stuffing all documents, tools, and history into the model, we keep references, such as paths, URLs, commit hashes, log files, database table names, and so on. When the model needs them, it loads them through tools. In this way, the agent’s working memory keeps indexes and currently relevant fragments, rather than all of the context.

Common context techniques in long tasks include:

    Compaction: When the context is almost full, compress the current conversation trace into a summary, then use that summary to start the next context. Usually this is still the continuation of the same task and the same agent loop. Its goal is to preserve conversational continuity as much as possible. In other words, the model should feel that “what I was just doing is still here”; only the history has been compressed.

    Structured note-taking: Let the agent actively write task state into persistent external artifacts, such as progress.md, feature_list.json, NOTES.md, or git commits. It is not necessarily only for the same task. A project-level NOTES.md can be read by later sessions, by planner/generator/evaluator, or reused in neighboring tasks.

    Context reset: Use external artifacts, such as files produced by note-taking, git logs, test results, task lists, and so on, as startup material to open a clean new context/session. It does not preserve the full conversation history, and can clear noise and bad assumptions so that a new agent can reorient itself.

    Multi-agent architecture: Do role division and context isolation. Different agents receive different context, do different subtasks, and finally pass only high-density results back to the main flow.

===

Tool use

There are also some things to watch out for in tool use. For example, the number of tools cannot expand without limit, otherwise the model wastes attention on choosing. Tools should not overlap too much in functionality, otherwise the model does not know which one to use. Tool descriptions should clearly explain when to use them and when not to use them, namely they should make the boundaries clear. Return values should be token-efficient and should not pour irrelevant data into context. Returned error messages should let the model know what to repair next. Names should be clear, so the agent can infer boundaries from the names. Also, tools are preferably not exposed directly to the model as direct tool calls, but mapped into code APIs / a file tree, so the agent can call them through code. This way, the agent can read only the tool definitions it currently needs, filter large data inside the code execution environment, and return only summaries or a small number of results to the model. It can use loops, conditionals, and exception handling to complete complex control flow, or write intermediate state into files instead of stuffing it into context. Tool outputs are deterministic; we should leave deterministic computation to code. Judgment and planning, such as how to call tools, which tools to call, in what order, and how many times, should be left to the model.

===

https://jinyansu1.github.io/blog/2026/07/agent-context-engineering-long-running-harness/

===

check current existing .logs/.checkpoint/.sessions infra, and see if they're redudant or can be unified. The goal is to have a single source of truth for the agent's state, context, and history, which can be efficiently queried and updated as the agent progresses through its tasks. This may involve consolidating logs, checkpoints, and session data into a more streamlined format that supports both real-time inference and long-term learning. It's also prevent overhead from accumulating in the context, ensuring that the runtime environment remains responsive and efficient.

===

IMPORTANT for vtcode's compaction engine. Compaction: When the context is almost full, compress the current conversation trace into a summary, then use that summary to start the next context. Usually this is still the continuation of the same task and the same agent loop. Its goal is to preserve conversational continuity as much as possible. In other words, the model should feel that “what I was just doing is still here”; only the history has been compressed.

===

IMPORTANT CRITICAL: review vtcode's tool systems. the number of tools cannot expand without limit, otherwise the model wastes attention on choosing.

Tool use

There are also some things to watch out for in tool use. For example, the number of tools cannot expand without limit, otherwise the model wastes attention on choosing. Tools should not overlap too much in functionality, otherwise the model does not know which one to use. Tool descriptions should clearly explain when to use them and when not to use them, namely they should make the boundaries clear. Return values should be token-efficient and should not pour irrelevant data into context. Returned error messages should let the model know what to repair next. Names should be clear, so the agent can infer boundaries from the names. Also, tools are preferably not exposed directly to the model as direct tool calls, but mapped into code APIs / a file tree, so the agent can call them through code. This way, the agent can read only the tool definitions it currently needs, filter large data inside the code execution environment, and return only summaries or a small number of results to the model. It can use loops, conditionals, and exception handling to complete complex control flow, or write intermediate state into files instead of stuffing it into context. Tool outputs are deterministic; we should leave deterministic computation to code. Judgment and planning, such as how to call tools, which tools to call, in what order, and how many times, should be left to the model.

===

Fix plan: tool-free recovery dies instead of retrying synthesis (checkpoint turn_621)

Root cause chain

1. Turn hit the 600s tool wall-clock budget mid-exploration; harness correctly switched to a tool-free recovery pass ([run_loop_context.rs#L154](file:///Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/src/agent/runloop/unified/run_loop_context.rs#L154)).
2. The model emitted its answer as text containing <tool_call>…</tool_call> markup. In [result_handler.rs#L204-L272](file:///Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/src/agent/runloop/unified/turn/turn_processing/result_handler.rs#L204-L272), the two cleanup attempts (strip_dsml_markup, strip_textual_tool_call_regions) failed, so it broke with RECOVERY_CONTRACT_VIOLATION_REASON — with no retry.
3. normalize_tool_free_recovery_break_outcome ([post_tool_recovery.rs#L145-L165](file:///Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/src/agent/runloop/unified/turn/turn_loop/post_tool_recovery.rs#L145-L165)) immediately converted that to the canned final answer "Recovery synthesis failed; no tool call applied…", discarding ~60 messages of gathered context.

Contrast: the Empty + native-tool-calls path in [turn_loop.rs#L862-L878](file:///Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/src/agent/runloop/unified/turn/turn_loop.rs#L862-L878) does retry up to MAX_RECOVERY_RETRIES (3) with a corrective directive. The textual-markup path and the ToolCalls-variant path ([result_handler.rs#L112-L118](file:///Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/src/agent/runloop/unified/turn/turn_processing/result_handler.rs#L112-L118)) get zero retries.

Contributing factors

- RECOVERY_SYNTHESIS_MAX_TOKENS = 1024 ([turn_loop.rs#L68](file:///Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/src/agent/runloop/unified/turn/turn_loop.rs#L68)) — far too small to synthesize a launch-time plan from a 60-message exploration; likely truncates and destabilizes the response.
- Three stacked, partly contradictory system messages injected before recovery (budget exhausted / synthesize now, POST_TOOL_RESUME_DIRECTIVE "follow tool guidance first", POST_TOOL_RECOVERY_REASON).
- The turn burned budget on 3 identical unified_search preflight validation failures (invalid content arg) with no corrective schema hint injected.

Changes

1. Retry instead of bail on recovery contract violation (primary fix)

- In result*handler.rs, both Blocked-with-RECOVERY_CONTRACT_VIOLATION_REASON sites: instead of breaking immediately, route through the same retry mechanism as the Empty path — if recovery_retry_count() < MAX_RECOVERY_RETRIES, push RECOVERY_TOOL_CALL_RETRY_DIRECTIVE, call retry_recovery_pass(), and continue the loop. Only after retries exhaust fall through to the fallback. This likely needs a new TurnHandlerOutcome/TurnLoopResult signal (e.g. Blocked reason checked in turn_loop.rs before normalize*… converts it) so the retry counter lives in one place.

2. Salvage a real answer in the fallback

- In complete_turn_after_failed_tool_free_recovery, before pushing the canned string: if the rejected response's markup-stripped text has non-trivial prose (even if markers remain), use that as the final answer with a [!] recovery note, rather than discarding it. Requires threading the last rejected text into the fallback (or storing it on the turn ctx).

3. Raise recovery synthesis token budget

- Bump RECOVERY_SYNTHESIS_MAX_TOKENS from 1024 to ~4096. Synthesizing a plan from a long exploration cannot fit in 1024.

4. Consolidate recovery system messages

- In prepare_post_tool_tool_free_recovery / budget-exhaustion path, emit one coherent directive instead of stacking POST_TOOL_RESUME_DIRECTIVE (which tells the model to follow tool-output guidance/next_action — wrong when tools are disabled) plus the recovery reason. When tool-free recovery is active, suppress POST_TOOL_RESUME_DIRECTIVE.

5. (Secondary) Preflight-failure schema hint

- After a repeated identical preflight validation failure for the same tool, append the tool's valid parameter list to the error payload so the model self-corrects instead of retrying blind. Scope: wherever the "Tool preflight validation failed" error is built.

Verification

- Unit tests in turn_loop/tests.rs / result_handler.rs tests: recovery pass returning textual tool-call markup retries up to MAX_RECOVERY_RETRIES, then falls back; fallback prefers salvaged prose over canned string; POST_TOOL_RESUME_DIRECTIVE absent when tool-free recovery active.
- ./scripts/check-dev.sh --test, plus cargo nextest run -p vtcode -E 'binary(/inline_events/)' for harness regressions.

Items 1–4 are one coherent change in src/agent/runloop/unified/turn/; item 5 is separable. Want me to implement 1–4 now?

===

amp fable stash : "WIP on main: 383c71516 fix(runloop): raise recovery synthesis token cap 1024 -> 4096"

===

https://github.com/vinhnx/VTCode/issues/698
```
