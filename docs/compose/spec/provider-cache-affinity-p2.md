---
feature: provider-cache-affinity-p2
status: delivered
updated: 2026-09-19
branch: fix/harness-stability-cost-audit
commits: 3abc6978f..a93f7fa3f
---

# Provider Cache Affinity P2

## Report

### What was built

Provider cache/session affinity and cost-stability analogues beyond P1:

1. **Session affinity end-to-end for OpenRouter + xAI.** Shared `session_affinity_key_enabled` /
   `build_session_affinity_prompt_cache_key` in `vtcode-config`; interactive `request_builder` and AgentRunner
   `execute.rs` both populate `LLMRequest.prompt_cache_key` as `vtcode:openrouter:{lineage}` / `vtcode:xai:{lineage}`
   when global `[prompt_cache] enabled=true` (independent of the OpenAI cache block and `prompt_cache_key_mode`).
   OpenRouter dispatch sends body `session_id` + header `x-session-id` on primary and feature-fallback paths; xAI
   `finish_payload` inserts lineage `prompt_cache_key` and dispatch attaches `x-grok-conv-id`. Shared lineage helper
   strips `vtcode:{merge,openai,openrouter,xai}:` and residual `-{16 hex}` suffixes. Automatic-cache providers
   (Anthropic/Gemini/DeepSeek/Kimi/Z.AI native) intentionally get no invented session wire fields.

2. **Timeout + billing advisory.** Merge-gateway planning first-progress floor is `max(budget, 120)` when non-streaming
   fallback is available (still capped at 180). Remote non-streaming-capable providers get a once-per-session
   stream-timeout billing advisory (local providers excluded); merge-specific wording retained.

3. **Usage dual-field mapping.** `parse_usage_openai_format` maps OpenAI-style cache hits into both
   `cached_prompt_tokens` and `cache_read_tokens` (cost still bills only `cache_read_tokens`). Shared write-token parser
   also accepts top-level `prompt_cache_write_tokens` / `cache_write_tokens` for OpenRouter-style hosts.

4. **Docs.** `docs/tools/PROMPT_CACHING_GUIDE.md` provider affinity table, production lineage gating, usage notes, merge
   120s floor + generic advisory, and validation paths updated to `vtcode-llm` + `cargo nextest run`.

Memory-envelope isolation and blocker retention pinning remain provider-agnostic from P1; no further P2 work.

### Verification

- `cargo fmt --all` PASS
- `cargo clippy` (default members / changed crates, `-D warnings`) PASS
- Targeted nextest: vtcode-config affinity builders, vtcode-llm OpenRouter/xAI/session/usage, vtcode
  timeout/advisory/gate tests — all PASS
- `./scripts/check-dev.sh --test` PASS — **8138 tests passed, 12 skipped**
- Independent review (post-critical-fix): **success** — C1/C2 closed in production wiring; T1–T8 acceptance evidence
  present; no remaining critical/medium findings

### Journey log

1. Grill settled scope: OpenRouter+xAI session identity, merge planning floor + generic advisory, OpenAI-format usage
   dual-map; no invented session keys or per-provider SLAs.
2. Implemented provider plumbing + runloop floors; first review found production `prompt_cache_key` still gated to
   openai|merge only (dead OpenRouter/xAI identity) and missing T2/T3 acceptance tests.
3. Added shared session-affinity builders, wired interactive + AgentRunner paths, cleaned OpenRouter fallback injection,
   extended usage write-token parsing, added wiremock/unit acceptance coverage.
4. Re-verified with `./scripts/check-dev.sh --test` (8138 pass) and independent re-review (success).

## [S1] Problem

P1 (`harness-stability-cost-p1`) fixed Merge Gateway session identity, merge-only first-token timeout floor,
memory-envelope objective isolation, and blocker retention pinning. Research of all VT Code providers showed remaining
gaps:

1. **OpenRouter** documents `session_id` / `x-session-id` as required for agent sticky routing (10 min); without it
   cache affinity depends on opener-hash and hit rate collapses. **xAI** documents `x-grok-conv-id` / `prompt_cache_key`
   for per-server cache affinity. Neither path sends lineage-stable session identity today.
2. **Merge-gateway planning turns** can still abandon at 90s first-progress (below Gateway’s documented 120s first-frame
   silence). Many other paid providers advertise `supports_non_streaming=true` (stream-timeout → full-prompt retry) with
   **no billing advisory** when that path fires.
3. **Shared usage parsers** often leave `cache_read_tokens=None` even when the host returns OpenAI-style `cached_tokens`
   / `prompt_cache_hit_tokens` / Anthropic-style `cache_read_input_tokens`, so trajectory/cache-health telemetry
   under-reports hits on DeepSeek, OpenAI-compat hosts, and some gateway routes.
4. Memory-envelope isolation and blocker retention pinning are already provider-agnostic (P1); no further work unless a
   provider-specific session store appears.

## [S2] Design

Settled decisions (2026-09-19 grill):

### Session identity (covers S2-session)

