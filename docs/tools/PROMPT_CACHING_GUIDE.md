# Prompt Caching Guide

The harness treats a cache prefix as an immutable segment. Tool definitions use deterministic core-first ordering
followed by tool name and type, and the serialized catalog is reused for the segment. Catalog expansion through tool
discovery takes effect at the next segment boundary.

Prompt caching lets VT Code reuse validated conversation prefixes across providers to reduce latency and token
consumption. This guide explains how to configure the feature globally and fine-tune the per-provider behaviour exposed
in `vtcode.toml`.

## Global Settings

All prompt caching controls live under the `[prompt_cache]` section in `vtcode.toml`.

| Key                             | Type    | Description                                                                                                               |
| ------------------------------- | ------- | ------------------------------------------------------------------------------------------------------------------------- |
| `enabled`                       | bool    | Master switch for the caching subsystem. When disabled, per-provider overrides are ignored.                               |
| `cache_dir`                     | string  | Path (supports `~`) where cache entries are persisted. Relative paths resolve against the workspace root.                 |
| `max_entries`                   | integer | Maximum entries persisted on disk before rotation.                                                                        |
| `max_age_days`                  | integer | Maximum age of an entry before automatic eviction.                                                                        |
| `enable_auto_cleanup`           | bool    | If `true`, stale entries are purged during startup and shutdown.                                                          |
| `min_quality_threshold`         | float   | Minimum quality score a completion must meet before it is cached.                                                         |
| `cache_friendly_prompt_shaping` | bool    | Default-on prompt shaping that keeps volatile runtime context at the end of system prompts for better cache-prefix reuse. |

## Provider Overrides

Each provider exposes an override block under `[prompt_cache.providers]`. Overrides are only honoured when both the
global `enabled` flag and the provider-level `enabled` flag are `true`.

### OpenAI

```toml
[prompt_cache.providers.openai]
enabled = true
surface_metrics = true
auxiliary_explicit_cache = false # opt-in experiment on supported native models
prompt_cache_key_mode = "session" # session | off
# prompt_cache_ttl = "12h" # opt-in; 30m is the default
```

- `enabled` — controls VT Code's explicit cache hints and comparison diagnostics together with the global flag.
  Disabling either flag omits OpenAI cache keys, retention options, and explicit breakpoints. OpenAI's automatic
  server caching can still occur; the local switch cannot disable it.
- `surface_metrics` — enables optional comparisons against the latest successful foreground response on supported
  native Responses models. Provider-reported cache-read and cache-write usage is always retained for accounting,
  including when this setting or caching is disabled. Missing counters remain unknown rather than reported zeros
  in provider usage; cost estimates without those counters cannot establish actual cache savings.
- `auxiliary_explicit_cache` — opt-in explicit-only caching for independent auxiliary requests on native GPT-5.6/GPT-6
  Responses routes. Only stable developer instructions receive a breakpoint; disposable history and runtime context
  stay outside cache writes. Interactive requests retain implicit caching. Default: `false`; compare total cost before
  enabling it, since retries may reuse less of an auxiliary suffix. Other routes ignore this setting.
- `min_prefix_tokens` and `idle_expiration_seconds` are accepted legacy settings; the OpenAI provider does not use
  them to request caching or set server expiry. GPT-5.6 and later require at least 1,024 visible input tokens in a
  reusable prefix. Short prompts need no padding; measure total cost before adding context.
- `prompt_cache_retention` is opt-in for older supported Responses models (`"in_memory"` or `"24h"`). GPT-5.6/GPT-6
  use `prompt_cache_options.ttl` instead; their TTL is not extended by configuring legacy 24h retention.
