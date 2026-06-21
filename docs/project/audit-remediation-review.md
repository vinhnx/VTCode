# Audit Remediation (Blocks A-J) — Code Review

**Date:** 2026-06-21
**Scope:** Uncommitted working-tree diff implementing the "VTCode Modes & Subagents Audit" remediation plan (Blocks A-J). ~29 files, ~526 insertions.
**Reviewer:** pr-code-review agent
**Outcome:** Build clean (`cargo build --workspace`), clippy clean (`cargo clippy --workspace --all-targets`). One real issue found and fixed (Medium). All other blocks verified correct.

## Summary

The remediation is well-executed. Each block's intent is realized correctly:

- **Block A** (silent fallback warning): `fallback_notice` predicate correctly suppresses `build`/empty and surfaces any other mismatched name. Pure predicate is unit-tested.
- **Block B** (interactive trust gate): `require_full_auto_workspace_trust` is invoked at the start of `run_interactive_session` when `full_auto` is true, sharing the strict gate already used by `--auto`/`exec`. No double-prompt risk (later `ensure_full_auto_workspace_trust` no-ops once trust is granted).
- **Block C** (alias unification + redundant `enable` removal): `EXECUTION_MODE_ALIASES` + `FLAG_CLEARING_ONLY_ALIASES` unified behind `execution_alias_match`. "exit planning workflow" behavior preserved exactly (clears flag, passes through verbatim). Redundant `plan_state.enable()` correctly removed since `enable_planning()` already sets the flag.
- **Block D** (`plan` mode `All` → `Primary`): consistent with `duck`. Delegation to built-in `plan` as a subagent is now blocked by `spawn_custom`'s `is_subagent()` guard; `@plan` mentions no longer trigger subagent delegation. Deliberate, documented behavior change.
- **Block E** (read-only primaries rely on allow-list): verified `primary_agent_allows_tool` hard-blocks mutating tools; no Deny overrides needed.
- **Block F** (`set_planning_phase` migration): all 8 call sites in `execution_planning.rs`/`execution_run.rs` migrated correctly. One missed call site found and fixed (see below).
- **Block G** (`LIFECYCLE_CLEANUP_TOOLS`): behavior-preserving; `RESUME_AGENT` correctly excluded (not a cleanup op).
- **Block H** (`LazyLock` memoization): `builtin_subagents()` returns `BUILTIN_SUBAGENTS.clone()` — deep clone, no shared mutable state leak. `discover_subagents` extends with owned specs.
- **Block J** (`push_tree_prefix`): clean DRY extraction, reused in `tool_summary.rs`.

The pre-existing 6 let-chain `ast-grep scan` lints are out of scope (not introduced by this diff) and were not touched. The ~12 rustfmt cosmetic reformats introduce no logic changes.

## Findings

### [Medium] M-1: Missed `set_planning_phase` migration in `response_handling.rs`

**File:** `src/agent/runloop/unified/turn/context/response_handling.rs:240`
**Status:** Fixed.

The Block F migration covered the 6+2 call sites in `execution_planning.rs`/`execution_run.rs` but missed a 9th external phase transition in `response_handling.rs`. When a turn completes with a proposed plan while planning is active, the phase was set via the direct `self.tool_registry.planning_workflow_state().set_phase(...)` call, bypassing catalog invalidation.

The `set_planning_phase` facade's own doc comment states: *"Callers that mutate the phase through `PlanningWorkflowState::set_phase` directly bypass this invalidation; route phase changes through this method whenever a `ToolRegistry` is available so a future phase-aware tool filter never serves a stale snapshot."* Since `self.tool_registry` is available at this call site, this was an inconsistency that defeated the migration's stated future-proofing goal.

Currently harmless (the catalog is not yet phase-aware), but left a landmine for the future phase-aware filter. Fixed by routing through `set_planning_phase`:

