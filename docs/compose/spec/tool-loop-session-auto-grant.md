---
feature: tool-loop-session-auto-grant
status: delivered
updated: 2026-09-26
branch: feat/tool-loop-session-auto-grant
commits: c9635e4d0..d2c0c747c
---

# Tool Loop Session Auto-Grant

## Report

**What was built** — Interactive tool-loop limit hits no longer re-prompt after the user grants one increase. The first
successful `prompt_tool_loop_limit_increase` approval latches `SessionStats::tool_loop_grant_preauthorized` for the
process session. Later limit hits take the auto-grant path with the same max `+N` arithmetic as full-auto
(`auto_tool_loop_grant_increment`), continue the turn, and never open the modal. Denial/Esc/Stop leaves the latch clear
so the next hit prompts again. Full-auto auto-grant, the absolute hard cap, and the planning research synthesis path are
unchanged. Session-preauthorized grants use distinct wording ("Auto-granted … earlier grant this session preauthorized
further increases") and keep the existing `ToolLoopLimitIncreased` event kind.

**Verification** — `./scripts/check-dev.sh` PASS (fmt, clippy, cargo check). Focused nextest: tool-loop / latch /
grant-source / limit-prompts suites 19–24/19–24 PASS across runs. Two independent reviews: all 8 acceptance criteria
met, no critical findings; post-fix re-review CLEAN.

**Journey log** —

- Full-auto already had auto-grant (`full_auto_loop_grants_enabled` + `auto_tool_loop_grant_increment`); the interactive
  gap was a missing session latch, not missing increment math.
- `CrossTurnTracker` is session-scoped but lives in the outer orchestration loop, not `TurnLoopContext`. `SessionStats`
  is already `&mut` in the turn context and holds other session flags, so the latch belongs there.
- Reviewer noted the original `session_preauthorized_auto_grant_matches_full_auto_increment` test was tautological;
  replaced with an assertion that the latched source is never `Manual` plus explicit max-increment cases.
- `reset_for_fresh_execution` deliberately keeps the latch: the one-time HITL preference is process-session scoped, not
  conversation-context scoped (covered by `tool_loop_grant_preauthorized_survives_fresh_execution_in_session`).
- fmt-only import reorders in `session_loop.rs` / `session_loop_impl.rs` rode along from `cargo fmt --all` required by
  the gate.
- Post-delivery review-fix loop: moved the latch into `apply_tool_loop_grant` (Manual only), replaced `!= Manual` with
  an explicit `is_automatic()` allowlist, and collapsed three overlapping grant-source tests into one decision matrix.

## [S1] Problem

Interactive turns that exhaust the per-turn tool-loop limit open a human-in-the-loop overlay ("Tool Loop Limit Reached")
every time the limit is hit. Operators who already approved one increase must re-approve on every later hit in the same
session, which interrupts long builds with identical prompts.

The requested behavior: after the user grants a tool-loop increase once in a session, later limit hits auto-grant the
maximum remaining `+N` increment and continue without another prompt. The HITL overlay should appear only until that
first successful grant.

## [S2] Design

### Scope

- Applies to the ordinary (non-planning) tool-loop grant path in `maybe_handle_tool_loop_limit`
  (`src/agent/runloop/unified/turn/turn_loop_helpers.rs`).
- Session = current process session. The latch lives in session runtime state and is not persisted across restarts or
  session-archive resume.
- Full-auto auto-grant (`automation.full_auto.auto_grant_tool_limits`) is unchanged and still takes precedence.
- Planning research's budget-exhausted synthesis path is unchanged (it never shows this prompt).
- The absolute tool-loop hard cap (`tool_loop_hard_cap`) is unchanged and remains terminal.

### Latch semantics

Add a session-scoped flag on `SessionStats` (`src/agent/runloop/unified/state.rs`):

- `tool_loop_grant_preauthorized: bool` — `false` at session start.
- Set to `true` only after a **successful** interactive tool-loop grant (`prompt_tool_loop_limit_increase` returns a
  positive increment that `apply_tool_loop_grant` accepts).
- Denial / Esc / Stop / failed prompt does **not** set the flag. The next limit hit prompts again.
- No reset path in-session.

### Grant decision at limit hit

When `step_count >= current_max_tool_loops` and the turn is not planning and not already at the hard cap:

1. If `full_auto_loop_grants_enabled(...)` → existing full-auto auto-grant path (max increment via
   `auto_tool_loop_grant_increment`). Unchanged.
2. Else if `session_stats.tool_loop_grant_preauthorized` → auto-grant the same maximum increment
   (`auto_tool_loop_grant_increment`), without showing the modal.
3. Else → show `prompt_tool_loop_limit_increase` as today. On success, apply the clamped increment and set
   `tool_loop_grant_preauthorized = true`. On denial, keep the existing tool-free synthesis recovery path.

### Auto-grant amount

"Max `+N`" reuses `auto_tool_loop_grant_increment(current_limit, hard_cap, planning_active)`: the larger of
`MAX_TOOL_LOOP_INCREMENT_PER_PROMPT` (50) and remaining headroom, already clamped. The same arithmetic as full-auto, so
one unit-tested pure function covers both.

### User-facing wording

- Session-preauthorized auto-grant status/event must not claim "Full-auto". Wording distinguishes:
  - full-auto: existing "Full-auto auto-granted +N ..."
  - session-preauthorized: "Auto-granted +N tool loops (limit L, cap C); earlier grant this session preauthorized
    further increases."
  - manual: existing grant wording.
- Event kind remains `HarnessEventKind::ToolLoopLimitIncreased` (no new ThreadEvent type).

### Testing boundaries

- Pure decision helper for prompt-vs-auto (full-auto, preauthorized, first-hit).
- Latch set-on-success / not-set-on-denial.
- Increment math already covered by `auto_tool_loop_grant_increment` tests; add a case that session-preauthorized path
  uses that same helper result.
- Hard-cap still terminal when latched.
- Full-auto gate still wins over the session latch.

## [S3] Out of Scope

- Persisting the latch across process restarts or session archives.
- Changing session tool-call limit prompts (`prompt_session_limit_increase`) or `MAX_SESSION_AUTO_GRANT_TOTAL_HEADROOM`.
- Changing planning research budget-exhausted behavior or hard-cap arithmetic.
- New configuration fields. Behavior is unconditional after the first grant.
- A revocation/reset slash command for the latch.

## Tasks

- [x] T1: Add `tool_loop_grant_preauthorized` to `SessionStats` with accessors — acceptance: field defaults false;
      `mark_tool_loop_grant_preauthorized` / `tool_loop_grant_preauthorized` compile and unit-test. (covers: S2)
- [x] T2: Extract pure grant-policy helper and wire auto-grant when latched — acceptance: at limit with
      `preauthorized=true` and not full-auto, `maybe_handle_tool_loop_limit` uses `auto_tool_loop_grant_increment` and
      never calls `prompt_tool_loop_limit_increase`. (covers: S2; depends: T1)
- [x] T3: Set the latch only on successful interactive grant; leave denial unlatched — acceptance: successful grant path
      sets the flag; denial/synthesis recovery leaves it false. (covers: S2; depends: T1)
- [x] T4: Distinct session-preauthorized wording in status + harness event — acceptance: auto-grant after latch does not
      use the "Full-auto" string; manual wording unchanged. (covers: S2; depends: T2)
- [x] T5: Regression tests for scenarios (first prompt, post-grant auto, denial keeps prompt, hard cap terminal,
      full-auto precedence) — acceptance: `cargo nextest run` covers the decision matrix and latch transitions. (covers:
      S2; depends: T2, T3, T4)
- [x] T6: Update tool-loop user docs (`docs/config/TOOLS_CONFIG.md` and related grant docs) — acceptance: docs describe
      one-time HITL then session auto-grant of max +N. (covers: S2)
