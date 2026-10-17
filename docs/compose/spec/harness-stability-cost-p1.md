---
feature: harness-stability-cost-p1
status: delivered
updated: 2026-09-19
branch: fix/harness-stability-cost-audit
commits: a15e2c938..1423f36e5
---

# Harness Stability Cost P1

## Report

**What was built** — Four P1 harness fixes from the 2026-09-14 session-run audit: (1) Merge Gateway native requests now
send lineage-stable `session_id` body + `X-Session-Id` header for automatic cache-aware routing; interactive
`prompt_cache_key` stays session-stable (no prefix-hash suffix), and `merge_session_identity` strips any legacy
`-{16 hex}` suffix defensively. (2) Merge-gateway first-progress timeout floors at 120s when non-streaming fallback is
available, with a once-per-session advisory that abandoned streams may still be billed. (3) Memory envelopes prefer live
task-tracker objective over a stale prior envelope; on objective change, prior `task_summary`/`verification_todo` are
dropped rather than inherited. (4) Blocked handoffs copy session `events.jsonl`/ATIF into `{archive}-forensics/` and pin
the session against retention until resolved; resolution unpins when no other unresolved archive references the session.

Gate fixes required for green CI on this branch (aligned with existing contracts): sparse plan tracker recognizes bare
`Summary` labels; CLI smoke tests use `tool-policy status -- <workspace>` / `-- hellp`; default compiled prompts omit
literal `task_tracker` while keeping follow-through guidance.

**Verification** — `cargo fmt --check` PASS; `cargo clippy` default-members PASS; `cargo check` default-members PASS;
`./scripts/check-dev.sh --test` PASS — **8135 tests passed, 12 skipped**. Reviewer critical finding (session_id carried
prefix-hash suffix) fixed: interactive key unsuffixed + `merge_session_identity` strips residual hex suffix;
re-verified.

### Journey log

- Spec assumed P0 left `prompt_cache_key` session-stable; worktree base `a15e2c938` still suffixed it — T1 session
  identity was non-stable until the interactive key fix landed on this branch.
- `tool_catalog_cache_metrics.cache_hit` is harness catalog cache, not provider billing cache — do not treat ~95% as
  “provider cache works.”
- Sept 14 trajectory cache zeros were largely `parse_native_usage` hard-coding `None` before `4c6db1483`; later GLM logs
  show real `cache_read_tokens` (70–90%).
- Compiled default prompts must not name non-baseline tools (`task_tracker`) or baseline-tool tests fail; keep
  follow-through wording without the tool name.
- Retention deletes completed/blocked session dirs while leaving empty `active` shells — pin + archive forensics are
  required for blocker forensics.

## [S1] Problem

Audit of 2026-09-14 VT Code sessions found: weak Merge cache affinity without session identity; 60s first-token timeouts
double-billing via Gateway drain + non-stream retry; memory envelopes inheriting another session’s objective from
workspace-global `current_task.md`; retention erasing blocker forensics.

## [S2] Design

Settled decisions (2026-09-19 grill):

### Session identity on Merge native (covers S2-session)

- Native generate + stream send body `session_id` and header `X-Session-Id` when lineage is non-blank.
- Lineage = `prompt_cache_key` with `vtcode:merge:`/`vtcode:openai:` prefix and any residual `-{16 hex}` prefix-hash
  suffix stripped.
- Interactive assembly passes session-stable `prompt_cache_key` (no per-prefix suffix); prefix identity stays in
  `tool_catalog_hash` / `system_prompt_prefix_hash`.
- Blank lineage omits both fields. OpenAI-native / Anthropic shapes unchanged.

### Merge first-token timeout floor + billing advisory (covers S2-timeout)

- `llm_first_progress_timeout_secs(..., provider_name)`: non-planning + `merge-gateway` + `supports_non_streaming` →
  `baseline.max(120)`. Planning path unchanged (still ≤180).
- First merge stream-timeout → non-streaming fallback emits one session-scoped billing advisory.

### Memory envelope objective isolation (covers S2-memory)

- `objective`: update → live `task_snapshot` → prior only if live empty.
- When live objective differs from prior: take live `task_summary` or empty (no prior inherit); `verification_todo` =
  live + update only.
- Same objective keeps merge behavior.

### Blocker forensics + retention pin (covers S2-retention)

- Handoff copies `events.jsonl` + `derived/atif-trajectory.json` into `{archive}-forensics/` (best-effort).
- `retention-pin.json` sidecar; retention skips pinned sessions.
- Resolution unpins when no other unresolved archive references the session (including already-resolved early return).

### Docs

- `docs/tools/PROMPT_CACHING_GUIDE.md`: Merge session identity, stable keys, timeout floor, forensics/pin.
- `docs/development/vtcode-binary-gotchas.md`: blocker forensics + retention pin.

## [S3] Out of Scope

- P0 interactive cache-key/usage-mapping on main (landed separately; this branch includes the interactive key stability
  fix T1 requires).
- Per-session tracker files under `sessions/<id>/derived/`.
- Raising global `max_sessions`/`max_age_days`.
- Anthropic `cache_control` on merge `anthropic/*` routes.
- Verification-gate auto-continue policy for piped verifiers.
- Cost-oriented compaction for flash models on large windows.

## Tasks

- [x] T1: Merge native session identity — acceptance: unit test shows body `session_id` + lineage strip including legacy
      hex suffix; blank lineage omits both (covers: S2-session)
- [x] T2: merge-gateway first-progress timeout floor — acceptance: unit tests floor ≥120s for merge-gateway +
      non-streaming non-planning; planning cap still ≤180 (covers: S2-timeout)
- [x] T3: merge stream-timeout billing advisory — acceptance: first fallback emits one session warning; second does not
      (covers: S2-timeout; depends: T2)
- [x] T4: memory envelope live-objective precedence — acceptance: tests cover prior-A/live-B swap, todo replace on
      objective change, empty live summary drops prior, same-objective merge (covers: S2-memory)
- [x] T5: blocker forensics copy — acceptance: handoff copies events/ATIF when present; missing sources non-fatal
      (covers: S2-retention)
- [x] T6: retention pin for unresolved blockers — acceptance: retention skips pinned; resolution unpins when no other
      open blocker (covers: S2-retention; depends: T5)
- [x] T7: docs — acceptance: PROMPT_CACHING_GUIDE + binary gotchas document session identity, timeout floor,
      forensics/pin (covers: S2)
- [x] T8: verify — acceptance: `./scripts/check-dev.sh --test` green in worktree — 8135 passed (covers: all)
