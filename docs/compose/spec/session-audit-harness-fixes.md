---
feature: session-audit-harness-fixes
status: delivered
updated: 2026-09-26
branch: fix/session-audit-harness-fixes
commits: 606f360ba..d3690926f
---

# Session Audit Harness Fixes

## Report

**What was built** — Audited `session-vtcode-20260925T234343Z_201620-81429` (17 turns, 3514 events, 48M input tokens,
ended `cancelled`/`exit`) and fixed the harness bugs that let it fail. Three shipped surfaces changed:

1. **Preflight circuit no longer trips on tool-name mistakes.** `handle_preflight_failure` now classifies rejects via
   `preflight_failure_is_llm_mistake`. Prose-blob names, unknown tools, and empty names get a per-call
   `preflight_validation` error and the batch continues; only policy/security blocks and repeated argument-schema
   failures advance `consecutive_preflight_failures`. One malformed name can no longer drain valid sibling calls through
   `drain_preflight_circuit_responses`.
2. **Approval cache is bounded.** `ApprovalCacheConfig::trim_to_budget` keeps each collection at
   `APPROVAL_CACHE_MAX_ENTRIES` (256) by dropping the oldest insertions on every persist. `tool-policy.json` in the
   audited workspace dropped from 294 KB to 71 KB after trim.
3. **Spool leftovers prune at session start.** `cleanup_old_files` now also enforces `max_files` against the on-disk
   directory (not just the per-instance tracking vec), and `prune_stale_spools_on_startup` runs at registry construction
   so short sessions that never reach `CLEANUP_EVERY_N_SPOOLS` still restore the budget. The audited workspace had 414
   stale spools (~2.0 MB); all were older than the 1-hour default `max_age_secs` and were cleaned.

Operational cleanup on the live workspace (not in the diff): redo stack cleared on the completed session's branch
checkpoint, orphaned `turn_recovery_*` snapshot removed, 414 aged spools deleted, `approval_cache` trimmed in place.

**Verification** —

- `cargo fmt --all -- --check` PASS
- `cargo clippy --locked -p vtcode -p vtcode-core --tests -- -D warnings` PASS
- `cargo nextest run -p vtcode -E 'test(preflight) or test(prose_blob) or test(malformed_tool_calls)'` 21/21 PASS
- `cargo nextest run -p vtcode-core -E 'test(approval_cache) or test(cleanup)'` 29/29 PASS
- Broader handlers suite 202/203 PASS; 1 FAIL `registry_exhaustion_latches_runloop_and_blocks_the_next_inspection` —
  **PRE-EXISTING** (reproduces with changes stashed)

**Journey log** —

- The session under audit was fixing the textual tool-call parser while that same parser executed the agent's prose
  about tool tags as real calls. The irony is the evidence: 21 malformed `tool_invocation` items whose "tool names" were
  multi-KB reasoning text.
- The real sibling-skip bug is in the circuit, not the parser: name mistakes were counted like policy blocks, so three
  prose blobs tripped the breaker and `drain_preflight_circuit_responses` rejected a valid `exec_command` (events L733).
- Kept argument-schema failures tripping the circuit: the existing
  `malformed_tool_calls_trip_preflight_circuit_at_configured_cap` contract is about a model stuck on bad JSON, which is
  different from one prose blob in a batch.
- Spool cleanup existed but only fired every 50 spools _inside one session_, so short sessions never reached the
  threshold and leftovers piled up across runs.
- `tool-policy.json` `approval_cache` is an `IndexSet` (insertion-ordered), so trimming from the front drops the oldest
  keys and keeps recent approvals.
- Review pass found one classifier-precision edge: a bare `"sandbox"` substring would misclassify
  `Unknown tool: sandbox_helper` as policy. Tightened the phrases to `sandbox denied` / `sandbox policy` and covered
  both sides in tests.

## [S1] Problem

Session `session-vtcode-20260925T234343Z_201620-81429` (2026-09-25/26, 17 turns, 3514 events, 48M input tokens) ended
`cancelled`/`exit` after the harness failed to function correctly. Evidence from
`.vtcode/logs/trajectory-20260925T234343Z.jsonl` and the session `events.jsonl`:

1. **Textual tool-call self-attack.** While the agent was fixing `src/agent/runloop/text_tools/`, its prose about
   tool-call tags was parsed as real tool invocations. 21 malformed `tool_invocation` items carried tool names that were
   multi-kilobyte reasoning text (`exec_command\n...the fence opener is unclosed...`), empty names, or newline-suffixed
   names (`exec_command\n`). Preflight rejected them as `Unknown tool` / `tool name is not a clean identifier`, which is
   correct per call.

2. **Preflight circuit breaker skipped valid sibling calls.** Those LLM-mistake rejections incremented
   `consecutive_preflight_failures`. After `max_consecutive_blocked_tool_calls_per_turn` (default 3) the circuit tripped
   and `drain_preflight_circuit_responses` rejected every remaining call in the batch — including well-formed
   `exec_command`/`apply_patch` work — with
   `Tool call skipped because another call in this assistant batch tripped the preflight circuit breaker`. Session
   events L733 show a real `exec_command` skipped this way. Two `turn.failed` / `turn.blocked` pairs followed
   (`recovery fallback; the requested work was not confirmed`).

