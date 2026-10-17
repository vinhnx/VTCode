---
feature: todo-continuation-hardening
status: delivered
updated: 2026-09-19
branch: fix/todo-continuation-review
commits: b998eb6f7..e5d26168f
---

# TODO Continuation Hardening (Residual)

Residual work after delivered `tracker-continuation`, plus a post-merge review cycle that closed correctness gaps on the
same feature.

## Report

**What was built** — TODO/tracker work continues across turns without user nudges when no true handoff is required, and
the harness no longer auto-continues past genuine questions, permission/policy denials, or planning interview waits.

Shipped behavior: expanded recoverable classifiers (tool-budget/loop, planning recovery-fallback); default
`cross_turn_turns = 32` with **progress-reset on any completed-count change** (including checklist recreate);
`TrackerProbeOutcome` so Complete clears incomplete caches and Unavailable keeps last; in-turn override on status recaps
**except tool-free recovery** (stays terminal; outer queue continues next turn); outer Completed auto-queue requires
`!final_text_requires_user_input`; safety-handoff vocabulary denies permission/policy **before** budget tokens;
plan-mode deny-first for compound handoffs while `PLANNING_COMPLETED_TURN_FALLBACK_REASON` remains recoverable;
planning/tracker resume paths mutually exclusive; malformed checklist probes are Unavailable.

**Verification** —

- `cargo nextest run -p vtcode-core -E 'test(tracker_final_text) or test(runtime_guidance)'` — PASS (5)
<!-- markdownlint-disable-next-line MD013 -->
- `cargo nextest run -p vtcode -E 'test(outer_queue) or test(plan_mode) or test(tracker_probe) or test(session_stats_progress) or test(tracker_incomplete) or test(recoverable_block) or test(tracker_config) or test(resume_gate) or test(parse_tracker)'`
  — PASS (29)
- Re-review (general-5): findings 1–8 fixed; no new criticals; targeted 5/5 PASS
- `./scripts/check-dev.sh` — PASS
- PRE-EXISTING / unrelated: `sparse_approved_plan_is_distilled_into_tracker_items`

**Journey log** —

1. Empirical session `session-vtcode-20260918T030054Z_141498-26410` already auto-continued 20 turns; residual work was
   classifiers/budget/UX, not “no auto-continue”.
2. First post-merge review: tool-free recovery override could re-enable tools; outer Completed ignored user questions;
   budget_like short-circuited safety handoffs; plan-mode gate test was stale vs allow-list-first.
3. Spec S2B originally asked for in-turn override on tool-free recovery — **reversed** after code evidence that recovery
   is intentionally terminal (`tool_free_recovery_terminal`).
4. Safety/policy tokens must be checked **before** budget tokens; the opposite order mis-classifies compound recaps as
   continue.
5. Resume auto-queue paths must exclude each other: planning-active blocks tracker resume; plan resume requires
   `"planning"` in the blocked summary.

## [S1] Problem

TODO/tracker work still looked like “agent stopped, take action”: status recaps, planning “Type continue”, budget
episode 8 that only reset on full tracker complete, recoverable classifiers missing production blocked-reasons, and
post-merge review found outer Completed could auto-queue past genuine user questions and compound permission handoffs.

## [S2] Design

User-confirmed: full residual hardening; progress-reset + default 32; **true handoffs only**.

### True-handoff stop list (normative)

End turn and wait when tracker work remains **only if**:

1. Final text asks a genuine user question/decision (trailing `?` / clause-start / strong interview phrases), or
2. Permission / policy / safety fuse / missing credentials (even if a budget is also mentioned), or
3. Verification block after autonomous recovery is exhausted/escalated, or
4. Session exit / hard interrupt.

Budget / tool-loop / preview / recovery ends auto-continue. Tool-free recovery ends the **turn** but outer queue
schedules the next tracker turn.

### Contracts (review-cycle corrections)

- `completion::tracker_final_text_requires_user_input` — shared outer + in-turn question filter.
- `tracker_final_text_is_safety_handoff` — safety/policy **first**; pure budget is not a handoff.
- `should_queue_tracker_auto_continue(..., final_text_requires_user_input)`.
- `plan_mode_recoverable_block` — deny true handoffs first; then recovery allow-list; then planning handoffs.
- `note_tracker_completed_count` — reset episode on **any** completed-count change.
- In-turn override: `!tool_free_recovery_pass` required.
- Resume: tracker requires `!planning_active`; plan resume requires planning context + recoverable summary.
- Probe: checklist without `items` → Unavailable.

### Config

```toml
[agent.harness.continuation]
auto_continue_tracker = true
cross_turn_turns = 32   # progress-resets on completed-count change
```

## [S3] Out of Scope

- Auto-approving plans/tools/permissions.
- Changing anti-blind verification semantics beyond nudge suppression when recovery was queued.
- Infinite auto-run without hard session/turn caps or Esc.
- Diff-harness compose-next checklist (separate).
- Uncommitted main-tree WIP not owned by this branch.

## Tasks

- [x] T1: Expand recoverable classifiers (covers: S2A)
- [x] T2: In-turn override + probe cache; tool-free recovery terminal (covers: S2B)
- [x] T3: Progress-reset + default 32 (covers: S2C)
- [x] T4: Planning resume + no Type-continue on successful queue (covers: S2D,S2E)
- [x] T5: Runtime guidance + docs (covers: S2E)
- [x] T6: Post-merge review-cycle fixes (outer question filter, handoff order, plan deny-first, resume gates,
      Unavailable malformed probe) (covers: S2; review)