```rust
self.tool_registry
    .set_planning_phase(if persisted.validation.is_ready() {
        PlanLifecyclePhase::DraftReady
    } else {
        PlanLifecyclePhase::ActiveDrafting
    });
```

After the fix, `grep` confirms no external `planning_workflow_state().set_phase(` direct callers remain (only the internal `self.state.set_phase(...)` mutations inside `planning_workflow.rs`, which are the authoritative handler-internal transitions without registry access).

## Verified (no action)

The following were investigated and confirmed correct:

- **Trust gate placement** (`src/codex_app_server/runtime.rs:412`): runs before `CodexAppServerClient::connect`; `--auto` path (`src/cli/auto.rs:31`) uses `run_codex_noninteractive_*` and does not flow through `run_interactive_session`, so there is no double-gating.
- **Alias unification** (`src/codex_app_server/runtime.rs:1041-1056`): `execution_alias_match` chains both alias sets; `is_planning_active_implementation_alias` correctly restricts the rewrite to `EXECUTION_MODE_ALIASES` only. New tests cover the "exit planning workflow" pass-through and the unrelated-input no-op.
- **`fallback_notice` predicate** (`vtcode-core/src/primary_agent.rs:220`): suppresses `build` (case-insensitive) and empty/whitespace; returns the trimmed name for any other miss. `DEFAULT_PRIMARY_AGENT_NAME == "build"`, matching the `from_spec(&builtin_primary_build_agent())` fallback target.
- **`LazyLock` memoization** (`vtcode-config/src/subagents.rs:478`): `BUILTIN_SUBAGENTS.clone()` returns an owned `Vec<SubagentSpec>`; the static is never mutated post-init. `discover_subagents` (`subagents.rs:398`) extends with owned specs. Individual `builtin_*_agent()` constructors remain fresh-allocating `pub fn`s.
- **`plan` mode change** (`vtcode-config/src/subagents.rs:651`): `is_primary()` still true (so `resolve_primary_agent` still discovers it); `is_subagent()` now false, so `/agents` palette and `@plan` mentions no longer treat it as delegatable. Matches the documented design ("projects that want a delegatable plan subagent define their own `.vtcode/agents/plan.md`").
- **`LIFECYCLE_CLEANUP_TOOLS`** (`vtcode-config/src/constants/tools.rs:122`): `&[WAIT_AGENT, CLOSE_AGENT]` — exact equivalence to the prior `==` check; `RESUME_AGENT` intentionally excluded.
- **`transition_to_planning_workflow`** (`planning_workflow_state.rs:123`): `enable_planning()` already calls `planning_workflow_state.enable()`; the removed redundant call was safe to drop. The extra catalog epoch bump from `set_planning_phase` (on top of `enable_planning`'s bump) is a harmless no-op.

## Fixes Applied

1. **M-1** — Migrated the missed phase transition in `src/agent/runloop/unified/turn/context/response_handling.rs:240` from `planning_workflow_state().set_phase(...)` to `set_planning_phase(...)`, closing the catalog-invalidation gap.

## Verification After Fix

- `cargo build --workspace` — clean.
- `cargo clippy --workspace --all-targets` — clean.
- `cargo test -p vtcode-core --lib primary_agent` — 30 passed.
- `cargo test -p vtcode --bin vtcode runtime::tests` — 43 passed (includes new alias tests).
- `cargo test -p vtcode --bin vtcode workspace_trust` — 9 passed (includes 2 new trust-gate tests).
- `cargo test -p vtcode-core --lib planning_workflow_facade` — 2 passed (new facade tests).
- `cargo test -p vtcode-config --lib subagents` — 27 passed.
- `cargo test --workspace` — exit 0 (full suite green after the pre-existing-drift fixes below).

## Pre-existing test/code drift found during verification (all fixed)

While running the full workspace test suite as a final gate, four pre-existing test failures surfaced. None were introduced by this remediation; all are test/code drift from earlier feature commits, exposed/fixed during this review. All fixed at the root cause (KISS):

### [Low] V-1: Recovery fallback wording assertion drift

**File:** `src/agent/runloop/unified/turn/turn_loop/tests.rs:22`
**Root cause:** commit `3079b7620` ("clean up recovery messaging") reworded `RECOVERY_SYNTHESIS_FALLBACK_FINAL_ANSWER` from "...No additional tool call was applied..." to "...no tool call applied...", but the test still asserted the old substring.
**Fix:** assert `.contains("no tool call applied")` (the actual current wording).

### [Low] V-2: Cache e2e exact-equality drift

**File:** `tests/cache_e2e_tests.rs:167`
**Root cause:** commit `89e92b277` ("duplicate tool call detection") annotates reused read-only responses with `reused_recent_result`/`tool`/`reused_result_note`; the test asserted exact `==` between first and second calls.
**Fix:** compare the cached `items`/`total` (content equality) and assert `reused_recent_result == true` on the second call — a stronger, less brittle assertion that verifies the caching contract directly.

### [Low] V-3: `test_config_module_integration` CWD-config brittleness

**File:** `tests/integration_modular.rs:40`
**Root cause:** the test loaded config from `.` (CWD), picking up any local gitignored `vtcode.toml` (e.g. a custom provider such as "atlascloud"), then asserted `Provider::from_str(...).is_ok()`. Environment-dependent and non-deterministic.
**Fix:** load from an isolated `tempfile::tempdir()` so defaults apply deterministically. (The user's local config is legitimate and untouched.)

### [Low] V-4: `real_execute_tool_ref_optimization_test` drift + flaky timing

**File:** `vtcode-core/tests/real_execute_tool_ref_optimization_test.rs:54,196-197`
**Root cause:** (a) same duplicate-call-detection drift as V-2 — exact-equality on reused responses; (b) `assert!(unoptimized_duration.as_millis() > 0)` flakes on fast machines where the 5-iteration loop completes in <1ms.
**Fix:** (a) compare `content`/`metadata` instead of full equality; (b) use `as_nanos() > 0` for a precision that never flakes.

### [Low] V-5: Stale `removed_mode_command_is_not_registered` test

**File:** `vtcode-skills/src/command_skills.rs:598`
**Root cause:** commit `da316c634` added a test asserting `/mode` was unregistered; later commit `ef1b52b6f` re-added `/mode` as a live feature (mode palette + `SelectPrimaryAgent` for build/auto/duck/plan) but the stale test was never updated. This sits squarely in the audited mode system.
**Fix:** replaced the stale assertion with `mode_command_switches_active_agent_mode`, asserting `/mode` is registered and described as a mode switch.

## Bug ranking summary

| Severity | Count | Items |
|----------|-------|-------|
| Critical | 0 | — |
| High | 0 | — |
| Medium | 1 | M-1 (missed `set_planning_phase` migration) |
| Low | 5 | V-1..V-5 (pre-existing test/code drift) |

All real bugs fixed. No false positives remain unfixed in the audited code paths.

## Out of scope (deliberately deferred)

- **Block I (unify `WorkspaceTrustLevel`/`SafetyTrustLevel` behind `TrustTier`):** assessed and deferred. Verified the two enums do NOT interact at any bridge (`should_enforce_safe_mode_prompts` uses only `WorkspaceTrustLevel`; `SafetyTrustLevel` is an internal safety-gateway tier that defaults to `Standard` in production and is only set to `Full` in tests, with no serde). `WorkspaceTrustLevel` is part of the serialized ACP config schema (public protocol). Forcing unification touches the schema and conflates two concepts with different cardinalities (workspace policy vs session approval-bypass tier) — design churn, not KISS, with no correctness bug. Recommended as a separate focused follow-up.
- **6 pre-existing `ast-grep scan` let-chain lints:** baseline-confirmed pre-existing (present with this diff stashed); not introduced by this work. Out of scope for this release.

