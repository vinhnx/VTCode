---
feature: harness-fence-share-and-close-race
status: delivered
updated: 2026-09-25
branch: fix/harness-fence-share-and-close-race
commits: e127c0908..2a48c2705
---

# Harness Fence Share and Close Race

## Report

**What was built** — Fence scanning and tool-name hygiene now live in `vtcode_commons::text_fence` (shared by runloop
`text_tools` and skill `executor`); unclosed fences fail open so truncated streams still parse calls. `exec_session`
close-race regression holds `output_read_lock` past the acquire timeout and proves clean close + clean post-close read
error. Build↔Plan mid-turn auto-switches now grant a full **remaining** mode floor (tool calls + tool-loops) so
research/implementation is not starred by the prior phase's consumption — the failure mode in the user report (explicit
Plan works, auto-switch hits the tool-call limit).

**Verification** — `./scripts/check-dev.sh` PASS. Focused nextest: text_fence 3/3, text_tools + mode_switch +
prepared_tool + aliases 115/115, close-race + textual_skill + skill_executor 21/21.

**Journey log** —

- Mode floors of `max(limit, 120)` only help on an empty turn; mid-switch must grant `used + floor` remaining or the new
  phase inherits a spent cap.
- Unclosed-fence fail-open is an existing runloop contract (`unclosed_fence_tail_still_parses_tool_call`) — shared
  `unfenced_byte_ranges` must include the tail after a lone opener.
- `canonicalize_shell_tool_alias` must not collapse PTY tools into `exec_command`.
- Close-race test can hold the private `output_read_lock` from the same-module tests without production test hooks.

## [S1] Problem

Follow-up to `harness-textual-tool-call-harden` (merged `6ed1ea8b6`):

1. **Unfinished T5.** No regression proves that `exec_session` close proceeds cleanly when a pipe reader still holds
   `output_read_lock` past `EXEC_SESSION_OUTPUT_READ_LOCK_TIMEOUT` (no panic, no partial metadata).
2. **DRY violation.** Fence scanning (`fence_delimiter_line` / unfenced search) and clean-name checks are duplicated in
   `src/agent/runloop/text_tools/` and `vtcode-core::skills::executor`. Divergence risk is real — the first skill-side
   draft had a closer-line bug the binary copy did not.

## [S2] Design

### Shared fence + name module (`vtcode-commons`)

Add `vtcode_commons::text_fence` (pure infrastructure, no business logic):

- `is_fence_delimiter_line`, `unfenced_byte_ranges`, `find_unfenced_from`
- `is_clean_tool_name` (identifier, ≤ 64)
- `is_dispatchable_tool_name` (MCP-safe: no whitespace/backticks/dashes-of-prose)

Binary `text_tools` and `skills/executor.rs` re-use these; local copies deleted. `text_tools::canonical` keeps alias
mapping (tool policy, not fence infra).

### Close-race regression (`exec_session.rs` tests)

Same-module test: hold `ExecSessionRecord::output_read_lock` past the acquire timeout while `close_session` runs.
Assert:

- `close_session` returns `Ok` (or a clean timeout error — never panics)
- a subsequent `read_session_output` fails with a normal `Err`, not a panic
- no partial metadata: close result is a complete `VTCodeExecSession` or a single well-formed error

### Build↔Plan mode-switch budgets

Mid-turn mode switches (Build→Plan auto-`start_planning`, Plan→Build after approval) only raised the turn cap to the
mode floor (`max(limit, 120)`). If the prior mode had already consumed most of that cap, the new mode had almost no
remaining headroom and hit the tool-call / tool-loop limit. Explicit Plan worked because the turn started with a full
floor.

Contract: on a planning-on/off transition inside a turn, grant **remaining** headroom of at least one mode floor from
_now_:

- planning on: `max_tool_calls = max(current, used + PLANNING_WORKFLOW_MIN_TOOL_CALLS_PER_TURN)` and
  `max_tool_loops = max(current, step_count + PLANNING_WORKFLOW_MIN_TOOL_LOOPS)`
- planning off (implementation): same with `APPROVED_PLAN_MIN_TOOL_CALLS_PER_TURN`

Session safety limits are re-applied with the new `planning_active` so the session fuse is unlimited during research and
finite but not retroactively failing after implementation.

### Out of Scope

- Changing `EXEC_SESSION_CLOSE_TIMEOUT` / lock timeout budgets
- Recovery-path redesign (already gated by the clean-name/fence work)
- Windows process-group semantics (documented residual)

## Tasks

- [x] T1: Add `vtcode_commons::text_fence` + unit tests — acceptance: fence/name helpers covered; crate builds (covers:
      S2)
- [x] T2: Point `text_tools` and `skills/executor` at the shared module — acceptance: no local fence/name duplicates;
      existing textual-tool tests still pass (covers: S2; depends: T1)
- [x] T3: Close-race regression in `exec_session` tests — acceptance: test holds `output_read_lock` past timeout and
      proves clean close + clean post-close read error (covers: S2)
- [x] T4: Verify (check-dev + focused nextest) — acceptance: exit 0 or PRE-EXISTING marked (covers: S2)
- [x] T5: Build↔Plan mid-turn budget headroom — acceptance: unit/turn tests prove enter-planning and exit-to-build
      grant remaining floor after prior consumption (covers: S2)
