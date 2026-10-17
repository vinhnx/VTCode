---
feature: session-efficiency
status: delivered
updated: 2026-09-28
branch: feat/session-efficiency
commits: 28e10edda..1df97c14b
---

# Session Efficiency (cost + reliability)

## Report

**What was built** — Session-store telemetry (2026-09-27 vs 09-28) showed three efficiency bottlenecks: 8–9 empty 0-turn
sessions/day, research runs at ~1M input tokens/turn, and deferred `code_search` pushing models to shell out
(`exec_command` 495→1110). This delivery closes all three plus exit visibility. Retention phase-0 and
`evict_zero_turn_completed_store` drop 0-turn completed shells (including NewSession/resume close paths after the
liveness lock is released). `tool_result_clearing` defaults drop 100k/3 → **40k/2** so tool results clear before they
dominate research turns. `code_search` + `grep_file` stay always-eager — extra schema tokens are cheaper than huge shell
outputs. Exit stats always show `Cost $…` when pricing is known and `End <reason>`, including cancelled runs.

**Verification** — `./scripts/check-dev.sh` PASS.
`cargo nextest run -p vtcode-memory -p vtcode-core -p vtcode-config --lib` **4340 passed, 2 skipped**. Postamble/exit
tests 15/15. First review **request-changes** (missing `end_reason_label`, NewSession/resume skip eviction, untested
liveness) → fixed in `1df97c14b` (End label, eviction after `emitter.finish()`, held-lock + pinned no-op tests).

### Journey log

- Empty shells have `status: completed` only after a `thread.completed` event — `complete()` alone leaves `active`;
  tests must append the terminal event.
- `session_dir_is_live` cannot tell self vs other process: delete only after the owning `SessionEventLog` is dropped
  (`emitter.finish()`), or eviction silently no-ops.
- Loop-exit `continue`s (NewSession/resume) bypass postamble tail code — session-end hooks belong right after
  `emitter.finish()`, not in the exit-summary block.
- Deferred structured search looked leaner on paper but cost more in practice (shell-out + huge tool outputs). Empirical
  trade-off over the Codex-4 purity rule.

## [S1] Problem

Session-store comparison of 2026-09-27 vs 2026-09-28 (13 sessions each) shows three efficiency bottlenecks that fight
both cost and reliability (HarnessTax: lower first-call/per-turn tax without hurting success):

1. **Empty session spam** — 8–9 of 13 sessions per day are 0-turn cancelled shells (`thread.started` +
   `thread.completed` only, ~457 B). They pollute `.vtcode/sessions/`, hide real work, and suggest abandoned/failed
   opens.
2. **Long-run context cost** — research/audit sessions run at **~1M input tokens/turn** (median active day: 164k/turn;
   heavy runs 1.0–1.7M). `tool_result_clearing.trigger_tokens` defaults to **100_000** with `keep_tool_uses: 3`, so tool
   results accumulate far past what a research turn needs before any clearing.
3. **Search → shell drift** — `code_search` fell from 60 calls (09-27) to 12 (09-28) while `exec_command` doubled (495 →
   1110). After lean defaults, `code_search`/`grep_file` are deferred outside planning, so models shell out via
   `rg`/`git` and feed huge outputs into the prompt.

Additionally, heavy sessions that end **cancelled** print tokens/cache but **not USD cost** (`budget_limit` is only
shown when a USD budget is configured), so a 17M-token cancelled run leaves no cost summary.

## [S2] Design

Settled (grill 2026-09-28): all three bottlenecks in one delivery; optimize **both cost and reliability**; follow the
HarnessTax research plan (lower tax per solve, keep success).

### S2-empty: do not retain 0-turn cancelled session stores

- A session whose `manifest.turn_count == 0` and `status` is terminal (`completed`/`cancelled` outcome with zero turns)
  is **not worth keeping**.
- On thread completion/close: if `turn_count == 0`, delete the session store directory (events + manifest + index)
  instead of leaving an empty shell. Best-effort; failure is logged and non-fatal. Runs on every close path (including
  NewSession / resume) after `emitter.finish()` drops the liveness lock.
- Periodic/startup retention also drops any existing 0-turn completed stores (migration for today’s spam).
- Live-session liveness lock (`session_dir_is_live`) still protects sessions a process holds open.
- Do not change behavior for sessions with `turn_count >= 1`.

### S2-cost: cheaper long research turns

- Lower `agent.harness.tool_result_clearing` defaults: `trigger_tokens` **100_000 → 40_000**, `keep_tool_uses` **3 →
  2**. Same contract (request-only stub of older tool results; durable history unchanged).
- Keep `auto_compaction_enabled` default true. Do not change compaction thresholds in this delivery (out of scope) —
  clearing is the cheap lever.
- Docs: CONFIG_FIELD_REFERENCE + EXECUTION_POLICY note the new defaults and why (HarnessTax per-turn tax).

### S2-search: keep structured search on the wire

- Extend the always-eager set in `is_core_tool_entry` with **`code_search` and `grep_file`** (in addition to Codex-4
  `exec_command`/`write_stdin`/`search_tools`/`apply_patch`).
- Rationale: empirical session data shows deferred structured search is worse for cost than the extra ~schema tokens —
  models shell out and pay in huge tool outputs. HarnessTax treats complexity as an empirical trade-off; this is that
  trade.
- Planning exception and `keeps_entry_available` unchanged. Update
  `lean_defaults_defer_planner_and_skills_tools_outside_planning` (code_search/grep_file no longer in the deferrable
  list).

### S2-exit: always show cost + outcome on exit

- `ExitData` gains `total_cost_usd: Option<f64>` and `end_reason_label: &'static str` (e.g. `completed` / `cancelled` /
  `error`).
- Stats line (or one extra dim line) always prints estimated session cost when known, even without a configured USD
  budget: `Cost $0.42` (effective) plus existing tokens/cache/first-call overhead, and `End <reason>`.
- Printed on every exit path that already calls `print_exit_summary`, including cancelled.
- Unknown pricing stays unknown (no fake $0).

## [S3] Out of Scope

- Changing auto-compaction thresholds or compaction prompts.
- Removing tools from the registry.
- Raising schema/first-request budgets.
- Windows PTY / auth-env flake fixes.
- Model-picker WIP (already in tree, separate).

## Tasks

- [x] T1: zero-turn session store cleanup — acceptance: close/complete with `turn_count==0` deletes the store; startup
      retention drops existing empty completed stores; `turn_count>=1` untouched; liveness lock still protects open
      sessions (covers: S2-empty)
- [x] T2: tool-result clearing defaults 40k/2 — acceptance: config defaults and validation tests assert 40_000 / 2;
      CONFIG_FIELD_REFERENCE updated (covers: S2-cost)
- [x] T3: `code_search` + `grep_file` always-eager — acceptance: `is_core_tool_entry` keeps both without planning;
      lean-defer test updated; Codex-4 baseline test extended (covers: S2-search)
- [x] T4: exit cost + reason always shown — acceptance: `build_stats_line` includes `Cost $…` when known and the
      end-reason label on cancel/error/completed; unit tests cover priced and unknown-cost cases (covers: S2-exit)
- [x] T5: docs (EXECUTION_POLICY + CONFIG_FIELD_REFERENCE) — acceptance: new defaults and search-trade rationale
      documented (covers: S2-cost, S2-search; depends: T2, T3)