- `prompt_cache_ttl` accepts `"30m"` and `"12h"`. Explicit configuration takes precedence over the model catalog,
  then defaults to `"30m"`. It applies to eligible native GPT-5.6+ Responses requests and explicitly opted-in custom
  `openai-responses` routes. Chat Completions, ChatGPT subscriptions, and other endpoints omit the new options.
  Existing explicit breakpoints inherit the request lifetime; VT Code does not expose per-breakpoint TTL overrides.
  Cache-gap advisories follow the effective lifetime (1,800 or 43,200 seconds), unless an explicit warning threshold
  is configured. The lifetime is a minimum for cache-read eligibility, not a deletion deadline or a latency guarantee.
  According to the [official OpenAI guide](https://developers.openai.com/api/docs/guides/prompt-caching),
  30-minute writes cost 1.25× ordinary input and 12-hour writes cost 2.5×; these are total write rates.
  Read pricing is independent of the lifetime. Third-party pricing and compatibility depend on the endpoint.
  Provider rejection remains visible; VT Code does not silently downgrade the requested lifetime.
- Native GPT-5.6/GPT-6 requests keep implicit caching and use at most **three** explicit write slots. A supplied
  stable system prompt moves into the first developer `input_text` block with a breakpoint, without duplication.
  The remaining slots mark recent stable history boundaries. This preserves an instruction anchor when history
  is compacted. Replay-only fallback instructions keep their existing top-level representation.
- Volatile runtime sections and history system reminders stay in a trailing developer-role `[System reminder]` item
  with no explicit marker on native GPT-5.6/GPT-6 cache-enabled requests. A developer message after the initial
  developer group is not an implicit cache boundary, so the implicit write covers the latest reusable user message
  or tool result. A temporary user-role reminder would consume that write and be replaced on the next request,
  preventing reuse of the growing tool history. Older models, ChatGPT subscriptions, third-party endpoints, and
  requests with explicit cache hints disabled keep their compatible user-role reminder shape.
- Keys remain stable per session. On GPT-5.6 and later, keys separate cache accounting; older models use them to
  help routing locality. Use `prompt_cache_key_mode = "off"` when separate accounting is unnecessary.

These rules follow the [OpenAI prompt caching guide](https://developers.openai.com/api/docs/guides/prompt-caching).
Cache writes, reads, and uncached input are different billing categories; a cache miss does not mean all input was
charged at the uncached rate. Compaction can reduce total input cost despite a lower hit rate.

For local reporting, experimental comparisons, and known limitations, see
[prompt-cache measurement](../development/prompt-cache-measurement.md).

### OpenAI cache comparison diagnostics

With global/provider caching and `surface_metrics` enabled, native GPT-5.6/GPT-6 generation requests send
`prompt_cache_options.comparison_response_id` after the first successful foreground response. The provider retains
one bounded response ID scoped to the request's cache key. A different key starts without a comparison; auxiliary
summaries, standalone compaction, failed requests, and streams dropped before completion do not advance the baseline.
The comparison survives replay-lineage clearing so a post-compaction request can explain a changed history prefix.
It does not restore conversation content or alter generation.

Both HTTP streaming surfaces, non-streaming HTTP, and WebSocket completions log comparison evidence independently
of final content parsing, including delta-only replies with an empty completed `output`, in
`OpenAI Responses prompt cache comparison` debug-log entries:

| Field | Meaning |
| --- | --- |
| `cache_comparison_type` | `cache_hit`, `cache_miss`, `comparison_response_not_found`, or `unavailable` |
| `cache_miss_reason` | Provider-classified model, tier, tools, format, effort, verbosity, input, key, or compaction change |
| `cache_missed_tokens` | Estimated comparison-prefix tokens that were not reused |
| `comparison_reusable_tokens` | Optional estimated reusable size of the baseline prefix |

Only known status/reason values and numeric counts are logged. Missing, malformed, or expired comparison records do
not fail generation. `cache_hit` does not imply 100% reuse; use actual usage counters for billing and hit rates.
See [OpenAI cache diagnostics](https://developers.openai.com/api/docs/guides/prompt-caching/diagnostics).

WebSocket `generate: false` transport warmup is separate from `prompt_cache_options.prewarm`. VT Code does not send
additional paid cache-prewarm requests. It also keeps reasoning/model changes and catalog segment boundaries
explicit rather than silently changing requested behavior to improve cache percentages.

## Prefix Stability Rules

Prompt caching on Responses-style providers only hits when the new request keeps an exact prefix match. In VT Code, the
most common cache breakers are:

- Changing `model`, `tools`, or sandbox/environment instruction blocks mid-session.
- Reordering tools between requests.
- Injecting new dynamic context above existing prompt items.
- Putting a per-second timestamp in the static system prompt.
- Sending compaction/summarization as a fresh single-message prompt instead of forking the parent prefix.

To reduce avoidable misses, VT Code keeps tool ordering deterministic and defers MCP `tools/list_changed` refreshes to
turn boundaries so an active turn sees a stable tool catalog. VT Code enables
`prompt_cache.cache_friendly_prompt_shaping = true` by default. When it is enabled, VT Code applies provider-aware
shaping:

- OpenAI, Gemini, DeepSeek, OpenRouter, Moonshot, Z.AI: move volatile counters to a trailing `[Runtime Context]` block.
- Anthropic and MiniMax: same trailing runtime block, plus Anthropic-format system prompt splitting so runtime context
  is sent as an uncached block.

### Static first, dynamic last

The harness lays out every prompt so stable pieces stay cached and only the conversation grows turn by turn:

1. Static system prompt and tool definitions (globally cached).
2. Project instruction layers (`AGENTS.md` / `CLAUDE.md`, cached within a project).
3. Session-stable routing (`## Skills` before the more volatile `## Active Tools`).
4. Dynamic suffix: `## Environment` (date-only, never clock time), planning/full-auto notices, `[Harness Limits]`,
   `[Runtime Tool Catalog]`, conversation messages.

Planning and full-auto transitions are runtime mode changes: they are conveyed in the uncached dynamic suffix (and gated
at execution), never by rewriting the stable prefix or swapping the tool set mid-segment. A planning toggle ends the
current cache-stable segment once by design (logged as `planning_workflow_enabled/disabled`); the next request
re-establishes the prefix and subsequent turns hit again.

Temporal context in the system prompt is date-only (`- Date:`). Precise clock time belongs in a `<system-reminder>`
history message on turns that actually need it, so the cached prefix stays stable all day.

### Cache-safe compaction forking

For a live session with a matching provider/model envelope, local compaction carries the parent's exact system prompt
and ordered tool catalog, with `tool_choice: none`, and appends the compaction instruction to the summary input. Both
runloops use the existing `CompactionParentContext`: the interactive path forwards it for manual, recovery, targeted,
and automatic compaction. A stale provider/model envelope is not reused. History bounding or repeated-read reduction
can shorten or change the conversation portion, so full-prefix cache reuse is not guaranteed.

Flat summaries, hierarchical abstract/detail requests, prefire pass-2 requests, and native inline compaction
(`compact_20260112`) propagate a supplied parent through their existing shared builders. Inline failures retain that
parent for local fallback. Saved-session forks and requests without a live matching envelope use the existing bounded
fallback. Provider-native standalone compaction (OpenAI `/responses/compact`) has its own server-side contract.
The summary is then installed and a new immutable segment begins; `thread.compact_boundary` records the prefix and
catalog hashes. Compare actual read/write/uncached token costs instead of assuming a summary fork is a cache hit.

### Tool-free recovery keeps the tool prefix

Tool-free recovery (budget exhaustion, blocked-call fuse, loop detector) must not rewrite the provider prefix. OpenAI's
prompt-caching guide is explicit: disable tool use with `tool_choice: "none"` instead of removing tool definitions —
omitting `tools` rewrites the rendered prefix and invalidates the cache.

VT Code therefore:

- Keeps the same ordered tool catalog on the wire during recovery (and in the `[Runtime Tool Catalog]` section) as on
  tool-enabled turns.
- Sets `tool_choice: none` on providers that accept it (OpenAI Responses/Chat, Anthropic).
- On Merge Gateway, keeps the tool definitions but omits `tool_choice` entirely (Bedrock routes reject
  `tool_choice: "none"`). The harness still rejects tool calls during recovery.
- Delivers the `[Recovery Mode]` contract as a request-only tail-position turn-scoped system message (the same mechanism
  as the collapsed-output notice), never as a system-prompt section, and freezes the `recovery_reason` inside it when
  the recovery activation starts so retries do not rotate the directive bytes. Later telemetry reasons do not rewrite
  that message.

Because the system prompt is at the front of every request, any per-turn delta there (the old in-prompt
`[Recovery Mode]` block included) would bust the entire cached prefix — tools, system, and all history. The tail-message
placement keeps the system prompt byte-identical to the tool-enabled turns that gathered the evidence, so consecutive
recovery turns reuse the same cached prefix. The directive is never persisted to canonical history, so it cannot outlive
the recovery episode; routes without native turn-scoped support receive it through the request-only wire translation.

When a recovery pass ends and a new activation starts, the frozen reason may refresh once (one intentional segment
boundary). Fingerprint telemetry records `tools_omitted` and `recovery_reason` as distinct change causes alongside
`stable_prefix` / `tool_catalog` / `model`.

### Cache health monitoring

Beyond per-event advisories (reasoning-effort changes, idle-gap expiry, planning transitions), both runloops feed every
turn's normalized usage into a shared session health monitor (`core::agent::cache_health::PromptCacheHealthMonitor`).
Turns without provider cache metrics or below 1,024 input tokens are ignored as noise. Two session-scoped alerts fire at
most once each, via `tracing::warn` plus the runloop's user-warning channel:

- **Sustained low reuse** — 3 consecutive measured requests each reusing under 50% of their input. Partial hits
  count as degraded; the warning does not describe them as full-cost misses. It includes the cumulative hit rate
  (`cached/total input`) over measured turns.
- **Low hit rate** — after 8 measured turns, the cumulative hit rate (`cached/total input`) is under 25%.

Successful compaction resets the consecutive streak in both runloops, including manual, recovery, and model-switch
compaction. Session totals, cumulative hit-rate alerts, and one-shot warning latches remain intact. Failed or no-op
compaction preserves the streak. Initial cold writes and intentionally compacted history can have low reuse even
when the client prefix/catalog hashes are unchanged; check comparison evidence before attributing churn to VT Code.
An unchanged cached-token count with growing total input can also lower the ratio: inspect eligible cache-write
boundaries and request-only reminders, rather than assuming model switches or prefix churn. Stable prefix/catalog
hashes do not cover the entire serialized conversation. These alerts measure reuse, not its cause.

No-op catalog refreshes (version/epoch bump with identical tool bytes and stable prompt) preserve the frozen request
envelope so the provider prefix stays hot; only provider-visible changes rebuild it.

Either alert names the likely causes to check: prompt/tool-catalog churn (model/reasoning switches, MCP refreshes,
planning toggles) or idle gaps expiring the provider cache. The sustained-miss warning also points to earlier
single-event warnings and the trajectory log for prefix/catalog hash changes.

### Mid-session model switches

Prompt caches are unique per model: switching rebuilds the cache at full input cost even when the rest of the prefix is
unchanged, so it is the most expensive single cache event in a session. Both runloops emit a one-line advisory on the
first request carrying a new model (headless: tracing warning plus session warning; interactive: tracing warning plus
renderer warning line), mirroring the existing reasoning-effort-change advisory. Prefer resolving the model up front;
when a switch is unavoidable (e.g. escalation or `/model`), expect one full-price request before hits resume. For cheap
exploratory work, prefer delegating to a subagent on the cheaper model with a handoff summary over switching the main
session's model.

### Why mode content stays in the prompt (triage note)

Two further article prescriptions were researched and deliberately deferred:

- **Planning/full-auto via `<system-reminder>` messages instead of system-prompt sections.** Snapshots are deterministic
  per mode, so the wire prefix is already stable turn-to-turn within a mode on every provider; the only residual cost is
  the single toggle-transition miss (zero on Anthropic, where the wire split keeps the cached stable prefix across the
  toggle). Moving the full planning contract — read-only enforcement, plan-quality spec, research floor — out of the
  system prompt risks planning behavior with no eval to verify, and would require reworking the prompt/catalog alignment
  guard that pins the interview-policy line. Revisit only with eval coverage for planning adherence.
- **Always exposing the full tool catalog (no planning filtering).** Same steady-state analysis: deterministic per-mode
  filtering means no per-turn churn today; always-expose would save only the toggle-transition miss while showing
  mutating tools on every planning turn, trading rare one-time savings for per-turn model-confusion risk and denied-call
  waste across all supported models (including small ones). The fail-closed execution gate stays as the safety net, and
  tool hiding stays as defense-in-depth. Revisit only with eval evidence that target models obey read-only instructions
  reliably when mutating tools are visible.

OpenAI and Merge Gateway keep `prompt_cache_key` stable per session (unless `prompt_cache_key_mode = "off"`). The wire
key is `vtcode:openai:{lineage}` on OpenAI and `vtcode:merge:{lineage}` on merge-gateway; per-turn capability/catalog
hashes are tracked separately in `tool_catalog_hash` / `system_prompt_prefix_hash` and never mixed into the routing key.
Both the interactive runloop and the headless AgentRunner follow this rule.

OpenRouter and xAI also receive production lineage when global `[prompt_cache] enabled=true` (independent of the
OpenAI-specific cache block / `prompt_cache_key_mode`): OpenRouter body `session_id` + header `x-session-id` from
`vtcode:openrouter:{lineage}`; xAI body `prompt_cache_key` + header `x-grok-conv-id` from `vtcode:xai:{lineage}`. Blank
lineage omits the wire fields.

Merge Gateway native `/v1/responses` also sends session identity for automatic cache-aware routing: body `session_id`
and HTTP header `X-Session-Id`, both equal to the session lineage id (the cache key with any
`vtcode:merge:`/`vtcode:openai:`/`vtcode:openrouter:`/`vtcode:xai:` prefix and any residual `-{16 hex}` prefix-hash
suffix stripped). Blank lineage omits both fields.

### Provider cache affinity (session keys)

| Provider path                                          | Session / cache identity                             | Notes                                                                                                                       |
| ------------------------------------------------------ | ---------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------- |
| OpenAI native / ChatGPT / OpenResponses                | `prompt_cache_key` = `vtcode:openai:{lineage}`       | Routing hint on earlier models; optional separate cache accounting on GPT-5.6+                                              |
| Merge Gateway native                                   | `session_id` + `X-Session-Id` (+ `prompt_cache_key`) | Automatic caching on many upstream routes; no explicit `prompt_cache_breakpoint` markers (Merge rejects the field with 400) |
| OpenRouter                                             | body `session_id` + header `x-session-id`            | Sticky provider routing; body wins over header; **requires global `[prompt_cache] enabled=true`**                           |
| xAI native Chat Completions                            | header `x-grok-conv-id` + body `prompt_cache_key`    | Per-server cache affinity; **requires global `[prompt_cache] enabled=true`**                                                |
| Anthropic / Gemini / DeepSeek / Moonshot / Z.AI native | automatic or `cache_control`                         | No client session key                                                                                                       |

Usage telemetry: OpenAI-style hosts report cache reads as `prompt_tokens_details.cached_tokens` /
`prompt_cache_hit_tokens`; Anthropic-style as `cache_read_input_tokens`. VT Code maps these into both
`cached_prompt_tokens` and `cache_read_tokens` so trajectory `prompt_cache_metrics` and cache-health are not
zero-filled. Cost accounting still uses `cache_read_tokens` without double-counting. Merge Gateway native Responses
usage often reports cache activity only as Anthropic-style `cache_read_input_tokens` / `cache_creation_input_tokens`;
the same dual mapping keeps trajectory and session cache summaries honest.

The trajectory's `prompt_cache_metrics.cache_hit_ratio` is cached input divided by total normalized input (0 to 1),
including uncached tails and every sampling iteration. Anthropic and Minimax counters add cache reads and writes to
their exclusive input count before division. Raw token fields remain provider-reported; the ratio uses the same
normalization as canonical session usage. Older trajectory ratios used cache reads divided by reads plus writes and
could report 1.0 despite uncached input; use the recorded token counters when auditing those historical records.

Merge Gateway first-progress timeout is floored at **120s on planning and non-planning turns** when non-streaming
fallback is available. Other remote providers with non-streaming fallback get a once-per-session advisory when a stream
first-token timeout triggers the full-prompt retry (local providers excluded).

Blocked-session forensics: a blocked handoff copies `events.jsonl` and `derived/atif-trajectory.json` into
`{archive}-forensics/` beside the blocker archive when present, and writes `.vtcode/sessions/<id>/retention-pin.json` so
ordinary session retention does not evict the session while the blocker is unresolved. Resolving the archive unpins when
no other unresolved archive references the session.

VT Code surfaces prompt-cache churn using the same terminology in `vtcode trajectory` and `/share-log` exports:

- `stable_prefix` means the cache-friendly stable system prefix changed while the model stayed the same.
- `tool_catalog` means the tool-definition hash changed.
- `stable_prefix+tool_catalog` means both changed in the same observation.
- `model` means the model changed, so the provider-side cache lineage changed with it.

The share-log HTML overview also reports prompt-cache observations, churn breakdown, and the last visible change reason.

### Request segments and telemetry

Each request prefix is frozen in an immutable `SessionRequestEnvelope` for a request segment. Compaction starts the next
segment before the compacted history is installed; the old envelope, compacted-history artifact, and
`ThreadCompactBoundary` event remain recoverable. Automatic, manual, recovery, and model-switch compaction use the same
boundary path. A change to the model, provider, mode, instruction snapshot, or tool-catalog epoch starts a new segment;
unchanged turns reuse the existing envelope byte-for-byte.

The `tool_catalog_cache_metrics` trajectory record includes ordered wire tool names, catalog/wire/deferred counts, and
active loaded-skill names on startup or when that catalog identity changes. Unchanged turns omit those repeated lists.
`vtcode trajectory` compares the ordered snapshots so a catalog reorder across starts is visible instead of being hidden
by an unchanged tool count.

### Anthropic (Claude)

```toml
[prompt_cache.providers.anthropic]
enabled = true
tools_ttl_seconds = 3600
messages_ttl_seconds = 300
extended_ttl_seconds = 3600
max_breakpoints = 4
cache_system_messages = true
cache_user_messages = true
cache_tool_definitions = true
min_message_length_for_cache = 256
```

- `tools_ttl_seconds` — TTL for tool definitions and system prompt cache hints.
- `messages_ttl_seconds` — TTL for user message cache hints.
- `extended_ttl_seconds` — optional longer-lived TTL. When present, VT Code automatically opts into Anthropic’s extended
  prompt caching beta header. Budget-continuation profile requests use this TTL for the messages breakpoint (default
  1h), and the beta header follows whichever breakpoint actually resolves to 1h — including that profile promotion — so
  a promoted breakpoint is never sent without the header.
- `prefer_extended_ttl` — when true, tools/system/message breakpoints upgrade from 5m to 1h (2× write cost) for sessions
  that idle past the 5m window.
- `max_breakpoints` — maximum number of cache insertion points per request (tools, system prompt, user messages).
- `cache_system_messages` / `cache_user_messages` / `cache_tool_definitions` — toggle cache hints for each content type.
- `min_message_length_for_cache` — avoids setting cache hints on very short user messages.
- The tools-prefix breakpoint attaches to the last function tool: native server tools (web search, code execution,
  memory) carry no `cache_control` slot, so a trailing native tool no longer drops the whole tools-prefix breakpoint.
  Message breakpoints roll on the last two qualifying user messages; hardening passes (orphan-strip, adjacency,
  hoist-to-front, trailing-assistant guard) are deterministic repairs that keep grown histories byte-stable.

### Gemini

```toml
[prompt_cache.providers.gemini]
enabled = true
mode = "implicit"       # implicit | explicit | off
min_prefix_tokens = 128
explicit_ttl_seconds = 900
```

- `mode` — `implicit` leverages built-in cache detection; `explicit` uses the `cachedContents` lifecycle; `off` disables
  all Gemini caching.
- `min_prefix_tokens` — minimum prompt size before requesting cache evaluation.
- `explicit_ttl_seconds` — TTL for `cachedContents.create` (config default 1h; a null/absent value falls back to 900s).
  Explicit mode creates a cache of the system instruction + tools while the segment fingerprint (model/system/tools) is
  stable, then sends `cachedContent` on `generateContent`/`streamGenerateContent` while keeping conversation contents on
  the body. A fingerprint change creates a new cache and best-effort deletes the old name. When the provider reports the
  cached name as stale (expired or evicted), VT Code drops the slot and retries the request once without the cache on
  both the streaming and non-streaming paths. The API rejects `systemInstruction`, `tools`, and `toolConfig` next to
  `cachedContent`, so a request whose tool configuration is not the plain default `AUTO` (a constrained `tool_choice` of
  none/any/specific, `VALIDATED` decoding, or server-side tool invocation flags) skips the explicit cache and keeps its
  `toolConfig` on the body — the configuration stays enforceable instead of being dropped, and the installed segment
  entry is left intact for the next default-config turn. Those skipped requests fall back to implicit caching, so a
  session that always sends hosted/built-in tools (code execution, Google Search, URL context, …) will not use
  `cachedContents` at all. Create failures fall back to the implicit shape. Implicit caching needs 2,048+ tokens (2.5
  family) or 4,096+ (3.x) of stable prefix — below that, expect no hits by design.
- **Implicit-caching-only models:** Gemini 4 Argon rejects `cachedContents` entirely (creation fails and references in
  `generateContent` bodies error), so an `explicit` mode config is ignored for it and requests always use the implicit
  shape. Add future implicit-only models to `IMPLICIT_CACHING_ONLY_MODELS` in
  [`crates/codegen/vtcode-config/src/constants/models/google.rs`](../../crates/codegen/vtcode-config/src/constants/models/google.rs).

### Stable tool catalog across planning modes

`prompt_cache.stable_tool_catalog_across_modes` (default `true`) keeps the wire tool array identical on planning and
execution turns (union of both modes' tools). Planning toggles then do not rewrite the tool prefix. The fail-closed
execution gate still blocks mutations during planning; interview / `request_user_input` visibility remains mode-scoped.

### Cache write pricing and rate limits

Cache writes cost more than uncached input (default 1.25×; Anthropic 1h extended TTL 2×). `usage_cost::cache_write_rate`
applies that multiplier when a model catalog does not declare `cache_write`. Provider TPM-style limits still count the
full prompt volume (`uncached + cached + creation`); use `usage_cost::prompt_tokens_for_rate_limit` when estimating
rate-limit pressure.

### OpenRouter

```toml
[prompt_cache.providers.openrouter]
enabled = true
propagate_provider_capabilities = true
report_savings = true
```

- `propagate_provider_capabilities` — pass provider cache instructions straight through to upstream models.
- `report_savings` — surface cache-hit metrics returned by OpenRouter alongside standard usage data.

### DeepSeek

```toml
[prompt_cache.providers.deepseek]
enabled = true
```

DeepSeek caches automatically on disk with 64-token prefix units; no wire flags are needed. VT Code sends the system
prompt as the leading `messages[0]` system message so the stable prefix starts at token 0 — a requirement for DeepSeek
prefix matching. Keep volatile content at the end of the history; any byte difference at the front busts the whole
prefix.

### Z.AI

```toml
[prompt_cache.providers.zai]
enabled = true
```

Z.AI handles caching server-side. When the override is enabled, VT Code honors upstream behavior and surfaces usage
metrics when available.

## Usage Telemetry

When caching is active, `Usage` structs now include:

- `cached_prompt_tokens` — tokens served from cache (OpenAI, OpenRouter).
- `cache_creation_tokens` — tokens spent establishing a new cache entry (including OpenAI `cache_write_tokens`).
- `cache_read_tokens` — tokens satisfied from an existing cache entry (Anthropic, OpenRouter).

These metrics flow through `vtcode-core::llm::types::Usage` and appear anywhere VT Code reports token accounting.

## Validation & Testing

- Unit tests in `crates/codegen/vtcode-llm/src/providers/anthropic.rs` validate cache control insertion and beta header
  composition.
- `crates/codegen/vtcode-llm/src/providers/openrouter/` exercises usage parsing and session-affinity wire identity (body
  `session_id` + `x-session-id`).
- `crates/codegen/vtcode-llm/src/providers/xai.rs` covers lineage `prompt_cache_key` injection and `x-grok-conv-id`
  header naming.
- Local cache behavior tests in `crates/codegen/vtcode-core/src/core/prompt_caching.rs` verify caching, eviction, and
  persistence.
- Configuration loading tests ensure settings from `vtcode.toml` are applied correctly.
- Run `cargo nextest run` to execute all fast tests after updating configuration or provider cache logic (never
  `cargo test`).

## Implementation Architecture

The prompt caching system is implemented as a multi-layered architecture:

1. **Global Configuration Layer**: Managed in `crates/codegen/vtcode-config/src/core/prompt_cache.rs` with global and
   per-provider settings
2. **Provider Integration Layer**: Each provider has specific cache control implementation in
   `crates/codegen/vtcode-llm/src/providers/`
3. **Local Caching Layer**: File-based caching engine in `crates/codegen/vtcode-core/src/core/prompt_caching.rs` for
   optimized prompt storage
4. **Runtime Integration**: Cache configuration flows through the provider factory to ensure proper initialization

## Migration Guide

When upgrading to the new prompt caching system:

1. Add the `[prompt_cache]` section to your `vtcode.toml` if you want to customize caching behavior
2. Review provider-specific settings to optimize for your usage patterns
3. Monitor cache metrics to verify the system is performing as expected

## Troubleshooting

- If caching isn't working as expected, verify that both global and provider-specific `enabled` flags are set to `true`
- Check that your prompts meet the minimum token requirements for each provider
- Enable verbose logging to see cache interaction details

By tuning these values you can balance latency, cost, and cache freshness per provider while keeping the behaviour
consistent across the VT Code agent ecosystem.
