Agent Loop + Harness Hardening (surgical, scope: vtcode-core)
Targeting priorities #1 (DRY), #2 (loop coherence), #3 (harness safety) within the agent runner. Priority #4 (architecture) is already sound here — ContinuationController, RetryPolicy, validation, and tool-access gating are cleanly separated. No rewrites; only consolidations and one verified guard.

Step 1 — Verify harness wall-clock enforcement (priority #3)
Read AgentHarnessConfig.max_tool_wall_clock_secs usages. Confirm whether the turn loop / execute_prepared_tool_internal enforces a per-tool-batch wall-clock ceiling today.

If already enforced: document the call site and stop here for #3.
If missing: add a guard in execute_prepared_tool_internal (or the batch executor) that aborts a batch exceeding max_tool_wall_clock_secs, records a warning + HarnessEventKind, and returns a ToolExecutionError::policy_violation(...) so the loop falls back to its existing failure/recovery path (no new state type). This reuses the existing failure-handling path as the fallback mechanism.
Step 2 — Extract resolve_completion_assessment helper (priorities #1, #2)
Consolidate the two duplicated CompletionAssessment handlers in vtcode-core/src/core/agent/runner/execute.rs (~lines 877-1005) into one method:

rust
enum AssessmentResolution { Break, ForceContinue }

async fn resolve_completion_assessment(
&mut self,
assessment: CompletionAssessment,
verification_results: &[VerificationResult],
effective_task: &Task,
runtime: &mut AgentRuntime,
event_recorder: &mut ExecEventRecorder,
orchestration_enabled: bool,
revision_rounds_used: &mut usize,
max_revision_rounds: usize,
should_write_blocked_handoff: &mut bool,
) -> Result<AssessmentResolution>
Behavior (unified, matching current semantics exactly):

Accept → resolve_completion_acceptance(...verification_results...); Break if accepted else ForceContinue.
SkipAccept { reason } → emit ContinuationSkipped, set is_completed=true + outcome=Success, Break.
Continue { reason, prompt } → emit ContinuationStarted, add_user_message(prompt), ForceContinue.
Verify { .. } → stays in the caller: first call site runs verification then re-dispatches the after_verification result through this same helper; second call site maps Verify to a no-op (preserving current behavior).
Both call sites in the turn loop then collapse to a single match on the returned AssessmentResolution.

Step 3 — Reuse build_verification_failure_summary (priority #1)
Replace the inlined "Verification failed: ..." formatting at execute.rs:936-951 with a call to the existing continuation::build_verification_failure_summary(failure), appending the trimmed output the same way. Removes the format-string duplication.

Step 4 — Remove dead duplicate batch helpers (priorities #1, #2)
Confirm via grep that execute_parallel_tool_calls / execute_sequential_tool_calls (tool_exec.rs:978, :1031) are only referenced from tests.
Migrate any test call sites to execute_tool_call_batches (the production path).
Delete both helpers and their #[cfg_attr(not(test), expect(dead_code))] attributes.
Step 5 — Verify
cargo check -p vtcode-core (fast gate).
cargo nextest run -p vtcode-core (runner + continuation tests; the continuation.rs tests at lines 485-588 are the regression net for Step 2/3).
./scripts/check-dev.sh --test if the above pass, for the broader gate.
Out of scope (explicitly)
Provider-tree merge (vtcode-core↔vtcode-llm) — separate, higher-risk task; flagged for later.
Error-taxonomy consolidation — separate task.
Any change to ContinuationController, RetryPolicy, AgentHarnessConfig schema, or ThreadEvent contract — none needed; existing types are reused as-is.
Per the "Normal flow" choice: edits land in files via tools; chat stays to brief per-step status with test evidence. No change logs or doc files written.

note: continue from stash `WIP on main: a7ab54fb9 feat: add GLM-5.2 model support and update related configurations`
