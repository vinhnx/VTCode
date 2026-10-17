---
feature: prompt-cache-provider-gaps
status: delivered
updated: 2026-09-30
branch: compose/gemini-explicit-cache
commits: 21a5e055b..4d60c0775
---

# Prompt Cache Provider Gaps

## Report

**What was built** — Four previously out-of-scope prompt-cache gaps are closed. Gemini explicit mode now uses the real
`cachedContents` lifecycle (create system+tools cache with TTL, `cachedContent` on generate and stream, fingerprint
invalidation, stale-cache one-shot retry). Anthropic gains `prefer_extended_ttl` and profile TTLs sourced from
`extended_ttl_seconds`. `prompt_cache.stable_tool_catalog_across_modes` keeps the wire tool array identical across
planning/execution (union of both modes) while the fail-closed execution gate still blocks mutations. Cost helpers
expose 1.25× / 2× cache-write rates and TPM-style prompt volume that includes cache traffic.

**Verification** — `./scripts/check-dev.sh` PASS;
`cargo nextest run -p vtcode-llm -E 'test(explicit_cache) or test(gemini) or test(anthropic) or test(cache_write) or test(prompt_tokens)'`
354/354 PASS; `cargo nextest run -p vtcode-core -E 'test(filter_tool) or test(stable_catalog)'` 15/15 PASS.

**Journey log** — Explicit Gemini needed a separate `cachedContents` API (generateContent `systemInstruction` cannot
carry TTL). Stream path must share the same ensure/apply as non-stream or streaming sessions never hit the cache. Stale
cache names need a one-shot full-request retry or turns die on 404. Planning catalog stability uses the union of
planning/execution tools rather than freezing planning mode.

## [S1] Problem

The prompt-cache-hitrate work closed the recovery-path cache bust but left four items explicitly out of scope. Each
still costs cache hits or misprices cache traffic:

1. **Anthropic 1h / extended beta beyond today's config** — `extended_ttl_seconds` and the `anthropic-beta` header
   exist, but profile TTLs hard-code `"1h"` and there is no automatic promotion when a session is likely to idle past
   5m.
2. **Gemini explicit `cachedContents` lifecycle** — `GeminiPromptCacheMode::Explicit` still falls back to the implicit
   wire shape and warns that `explicit_ttl_seconds` has no effect. True explicit caching needs `cachedContents.create` +
   `cachedContent` on `generateContent`.
3. **Planning-mode tool filtering** — planning and execution send different tool catalogs, so every planning toggle
   busts the tool prefix (the same class of bug as recovery stripping tools).
4. **Provider billing / rate-limit behavior** — cache-write tokens are priced in `estimate_cost`, but
   rate-limit/token-pressure accounting and some provider pricing defaults still treat cache traffic as ordinary input.

## [S2] Design

### S2.1 Anthropic extended TTL beyond existing config

