# Agent / Harness Duplication & Optimization Report

**Date:** 2026-06-05 (initial audit) / 2026-06-06 (implementation)
**Scope:** `vtcode-core/src/core/agent/**`, `vtcode-core/src/prompts/**`, `vtcode-core/src/tools/**`, `src/agent/runloop/unified/**`
**Status:** Implemented — F1 and F4 resolved; F2/F3/F5 verified as non-issues.

All findings below are verified against source and ordered by value-to-risk.

---

## Architectural root cause

```
                    ╭──────────────────────────╮
                    │   shared: AgentRuntime    │
                    │   LoopDetector, policy    │
                    ╰────────────┬──────────────╯
            ┌────────────────────┴────────────────────┐
            ▼                                          ▼
 ╭───────────────────────╮               ╭───────────────────────────╮
 │ AgentRunner (core)    │   PARALLEL    │ UnifiedSessionRuntime      │
 │ runner/tool_exec.rs   │◀── DUP ──────▶│ runloop/unified/**         │
 │ runner/tool_args.rs   │   admission   │ tool_pipeline/, tool_      │
 │ compose_task_prompt   │   prompt      │ outcomes/, incremental_    │
 │ reduce_tool_result    │   output fmt  │ system_prompt              │
 ╰───────────────────────╯               ╰───────────────────────────╯
```

Two independently-grown agent loops (autonomous `AgentRunner` vs. interactive
`UnifiedSessionRuntime`) re-implement the same four concerns — tool-call
admission, argument normalization, output formatting/truncation, and
system-prompt assembly — instead of sharing a single path. This is the source of
most duplication below.

---

## Findings

### F1 — Tool-output truncation reimplemented ~15+ times — ✅ DONE
**Value: High · Risk: Low**

> **Resolved.** Added `truncate_within`, `head_tail_truncate`, and
> `wrap_text_words` to `vtcode-commons::formatting` (pure, unit + doc tested).
> Migrated `truncate_text_for_model`, `truncate_summary`, `truncate_preview`,
> `truncate_description`, `truncate_chars`, and `wrap_text_words` (2 local copies
> + helpers in `pty_stream/state.rs` and `tool_summary.rs`) to delegate.
> `truncate_line` (zero-alloc `Cow`, byte-based) and `truncate_output` (`&str`
> slice, now UTF-8 safe) left as-is to avoid perf/signature regressions.
> Fixed UTF-8 panic in `truncate_output` (`tool_orchestrator.rs`).

Char-based head/tail/middle truncation was duplicated across the tree. The table
below shows the original sites and their current status:

| Location | Function | Style | Status |
|---|---|---|---|
| `src/.../error_handling.rs:65` | `truncate_text_for_model` | head+tail with marker | **Migrated** → `head_tail_truncate` |
| `vtcode-core/.../exec_support.rs:490` | `build_head_tail_preview` | line-based head+tail | Left as-is (line-based, not char-based) |
| `vtcode-core/.../harness_kernel.rs:567` | `truncate_lines` | line-based | Left as-is (line counting, different op) |
| `vtcode-core/.../orchestration.rs:834` | `truncate_chars` | char prefix | **Migrated** → `truncate_within` |
| `vtcode-core/.../summarizers/mod.rs:27` | `truncate_line` | byte prefix, `Cow` | Left as-is (zero-alloc `Cow`, byte-based) |
| `vtcode-core/.../tool_orchestrator.rs:175` | `truncate_output` | `&str` slice | Left as-is; **UTF-8 panic fixed** |
| `src/.../tool_summary_helpers.rs` | `truncate_middle` | middle | Left as-is (specialized whitespace sanitization) |
| `vtcode-core/.../session_archive.rs:1765` | `truncate_preview` | char prefix | **Migrated** → `truncate_within` |
| `vtcode-core/.../snapshots.rs:246` | `truncate_description` | char prefix | **Migrated** → `truncate_within` |
| `vtcode-core/.../harness_artifacts.rs:99` | `truncate_summary` | char prefix | **Migrated** → `truncate_within` |
| `src/.../pty_stream/state.rs:357` | `wrap_text_words` | word-wrap | **Migrated** → shared `wrap_text_words` |
| `src/.../tool_summary.rs:379` | `wrap_text_words` | word-wrap | **Migrated** → shared `wrap_text_words` |