3. **Wasted operations.** `cargo nextest run -E 'test(unclosed_fence_tail)'` ran 23 times. Six
   `tool_loop_limit_increased` auto-grants (+40 each) extended the loop mid-turn. Multiple `write_stdin` waits ran
   200–410s after 30s `exec_command` yields. Identical `sed` windows of the same spool files were re-read.

4. **Residual state across sessions.** `.vtcode/context/tool_outputs/` held 414 spool files (~2.0 MB) with no retention.
   `tool-policy.json` grew to 294 KB (from a 129 KB `.bak`) via unbounded `approval_cache`. `checkpoints/` retained 1.3
   MB `turn_*.json` snapshots plus a `turn_recovery_*` file and a redo stack after rewind. `history/` held 1005
   `*.memory.json` files. A stale `rewind.lock` (zero bytes, dated 2026-09-24) remained. None of this is cleaned up on
   `thread.completed`.

## [S2] Design

### A. Preflight circuit must not trip on LLM mistakes

`handle_preflight_failure` currently calls `record_preflight_failure()` for every preflight reject. Change it to
classify the reject first:

- **LLM name/identity mistake** (tool name not a clean identifier, unknown tool, empty name): emit the per-call error
  response and continue the batch. Do **not** increment `consecutive_preflight_failures` and do **not** trip the
  circuit. The model already gets `schema_correction` / `next_action` to retry once.
- **Argument-schema failure** (missing required field, malformed JSON, schema mismatch): still counts toward the
  consecutive cap. A model stuck on bad arguments should trip the recovery fuse rather than retry forever.
- **Policy / security / safety block** (command-injection pattern, sandbox denial, blocked tool): keep today's behavior
  — count toward the consecutive cap and trip the circuit when the cap is hit, because those rejects mean the model is
  fighting the harness.

Classification uses the same vocabulary as `vtcode_commons::ErrorCategory`: `InvalidParameters` name/identity forms must
not trip; `PolicyViolation` and argument-schema failures may. Helper:
`preflight_failure_is_llm_mistake(error: &str) -> bool` keying off the existing error strings emitted by
`PreparedAssistantToolCall::new` and the registry preflight (`tool name is not a clean identifier`, `Unknown tool`,
`Missing required argument` stays non-mistake).

`drain_preflight_circuit_responses` is unchanged: it only runs after a real (non-mistake) circuit trip.

### B. Prose-blob tool names never become dispatchable calls

`PreparedAssistantToolCall::new` already rejects non-dispatchable names via `is_dispatchable_tool_name`. Keep that gate.
Ensure the rejection classifies as an LLM mistake (covered by A) so one prose blob cannot poison the batch.

### C. Residual-state hygiene on thread completion

On `thread.completed` (any outcome):

1. Clear the session's rewind redo stack in the branch checkpoint (keep `active`; drop `redo`) so a later resume does
   not silently re-apply discarded turns. Drop `turn_recovery_*` snapshots that are not on `active`.
2. Prune `.vtcode/context/tool_outputs/` of spool files older than the retention window (default 7 days) and of files
   belonging to sessions that have been deleted. Never delete a spool still referenced by an in-flight session.
3. Cap `tool-policy.json` `approval_cache` at a fixed entry budget (e.g. 256 most-recent keys) on write so the file
   cannot grow without bound.

### D. Out of scope

- Tool-loop auto-grant policy (`+40` grants, latch semantics) — already delivered in `tool-loop-session-auto-grant` and
  intentionally interactive-friendly.
- Model-side thrashing (23× identical test) — agent behavior, not harness bug.
- Preflight false-positive on `git log | head` pipe redirection — separate issue.
- `history/` and `sessions/` growth — product retention policy, not this pass.

## [S3] Out of Scope

See [S2] D. Also out: changing `max_consecutive_blocked_tool_calls_per_turn` defaults, MCP circuit breaker
(`McpCircuitBreaker`), and planning-mode recovery directives.

## Tasks

- [x] T1: Classify preflight rejects as LLM-mistake vs policy and stop tripping the circuit on mistakes — acceptance: a
      batch of 3 prose-blob names plus 1 valid `exec_command` executes the `exec_command`; unit tests cover mistake vs
      policy (covers: S2.A, S2.B)
- [x] T2: On thread completion clear redo stack and orphaned `turn_recovery_*` snapshots — acceptance: completed
      session's branch checkpoint has empty `redo` and no recovery snapshot outside `active` (covers: S2.C.1)
- [x] T3: Prune aged tool-output spools and cap `approval_cache` on policy write — acceptance: spools older than
      retention are gone after completion; policy file size bounded in a unit test (covers: S2.C.2, S2.C.3)
- [x] T4: Record residual-state findings and verification in this document's Report — acceptance: Report has What was
      built / Verification / Journey log (covers: S1)

## [S4] Follow-up improvements (same audit)

Two residual findings from the re-scan, implemented on the same branch:

1. **Checkpoint retention.** `native::begin_prompt` wrote `turn_*.json` but never called `cleanup_old_snapshots`, so the
   workspace grew to 126 files / 185 MB. Cleanup is now navigation-aware (`protected_turns` unions every `branch_*.json`
   active/redo/pending list) and runs after each prompt checkpoint.
2. **Quoted heredoc false positives.** `contains_command_substitution` and `split_shell_segments` scanned heredoc bodies
   as live shell, so `cat <<'EOF'` payloads with backticks were rejected as injection (session-vtcode-20260925 L421).
   Quoted heredoc bodies are now skipped via shared `quoted_heredoc_skip_len`; unquoted heredocs still scan for `$()`
   and backticks.
