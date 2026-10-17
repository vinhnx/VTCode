# Preview Budget, Blocked Turns, and Replans

Bounded provider-error previews preserve a complete identifier token when truncation would otherwise make fields such as
`reasoning_content` ambiguous; the full provider error remains available through the existing tracing warning path.

Model-facing tool output is bounded so one noisy command cannot crowd out the rest of a turn. Each turn shares an
aggregate budget across all tool-result previews copied into provider history (`64 KiB` execution, `96 KiB` planning).
The existing per-result spool limit still applies inside that budget; plan-mode non-verification inspections use a
smaller per-result default (`~8 KiB` when omitted) and explicit values are clamped to `~16 KiB` so research fan-out fits
the turn. Execution-mode non-verification inspections are likewise bounded (`~24 KiB`, omitted or explicit) so one
default inspection stays inside the tighter execution budget; verification commands are exempt in both modes so
build/test output stays authoritative. When the budget runs out, later results keep only small outcome metadata such as
the tool name, spool path, byte count, completion state, and a short note. The full output stays in the internal spool
and the current-session viewer, so nothing is lost for the person at the terminal. What changes is what the model sees
next.

When the model sees `preview_budget_exhausted`, the correct response is to trust the preserved metadata and move on. The
runtime guidance in `crates/codegen/vtcode-core/src/prompts/runtime_guidance.rs` states this directly: trust the
preserved metadata and page a known `spool_path` in small ranges instead of repeating the call; verifiers run standalone
or as a pure `&&` chain (a pure `head`/`tail` truncator counts as standalone), then synthesize from evidence. Repeating
an equivalent call after exhaustion is the failure mode the eval guards against. Exhaustion has two phases: within the
turn, inspections are blocked but verification commands, `task_tracker`, session polling, spool paging in small ranges,
and plan-draft re-reads stay open; the planning balancer then schedules one tool-free `<proposed_plan>` synthesis pass
on the next request. A provider retry-exhaustion (`last_error`, e.g. HTTP 400) that also lands on a tool-free pass is a
different cause with the same shape: report the provider error, do not relabel it as preview exhaustion. In plan mode
the exhausted-preview recovery instead emits one tool-free `<proposed_plan>` directly from the preserved outcome
metadata (`tool`, `spool_path`, `byte_count`, `completion_state`) without re-reading exhausted calls, using
`Action -> files: [path] -> verify: [command]` steps; a failed synthesis stays a resumable planning handoff
(`keep planning` reuses the evidence) rather than another research pass. The compact and expanded summaries in
`docs/development/tool-summary-display.md` follow the same rule from the UI side, and the request assembly keeps history
Arc-shared so these bounded previews do not force extra copies.

The registry-owned `preview_budget_exhausted` marker is authoritative at the provider-history boundary. It must be
observed before empty-body or tiny-result bypasses because registry shaping may already have removed every payload
field. The transition is monotonic and suppression is counted by tool-call identity, so streamed/terminal replacement
cannot make diagnostics drift. Preview or navigation recovery never clears anti-blind editing state; verification
remains pending until a standalone or pure-`&&` verifier exits successfully.

A blocked turn is the resumable stop that follows repeated failure. The runtime emits `turn.blocked` alongside
`turn.failed` with streak, total, caps, and last-tool counters so the interface gets a first-class signal instead of
inferring one from silence. The canonical contract is `vtcode-exec-events::ThreadEvent`, and the surrounding lifecycle
is described in `docs/guides/agent-loop-contract.md`. Forced checkpoints live under the owning session, and an
archive-less runner handoff omits resume commands rather than inventing one. Recovery stays visible: the model produces
one tool-free synthesis pass, and the session can resume from the checkpoint with its prior progress intact.

Replans cover the middle of a run rather than its end. When new evidence invalidates the current approach, the planner
keeps existing scopes, records what would prove the new plan wrong, and continues the same run instead of restarting it.
Generalization notes stay bounded, validated, and scoped to the task so they cannot leak into global memory. The
user-facing flow is described in `docs/guides/planning-workflow.md`; this guide records the developer side, namely that
replan continuation reuses the existing `plan.delta` and approval events and needs no new contract variant.

To verify these behaviors locally, run the regression suite in
`crates/codegen/vtcode-eval/evals/preview-budget-blocked-replan.json` with
`vtcode exec eval --suite <path> --output report.md`. Each task carries environment-verified `verify_commands`, and the
suite uses three attempts so both `pass@k` and `pass^k` are meaningful. The same checks run headless through
`cargo nextest run -p vtcode-eval`. No schema or tool-spec change ships with this guide because `turn.blocked` and
`plan.delta` already exist; a future contract change would bump `EVENT_SCHEMA_VERSION` and update the prompt guidance
and schemas together.