> **Applied.** 7 of 12 sites migrated to `vtcode-commons::formatting`. 5 left
> as-is: 2 are line-based (different operation), 1 is byte-based `Cow` (perf),
> 1 is `&str` slice (signature), 1 has specialized sanitization logic. The shared
> module now provides `truncate_within`, `head_tail_truncate`,
> `truncate_byte_budget`, `truncate_text`, `wrap_text_words`, and
> `collapse_whitespace`.

---

### F2 — Tool-call admission across two loops — ⚠️ PARTIALLY REAL (corrected)
**Rate-limiting: not divergent. Loop detection: genuinely divergent.**

The original claim that the loops use *different rate-limiters*
(`ToolExecutionGuard` vs `AdaptiveRateLimiter`) is **wrong**:

- `ToolExecutionGuard` (`runner/tool_execution_guard.rs`) is a `Drop` guard that
  records an interruption error — not a rate limiter.
- `AdaptiveRateLimiter` (`tools/resilience/`) backs the circuit breaker, not
  per-call admission.
- The actual per-call rate limiter is a **single `SafetyGateway`** owned by the
  `ToolRegistry`. Both loops enforce it:
  - Runner → `execute_prepared_public_tool_request` (`tool_access.rs:166`) passes
    `safety_prevalidated=false`, so the registry runs `check_and_record_with_id`
    (rate limits + policy) at `execution_facade.rs:313`.
  - Unified → pre-validates via `ToolCallSafetyValidator`, which wraps the same
    `registry.safety_gateway()`.

  ⇒ **Rate-limiting and policy admission are already shared.** No action.

The **real** divergence is loop detection:

- Runner: client-side `LoopDetector` (`runner.rs:103`) checked *before* execution
  in `check_for_loop`; hard limit → `TaskOutcome::LoopDetected` ends the turn.
- Unified: reads a `loop_detected` marker *from tool output*
  (`tool_pipeline/execution_helpers.rs:14`) and converts it to an error outcome.

These are two different mechanisms with potentially different thresholds.
Reconciling them changes turn-termination semantics, so it is a **maintainer
decision**, not an autonomous refactor (see open questions).

---

### F3 — Tool-argument normalization duplicated — ❌ RETRACTED (false positive)
**Verified: not duplication.**

Line-by-line reading disproves the original claim:

- `runner/tool_args.rs:50` `normalize_tool_args` performs **session-state path
  inference** (fills a missing `path`/`file_path` from `last_file_path` /
  `last_dir_path`, workspace fallback). This is unique to the autonomous runner.
- `tool_routing/approval_policy.rs` does **risk/network classification**
  (`build_tool_risk_context`), not argument normalization at all.
- The genuinely shared, name-keyed arg-shape normalization already lives in one
  place — `tools/registry/execution_kernel.rs:245` `normalize_tool_args`
  (apply_patch wrapping, shell-arg/unified_search normalization, alias remap) —
  and **both** loops use it via the registry.

The interactive loop has **no** `last_file_path` inference (`rg` finds zero
references in `src/`). There is no duplicated logic to consolidate.

---

### F4 — System-prompt assembly "across 3 builders" — ✅ DONE (dead code removed)
**Not active duplication. `generator.rs` was dead code — deleted.**

The original "three fragmented builders" claim does not hold:

- **The two *live* builders already share the canonical base.**
  - Runner: `compose_task_system_prompt` returns the prebuilt `self.system_prompt`
    (or, for simple tasks, calls the shared
    `runner/helpers.rs:50 compose_system_prompt_with_appendix`).
  - Unified: `IncrementalSystemPrompt` takes that base and appends runtime
    addenda via the shared prompt composition helpers.
  - Planning workflow and contract text is centralized in `prompts/system.rs`. No duplicated section text.

- **`SystemPromptGenerator` (`prompts/generator.rs`) was dead code.** Its entire
  public surface (`SystemPromptGenerator`, `::new`, `::generate`, and the *sync*
  `generate_system_instruction_with_config`) was referenced **only** by the
  re-export at `prompts/mod.rs:25` — zero call sites workspace-wide. The live ACP
  path uses the *async* `prompts::system::generate_system_instruction_with_config`
  instead (`vtcode-acp/src/zed/session.rs:54`).

> **Applied.** Deleted `generator.rs` (260 lines) and its `mod.rs` re-export.
> No prompt-output change. The two live builders already share the canonical
> base — no consolidation work needed.

---

