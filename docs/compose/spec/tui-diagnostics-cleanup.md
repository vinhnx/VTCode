---
feature: tui-diagnostics-cleanup
status: delivered
updated: 2026-09-26
branch: fix/tui-diagnostics-cleanup
commits: 842238195..d29320c53
---

# TUI Diagnostics Cleanup

Broad audit of user-facing runloop diagnostic / error / info output. Screenshots from 2026-09-24 morning showed
multi-line `Diagnosis:` blocks, `# Last-Turn Diagnostics` forensics dumps, and a long blocked-handoff bullet stack.
Commit `cf4211c03` already collapsed diagnosis to one line and stripped the footer from the transcript. This pass
finishes the contract across the remaining runloop surfaces.

## Report

**What was built** — Runloop TUI diagnostics now obey a strict 2-line contract (status + one action). Blocked turns
print a short stop marker (`Turn blocked: recovery fallback` / `verification pending`) instead of repeating the
assistant-published reason, then a single action line that folds the verb, optional `vtcode --resume <id>`, and a
workspace-relative handoff pointer. Post-tool follow-up failures collapse to summary + one action with no standalone
category line; planning hint stacks emit at most two lines; empty-response fallback answers no longer embed multi-line
evidence dumps. Full reason, archive path, `# Last-Turn Diagnostics`, and Observed/Likely-cause/Next-action triples stay
in handoff markdown, `events.jsonl`, and tool payload `diagnosis` JSON for agent consumption. Prior `cf4211c03` one-line
diagnosis and footer strip remain locked by tests.

**Verification** — `./scripts/check-dev.sh` PASS;
<!-- markdownlint-disable-next-line MD013 -->
`cargo nextest run -p vtcode -E 'test(blocked) or test(diagnosis) or test(recovery) or test(fallback) or test(planning) or test(post_tool)'`
462/462 PASS (includes new `follow_up_failure_*`, `blocked_status_label_*`, `blocked_action_line_*` contract tests).
Fresh review of `842238195..7d67cb21b` found T3 test-coverage gap + stale helper name; both fixed in `d29320c53` and
re-review PASS.

**Journey log** — Screenshots predated `cf4211c03`; first pass only closed diagnosis/footer, leaving duplicate fallback
copy and bullet stacks. Broad-audit + strict 2-line cap chosen over a screenshot-only fix. Extracting pure
`follow_up_failure_*` helpers was required to make the render contract testable without a stdout renderer.

## [S1] Problem

TUI transcript diagnostics overwhelm the user with detail that is only needed by the coding agent. Observed classes:

1. **Duplicate stop copy.** Recovery fallback publishes an assistant reason (`Turn ended with a recovery fallback; …`)
   and then `Turn blocked:` repeats the same sentence.
2. **Bullet stacks.** A blocked turn prints 3–5 guidance bullets (continue / resume command / blocker path / archive
   path / plan-mode hints), often with absolute paths that wrap.
3. **Stacked recovery lines.** Post-tool LLM failure prints summary + error category + tip + action as separate lines.
4. **Forensics in the transcript.** Elapsed ms, tool-call counters, token usage — agent-only data — used to leak into
   the TUI (already stripped in `cf4211c03`; must stay out).

Users need the essential fact and the next action. Full diagnostic detail is for the coding agent via tool payloads,
`events.jsonl`, and handoff markdown.

## [S2] Design

### Normative TUI diagnostic contract

Every runloop diagnostic event that is **not** an assistant final answer or an explicit multi-line slash-command view
obeys a **strict 2-line cap**:

| Line | Role   | Content                                                |
| ---- | ------ | ------------------------------------------------------ |
| 1    | Status | What happened, one clause. No forensics.               |
| 2    | Action | One concrete next step (or a single combined pointer). |

Rules:

- **R1** — At most two `MessageStyle::{Info,Warning,Error}` lines per diagnostic event.
- **R2** — No forensics in the TUI: elapsed/token/tool-counter dumps stay in handoff markdown + `events.jsonl` only.
- **R3** — Workspace-local paths render via `vtcode_commons::workspace_relative_display` (or the shared
  `format_workspace_relative_paths`). Never print absolute workspace paths in TUI bullets.
- **R4** — Do not repeat a reason the assistant final message already published in the same turn. If the blocker
  headline is the recovery-fallback (or equivalent) reason just shown, the status line is a short stop marker and the
  reason lives only in the handoff file.
- **R5** — Full Observed/Likely-cause/Next-action triples stay in tool payload `diagnosis` JSON + harness `diagnosis`
  ReasoningItem (`render_text`). TUI keeps the one-line `Tool '<name>' failed: <observed headline>` form from
  `concise_diagnosis_line`.
