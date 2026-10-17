---
feature: continuation-end-turn-ux
status: in-progress
updated: 2026-09-19
branch: fix/continuation-end-turn-ux
commits: e01b174ab..HEAD
---

# Continuation End-Turn UX + Classifiers

Residual work after delivered `tracker-continuation` and `todo-continuation-hardening`.

## Report

## [S1] Problem

VT Code still looks like it stops mid-work on TODO/tracker sessions. Empirical session
`session-vtcode-20260918T030054Z_141498-26410` auto-continued 20 turns, but every turn ended with a `## Status` recap
that reads as a user-action stop (“blocked by turn budget”, “tool budget ran out”, “per-file read cap”, “needs
verification”). After the delivered features, three residual classes remain:

1. **In-turn classifier false-continue / false-stop.** `apply_tracker_continuation_override` force-continues any
   non-handoff text when tracker steps remain and `has_explicit_blocker` is false. Verification-pending recaps (“Turn
   blocked after repeated unverified assistant responses; verification is still pending”) do not match
   `has_explicit_blocker`’s “blocked by” token, so the in-turn path can continue past an anti-blind gate that outer
   auto-queue correctly treats as a true handoff. Conversely, budget/status phrases missing from
   `recoverable_tracker_budget_phrasing` still end the turn as if the user must act.
2. **Outer recoverable-classifier gaps.** `tracker_auto_continue_is_recoverable_block` and the in-turn budget vocabulary
   are separate lists. Production/session phrases can match one surface and miss the other, so a recoverable budget end
   still falls through to the full blocked handoff.
3. **Double nudge UX.** When tracker auto-queue is eligible but fails (queue full / `cross_turn_turns` exhausted),
   orchestration prints a tracker “Type `continue`” line and then still runs the generic blocked handoff (warning +
   “Type 'continue' to resume” + TUI placeholder “Turn blocked · Type 'continue'…”). Successful auto-queue already skips
   that path; the exhausted path over-nudges and re-reads as “agent stopped, take action” even when the correct story is
   “autonomous budget exhausted.”

Desired behavior: the harness continues TODO/tracker work across turns without user nudges whenever no true handoff is
required, and prompts the user **only** for genuine questions, permission/policy/safety/credential blocks, verification
after autonomous recovery is exhausted, or session exit.

## [S2] Design

User-confirmed scope: residual end-turn UX + classifiers. True-handoff stops stay. No verification-gate redesign; no
infinite auto-run.

### Normative stop list (unchanged)

End turn and wait when tracker work remains **only if**:

1. Final text asks a genuine user question/decision, or
2. Permission / policy / safety fuse / missing credentials (even if a budget is also mentioned), or
3. Verification block after autonomous recovery is exhausted/escalated, or
4. Session exit / hard interrupt.

Budget / tool-loop / preview / recovery ends auto-continue. Tool-free recovery ends the **turn**; outer queue schedules
the next tracker turn.

### Contracts

**S2A — Shared true-handoff vocabulary.**

One pure helper family (binary runloop + `vtcode_core::core::agent::completion`) classifies assistant text:

- `tracker_final_text_is_safety_handoff` — permission/policy/safety/credentials **first**, including
  verification-pending / anti-blind “still pending” recaps so in-turn override cannot continue past a gate the outer
  loop will not auto-queue.
- `tracker_final_text_requires_user_input` — trailing `?` / strong interview phrases only.
- `recoverable_status_recap_phrasing` — session + production budget/recovery vocabulary used by in-turn override **and**
  outer blocked-reason classifier tests
  (preview/turn/tool/tool-loop/tool-call/wall-clock/read-cap/work-budget/safety-cap/recovery-fallback/exhausted/ran
  out).

In-turn `apply_tracker_continuation_override`:

- Terminal when safety/policy/verification-pending handoff, even if tracker incomplete.
- Terminal when `has_explicit_blocker` and the text lacks recoverable budget/recovery phrasing.
- Otherwise force `should_continue = true`, reason `tracker_incomplete_continuation`, not relaxed.

