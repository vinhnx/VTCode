---
feature: plan-mode-entry-handoff
status: delivered
updated: 2026-09-27
branch: fix/plan-mode-entry-handoff
commits: a34d5bfca..55267a058
---

# Plan Mode Entry Handoff (Seamless Build → Plan Switch)

## Report

**What was built** — Mid-turn `start_planning` no longer ends the entry turn. After the user confirms planning, the
header badge switches to Plan immediately, mutating tools stay blocked, and the model continues researching in the same
turn. The full `plan` primary-agent switch is deferred via
`PlanningWorkflowSessionState::plan_entry_agent_switch_pending` and applied only after natural turn completion (and only
when no stronger approved-plan handoff owns the boundary). Plan approval still hands off to a write-capable Build/Auto
agent through the typed `PlanExecutionTarget` path. All mode-switch paths log under `vtcode.planning_workflow` with a
`switch_path` field (`plan_entry`, `plan_approval`, `plan_exit`, `startup_plan_entry`); selection/sync failures stay
recoverable instead of aborting the session. Recoverable `Blocked` ends (recovery fallback, blocked-tool fuse, no
published final) auto-continue in every mode — including build/auto without tracker items — via one shared
`RECOVERABLE_BLOCK_ALLOW_TOKENS` list.

**Verification** — commands run and observed results:

- `cargo check --locked -p vtcode` — PASS
- `cargo nextest run -p vtcode` planning/plan/mode filters — 362 passed
- Targeted deferred-switch / restore / approval handoff unit tests (10–13) — PASS
- Tracker/plan/handoff suites after all-modes blocked recovery — 50–92 passed
- `cargo fmt --all` — applied
- PRE-EXISTING: `cli_harness_failures print_mode_requires_prompt_or_stdin` (auth env; fails on base `a34d5bfca` too)
- Independent review + focused re-review of criticals — PASS after fixes

**Journey log** —

1. `ToolPipelineOutcome.pending_primary_agent` becomes `TurnHandlerOutcome::SwitchPrimaryAgent` immediately
   (`execution_result.rs`) and **breaks the turn**. Never use it for mid-turn plan entry.
2. Session evidence: stuck session `…T030348Z` had 13 events ending at `start_planning`; working session `…T023955Z`
   continued research in the same turn. Commit `a34d5bfca` introduced the break.
3. Take the deferred flag unconditionally at turn end even when discarding it; otherwise a stale plan switch can fire
   mid-implementation.
4. `harness_try!` in orchestration is a hard abort (`finish_after_unexpected_exit`). Mode-switch failures must `match` +
   log + `continue`.
5. tracing field named `display` collides with `tracing::field::display` — do not log `%display`. Session scan showed
   recovery-fallback/fuse blocked ends parking at `Continue…` in every mode; one shared allow-list prevents classifier
   drift.

## [S1] Problem

When an execution agent (Build/Auto) calls `start_planning` and the user confirms **Enter Planning workflow**, the
session shows the Plan header and the "Drafting plan — researching codebase" indicator, then **stops**. No research
tools run, no `<proposed_plan>` is drafted, and the session waits at `Continue…`. Plan mode is stuck.

Session evidence (`session-vtcode-20260927T030348Z_353055-84925`, 13 events):

1. `start_planning` tool succeeds with `status: success` and planning instructions.
2. `turn.completed` fires immediately after the tool result.
3. No step-2 research calls. The earlier working session (`session-vtcode-20260927T023955Z_014451-52652`) continued
   `exec_command` / `code_search` in the **same** turn after `start_planning`.

Commit `a34d5bfca` (Reapply "Merge branch 'fix/plan-mode-header-handoff'") introduced the regression:

- `handle_start_planning` sets `ToolPipelineOutcome.pending_primary_agent = "plan"`.
- `execution_result.rs` converts any `pending_primary_agent` into `TurnHandlerOutcome::SwitchPrimaryAgent`, which
  **breaks the turn**.
- Research never starts, despite docs stating "read-only research begins" and "the plan primary agent is selected
  **after the turn**".

The approval return path (plan → Build/Auto) is a separate handoff and is not this bug; it must keep working after the
fix.

## [S2] Design

### A. Plan-entry contract (mid-turn `start_planning`)

Entering planning from Build/Auto must:

1. Enable the planning workflow and block mutating tools (existing `transition_to_planning_workflow`).
2. Refresh the session header badge to Plan **immediately** (`apply_plan_agent_header`).
3. **Continue the current turn** so the model can research and draft `<proposed_plan>`. Do **not** emit
   `SwitchPrimaryAgent` / end the turn.
4. Queue a **deferred** full switch to the `plan` primary agent for **after the turn naturally completes** (prompt/tools
   match Plan for subsequent planning turns).
5. On plan approval, hand off to a write-capable Build/Auto agent through the existing typed `PlanExecutionTarget` path
   and return to implementation mode.

### B. Deferred switch mechanics

- `PlanningWorkflowSessionState` gains `plan_entry_agent_switch_pending: bool` (cleared on `enter`/`exit`/take).
- `enter_planning_workflow_after_start` sets the flag and updates the header; it must **not** set
  `ToolPipelineOutcome.pending_primary_agent`.