- **R6** — Resume command, when present, folds into the single action line. No separate "From terminal" bullet.
- **R7** — Plan-mode extra guidance folds into the action line (or is omitted when the action line already names the
  mode switch). Never add a third line.

Out of scope for the cap: assistant final answers / recovery fallback prose (tighten separately but they may exceed two
lines), `/status` `/checkup` and other explicit multi-line command views, PTY/tool output bodies, tracker transcript
blocks.

### Surface contracts

**S2A — Blocked handoff (`session_loop_runner/blocked_handoff.rs`).**

After writing handoff artifacts:

```text
Turn blocked: <short stop marker>            # R4: not a repeat of the assistant reason
  • <one action>: continue / verifier / resume id + relative pointer
```

- Collapse the current continue / resume / blocker-details / archived-details / plan-mode bullets into **one** action
  line.
- Action line may include: the `continue` nudge **or** verifier-first step, plus optional `vtcode --resume <id>`, plus a
  single relative handoff path (live pointer). Archive path is omitted from the TUI (still in the handoff file).
- `truncated_block_reason` keeps stripping `# Last-Turn Diagnostics` and multi-line bodies (already landed); tests
  remain.

**S2B — Tool failure diagnosis (`failure_diagnosis/presentation.rs`).**

- Keep one-line `Tool '<name>' failed: <observed headline>` (bounded ~160 chars).
- `render_text` stays the harness/agent artifact; no change to model-facing diagnosis JSON.

**S2C — Post-tool recovery (`turn_loop/post_tool_recovery.rs`).**

Collapse the stacked follow-up-failure report to two lines:

```text
<status>: tool results kept; model follow-up failed [<transient hint>]
<action>: <retry scheduled | tip | resume hint>
```

- Drop the separate `Follow-up error category:` line (fold label into status parenthetical or omit).
- Drop the long planning tip paragraph from the TUI; keep a short tip or rely on the handoff/next-turn directive.

**S2D — Empty-response / recovery notices (`turn_processing/recovery_guidance.rs`, empty-response notices).**

- Notices already one-liners (`[!] Empty model response…`); keep.
- Fallback **assistant** messages may stay multi-sentence but drop embedded multi-line evidence dumps from the
  user-visible final text where those dumps are already in tool history (bounded previews used for synthesis directives
  stay in system/directive messages, not the user-facing fallback answer).

**S2E — Planning workflow hint stacks (`planning_workflow_state.rs`).**

- When multiple planning hints would print, emit at most one status + one action line (combine confirmation +
  next-step).

**S2F — Shared helper (new, small).**

Add a tiny helper in the binary runloop (or reuse existing display module) for the 2-line action-line construction:
relative path + optional resume id + one verb phrase. Prefer extending `format_workspace_relative_paths` / `display`
rather than a new subsystem.

### Agent-only channels (unchanged)

- Handoff markdown (`current_blocked.md`, blockers archive): full reason + `# Last-Turn Diagnostics` + actionable next
  steps.
- `events.jsonl` / `ThreadEvent` harness export: `TurnBlocked` summary, `diagnosis` ReasoningItems (`render_text`).
- Tool result payloads: `diagnosis` JSON object.

## [S3] Out of Scope

- Slash-command views (`/status`, `/checkup`, model picker, tracker panel).
- Provider/prompt contracts, diagnosis model prompt, evidence sanitization.
- Changing `ThreadEvent` shapes or handoff markdown structure (except path wording already relative where printed).
- Broad rewrite of every `MessageStyle` string in the binary — only diagnostic/error/info stacks that violate R1–R7.
- `docs/project/TODO.md`.

## Tasks

- [x] T1: Add/extend shared action-line helper (relative paths + optional resume id) with unit tests — acceptance:
      helper emits one line with workspace-relative path and no absolute workspace prefix (covers: S2, S2F)
- [x] T2: Blocked handoff 2-line contract — acceptance: blocked turn renders exactly one status + one action line; no
      archived-details bullet; R4 suppresses duplicate fallback reason; tests cover fallback-duplicate and
      verification-block cases (covers: S2A, S2; depends: T1)
- [x] T3: Post-tool recovery collapse — acceptance: follow-up failure renders ≤2 lines; no standalone
      `Follow-up error category:` line; tests updated (covers: S2C, S2)
- [x] T4: Planning hint + empty-response notice tidy — acceptance: planning hint stacks emit ≤2 lines; empty-response
      notices stay one-liners; no user-facing multi-line evidence dump in fallback answers (covers: S2D, S2E, S2)
- [x] T5: Diagnosis one-liner lock — acceptance: existing `concise_diagnosis_line` tests pass; no new TUI multi-line
      diagnosis path (covers: S2B, S2; depends: none)
- [x] T6: Docs — acceptance: `docs/user-guide/interactive-mode.md` blocked-turn wording matches 2-line contract; compose
      spec Report filled at delivery (covers: S2)