### F5 — Error→recovery mapping & payload compaction duplicated — ❌ RETRACTED (false positive)
**Verified: not duplication — two distinct operations in two loops.**

- `reduce_tool_result` (`harness_kernel.rs:399`, used only by the runner) does
  **semantic content reduction**: dedupes grep matches, caps file/command output
  lines, normalizes search results.
- `compact_model_tool_payload` (`response_content.rs:27`, used only by the
  unified loop) does **metadata key-stripping**: drops bookkeeping keys
  (`wall_time`, `spool_hint`, `success`, `status`, …) before the payload reaches
  the model.

These shape payloads differently and share no logic. `rg` confirms the runner
never calls `compact_model_tool_payload` and the unified loop never calls
`reduce_tool_result`. Merging them would *invent* new combined behavior neither
currently has (over-engineering + behavior change), so no refactor is warranted.

> This **is** a behavioral *consistency* observation (each loop economizes
> context differently) of the same family as F2 — tracked below, not a DRY fix.

---

## Revised status after line-by-line verification

| Finding | Original claim | Verified verdict | Action |
|---|---|---|---|
| F1 | Truncation duplicated ~15× | ✅ Real | **Done** — consolidated into `vtcode-commons` (truncation + word-wrap) |
| F2 | Admission split / different rate-limiters | ⚠️ Mostly false | Rate-limit/policy already shared; only loop-detection differs (maintainer decision) |
| F3 | Arg normalization duplicated | ❌ False positive | None — already shared at registry layer |
| F4 | Prompt assembly fragmented (3 builders) | ✅ Corrected + Done | 2 live builders already share; `generator.rs` **deleted** (260 lines dead code) |
| F5 | Compaction/recovery duplicated | ❌ False positive | None — distinct operations, no shared logic |

Net: of five reported findings, **F1 and F4 were real issues — both fixed.** F1
consolidated 7 of 12 truncation/word-wrap sites into `vtcode-commons::formatting`
(the remaining 5 have different signatures or semantics). F4 deleted 260 lines of
dead code (`prompts/generator.rs`). F3 and F5 did not survive line-by-line
reading. F2's rate-limiting claim was wrong (a single shared `SafetyGateway`
already governs both loops); only its loop-detection mechanism genuinely differs.
The headline: the agent/harness layer is far less duplicated than a surface scan
suggests — assembly is already centralized in `ToolRegistry`/`SafetyGateway` and
`prompts/system.rs`.

---

## Open questions for maintainers (behavior-changing — not auto-applied)

1. **F2 (loop detection only)** — the runner uses a client-side `LoopDetector`
   checked *before* execution; the unified loop reacts to a `loop_detected`
   marker emitted *during* execution. Are the thresholds/semantics meant to
   match? Reconciling them changes turn-termination behavior. (Rate-limiting and
   policy admission are already shared via a single `SafetyGateway` — no change
   needed there.)
2. **F5/F2 family** — each loop economizes model context differently
   (`reduce_tool_result` content-capping vs `compact_model_tool_payload`
   metadata-stripping). Should both loops apply both strategies for consistent
   token usage? This changes model-facing payloads and needs eval/snapshot review.

These were deliberately **not** refactored: they alter shared runtime safety or
model-facing behavior, which warrants explicit confirmation rather than an
autonomous edit.

---

## Changelog

| Date | Change |
|---|---|
| 2026-06-05 | Initial read-only audit |
| 2026-06-06 | **F1**: Added `truncate_within`, `head_tail_truncate` to `vtcode-commons::formatting`. Migrated 5 call sites: `truncate_text_for_model` → `head_tail_truncate`; `truncate_chars`, `truncate_summary`, `truncate_preview`, `truncate_description` → `truncate_within`. |
| 2026-06-06 | **F1**: Added `wrap_text_words` to `vtcode-commons::formatting`; migrated 2 local copies in `pty_stream/state.rs` and `tool_summary.rs` (+121 lines removed). Fixed off-by-one in `split_at_word_boundary`. |
| 2026-06-06 | **F4**: Deleted `prompts/generator.rs` (260 lines) and `mod.rs` re-export. |
| 2026-06-06 | **Bugfix**: Fixed UTF-8 panic in `truncate_output` (`tool_orchestrator.rs`) — now scans to char boundary. |
| 2026-06-06 | `cargo fmt --all` reflow on 7 files (no semantic changes). |