- After final-response validation in `turn_loop` (so a normal completed planning turn still requires a published final),
  take the flag **unconditionally**; set `TurnLoopOutcome.pending_primary_agent = Some("plan")` only when the turn is
  `Completed` and no stronger handoff is already present (`SwitchPrimaryAgentWithPolicy` / approved-plan).
- Orchestration's existing `is_plan_entry_handoff` branch selects the plan agent (not the write-capable resolver) and
  must not set `plan_approved_execution_pending`.

### C. Mode-switch error handling and logging

Every plan-entry / plan-exit / approval handoff path must:

1. Catch selection/sync failures without panicking or silently dropping the session into a half-switched state.
2. Log with `tracing` at `warn!`/`error!` including: `switch_path` (`plan_entry` / `plan_approval` / `plan_exit` /
   `startup_plan_entry`), requested agent, resolved agent (when known), and the error.
3. On plan-entry switch failure: keep Planning workflow + Plan header, continue the turn (research still allowed under
   the read-only gate), and surface a single warning line. Do not abort the turn.
4. On plan-approval switch failure (including sibling orchestration sites): keep the approved plan, surface an error
   line, and leave the session able to retry approval / `/mode build`. Never `harness_try!` / `SessionEndReason::Error`.
5. Emit `tracing::info!(target: "vtcode.planning_workflow", ...)` on successful switches (entry deferred-switch,
   approval, `/plan off` restore, startup).

### C2. Entry-turn `Blocked` recovery (plan mode)

A plan-entry or planning research turn that ends `Blocked` must not park at the user `Continue…` prompt when the block
is recoverable. `plan_mode_recoverable_block` must treat these production shapes as recoverable so plan-mode
auto-continue queues the next research turn:

- blocked-tool fuse trips (`blocked tool-call limit`, `recovery tool-call limit`, `consecutive blocked calls`,
  `tool-call safety limit`) — common after mid-turn `start_planning` when a leftover mutating intent hits the read-only
  gate
- no published final (`without a harness-visible final assistant response`)

True handoffs stay denied (permission / user-input / interview / verification). The `start_planning` success message
must steer the remainder of the entry turn to read-only research so mutations do not burn the fuse.

### C3. Recoverable `Blocked` recovery (every mode)

Session evidence (`.vtcode/sessions`) shows the common stuck end outside plan mode is
`Turn ended with a recovery fallback; the requested work was not confirmed.` (plus blocked-tool fuse / no-final), which
parked at `Continue…` because `should_queue_tracker_auto_continue` required incomplete tracker items.

Contract:

- Recoverable **blocked** ends auto-continue in build/auto **without** tracker items (bounded by `cross_turn_turns`).
  Completed turns still require incomplete tracker work.
- One shared `RECOVERABLE_BLOCK_ALLOW_TOKENS` list feeds both the plan-mode and tracker classifiers (DRY). Deny lists
  stay per-mode because evaluation order differs (plan allows first so `PLANNING_COMPLETED_TURN_FALLBACK_REASON` is not
  shadowed by "approval-ready plan").
- When no tracker items remain, queue `recoverable_blocked_continue_follow_up` (internal-harness quiet prefix) instead
  of the tracker follow-up.
- Verification blocks, refusals, permission/user-input handoffs stay denied.

### D. Out of scope

- Plan validation, tracker distill, or approval policy changes.
- Build vs Auto authority rules.
- Header layout / badge styling.
- `/plan` slash-command entry (already performs a full select; leave unless a shared helper is extracted without
  behavior change).

## [S3] Out of Scope

See [S2] D. No new top-level harness subsystem. No `ThreadEvent` contract changes beyond existing plan-approval /
planning lifecycle events.

## Tasks

- [x] T1: Stop plan-entry from breaking the turn — remove `pending_primary_agent` from `start_planning` outcomes and add
      deferred `plan_entry_agent_switch_pending` — acceptance: after confirmed `start_planning`, the turn continues and
      can emit research tool calls (covers: S2 A,B)
- [x] T2: Apply deferred plan-agent switch after natural turn completion, without suppressing final-response validation
      — acceptance: a completed planning turn with a published final still validates the final, then orchestration
      selects `plan` (covers: S2 A,B)
- [x] T3: Harden mode-switch error handling and logging on entry/approval/exit/startup paths — acceptance: failed
      selection logs `switch_path`+error and leaves a recoverable session; success paths log `vtcode.planning_workflow`
      (covers: S2 C)
- [x] T4: Regression tests — deferred switch consume-once + enter/exit clearing + always-consume; plan-entry is not
      approved-plan execution — acceptance: `cargo nextest run -p vtcode` planning/mode filters pass (covers: S2 A–C)
- [x] T5: Update planning-workflow docs for deferred-switch + error contract — acceptance: docs state research continues
      in the entry turn and the plan agent is selected after the turn (covers: S2 A–C)
- [x] T6: Plan-mode auto-continue recovers entry-turn Blocked shapes (fuse trips, no-final) and `start_planning` steers
      read-only research — acceptance: `plan_mode_auto_continue_recovers_entry_turn_blocked_shapes` passes and
      permission handoffs still never auto-queue (covers: S2 C2)
- [x] T7: Recoverable blocked ends auto-continue in every mode without tracker items; generic blocked-end follow-up when
      tracker is empty — acceptance: `recoverable_blocked_continues_without_tracker_items` passes and Completed turns
      still require incomplete tracker work (covers: S2 C3)
