---
feature: plan-mode-tracker-continuity
status: delivered
updated: 2026-09-21
branch: feat/plan-mode-tracker-continuity
commits: 592c4f227..0fdad640e
---

# Plan Mode + Tracker Continuity (Dedupe, Scope/Task, Continuation, Progress)

## Report

**What was built** — Two related product gaps after `tracker-user-facing-ui` are closed:

1. **Single-writer tracker transcript (dedupe).** All user-facing tracker transcript writes go through
   `write_tracker_progress_transcript`, backed by session-scoped `transcript::REPLACEABLE_TRACKER_BLOCK` (approval
   handoff + pipeline share one store). Replace is **tail-safe**: only when the remembered block is still the transcript
   tail (`tracker_block_len_if_at_tail`) or the tail itself looks like a tracker progress line; otherwise the new block
   is appended and intervening lines are never clobbered. Identical repeats skip. The screenshot class of stacked TODO
   trees (approval write + pipeline update with changed counts) no longer stacks via the shared writer path;
   progress-only lines keep the surface small when an older block remains under intervening content.

2. **Plan-mode Scope / continuation / progress.** The `<proposed_plan>` template and compiled planning prompt request
   `## Scope` (In/Out). Scope is plan context only — structured distill takes only Implementation Steps/Steps, and the
   sparse scanner skips `## Scope` / non-tracker sections so numbered Scope lines never become checklist items. The
   runtime validator does **not** hard-require Scope (older plans stay valid). Plan-mode outer auto-continue uses
   `should_queue_plan_mode_auto_continue` + `plan_mode_recoverable_block`: only recoverable **blocked** planning ends
   (budget / safety-cap / tool-free recovery) queue another planning turn when no validated plan is approval-ready.
   Ordinary **Completed** planning turns are never auto-continued (interview/approval handoffs stay user-gated). Never
   auto-approves or auto-implements. User-facing planning status stays compact (`• Plan — research/synthesis` on blocked
   stalls; ready-for-approval when a completed turn has a validated plan). Queue-full / budget-exhausted planning
   continues emit explicit status lines.

**Verification** — commands run and observed results:

- `./scripts/check-dev.sh` — PASS (fmt, clippy `-D warnings`, compile, shell lint)
- `cargo nextest run -p vtcode-core -E 'test(plan_quality) or test(transcript_state) or test(tracker_block)'` — PASS (5)
<!-- markdownlint-disable-next-line MD013 -->
- `cargo nextest run -p vtcode -E 'test(tracker_) or test(plan_mode) or test(plan_progress) or test(write_tracker) or test(scope_section) or test(identical_task) or test(pipeline_replay) or test(successful_task) or test(clobber) or test(tail_tracker)'`
  — PASS (93)
- Independent reviews: first pass flagged unconditional `replace_last` (clobber/stack) and plan auto-continue on
  Completed turns (`recoverable_block(None)==true`); re-review confirmed both criticals fixed with tail-safe replace +
  blocked-only plan continue.

**Journey log** —

1. Screenshot `Screenshot 2026-09-17 at 13.45.47.png` showed two identical full TODO trees stacked — approval
   `append_pasted_message` did not share replace state with pipeline `apply_task_tracker_block`.
2. Tracker vs plan auto-continue are intentionally **asymmetric**: tracker may continue on Completed with incomplete
   items; plan-mode must not (interview/approval risk). Do not harmonize them.
3. `tracker_auto_continue_is_recoverable_block(None) == true` — hardcoding `turn_completed=false` does **not** restrict
   outer gates to blocked ends. Plan mode uses a dedicated allow-list classifier that treats `None` as non-recoverable.
4. Session-scoped replaceable transcript blocks must re-validate tail before `replace_last`; blind replace corrupts
   intervening lines.
5. Core `PlanningWorkflowState` has no interview/approval flags; safety rests on `plan_ready_for_approval`,
   `turn_completed==false`, and deny-list of planning handoff constants (`planning turn ended`, `approval-ready plan`).

## [S1] Problem

1. **Duplicated TODO/task transcript blocks.** Tracker transcript writes were not single-writer. Approval handoff used
   `append_pasted_message` without updating replace state; the pipeline appended when replace count was `None` and tail
   did not match. Slightly different progress text stacked a second block (screenshot evidence: two identical full
   trees).

2. **Plan-mode scope/task/continuation/progress.** The plan template lacked explicit Scope In/Out. Plan mode was
   terminal for tracker auto-continue, so budget/recovery ends nudged the user. User-facing plan progress was not
   aligned with the title+progress tracker contract.

## [S2] Design

Decision (user-confirmed): one package — dedupe + plan template Scope + plan-mode continuation + plan progress.

### A. Single-writer tracker transcript

```rust
pub(crate) fn write_tracker_progress_transcript(handle: &InlineHandle, lines: Vec<String>)
```

1. Tail already equals `lines` → remember and return.
2. Remembered block still at tail (`transcript::tracker_block_len_if_at_tail`) → `replace_last`.
3. Tail looks like tracker progress (`• Tasks…` / `• Title N/M`, not `• Plan`/`• Ran`) → replace that tail line.
4. Else append.
5. Always remember `lines` in session-scoped `transcript::REPLACEABLE_TRACKER_BLOCK`.

