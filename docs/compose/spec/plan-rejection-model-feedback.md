---
feature: plan-rejection-model-feedback
status: delivered
updated: 2026-09-25
branch: fix/plan-rejection-model-feedback
commits: 8101b4e51..a045db8f3
---

# Plan Rejection Must Reach Model History

## Report

**What was built** — Terminal plan-artifact rejections now put validator-owned feedback into model-visible assistant
history, not only the TUI. On `PlanArtifactError::Invalid`, history receives `PlanValidationReport::repair_feedback()`
(missing sections, invalid steps, empty summary/validation/assumptions, plus the canonical
`Action -> files: […] -> verify: […]` form and concrete verify examples). Non-Invalid errors use their Display text.
Both terminal surfaces share `plan_rejection_history_feedback`, so execution-mode (stable
`EXECUTION_PLAN_REJECTION_NOTICE` prefix + feedback) and planning-active terminal rejection cannot diverge.
Repair-scheduled paths are unchanged.

This fixes the session-vtcode-20260924T133543Z_155288-07964 failure mode: the TUI showed
`Plan revision rejected: invalid plan artifact: missing sections: …` while history only had a generic sentence, so a
later user `continue` resubmitted the same invalid `**Goal:**` plan and the agent claimed “no specific reason given”.

### Verification

- `./scripts/check-dev.sh` — PASS
<!-- markdownlint-disable-next-line MD013 -->
- `cargo nextest run -p vtcode -E '(test(reject) or test(execution_mode) or test(plan_rejection) or test(plan_only_execution) or test(repair_directive) or test(plan_mode) or test(planning_workflow) or test(tool_free) or test(untagged_planlike)) and not test(from_validated_debug_asserts_on_non_ready_report)'`
  — PASS (281)
- PRE-EXISTING (also fail on clean base `8101b4e51`): `from_validated_debug_asserts_on_non_ready_report`,
  `planning_preview_budget_keeps_midsize_payload_exec_strips_it` (SIGABRT debug-assert tests)

### Journey log

- The rejection text in the repro matches the execution-mode terminal renderer (`Plan revision rejected` +
  `Rejected plan revision:`), not the planning terminal wording — that path was publishing a fixed notice without
  `{error}`.
- `repair_feedback()` is the right history payload: `reasons()` can echo raw `open_decisions` lines; `repair_feedback()`
  stays validator-owned.
- Two `test(plan)` nextest filters over-matched provider/updater tests and unrelated auth failures; scope filters to the
  vtcode binary test names.
- Review pass collapsed four copy-pasted history assertions into `last_final_answer_text` +
  `assert_history_carries_validator_feedback`, and both terminal surfaces into `terminal_plan_rejection_message`.

## [S1] Problem

When a `<proposed_plan>` fails plan-artifact validation, the TUI shows the full validator reasons
(`Plan revision rejected: invalid plan artifact: missing sections: …`) but the model-visible assistant history does not.
Session `session-vtcode-20260924T133543Z_155288-07964` reproduced this: the agent resubmitted the same invalid
`**Goal:**` / `**Scope of edits:**` plan twice and reported "rejected … with no specific reason given", then asked the
user to approve a draft the validator had already rejected.

Root cause: the execution-mode terminal path publishes `EXECUTION_PLAN_REJECTION_NOTICE` (a fixed string with no
validation detail) via `handle_assistant_response`. Only the renderer line carries `{error}`. A later user `continue`
therefore has the rejected draft but not the contract it violated.

The planning-active terminal path includes `{error}` Display text, but not the canonical step format / verify examples
the repair directive already owns.

## [S2] Design

### S2.1 Model-visible rejection contract

Every **terminal** plan rejection (no repair scheduled) must put the following into the assistant final-response
history, not only the renderer:

1. The existing user-facing rejection sentence (so TUI and history stay aligned).
2. Validator-owned feedback from `PlanValidationReport::repair_feedback()` for `PlanArtifactError::Invalid` (missing
   sections, invalid steps, empty summary / validation / assumptions, plus canonical
   `Action -> files: […] -> verify: […]` and concrete verify examples). For other error variants, the error Display.
3. The rejected draft remains appended via `append_rejected_plan_draft_to_last_assistant` (unchanged).

Repair-scheduled paths already push `plan_repair_directive_for_error` and are unchanged.

### S2.2 Shared helper

One helper builds the history feedback for both terminal surfaces:

- execution-mode (`EXECUTION_PLAN_REJECTION_NOTICE` + feedback)
- planning-active terminal ("Revise the plan…" sentence + feedback)

so the two paths cannot diverge. `EXECUTION_PLAN_REJECTION_NOTICE` stays a stable prefix constant so existing tests and
log greps keep working.

### S2.3 Out of scope

- Changing validator rules or required headings.
- Increasing `MAX_PLAN_VALIDATION_REPAIR_REPROMPTS`.
- Prompt-text changes in compiled planning guidance (already document the format).
- The README work from the repro session (already landed on main).

## [S3] Out of Scope

See S2.3. No `ThreadEvent` contract change; the extra text is assistant-message content only.

## Tasks

- [x] T1: Add shared rejection-feedback helper and use it on both terminal reject paths — acceptance: execution-mode and
      planning terminal `handle_assistant_response` text includes validator repair feedback (covers: S2.1, S2.2)
- [x] T2: Regression tests — acceptance: tests assert final assistant history contains validation reasons (e.g. "missing
      required section") and canonical step format after terminal rejection; existing `EXECUTION_PLAN_REJECTION_NOTICE`
      prefix checks still pass (covers: S2.1; depends: T1)
