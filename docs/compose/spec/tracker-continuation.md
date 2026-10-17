---
feature: tracker-continuation
status: delivered
updated: 2026-09-18
branch: fix/tracker-continuation
commits: 873c2c8d1..00118e99b
---

# Tracker Continuation (Run Loop + TODO Resume)

## Report

**What was built** — VT Code now auto-continues when `task_tracker` still has incomplete TODO steps instead of ending
the turn and nudging the user to resume. Three shipped surfaces share one contract:

1. **In-turn** (`response_handling.rs` + `continuation.rs`): status-only assistant text (including “blocked by turn
   budget” / “next step on resume” recaps) is forced to continue when tracker steps remain, unless the text asks a
   genuine user question, hits a hard permission/policy handoff, or planning is active.
2. **Cross-turn** (`orchestration.rs`): after a Completed or recoverable Blocked turn (recovery fallback, text-cap
   safety cap, post-tool retry, preview/turn/wall-clock budget), the session loop queues the next tracker implementation
   turn, bounded by `[agent.harness.continuation].cross_turn_turns` (default 8). Queue-first then record budget;
   queue-full falls through to the blocked handoff. Verification-blocked turns keep their own recovery path.
   Tracker-complete or genuine user input resets the budget.
3. **Resume** (`orchestration.rs`): sessions restored with incomplete tracker steps inject remaining-step context and
   auto-queue one continuation turn when `cross_turn_turns > 0` and no pending user prompt is already queued.

AgentRunner parity (`execute.rs`) force-continues on text-only status responses only when `auto_continue_tracker` is on,
the idle-turn limit has not tripped, the text does not ask a question, and `assess_completion` reports incomplete
tracker work (`tracker_status_force_continue_eligible`). Compiled runtime guidance (`runtime_guidance.rs`) tells the
model not to end the turn asking the user to resume while tracker work remains. Kill-switch:
`[agent.harness.continuation].auto_continue_tracker = false`.

**Verification** — commands run and observed results:

- `cargo nextest run -p vtcode-config -E 'test(tracker) or test(continuation)'` — PASS (3)
- `cargo nextest run -p vtcode-core -E 'test(tracker_status) or test(runtime_guidance) or test(assess_completion) or test(exec_full_auto)'`
  — PASS (8; 1 PRE-EXISTING leaky guidelines test, not a fail)
<!-- markdownlint-disable-next-line MD013 -->
- `cargo nextest run -p vtcode -E 'test(tracker_) or test(outer_queue) or test(resume_gate) or test(parse_incomplete) or test(session_stats_resets) or test(recoverable_block) or test(tracker_incomplete)'`
  — PASS (acceptance coverage for pure gates, production-string recoverable classifier, override, budget reset)
- `./scripts/check-dev.sh` — PASS (fmt, clippy `-D warnings`, compile, shell lint)

**Journey log** —

1. Session log `session-vtcode-20260918T030054Z_141498-26410` showed repeated `## Status` / “Next step on resume” recaps
   then `turn.completed` — in-turn continuation classified those as conclusive/blocker even with incomplete tracker
   work.
2. Binary runloop already auto-queued only verification-blocked turns; TODO/tracker work had no outer auto-continue
   path.
3. First review: missing T3–T5 tests, budget reset gaps, AgentRunner kill-switch bypass, resume ignored
   `cross_turn_turns=0`, queue-full burned budget, substring classifier matched too broadly.
4. Second review: classifier still missed **production** blocked-reason constants (`COMPLETED_TURN_FALLBACK_REASON`,
   `ASSISTANT_TEXT_RESPONSE_CAP_REASON`, `POST_TOOL_TOOL_ENABLED_RETRY_FAILED_REASON`) — fixed by matching those
   literals + deny-list for verification/context-capacity/contract-violation; added `CONFIG_FIELD_REFERENCE.md` rows.
5. Pre-existing clippy/doc issues in `vtcode-ui/session/mod.rs` and `tool_output/streams.rs` blocked `-D warnings`;
   fixed only to clear the verify gate (no behavior change).

## [S1] Problem

VT Code ends turns and nudges the user to resume even when `task_tracker` still has incomplete TODO steps. Evidence from
`.vtcode/sessions/session-vtcode-20260918T030054Z_141498-26410/events.jsonl`: the agent repeatedly emits `## Status`
recaps (“blocked by turn budget”, “Next step on resume: …”) and then `turn.completed`, requiring the user to type
continue after every budget/recovery boundary. TODO tasks therefore do not continue seamlessly across turns or across
session resume.