Call sites: `tool_output_handler` pipeline path and `planning_workflow/task_tracker.rs` approval handoff.
`HarnessTurnState` no longer owns replaceable tracker state.

### B. Plan template Scope / tracker map

- Template + `PLANNING_WORKFLOW_PLAN_QUALITY_LINE` request `## Scope` In/Out.
- Distill still only Implementation Steps/Steps → tracker items.
- Validator required sections unchanged (Summary, Implementation Steps, Test Cases, Assumptions); Scope is documented
  for new drafts, not hard-required.

### C. Plan-mode continuation

```rust
pub(crate) fn should_queue_plan_mode_auto_continue(...) -> bool
pub(crate) fn plan_mode_recoverable_block(reason: &str) -> bool
```

- False when planning inactive, kill-switch off, zero budget, plan ready for approval, awaiting user, verification
  block, or **`turn_completed`**.
- True only when `blocked_reason` matches `plan_mode_recoverable_block` (allow-list: recovery fallback / safety cap /
  preview+turn budget / wall clock / tool-free recovery; deny-list first: planning turn ended, approval-ready plan,
  request, permission, user input, awaiting).
- Orchestration passes real `turn_completed` and production `blocked_reason`. Never auto-approves.

### D. Plan progress presentation

`plan_progress_line(title, ready, open_decisions, step_count)` →
`• Plan [title] — research/synthesis | open decisions: N | ready for approval (M steps)`. Production emits
research/synthesis when a planning turn ends without an approval-ready plan and without plan-mode auto-queue.

### Config

No new keys. Plan-mode auto-continue reuses `[agent.harness.continuation].auto_continue_tracker` + `cross_turn_turns`.

## Follow-up (2026-09-21) — auto-continue directive misread as a user stay signal

Regression against the S2C contract: the harness-generated plan-mode auto-continue follow-up
(`plan_mode_continue_follow_up()`) contains the phrase `do not implement`, which normalizes to the `STAY_PHRASES` entry
of the same name. The exit trigger (`maybe_handle_planning_exit_trigger`) checks stay intent first, so it classified the
machine directive delivered as a mid-turn User message as a genuine stay signal — breaking the turn before any provider
request, publishing `PLANNING_COMPLETED_TURN_FALLBACK_REASON`, and re-queueing the identical directive each outer turn
(session `session-vtcode-20260921T045723Z_452479-85724`, 34 blocked turns / 67 fallback events). Intended behavior:
`should_queue_plan_mode_auto_continue` completes a planning turn only toward synthesis, never toward an immediate
re-break.

Fix: a single shared `PLAN_MODE_AUTO_CONTINUE_MARKER` const in `tool_outcomes/helpers.rs` is emitted by both plan-mode
auto-continue producers (outer-loop turn queue + resume continuation) and read by the exit trigger, which early-returns
`PlanningTransition::None` for a marker-bearing last user message. Both producers call `plan_mode_continue_follow_up()`,
so both are covered.

Verification (final tree):

- `cargo nextest run -p vtcode -E 'test(/exit_trigger|plan_mode_auto_continue|plan_mode_continue/)'` — PASS (8)
- `cargo nextest run -p vtcode -E 'test(/turn_loop|planning_workflow/)'` — PASS (236)
- `cargo nextest run -p vtcode-core -E 'test(/planning|plan_intent|stay_intent/)'` — PASS (134)
- `cargo nextest run -p vtcode --no-fail-fast` — 3193/3195 PASS; 2 failures are `startup::` env tests needing
  `~/Library/Application Support` writes (sandbox-denied), unrelated
- Negative control: disabling the guard fails the loop-level regression with the production symptom (Blocked +
  `PLANNING_COMPLETED_TURN_FALLBACK_REASON`, 0 provider requests)

## [S3] Out of Scope

- Auto-approving plans or auto-implementing from plan mode
- Hard-requiring Scope in the runtime validator (would break older plans)
- Changing tool permission policy or planning read-only enforcement
- Infinite planning auto-run

## Tasks

- [x] T1: Session-scoped replaceable tracker transcript block + tail-safe writer — acceptance: identical skip,
      replace-on-change when block is tail, no clobber of intervening lines, tail-shaped fallback replace (covers: S2A)
- [x] T2: Route pipeline + approval transcript writes through the helper — acceptance: single-writer path for both; no
      raw tracker `append_pasted_message` (covers: S2A; depends: T1)
- [x] T3: Plan template Scope + prompt/docs; Scope never distilled into tracker items — acceptance: quality line + docs
      include Scope; distill test (covers: S2B)
- [x] T4: `should_queue_plan_mode_auto_continue` + blocked-only orchestration wiring — acceptance: Completed planning
      turns denied; recoverable blocked allowed; no auto-approve (covers: S2C)
- [x] T5: Plan progress presentation helper + docs — acceptance: shape tests; docs match production emit (covers: S2D;
      depends: T3)
- [x] T6: AGENTS / agent-loop-contract / interactive-mode updated — acceptance: shipped contracts documented (covers:
      S2; depends: T2,T4,T5)
