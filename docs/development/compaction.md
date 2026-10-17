# Compaction Engine

How VT Code keeps long sessions inside the model window. See `crates/codegen/vtcode-core/src/compaction/mod.rs` for the
implementation.

## Request history and local clearing

Session history and provider request history have separate lifetimes. The shared
`normalize_history_for_request_shared` helper repairs pairing and order on a request projection, retaining the shared
fast path when history is already valid. `clear_old_tool_results_with_recent_batches` then replaces eligible old results
in that projection.
Neither operation rewrites original outputs in session history or canonical `ThreadEvent` records. Actual compaction
replaces live history and records a recoverable full-history artifact and `thread.compact_boundary`.

The existing `agent.harness.tool_result_clearing` policy is enabled by default. Its configured trigger is 40,000 tokens
and both `keep_tool_uses` and `keep_recent_tool_batches` are 2. For local clearing, the runtime resolves:

```text
history_budget = capacity - reserved_output - system_and_tool_schema_overhead
local_trigger = min(configured_trigger, max(1, floor(history_budget * 3 / 5)))
```

Subtractions saturate at zero. `capacity` is the intersection of positive provider and session limits; a zero session
ceiling means automatic capacity resolution. If capacity remains unknown, the configured trigger is preserved. For a
32,000-token capacity with 4,000 tokens reserved for output and 2,000 for the prefix, the local cap is 15,600 tokens.
Lower configured triggers still win. `enabled = false` opts out; native edit thresholds retain their configured values.

Retention protects a recent evidence window bounded by half the resolved local trigger (20,000 tokens with the
default 40,000-token trigger), counting both assistant inputs and tool results. It walks complete batches newest-first
and stops before the next batch would exceed that budget. This keeps source available across short follow-up searches
without leaving an unbounded tail when projecting full durable history. Count-based retention additionally protects
the union of the newest `keep_tool_uses` results (expanded to their complete originating
assistant tool-call batches) and the newest `keep_recent_tool_batches` assistant messages containing tool calls.
Recent batches are selected by originating message order, independently of completion order. For batches of 1, 4,
and 3 results, the default count policy protects the newest two rounds: 7 results; the evidence window may retain more.
Setting `keep_recent_tool_batches = 0`
disables additional batch-count retention; the bounded evidence window still applies.
The `clear_old_tool_results` API remains a wrapper with zero additional recent batches.

Call-ID matching handles reused IDs, reordered completions, and intervening system messages. Unmatched results retain
the count-based policy. Small results stay unchanged when a stub would not reduce estimated tokens. Paired arguments
and signatures are retained for protected results. When replacing a result, arguments and freeform inputs of at most
1,024 bytes stay intact so ordinary commands and paths remain useful history rather than invalid callable examples.
Larger inputs are replaced when input clearing is enabled, and thought signatures are removed for cleared results.
Input clearing applies only to results actually replaced and cannot
increase the total projected estimate. Reapplying clearing is idempotent. The recent-batch setting applies only to local
clearing; native Anthropic edits keep their existing count-based behavior. Oversized protected evidence proceeds through
existing compaction.