| Provider                                                                                               | Contract                                                                                                                                                                                                                   |
| ------------------------------------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **OpenRouter**                                                                                         | Body `session_id` + header `x-session-id` from lineage (cache key with any `vtcode:` namespace / residual `-{16 hex}` suffix stripped). Blank lineage omits both. Prefer body over header per OpenRouter docs (body wins). |
| **xAI native**                                                                                         | Header `x-grok-conv-id` **and/or** body `prompt_cache_key` when the request targets an xAI/Grok route and lineage is non-blank; use the same stable lineage id. Omit when blank.                                           |
| **OpenAI / ChatGPT / OpenResponses**                                                                   | Keep existing `prompt_cache_key` forwarding; interactive key remains session-stable (P1). No `session_id` invented for native OpenAI.                                                                                      |
| **Anthropic / Gemini / DeepSeek / Moonshot / Z.AI native / NVIDIA / Mistral / Qwen / MiniMax / local** | No client session key (automatic, cryptographic prefix, or cache_control). Do not invent undocumented wire fields.                                                                                                         |
| **merge-gateway**                                                                                      | Already done in P1 (`session_id` + `X-Session-Id`).                                                                                                                                                                        |

Shared lineage helper: strip `vtcode:openai:` / `vtcode:merge:` / `vtcode:openrouter:` / `vtcode:xai:` prefixes and any
residual `-{16 hex}` suffix (same contract as `merge_session_identity`).

### Timeout + billing advisory (covers S2-timeout)

- **Merge-gateway planning + `supports_non_streaming`**: first-progress floor `max(planning_budget, 120)` still capped
  at 180 (closes the 90s residual). Non-merge providers keep existing planning math.
- **Generic advisory**: once per session, when **any** remote provider with `supports_non_streaming=true` falls back
  after stream first-token timeout, emit provider-agnostic warning: stream timed out; abandoned work may still be
  billed; non-streaming retry re-sends the full prompt. Merge-specific wording can remain in the merge path or share
  this helper.
- **Do not invent** per-provider first-progress floors for OpenAI/Anthropic/Gemini/etc. without a captured contract;
  120s remains Merge’s documented number only.
- Local providers (ollama/lmstudio/llamacpp) may use the same advisory (harmless) or be excluded; advisory text must not
  claim remote billing when provider is local — exclude local providers from the advisory.

### Usage cache-field mapping (covers S2-usage)

| Parser / provider                                | Behavior                                                                                                                                                                    |
| ------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `providers/common.rs::parse_usage_openai_format` | When include_cache_metrics, set `cache_read_tokens` from the same value as `cached_prompt_tokens` (OpenAI-style `prompt_tokens_details.cached_tokens` etc.).                |
| OpenRouter `parse_usage_value`                   | Keep `cached_prompt_tokens` = `cache_read_tokens`; also parse `prompt_tokens_details.cached_tokens` / `cache_write_tokens` if missing today.                                |
| Anthropic `parse_usage`                          | Keep mapping `cache_read_input_tokens` → both `cache_read_tokens` and `cached_prompt_tokens`.                                                                               |
| Gemini helpers                                   | Keep dual mapping of `total_cached_tokens`.                                                                                                                                 |
| Merge native                                     | Already dual-maps Anthropic-style reads (P1).                                                                                                                               |
| DeepSeek / OpenAI-compat hosts                   | Rely on shared `parse_usage_openai_format` + shared `parse_cached_prompt_tokens_from_usage`; ensure `prompt_cache_hit_tokens` / `cached_tokens` populate both Usage fields. |

Docs: extend `docs/tools/PROMPT_CACHING_GUIDE.md` with a provider cache-family table (automatic / explicit /
session-key) and usage field notes. Memory envelope + retention pinning documented in P1 remain unchanged.

### Out of scope (covers S3)

- P0/P1 code that already landed (except where this spec extends merge planning timeout).
- Cost-oriented compaction for large windows.
- Verification-gate auto-continue for piped verifiers.
- Per-session tracker files.
- Raising global retention limits.
- Wire-level Anthropic `cache_control` on merge `anthropic/*` routes.
- Changing `ThreadEvent` or ATIF schema.

## [S3] Out of Scope

See design out-of-scope table above. Also: inventing undocumented session-id fields on automatic-cache providers; OpenAI
Responses explicit breakpoint API; DeepSeek/Kimi wire flags that public docs do not define.

## Tasks

- [x] T1: Shared lineage helper — acceptance: unit tests strip vtcode namespace + residual `-{16 hex}` suffix; blank →
      None (covers: S2-session)
- [x] T2: OpenRouter session identity — acceptance: native/OpenAI-compat payload includes `session_id`; HTTP sends
      `x-session-id`; blank omits both (covers: S2-session; depends: T1)
- [x] T3: xAI session identity — acceptance: Grok route payload/header uses stable lineage when present; blank omits
      (covers: S2-session; depends: T1)
- [x] T4: Merge planning first-progress floor — acceptance: unit test
      `llm_first_progress_timeout_secs(150, true, true, "merge-gateway") == 120` (was 90); cap still 180 (covers:
      S2-timeout)
- [x] T5: Generic stream-timeout billing advisory — acceptance: once-per-session advisory for remote
      `supports_non_streaming` providers on stream-timeout fallback; local providers excluded; unit test
      first/second/non-stream/local cases (covers: S2-timeout)
- [x] T6: OpenAI-format usage mapping — acceptance: `parse_usage_openai_format` with cached_tokens sets both
      `cached_prompt_tokens` and `cache_read_tokens`; existing cache-disabled path still None (covers: S2-usage)
- [x] T7: OpenRouter/xAI/compat usage gaps — acceptance: parsers populate dual cache fields when host returns documented
      shapes; unit tests for OpenRouter cached_tokens + write tokens if missing (covers: S2-usage)
- [x] T8: Docs — acceptance: PROMPT_CACHING_GUIDE lists OpenRouter/xAI session identity, usage mapping, merge planning
      floor + generic advisory (covers: S2-session, S2-timeout, S2-usage)
- [x] T9: Verify — acceptance: `./scripts/check-dev.sh --test` green in worktree (covers: all)
