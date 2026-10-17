---
feature: plan-mode-header-handoff
status: delivered
updated: 2026-09-26
branch: fix/plan-mode-header-handoff
commits: ab236aa54..9c85c67c9
---

# Plan Mode Header + Modes Handoff Sync

## Report

**What was built** — Planning entry from Build/Auto now performs a full switch to the plan primary agent and keeps the
session header in sync. Confirmed `start_planning` updates the header badge to Plan immediately and queues
`pending_primary_agent = "plan"` on the existing post-turn modes handoff (without claiming an approved-plan execution
turn). `/plan` / `/plan on` selects the plan agent, records the previous execution agent, and refreshes the header;
`/plan off` restores previous/fallback and always rewrites the header so a Plan badge cannot stick on a no-op restore.
Startup auto-enter/prompt also selects plan and refreshes the header. Plan approval continues to hand off to Build/Auto
through the typed `PlanExecutionTarget` path.

**Verification** — commands run and observed results:

- `./scripts/check-dev.sh` — PASS
- `cargo nextest run -p vtcode -E '...planning/mode...'` (283–284 tests) — PASS
- `cargo nextest run -p vtcode-ui -E 'test(header) or test(primary_agent) or test(mode_switch)'` (103 tests) — PASS
- Independent review: critical stale-header path on `/plan off` when restore is a no-op/None; startup missing full plan
  select. Both fixed and re-verified.

**Journey log** —

1. `pending_primary_agent = "plan"` must not set `plan_approved_execution_pending` — that flag starts implementation.
   `is_plan_entry_handoff` splits plan entry from plan→build/auto.
2. `select_approved_plan_execution_agent` excludes `plan` by name (plan allows read-only bash so `is_read_only()` is
   false). Plan entry uses `select_from_specs` directly.
3. `previous_primary_agent`/`fallback_primary_agent` were stored but never read until this change; `/plan off` restore
   was the missing consumer.
4. Header refresh must run on every planning-exit path, including no-op restore and failed selection — early-returns on
   agent-name equality are where Plan badges stick.
5. Capture the restore agent _before_ `plan_session.exit()` and _before_ selecting plan (previous must be the execution
   agent, not `plan`).

## [S1] Problem

When the session is in **Build** or **Auto**, planning entry can happen in two ways:

1. The execution agent calls `start_planning` and the user confirms **Enter Planning workflow**.
2. The user runs `/plan` / `/plan on [task]`.

Both paths enable the planning workflow (ActivityState → Planning, mutating tools blocked) but **do not select the plan
primary agent and do not refresh the session header badge**. The header keeps showing the previous mode (`Build` /
`Auto`) while the session is already planning. Plan approval already hands off to Build/Auto and updates the header, so
the entry side of the modes handoff is the stale half.

`/mode plan` / Tab-to-plan already performs a full primary-agent switch and updates the header. The workflow entry paths
diverge from that contract.

## [S2] Design

Decision (user-confirmed): **full switch to the plan primary agent** on both entry paths, and keep the header in sync
for `/plan on|off` as well as the confirmed `start_planning` path. Plan approval continues to hand off to Build/Auto
through the existing typed `PlanExecutionTarget` path.

### A. Shared planning-mode entry contract

Entering planning from Build/Auto (or any execution agent) must:

1. Select the built-in `plan` primary agent (`identity.name == "plan"`), so prompt, tool catalog, and permissions match
   plan mode.
2. Refresh the session header badge to the plan agent display name (same `set_primary_agent` surface used by Tab /
   `/mode`).
3. Enable the planning workflow (`transition_to_planning_workflow`) with `previous_primary_agent` recorded for restore.
4. Keep ActivityState on `Planning`.

### B. Entry paths

| Path                          | Full agent switch                                                                                                       | Header update                       | Notes                                                                   |
| ----------------------------- | ----------------------------------------------------------------------------------------------------------------------- | ----------------------------------- | ----------------------------------------------------------------------- |
| `start_planning` confirmation | Queue `pending_primary_agent = "plan"` (turn-loop modes handoff; `is_plan_entry_handoff` so no approved-plan execution) | Immediate `apply_plan_agent_header` | Mid-turn cannot mutate `ActivePrimaryAgentState` from `RunLoopContext`. |
| `/plan` / `/plan on [task]`   | Full select in `handle_toggle_planning_workflow`                                                                        | `apply_agent_header`                | Captures previous agent _before_ selecting plan.                        |
| Startup auto-enter / prompt   | `apply_startup_plan_agent_selection`                                                                                    | Yes                                 | Session starts in plan mode when configured.                            |

### C. Exit paths / modes handoff

| Path                     | Behavior                                                                                                                                |
| ------------------------ | --------------------------------------------------------------------------------------------------------------------------------------- |
| Plan approval            | Existing typed `PlanExecutionTarget` handoff selects write-capable Build or Auto, updates header.                                       |
| `/plan off`              | Finish planning, restore `previous_primary_agent` else `fallback_primary_agent`, **always** rewrite header via `plan_exit_header_name`. |
| `/mode build` / Tab away | Existing `handle_select_primary_agent` + `leave_planning_for_execution`.                                                                |

### D. Modes handoff invariant

One typed destination/policy/context target continues to flow through current and fresh approvals. Queued mode input
must not overwrite an approved destination. Plan-entry `pending_primary_agent = "plan"` uses the same orchestration
handoff surface but selects the plan agent directly (not the write-capable resolver).

### E. Out of scope

- Changing plan validation, tracker distill, or approval policy.
- Changing Build vs Auto authority.
- Header layout / badge styling.

## [S3] Out of Scope

See [S2] E. No new top-level harness subsystem. No changes to `ThreadEvent` contracts beyond existing plan-approval
events.

## Tasks

- [x] T1: Shared plan-entry helper that selects `plan`, refreshes the header badge, and enters planning — acceptance:
      `/plan on` leaves `active_primary_agent.identity.name == "plan"` and header badge shows Plan (covers: S2 A,B)
- [x] T2: `start_planning` confirmation updates header immediately and queues `pending_primary_agent = "plan"` —
      acceptance: after confirmed entry the header shows Plan and the turn outcome carries the plan agent handoff
      (covers: S2 A,B)
- [x] T3: `/plan off` restores previous/fallback agent and refreshes the header — acceptance: after `/plan off` the
      header shows the restored execution agent (covers: S2 C)
- [x] T4: Regression tests for entry header sync and plan-approval handoff header update — acceptance: tests for restore
      preference, exit clearing, plan-entry handoff predicate, and plan-exit header name fallback (covers: S2 A–D)
- [x] T5: Update planning-workflow / commands docs for the full-switch contract — acceptance: docs state that planning
      entry selects the plan agent and updates the header (covers: S2 A–C)