Session `20261015T232419Z` demonstrated the failure: later searches displaced six README pages, rereads hit the path
cap, and placeholder arguments triggered validation recovery. Its successful read outputs now replay through request
assembly in a regression, covering parallel and sequential batches. This is a local retention fix, not an increase to
read, retry, or execution limits. Upstream discovery was checked against
[Codex history recording at `f0c5077`](https://github.com/openai/codex/blob/f0c5077c34ec7a76b4ad9134835925e6ae3d72ec/codex-rs/core/src/context_manager/history.rs#L555-L577),
which bounds tool outputs during recording; the local window and small-input policy are VT Code-specific.

The [bounded retention pilot](../../scripts/evals/README.md#recent-tool-batch-retention-pilot) compares settings 0 and 2
with fixed tools and planning. Numeric debug diagnostics report original/projected token estimates, cleared-result count,
and protected-batch count; canonical events and external checks establish outcomes. Unengaged runs cannot demonstrate
retention benefits.
Interactive requests use local clearing when they will not carry Anthropic native context edits. Headless `vtcode exec`
requests attach no native context management, so provider capability alone never suppresses their local fallback. This
request policy is distinct from the native inline compaction edit ladder described below.

## Headless staging and module ownership

Before estimating pressure, the headless runner synchronizes pending conversation messages into live history, normalizes
the request projection, and applies local clearing. Automatic readiness and preflight diagnostics use projected history
plus prompt overhead, with output reserved from the effective capacity. If clearing brings pressure below the trigger,
no summary call is needed. If compaction remains necessary, the engine receives full history; after a successful pass,
the runner rebuilds the projection. Failed or unchanged passes keep the existing projection. Preflight remains advisory.

| Responsibility | Source under `crates/codegen/vtcode-core/src/` |
| --- | --- |
| Pairing repair, complete-batch retention, local trigger cap | `core/agent/state.rs` |
| Synchronization, projected pressure, request assembly | `core/agent/runner/execute.rs` |
| Headless projection and compaction delegation | `core/agent/runner/summarize.rs` |
| Shared readiness, strategy orchestration, persistence | `compaction/auto.rs` |
| Local summary budgets and first-call aggregate validation | `compaction/summarize.rs` |
| Protocol-preserving input selection and preview fallback | `compaction/history_bounds.rs` |

Interactive wiring lives in `src/agent/runloop/unified/turn/turn_processing/llm_request/request_builder.rs`. Both request
paths reuse the core clearing resolver. Both runloops reuse `auto_compact_messages`; the headless caller checks its shared
readiness predicate before copy-on-write access so no-op compaction does not clone shared session history.

See the [architecture diagram](../ARCHITECTURE.md#request-projection-and-context-budgets),
[loop contract](../guides/agent-loop-contract.md#unified-auto-compaction), and
[configuration reference](../config/CONFIG_FIELD_REFERENCE.md) for the surrounding contracts.

## Strategy dispatch

`/compact` picks one strategy per provider/model (`manual_compaction_strategy`):

| Provider / model                                                        | Strategy           | How it works                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| ----------------------------------------------------------------------- | ------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| OpenAI Responses models (`/responses/compact`)                          | `NativeStandalone` | Server-side standalone endpoint; returns a `response.compaction` item plus retained items, used as-is.                                                                                                                                                                                                                                                                                                                                                                                                                     |
| OpenResponses models (`/v1/responses/compact`)                          | `NativeStandalone` | Vendor-neutral standalone endpoint; retains returned messages and opaque `compaction` items for replay.                                                                                                                                                                                                                                                                                                                                                                                                                    |
| Vercel AI Gateway `openai/*` routes                                     | `NativeStandalone` | Gateway serves `POST /v1/responses/compact` (forwarded to OpenAI). Gated on `openai/` model prefix and the gateway endpoint; other routes use `Local`.                                                                                                                                                                                                                                                                                                                                                                     |
| xAI Grok models (`POST /v1/responses/compact`)                          | `NativeStandalone` | xAI serves an OpenAI-compatible compact endpoint for curated Grok models on `api.x.ai`. Gated on the model allowlist and API host; otherwise `Local`.                                                                                                                                                                                                                                                                                                                                                                      |
| Anthropic Claude Opus 4.x / Sonnet 4.6+                                 | `NativeInline`     | Threshold-triggered `compact_20260112` edit on a `generate` request with `pause_after_compaction`. Falls back to `Local` when compaction does not fire or the edit is rejected.                                                                                                                                                                                                                                                                                                                                            |
| Merge Gateway, OpenRouter, Gemini, DeepSeek, MiniMax, local, all others | `Local`            | No native compact endpoint: Merge Gateway documents no `/responses/compact` (its native `/v1/responses` shape differs from OpenAI's); OpenRouter reports compaction unsupported (its `context-compression` plugin is opt-in middle-out truncation, never enabled implicitly). Universal fallback: bounded fork summarized via the provider's one-shot response collector (which transparently consumes a normalized stream for streaming-only routes), rebuilt as summary plus retained recent messages. Works everywhere. |

## Live parent-prefix reuse

Interactive manual, recovery, targeted, and automatic compaction reuse the active request envelope through the shared
`CompactionParentContext` API already used by the headless runner. The snapshot carries shared system-prompt and ordered
catalog owners only when the provider and model match; segment clearing, model switches, or a missing envelope leave
parent context absent. Local and inline requests keep the supplied catalog while disabling tool use. Existing history
and summary budgets still apply, and native standalone results remain canonical.

## Compaction messages and cache health

Successful compaction reports elapsed time, mode, and before/after context-item counts. Counts include opaque provider
items and are not token or cost estimates. The completion message explains that cache reuse may be lower on the next
request. Failures keep history, display a bounded root cause without the duplicate provider JSON body, and retain the
bounded, secret-redacted error chain in the debug log. Model-switch failures explicitly say that the model changed
and continuation uses the full history with a resume note.

The shared cache monitor resets only its consecutive low-reuse streak after successful compaction. Manual, recovery,
and model-switch commits use the same reset; failures and no-ops do not reset it. Session token totals and cumulative
alerts stay intact. Native OpenAI generation maintains a separate bounded comparison baseline across compaction,
while replay lineage is cleared normally. See the [prompt caching guide](../tools/PROMPT_CACHING_GUIDE.md#openai-cache-comparison-diagnostics)
for wire hints, supported backends, comparison evidence, and billing limits.

## OpenAI standalone request contract

OpenAI standalone compaction uses a synchronous JSON response. Its payload carries `model`, `input`, optional
`instructions`, and supported service-tier/cache fields (`service_tier`, `prompt_cache_key`, `prompt_cache_options`,
`prompt_cache_retention`). VT Code does not send generation-only fields: `stream` (even `false`), `max_output_tokens`,
`store`, `include`, `reasoning`, or `text`. Generation overrides in the shared compaction options do not apply to this
endpoint. Generation comparison IDs are also omitted from the compact options. Custom compaction instructions and
history system reminders remain available to the compactor, and retained messages plus encrypted compaction items are
replayed in the next Responses request.

See the [OpenAI compact API reference](https://developers.openai.com/api/reference/python/resources/responses/methods/compact)
for the endpoint schema; streaming server-side compaction on `/responses` is a separate request mode.

## Anthropic context-edit ladder

On the `NativeInline` path VT Code sends the documented edit ladder in order — `clear_thinking_20251015` (keep 2
thinking turns), then `clear_tool_uses_20250919` (trigger 100k input tokens, keep 3 tool uses), then `compact_20260112`
(trigger floor 50k, `pause_after_compaction: true`). The clearing edits are included only when the provider reports
`supports_context_edits`; otherwise the request carries the compact edit alone. Beta headers (`compact-2026-01-12`,
`context-management-2025-06-27`) are derived from the request payload by the Anthropic provider, so callers never set
them manually. Helper: `anthropic_inline_compaction_edits`.

The response's `compaction` block is provider state, not a replacement for a local `Previous conversation summary`
message. VT Code keeps the complete raw block (including signatures, cache controls, and unknown provider fields) in the
assistant message's `reasoning_details` and replays it as the first Anthropic message on the next request. Signed blocks
are required to remain first, and thinking/redacted-thinking blocks from before that signed boundary are dropped because
Anthropic's preserved-thinking validation cannot accept them after the summarized transcript has been removed. A
threshold response whose block has `content: null` is treated as a failed native compaction and uses the local fallback.

Anthropic usage with compaction may include separate `iterations` for the compaction and message sampling passes. VT
Code aggregates recognized `message`, `fallback_message`, `advisor_message`, and `compaction` iterations for normalized
usage and cost estimates; top-level usage is used only when no recognized iteration data is present.

Anthropic's newer on-demand `compact-2026-09-04` signed-summary beta is not exposed by the current `/compact` engine.
The engine is synchronous and uses the threshold/pause strategy; adding background keep-tail swapping requires a
separate history lifecycle and beta capability contract.

## Input bounding and overflow retry

Local summaries use `local_summary_history_budget` to intersect provider capacity and a positive session ceiling. The
resolver preserves existing safety headroom (one eighth of the window plus 512 tokens), then subtracts the frozen parent
system/tool-schema prefix and any explicit summary output limit. Summary instructions also consume this allowance.
Unknown capacity keeps the existing verbatim first-fork fallback.

`bound_local_summary_history` wraps `bound_history_for_summarization`: it selects complete recent protocol groups or
bounded previews, then checks their aggregate estimate plus instructions against the resolved allowance. A positive
budget alone is insufficient because protocol metadata and minimum per-message overhead can still exceed it. Impossible
fixed inputs or aggregate minimums return a budget error before dispatch. Hierarchical summarization prepares and
validates both bands before spending either provider call.

Inline native requests use `bound_history_for_summarization`; standalone native compaction uses the marker-preserving
`bound_history_for_native_compaction`. Those paths retain their existing strategy-specific bounds. Before bounding,
local paths run `prune_oversized_tool_outputs`: `Tool` messages over 4,096 tokens are replaced with previews (call IDs
preserved, so groups stay protocol-valid) so one dump cannot evict whole turns from the fork. Flat summaries and
hierarchical bands share one capacity-retry contract (`generate_summary_with_capacity_retry`): on a context-capacity
rejection the request is retried with progressively halved budgets, skipped when a retry cannot shrink the input. When
the window is unknown (`None` budget) the first fork is verbatim, but the retry derives a fallback budget from the
failing attempt (`input_tokens / 2`) so it can still shrink instead of resending the identical fork. Empty provider
summaries fail with a diagnostic (`provider returned an empty summary`) rather than producing an empty
`Previous conversation summary:` history. Provider summary failures carry route diagnostics (`provider / model`,
`input_tokens`, `budget`); pre-dispatch budget errors report the exhausted allowance in the error chain so `/compact`
reports the actionable cause instead of only
`Failed to generate compaction summary`. Local summaries are bounded on output with
`bound_compacted_history_to_context`; native provider responses are passed through unchanged because their retained
items and opaque compaction state are the canonical next context window. The native endpoint is responsible for
returning a replayable window that fits its model context.

## Per-route policy

`CompactionRoutePolicy` (`threshold_ratio`, `retain_ratio`, `max_overflow_retries`) tunes compaction per provider/model
route in the DeepSeek-harness shape. Defaults (`1.0`, `0.16`, `1`) preserve current behavior: the threshold ratio only
ever fires compaction _earlier_, the continuity tail is `min(20_000, window × retain_ratio)` (identical for windows ≥
125k, scaled down for small windows so the summary survives output bounding), and one halved retry happens on capacity
errors. Route overrides live in the `ROUTE_POLICIES` table matched on provider/model substrings; it is intentionally
empty until a route demonstrates a need.

## Verification

Successful compaction starts a new prompt-cache health streak in both runloops. Cumulative usage and hit rates remain
intact, so expected replay-window changes cannot join an earlier low-reuse streak while persistent degradation remains
visible. Failed and no-op compaction do not reset the streak. See the
[cache health guide](../tools/PROMPT_CACHING_GUIDE.md#cache-health-monitoring).

```bash
cargo nextest run --locked -p vtcode-core -E 'test(compaction::) | test(core::agent::state::) | test(core::agent::runner::tests::clearing::)'
cargo nextest run --locked -p vtcode -E 'test(llm_request::request_builder::tests::) | test(llm_request::context_management::tests::)'
./scripts/check-dev.sh
```

Regressions cover asymmetric 1/4/3 batches, reordered completions, trigger boundaries, provider/session intersections,
small-result non-growth, idempotence, durable history, and summary calls avoided or required at 32k/64k/96k/128k capacities.
Recording providers also check frozen summary prefixes, explicit output reserves, and impossible protocol minimums.
