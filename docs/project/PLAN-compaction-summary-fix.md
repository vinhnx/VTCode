# PLAN: `/compact` "Failed to generate compaction summary"

Status: **green**. Input bounding, error-chain surfacing, hierarchical bounding, native-path bounding, and the local
overflow retry are applied; `cargo nextest run -p vtcode-core --lib compaction` (68 passed) and `./scripts/check-dev.sh`
pass. Provider strategy matrix lives in `docs/development/compaction.md`.

Tracks the `.vtcode`-visible symptom:

```text
Compaction failed: Failed to generate compaction summary
```

## Root cause

The **local summary path bounded its output but not its input.**

1. `summarize_locally` (`crates/codegen/vtcode-core/src/compaction/mod.rs`) builds the summarization request with
   `build_cache_safe_compaction_history(history, instructions)` — the parent's **entire** conversation verbatim, plus
   the compaction instruction.
2. `context_bounded_compaction_config` only shrinks `retained_user_message_tokens` (an _output_ retention knob), and
   `bound_compacted_history_to_context` bounds only the **result** of summarization. Nothing bounded the **fork
   itself**.
3. `/compact` is normally run when the context is already near the model window, and model-switch compaction targets a
   _different_ (often smaller) window. The summary request therefore exceeded the summarizer's window, the provider
   rejected it, and `.context("Failed to generate compaction summary")?` aborted the whole command.

### Secondary defect: the error message was lossy

The slash command printed anyhow's plain `Display`, which renders only the **outermost** context. The outermost context
is literally `"Failed to generate compaction summary"`, so the real provider error (the actual cause) was discarded —
which is why the reported message was undebuggable.

## Evidence

- Log: `~/.vtcode/sessions/debug-session-vtcode-20260816t130623z_724352-43328.log`

  ```text
  ERROR vtcode_transcript: Model switched, but context compaction failed:
    Failed to generate compaction summary. Continuing with full history.
  ```

  Fired immediately after a switch from `deepseek/deepseek-v4-flash-0731` to `gemini-3.7-flash`
  (`compaction_mode=local`).

- Same log, minutes earlier: the **same** provider/model compacted fine with a tiny history (`turn=2`,
  `history_snapshot_bytes=2404`, `tool_count=0`). Large-history / small-window only → consistent with an input-size
  overflow, and inconsistent with a schema or `tool_choice` defect.
- Call sites of the wrapper context: `compaction/mod.rs` (legacy `compact_history_with_budget` and the manual
  `summarize_locally` path).

## Changes applied

| File                                                               | Change                                                                                                                                                                                                                                                                                                                                                                                                      |
| ------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `crates/codegen/vtcode-core/src/compaction/mod.rs`                 | New `bound_history_for_summarization(history, instructions, budget)`: keeps the newest **complete protocol groups** that fit the budget and drops the oldest; reuses `complete_protocol_group_prefix` so an unanswered trailing tool call is excluded rather than shipped as an invalid protocol suffix; degrades to `bounded_protocol_group` previews when no group fits (e.g. one oversized tool result). |
| `crates/codegen/vtcode-core/src/compaction/mod.rs`                 | `summarize_locally` (the `/compact` + model-switch path) now summarizes `summary_source`, the bounded fork, instead of raw `history`.                                                                                                                                                                                                                                                                       |
| `crates/codegen/vtcode-core/src/compaction/mod.rs`                 | The legacy `compact_history_with_budget` path is bounded the same way.                                                                                                                                                                                                                                                                                                                                      |
| `crates/codegen/vtcode-core/src/compaction/mod.rs`                 | `mod summarization_fork_bounds_tests` — 4 unit tests covering: fits-verbatim, unknown-budget-verbatim, over-budget trimming, no-group-fits fallback.                                                                                                                                                                                                                                                        |
| `src/agent/runloop/unified/turn/session/slash_commands/compact.rs` | `format!("... {err}")` → `format!("... {err:#}")` so the full anyhow context chain (the real provider error) is surfaced.                                                                                                                                                                                                                                                                                   |
| `crates/codegen/vtcode-core/src/compaction/mod.rs`                 | `anthropic_inline_compaction_edits`: `NativeInline` now sends the documented ladder (thinking → tool-uses → compact), gated on `supports_context_edits`.                                                                                                                                                                                                                                                    |
| `crates/codegen/vtcode-core/src/compaction/mod.rs`                 | `generate_local_summary_with_retry`: local summary retries once with a halved input budget on context-capacity errors.                                                                                                                                                                                                                                                                                      |
| `crates/codegen/vtcode-core/src/compaction/mod.rs`                 | Native inputs (`NativeStandalone`, `NativeInline`, legacy Responses path) bounded with `bound_history_for_summarization` like the local fork.                                                                                                                                                                                                                                                               |
| `docs/development/compaction.md`                                   | Provider × strategy matrix, ladder thresholds, bounding/retry contract.                                                                                                                                                                                                                                                                                                                                     |
| `crates/codegen/vtcode-llm/src/providers/vercel.rs`, `xai.rs`      | Native `POST /v1/responses/compact` via shared `OpenResponsesProvider::compact_endpoint_client`, gated on documented route + host.                                                                                                                                                                                                                                                                          |
| `crates/codegen/vtcode-core/src/compaction/mod.rs`                 | Shared `generate_summary_with_capacity_retry` now covers flat, legacy, and hierarchical band paths.                                                                                                                                                                                                                                                                                                         |
| `crates/codegen/vtcode-core/src/compaction/mod.rs`                 | `CompactionRoutePolicy` (threshold/retain/retries per route) with window-scaled tail targets; `prune_oversized_tool_outputs` trims tool dumps before bounding.                                                                                                                                                                                                                                              |

## Remaining work

- [x] Clippy, trimming budget, gate, hierarchical bounding, lossy logs — done.
- [x] Native-path input bounding — done.

1. **Clear `clippy::needless_range_loop`** — done (iterator form applied).

2. **Fix the trimming test's budget.** — done (budgets derived from measured estimates; all fork-bounds tests pass).

3. **Re-run the gate:** — done (`compaction` suite + `./scripts/check-dev.sh` green).

4. **Bound the hierarchical path.** — done (bands bounded + share the capacity-retry contract).

5. **Fix the remaining lossy error logs.** — done (`?error` / `{err:#}` everywhere compaction errors reach users or
   logs).

6. **End-to-end check.** Run `/compact` on a long session and confirm the reported failure is gone; if it still fails,
   the now-visible provider error identifies the next cause.

## Gotchas discovered

- `./scripts/check-dev.sh` **auto-runs `cargo fmt --all`**, rewriting files in place. A patch prepared against
  pre-format text can go stale mid-flight.
- `anyhow::Error`'s plain `{err}` prints only the outermost context. Use `{err:#}` (or the root cause) anywhere
  compaction errors reach the user or logs, or the actionable cause is silently dropped.

## Follow-ups

- Consider bounding the fork _before_ the cache-safe prefix is built for the provider-native paths too, so native and
  local strategies share one input ceiling (mirrors the existing `effective_context_budget` intersection).
