---
feature: session-close-and-skill-markup-harden
status: delivered
updated: 2026-09-25
branch: fix/session-close-and-skill-markup-harden
commits: 171b1dce2..f4de874ea
---

# Session Close and Skill Markup Harden

## Report

**What was built** — PTY session teardown now uses a two-phase bounded reap: poll `try_wait` for 2s, re-escalate
process-group + direct SIGKILL, poll 1s more, then abandon only truly unkillable children. `PtySessionHandle::Drop`
reaps after kill so Drop-only paths cannot leave zombies. Skill sub-LLM textual `tool_call` markup remains gated by
`SkillToolScope` and registry policy; a new regression proves out-of-scope textual calls never reach the registry and
force tool-free synthesis.

**Verification** — `cargo fmt --all -- --check` PASS; `cargo clippy -p vtcode-core --tests -- -D warnings` PASS;
`cargo nextest run` filters for skill_executor / exec_session / pty: 19/19 then 232/232 PASS, including
`force_terminate_reaps_live_pty_child` and `skill_executor_denies_textual_tool_markup_outside_skill_scope`. Independent
reviewer: approve, all T1–T3 met.

**Journey log** —

- Process-group kill was already present; the real gap was Drop never calling `try_wait` and reap giving up after one
  budget with no SIGKILL re-escalation.
- Reviewer flagged the first T1 test as overclaiming (timeout-only asserts); strengthened to require a reaped exit
  status and renamed off the inert `trap '' TERM` framing (`force_terminate` never sends SIGTERM).
- `try_wait` caches exit status after the first `Ok(Some)`, so post-reap Drop/close paths are cheap.

## [S1] Problem

The 2026-09-24/25 risk review of `4f90643b0..171b1dce2` found two residual high-confidence gaps after the session-close
and skill textual tool-call work:

1. **PTY close residual resource leak.** Close already sends process-group SIGTERM/SIGKILL and polls `try_wait` with a
   2s bound, but:
   - `reap_child_bounded` abandons after the first budget with only a warn — no second SIGKILL escalation when the child
     is still live.
   - `PtySessionHandle::Drop` kills the child but never reaps it, so a close-timeout or Drop-only path can leave a
     zombie until process exit.

2. **Skill textual `tool_call` markup lacks an out-of-scope regression.** `9c9f94909` routes textual markup through
   `skill_tool_scope.permits` and registry dispatch, and covers unknown tools, but there is no test that a textual call
   for a tool the skill scope does not allow is rejected and forced into tool-free synthesis (and never reaches the
   registry).

## [S2] Design

### PTY child termination

- Keep existing process-group kill (`graceful_kill_process_group_default`, `kill_process_group` /
  `kill_process_group_by_pid`) and direct `child.kill()`.
- `reap_child_bounded` becomes a two-phase reap:
  1. Poll `try_wait` for `CHILD_REAP_TIMEOUT_MS` (2s).
  2. On timeout, re-escalate: process-group SIGKILL (when `child_pid` is known)
     - `child.kill()`, then poll a shorter `CHILD_REAP_ESCALATED_TIMEOUT_MS` (1s). Return whether the child was observed
       exited.
- Callers (`graceful_terminate`, `force_terminate`) pass the pid so escalation can hit the process group. Drop uses the
  same reap after its kill (fixes zombie leak on Drop-only paths).
- Still never call unbounded `Child::wait`. Abandonment after the escalated budget remains for truly unkillable children
  (uninterruptible sleep); that residual is documented, not eliminated.

### Skill textual tool-call authorization

- No production behavior change required for the happy path: textual calls already go through
  `SkillToolScope::from_definitions(definitions)` and `skill_function_tool_permitted` before `execute_public_tool_ref`.
- Regression tests:
  - Textual `bash`/`exec_command` markup is parsed but **denied** when the skill tool definitions do not include that
    tool; the registry tool is never invoked; the executor forces tool-free synthesis.
  - The denied tool name appears in the forced-synthesis reason so the model cannot silently claim success.

### Out of Scope

- Changing skill location precedence or directory-name match policy.
- Raising/replacing session close outer timeouts (`EXEC_SESSION_CLOSE_TIMEOUT`).
- Re-running the full workspace suite beyond the changed crates.

## Tasks

- [x] T1: Two-phase reap with SIGKILL escalation + Drop reap — acceptance: `reap_child_bounded` escalates after first
      budget; Drop path reaps after kill; existing close tests still pass (covers: S2)
- [x] T2: Skill textual out-of-scope rejection regression — acceptance: test proves textual markup for a non-scoped tool
      never hits the registry and forces tool-free synthesis (covers: S2)
- [x] T3: Verify changed crates (fmt/check/nextest) — acceptance: commands exit 0 or failures marked PRE-EXISTING
      (covers: S2)
