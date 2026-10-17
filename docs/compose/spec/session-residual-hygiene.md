---
feature: session-residual-hygiene
status: delivered
updated: 2026-09-26
branch: fix/session-residual-hygiene
commits: 69c967153..f1aac4d8d
---

# Session Residual Hygiene

## Report

**What was built** — Re-audit of `session-vtcode-20260925T234343Z_201620-81429` after `session-audit-harness-fixes`.
Four residual defects were closed in shipped surfaces:

1. **Bounded rewind pins.** `SnapshotManager::complete_session_navigation` runs on `thread.completed`, keeps only
   `REWIND_ACTIVE_KEEP` (5) newest `active` turns, clears `redo` (retiring recovery records), and removes `rewind.lock`.
   Dropped turn snapshots become prune-eligible immediately instead of staying protected for the 30-day age window.
2. **Session-scoped context artifacts.** Orient and memory-envelope readers drop
   `current_spec`/`contract`/`evaluation`/`feature_list`/sprint/outcome summaries whose file predates the session
   directory's `created()`. A present-but-stale file no longer falls back to a prior envelope copy. Fully-checked
   `current_task.md` is archived on terminal completion so the live path is clear for the next plan.
3. **`verify:` parse.** `split_verify_command_tail` splits `verify: [a] and verify: [b]` and strips brackets, so
   `'] and verify: ['` can no longer leak into `verification_summary`.
4. **Retention.** `mark_abandoned_active_sessions` flips idle `active` manifests past `max_age_days` to `completed`
   (skipping the preserved session and pins) so crashed threads can be evicted; `apply_retention` then reclaims them.
   `prune_history_envelopes` caps `.vtcode/history/*.memory.json` at 50 newest / age cutoff, matching filenames on the
   32-char sanitized session id.

Live workspace cleanup (operational, not in the diff): removed `rewind.lock`, Jul 24 fixture trio, and 941 aged history
envelopes; archived the finished `current_task.md`; trimmed 5 branch files to a 5-turn rewind window; marked 8 abandoned
actives and removed 9 sessions past the count cap; pruned 80 unprotected turn snapshots (31.7 MB). `filesnap/`
content-addressed blobs (~124 MB) remain and are reclaimed through `retire_snapshot` GC on the next age-based prune.

**Verification** —

- `cargo fmt --all -- --check` PASS
- `cargo clippy --locked -p vtcode -p vtcode-core -p vtcode-memory --tests -- -D warnings` PASS
- `cargo nextest run --locked -p vtcode-memory -E 'test(retention) or test(abandoned) or test(mark_abandoned) or test(symlink)'`
  12/12 PASS
<!-- markdownlint-disable-next-line MD013 -->
- `cargo nextest run --locked -p vtcode-core -E 'test(complete_session_navigation) or test(split_verify) or test(structured_verify) or test(stale_spec) or test(handoff_artifact) or test(archive_completed) or test(envelope) or test(orient)'`
  22/22 PASS
- `cargo nextest run --locked -p vtcode -E 'test(prune_history) or test(harness) or test(retention)'` 82/82 PASS
- `cargo nextest run --locked -p vtcode-memory -p vtcode-core --no-fail-fast` 4058/4059 PASS; 1 FAIL
  `harness_terminal_runs_retain_completed_sessions_until_close` — **PRE-EXISTING** (reproduces with changes stashed)
- `cargo nextest run --locked -p vtcode --no-fail-fast -E 'not binary(/cli_harness_failures/)'` 3368/3369 PASS; 1 FAIL
  `registry_exhaustion_latches_runloop_and_blocks_the_next_inspection` — **PRE-EXISTING**

**Journey log** —

- The prior audit's age-cutoff protection ("branch files older than 30 days stop pinning turns") was correct but too
  slow: 16-turn `active` lists from recent completed threads kept most of `checkpoints/` pinned. Bounding the list at
  completion is the missing half.
- History envelope filenames use `sanitize_session_id` (32-char truncate), not the raw session id. Matching on
  `contains(raw_id)` is a dead preserve branch — a reviewer caught this after a toy-id unit test passed by accident.
- `retention_preserves_active_sessions` is a hard contract (never evict `status=active`). Abandoned cleanup therefore
  has to flip stale actives to `completed` first (`mark_abandoned_active_sessions`), not make them direct eviction
  candidates.