## [S2] Design

Decision (user-confirmed):

- Auto-continue whenever `task_tracker` has incomplete items and no genuine user-input/permission need — in
  Build/Auto/full-auto **and** interactive TUI.
- On resume with incomplete tracker items, auto-queue one continuation turn after injecting remaining-step context.
- After budget/recovery turn ends with tracker work remaining, auto-queue the next turn when the blocker is recoverable
  (budget/preview/tool-free recovery), not user-input or permission.

### Contracts

**Tracker incomplete helper.** Live load + pure parse in `src/agent/runloop/unified/turn/tool_outcomes/helpers.rs`:

```rust
pub(crate) fn parse_incomplete_tracker_items(payload: &serde_json::Value) -> Option<Vec<String>>
pub(crate) async fn incomplete_tracker_items(tool_registry: &ToolRegistry) -> Option<Vec<String>>
```

Returns `None` when tracker is empty/absent; `Some(items)` with incomplete step descriptions otherwise. Cap display at 4
items.

**In-turn continuation override.** After `evaluate_interim_text_continuation`, apply tracker-aware override when tracker
incomplete:

- Still end turn when: planning active; text asks for user input (`?` / request-user-input phrases); empty response;
  hard permission/policy handoff.
- Otherwise force `should_continue = true`, reason `tracker_incomplete_continuation`, not relaxed.
- Status phrases like “blocked by turn budget” / “next step on resume” are **not** terminal when tracker incomplete.
- Tool-free recovery stays terminal _this turn_; outer loop schedules the next tracker turn.

**Outer session auto-queue.** Pure gate `should_queue_tracker_auto_continue` + production-string recoverable classifier
`tracker_auto_continue_is_recoverable_block`. Queue-first then record budget. Budget reset on tracker-complete and via
`reset_verification_recovery_episode` (genuine user input / completed episodes).

**Resume auto-queue.** Pure gate `should_queue_tracker_resume_continuation` honors `cross_turn_turns == 0`.

**Prompt guidance (shipped surface).** Compiled `runtime_guidance.rs` line + presence/budget assertion.

**AgentRunner parity.** `tracker_status_force_continue_eligible` honors kill-switch, idle-limit exception, user
questions, and incomplete-tracker reason filter only.

### Config

```toml
[agent.harness.continuation]
auto_continue_tracker = true   # default
cross_turn_turns = 8           # default; 0 disables cross-turn tracker auto-queue
```

Documented in `docs/config/CONFIG_FIELD_REFERENCE.md` and `docs/guides/agent-loop-contract.md`.

## [S3] Out of Scope

- Changing tool budgets, preview budgets, or permission policy semantics.
- Auto-approving tools outside existing allow-lists.
- Redesigning planning workflow (planning remains terminal for continuation).
- Infinite auto-run when tracker never progresses — bounded by `cross_turn_turns`, verification escalation, hard
  turn/session caps, and user Esc/interrupt.
- Fixing unrelated TODO.md owner notes.

## Tasks

- [x] T1: Add `AgentHarnessConfig.continuation` knobs — acceptance: defaults `auto_continue_tracker=true`,
      `cross_turn_turns=8`; parse tests in vtcode-config (covers: S2)
- [x] T2: Implement `incomplete_tracker_items` + `apply_tracker_continuation_override` with unit tests — acceptance:
      pure tests cover terminal user-input vs tracker-incomplete continue; parse helper tested (covers: S2; depends: T1)
- [x] T3: Wire override into binary response handling + outer auto-queue for Completed/Blocked recoverable ends —
      acceptance: pure gate tests + production-string recoverable classifier; no queue when planner active / user asked
      / verification block / unknown reason (covers: S2; depends: T2)
- [x] T4: Resume auto-queue when tracker incomplete — acceptance: `should_queue_tracker_resume_continuation` covers
      inject+queue eligibility including `cross_turn_turns=0` (covers: S2; depends: T2)
- [x] T5: AgentRunner forced continuation on incomplete tracker for text-only status — acceptance:
      `tracker_status_force_continue_eligible` unit tests (kill-switch, idle, question, incomplete-only) (covers: S2;
      depends: T2)
- [x] T6: Update runtime_guidance + docs (agent-loop-contract + CONFIG_FIELD_REFERENCE) — acceptance: presence test
      asserts new guidance; docs mention tracker auto-continue bounds (covers: S2; depends: T3)