**S2B — Outer recoverable classifier parity.**

`tracker_auto_continue_is_recoverable_block` keeps deny-first true handoffs, then allow-lists the same recoverable
production constants plus the shared status-recap vocabulary. Unknown/missing reasons stay non-auto-queue on Blocked
ends. Completed ends with incomplete tracker still auto-queue unless final text is a safety/user-input handoff.

**S2C — Single exhausted-path UX.**

Orchestration after a turn end:

1. Probe tracker; progress-reset budget on completed-count change.
2. If plan-mode recoverable blocked → queue plan continuation quietly (`tracing::debug!`, no TUI echo) or one compact
   plan-status line (no generic blocked nudge).
3. Else if tracker auto-queue eligible → queue follow-up + system directive; on success stay quiet (`tracing::debug!`,
   no `Tracker auto-continue turn N/M` TUI line) and **continue** the session loop (no blocked handoff, no blocked
   placeholder). Mid-turn application of the queued harness follow-up also stays quiet — only genuine user steering
   prints `Steered into active turn`.
4. Else if tracker auto-queue was eligible but **exhausted/queue-full** → print **one** info line stating autonomous
   continuation cannot resume and the user must type `continue` (or fix the named blocker). Do **not** also emit the
   generic blocked-handoff “Type 'continue'” stack or set the blocked TUI placeholder for this recoverable budget end
   when tracker steps remain.
5. Else if Blocked and not auto-queued → existing blocked handoff + placeholder (true handoffs, verification escalation,
   unknown reasons).
6. Tracker-complete / no incomplete work → reset budgets; ordinary completion path.

**S2D — Model-facing guidance (shipped surface).**

Compiled `runtime_guidance.rs` keeps the tracker in-run rule. Add/adjust only if a presence test requires the
verification-pending vs status-recap distinction; stay within the guidance token budget test.

### Config

Unchanged:

```toml
[agent.harness.continuation]
auto_continue_tracker = true
cross_turn_turns = 32   # progress-resets on completed-count change
```

## [S3] Out of Scope

- Redesigning anti-blind verification execution or auto-running arbitrary verifiers beyond the existing cross-turn
  recovery path.
- Auto-approving plans, tools, or permissions.
- Infinite auto-run without `cross_turn_turns` / session caps / Esc.
- Changing tool budgets, preview budgets, or permission policy semantics.
- Uncommitted main-tree WIP not owned by this branch.
- compose-next checklist mechanics outside VT Code’s agent run loop.

## Tasks

- [ ] T1: Shared handoff/recoverable vocabulary + in-turn override terminal on verification-pending recaps — acceptance:
      unit tests cover session verification recap → no in-turn continue; budget recap with tracker incomplete →
      `tracker_incomplete_continuation`; permission recap still terminal (covers: S2A)
- [ ] T2: Outer recoverable-classifier parity tests for session/production phrases — acceptance:
      preview/tool/read-cap/wall-clock/recovery reasons auto-queue eligible; verification/permission/user-input reasons
      denied (covers: S2B)
- [ ] T3: Exhausted-path UX — one handoff when tracker auto-queue cannot resume; no double Type-continue + blocked
      placeholder on recoverable budget ends with incomplete tracker — acceptance: pure/string tests on the
      orchestration messaging helpers + integration-style assertions where cheap (covers: S2C)
- [ ] T4: Runtime guidance presence/budget if wording changes — acceptance: `runtime_guidance` tests still pass (covers:
      S2D)
- [ ] T5: Docs — `docs/guides/agent-loop-contract.md` tracker section reflects residual UX/classifier contract —
      acceptance: contract documents true-handoff-only stops and exhausted-path single nudge (covers: S2A–S2C)
- [ ] T6: Verify + regression suite — acceptance: `cargo nextest run` filters for
      tracker/continuation/outer_queue/plan_mode/recoverable_block/resume_gate/runtime_guidance pass;
      `./scripts/check-dev.sh` clean (covers: S2)