- `session_artifact_cutoff` is intentionally fail-open on `created()` only. Falling back to `modified()` made the cutoff
  "now" and over-filtered artifacts written at session start. A 24h grace keeps handoff artifacts written before
  `vtcode` launches while still dropping week-old fixtures.
- filesnap blob GC only runs through `retire_snapshot`/`cleanup_retired_snapshots`. Force-deleting turn JSON orphans the
  blobs; do not hand-delete snapshots if you want the content store reclaimed.
- Deep review of the first implementation pass found nine issues (handoff cutoff regression, loose envelope matching,
  fixed temp name, symlink follow in `mark_abandoned`, constraint re-adoption, unconditional `rewind.lock` delete). All
  nine were fixed and re-reviewed clean.

## [S1] Problem

Re-audit of `session-vtcode-20260925T234343Z_201620-81429` after the delivered `session-audit-harness-fixes` pass. That
pass fixed preflight-circuit sibling skips, approval-cache growth, and spool leftovers. Residual state and context
contamination still let one session affect later ones and still grow `.vtcode` without bound.

Evidence on the live workspace after the prior pass:

1. **Completed sessions pin turn checkpoints for the full 30-day window.** `.vtcode/checkpoints/` holds 125
   `turn_*.json` snapshots (~59 MB) and 14 `branch_*.json` navigation files; the directory is 185 MB total.
   `protected_turns_with_cutoff` only ignores a branch file after `max_age_days` (30). The audited session's checkpoint
   still has `active: [1435..1450]` (16 turns) even though the thread ended `cancelled`/`exit`. `prune_snapshot_budget`
   never evicts protected turns, so a handful of recent sessions pin most of the directory.

2. **Global task artifacts leak into later sessions' memory envelopes.** `.vtcode/tasks/current_spec.md` is a Jul 24
   fixture (`# Execution Spec` / `Explore the codebase and summarize.`). The audited session's `history/*.memory.json`
   recorded `spec_summary: "Spec: Explore the codebase and summarize."` from that file, not from the session's real
   objective ("Fix review finding 1"). `current_contract.md` and `current_feature_list.md` are the same stale fixture
   family. `current_task.md` still holds the finished checklist.

3. **Malformed `verify:` lines leak into `verification_summary`.** Plan step 5 of the audited work wrote
   `verify: [cargo nextest run -E '...'] and verify: [cargo check --locked` on one line.
   `collect_structured_verify_commands` takes the whole tail of `verify:` as one command, so the history envelope's
   `verification_summary` contains the literal `'] and verify: ['` splice.

4. **Session retention skips abandoned `active` stores.** `vtcode_memory::apply_retention_preserving` (default 50
   sessions / 30 days) is wired at harness finalization, but `retention_candidates` skips every store whose manifest
   `status == "active"`. On disk: 167 manifests, 111 still `active`, 35 of those older than 7 days, oldest 43 days.
   Crashed/killed threads never see `thread.completed`, so they stay `active` forever and retention never removes them.
   5 sessions are retention-pinned.

5. **`history/` envelopes have no retention.** Memory envelopes are still written to `.vtcode/history/*.memory.json`
   (1005 files). `gc_legacy` can delete the directory after migration, but interactive sessions keep writing envelopes
   there and nothing caps the count.

6. **Stale rewind lock.** `checkpoints/rewind.lock` is a zero-byte file dated 2026-09-24 and is never cleared on thread
   completion.

## [S2] Design

### A. Stop completed threads from pinning their whole turn history

On `thread.completed` (any outcome), rewrite the session's `branch_*.json` navigation record so only a **bounded rewind
window** of turns stays protected:

- Keep `active` as the last `REWIND_ACTIVE_KEEP` (5) turn numbers, plus any `pending` recovery, plus `redo` (already
  cleared on completion).
- Drop older `active` entries from the branch file. Their `turn_*.json` snapshots become eligible for
  `prune_snapshot_budget` / `cleanup_old_snapshots`.
- The existing age-cutoff rule in `protected_turns_with_cutoff` stays as the backstop for branches that never see a
  completion event.

Acceptance shape: after a thread completes with `active: [1..20]`, the branch file lists at most 5 active turns (the
highest numbers) and the other 15 snapshots are unprotected.

### B. Session-scoped context artifacts

Stop later sessions from inheriting stale global task/spec files:

1. When building a memory envelope / orient snapshot, drop `spec_summary`/`contract_summary`/`feature_list_summary`
   whose source file `mtime` is older than the current session's `created_at`. A leftover fixture must not describe a
   new session.
2. On `thread.completed` with a terminal outcome (`completed`, `cancelled`, `exit`): if `current_task.md` exists and
   every checklist item is `- [x]`, move it aside to `.vtcode/tasks/archive/current_task-<session_id>.md` (keep the live
   path clear for the next plan). Do **not** delete uncompleted checklists.
3. Delete the known-stale Jul 24 fixture trio live (operational, not in the diff): `current_spec.md`,
   `current_contract.md`, `current_feature_list.md` when their content is exactly the fixture text.

### C. Parse `verify:` lines without splicing

`collect_structured_verify_commands` must:

- Strip surrounding `[` `]` from a command.
- Split a single `verify:` line on `and verify:` so `verify: [a] and verify: [b]` yields two commands.
- Drop empty commands after strip.

The plan tracker already accepts repeated `verify:` lines; the fix is downstream of that so both shapes land in
`verification_summary` as separate bullets.

### D. Retention: abandoned `active` sessions and `history/` envelopes

1. **Abandoned active stores become eligible.** In `retention_candidates`, treat `status == "active"` as evictable when
   `updated_at` is older than `RetentionPolicy::max_age_days` (default 30) — the same age budget already applied to
   completed sessions. Still skip a preserved session id and retention-pinned stores. A live session is younger than the
   cutoff, so it remains safe.
2. **`history/` envelope cap.** After session finalization (same `spawn_blocking` site that already calls
   `apply_retention_preserving`), prune `.vtcode/history/*.memory.json`:
   - Keep the most recent `HISTORY_ENVELOPE_KEEP` (50) files by mtime.
   - Delete envelopes older than `max_age_days`.
   - Never delete the envelope belonging to the session being finalized.
3. Existing `RetentionPolicy::default()` (50 sessions / 30 days) and `apply_retention_preserving` keep their contract.

### E. Clear stale `rewind.lock`

On `thread.completed`, remove `checkpoints/rewind.lock` when this session owns it (best-effort; missing file is
success). Live cleanup also deletes the current zero-byte Sep 24 lock.

## [S3] Out of Scope

- Already delivered by `session-audit-harness-fixes`: preflight LLM-mistake classification, approval-cache budget, spool
  startup prune, quoted-heredoc injection false positives.
- Unifying `history/` envelopes into the per-session `derived/memory.json` store (path migration). This pass only caps
  the legacy directory.
- Model-side thrashing (repeated identical test invocations).
- Changing `max_snapshots` (50) or `DEFAULT_MAX_AGE_DAYS` (30) defaults.
- `/revert` UX and checkpoint format changes beyond the branch-file `active` trim.
- TODO.md (owner-only).

## Tasks

- [x] T1: Trim completed threads' branch `active` list to a bounded rewind window on `thread.completed` — acceptance:
      unit test writes a branch file with 20 active turns, runs the completion trim, asserts ≤5 remain and the dropped
      turn ids are unprotected (covers: S2.A)
- [x] T2: Drop stale global task/spec summaries older than session start, and archive a fully-checked `current_task.md`
      on terminal completion — acceptance: envelope built with an older-than-session `current_spec.md` has
      `spec_summary: None`; fully-checked tracker is archived and live path cleared (covers: S2.B)
- [x] T3: Split/strip `verify:` command collection so bracketed multi-commands do not splice — acceptance:
      `verify: [cargo nextest run -E 'x'] and verify: [cargo check --locked` yields two clean commands in
      `verification_summary` (covers: S2.C)
- [x] T4: Evict abandoned `active` session stores past `max_age_days`, and cap `history/*.memory.json` at 50 newest /
      age cutoff at finalization — acceptance: retention removes an `active` store whose `updated_at` is older than the
      cutoff and skips a young `active` one; history prune keeps the current session's envelope (covers: S2.D)
- [x] T5: Remove stale `rewind.lock` on terminal completion — acceptance: completion of a session that created the lock
      deletes it (covers: S2.E)
- [x] T6: Live residual cleanup on the workspace and record results in Report — acceptance: checkpoints pruned to
      budget, fixtures gone, rewind.lock gone, history capped, Report filled (covers: S1)