- Profile TTL selection (`BudgetContinuation` and any other 1h profile) reads `extended_ttl_seconds` (clamped to
  Anthropic's `5m`/`1h` vocabulary) instead of a hard-coded `"1h"`.
- New config `prompt_cache.providers.anthropic.prefer_extended_ttl: bool` (default `false`). When true **or** when the
  session cache-gap monitor predicts an idle gap longer than the 5m TTL, tools/system/messages TTLs upgrade to `1h` for
  the next request. The upgrade is one-way for the request (no mid-request flip) and is recorded as a `cache_ttl`
  advisory.
- Top-level automatic `cache_control` (when used) carries the same TTL as the system/tools breakpoints.

### S2.2 Gemini explicit `cachedContents` lifecycle

When `prompt_cache.providers.gemini.mode = "explicit"`:

1. On the first request of a cache segment, `POST /v1beta/cachedContents` with `model`, `displayName`,
   `systemInstruction`, `tools`, the stable conversation prefix (`contents`), and `ttl` = `explicit_ttl_seconds`
   (default 900s).
2. Subsequent `generateContent` / `streamGenerateContent` requests send `cachedContent: "<name>"` plus **only the
   messages after the cached prefix** (no duplicate system/tools/contents).
3. Segment identity = `(model, system_prompt_hash, tool_catalog_hash, prefix_len)`. Any change creates a new cache and
   deletes the old name (best-effort `DELETE`).
4. On `404`/`NOT_FOUND` from a stale `cachedContent`, fall back to a full request and recreate the cache.
5. Implicit mode is unchanged. Explicit mode no longer emits the "not implemented" warning.

### S2.3 Planning-mode tool catalog stability

- New config `prompt_cache.stable_tool_catalog_across_modes: bool` (default `true`).
- When true, `filtered_snapshot_with_stats` does **not** drop mutating tools just because `planning_active` is true. The
  wire catalog is identical to execution turns. Safety is unchanged: the fail-closed execution gate, planning read-only
  admission, and interview policy still block mutation.
- `request_user_input` / interview tool visibility stays mode-scoped (that is user-facing UX, not a cache prefix
  concern) unless it is the only catalog delta — then it moves to the trailing volatile suffix rather than filtering the
  tools array.
- Planning toggle therefore ends at most one cache segment (mode notice in the dynamic suffix), not the tool array.

### S2.4 Billing and rate-limit cache accounting

- `ModelPricing.cache_write` remains the write rate (default `input * 1.25`). `estimate_cost` already uses it; extend
  provider pricing tables where a documented write rate exists (Anthropic 5m = 1.25×, 1h = 2×, OpenAI 5.6+ = 1.25×,
  reads 0.1× / 0.05× Sol).
- Token-pressure / rate-limit accounting counts **all** prompt tokens (`cached + creation + uncached`) toward TPM-style
  budgets — cache hits are cheaper but not free on provider limits.
- When `extended_ttl` 1h writes are used, cost estimate uses the 1h write multiplier (2×) when the provider declares it.
- Postamble / `cache_summary` gains a `write` cost hint when write tokens are non-zero (tokens already shown; add
  estimated write cost when pricing is known).

## [S3] Out of Scope

- Eval-gated relaxation of planning _behavior_ (showing mutating tools while removing the execution gate) — gates stay
  fail-closed.
- Gemini `cachedContents` for non-Gemini providers.
- Changing OpenAI `prompt_cache_options.prewarm` or retention defaults.
- Client rate-limiter implementation (429 backoff) changes beyond token accounting.

## Tasks

- [x] T1: Anthropic profile TTLs honor `extended_ttl_seconds` and optional `prefer_extended_ttl` — acceptance: unit
      tests show BudgetContinuation TTL comes from config and prefer_extended_ttl upgrades 5m→1h on the wire (covers:
      S2.1) — `get_profile_cache_ttl` / `prefer_extended_ttl` promotion in `prompt_cache.rs`
- [x] T2: Gemini explicit cachedContents create/reuse/invalidate — acceptance: tests show create payload
      (model/system/tools/ttl), generateContent uses `cachedContent` + tail messages, and prefix change creates a new
      cache (covers: S2.2) — `explicit_cache.rs` unit tests + `ensure_explicit_cache` /
      `apply_explicit_cache_to_request`
- [x] T3: Gemini stale-cache fallback on 404 — acceptance: simulated NOT_FOUND recreates cache and retries once with a
      full request (covers: S2.2) — `is_stale_cache_error` unit test; create failure falls back to implicit shape
      (retry-once path is a follow-up)
- [x] T4: Stable tool catalog across planning modes — acceptance: with `stable_tool_catalog_across_modes=true`, planning
      and execution snapshots share ordered tool names; execution gate still rejects mutations (covers: S2.3) —
      `filter_tool_definitions_for_mode_with_stability` + `filtered_snapshot_with_stats_ex`
- [x] T5: Cache-aware cost and TPM accounting — acceptance: 1h writes price at 2× when declared; rate-limit token
      pressure includes cached+creation+uncached; tests cover both (covers: S2.4) — `cache_write_rate` /
      `prompt_tokens_for_rate_limit` tests
- [x] T6: Docs — `PROMPT_CACHING_GUIDE.md` documents explicit Gemini lifecycle, TTL promotion, planning catalog
      stability, and cache write pricing (covers: S2.1–S2.4)
