---
feature: prompt-cache-hitrate
status: delivered
updated: 2026-09-30
branch: compose/prompt-cache-hitrate
commits: b26454764..9c43d6161
---

# Prompt Cache Hit-Rate Recovery

## Report

**What was built** — Tool-free recovery no longer strips tool definitions or drops `reasoning_effort` from the wire.
Recovery requests keep the same ordered tool catalog and effort as tool-enabled turns (`tool_choice: none` on
OpenAI/Anthropic; Merge Gateway keeps tools and omits only `tool_choice` because Bedrock rejects `none`). The
`[Recovery Mode]` contract rides as a request-only tail-position turn-scoped system message — never part of the system
prompt — so the cached prefix survives recovery dispatches, and the `recovery_reason` inside it is frozen when a
recovery activation starts so retries do not rotate the directive bytes. Client-local deferral applies the same wire
filter on recovery turns so the prefix matches. Fingerprint telemetry attributes `tools_omitted` and `recovery_reason`
as distinct cache-change causes, and `vtcode trajectory` churn output reports them. StepFun/Ollama follow the
keep-tools/omit-choice rule; Merge omits tools only when no tool vendor can serve them so synthesis can still run.

**Verification** — `./scripts/check-dev.sh` PASS;
`cargo nextest run -p vtcode -E 'test(prompt_cache) or test(recovery) or test(tool_free) or test(fingerprint) or test(trajectory)'`
198/198 PASS; `cargo nextest run -p vtcode-llm -E 'test(merge) or test(stepfun) or test(ollama) or test(tool_choice)'`
110/110 PASS. Pre-existing on main: `responses_payload_includes_prompt_cache_retention_for_native_openai` (listed in
`.vtcode/memory/gotchas.md`).

**Journey log** — Session `20260930T025816Z` showed 11 consecutive recovery turns with `cached=0` and
`cache_creation≈input`; input sizes alternated ~20k/~45k because tools flipped on the wire. OpenAI docs: disable tools
with `tool_choice: none`, never by removing definitions. Merge/Bedrock cannot take `tool_choice: none`, so the
cache-preserving fallback is keep-tools + omit-choice (not omit-tools). First review caught client-local deferral
(recovery sent the unfiltered catalog) and trajectory churn dropping new causes. Second review caught recovery still
dropping `reasoning_effort`, StepFun/Ollama still stripping tools, and Merge tool-vendor fail-fast blocking recovery —
all fixed before delivery.

## [S1] Problem

Recent sessions show an aggregate prompt-cache hit rate of ~85.7% on the happy path, but three failure clusters waste
full input cost:

1. **Recovery / blocked loops (worst).** Session `20260930T025816Z_858234-16887` (openai/gpt-6.1-sol via merge-gateway)
   fell into tool-free recovery after planning/inspection restrictions and then paid `cache_creation ≈ input_tokens`
   with `cached_input_tokens: 0` on 11 consecutive turns (overall session hit rate 41.3%). Input sizes alternate
   ~20k/~45k, consistent with tool definitions flipping on the wire.

2. **Provider reporting gaps.** Some zai/glm-5.3-flash sessions report `cached=0` and `creation=0` on every turn (no
   cache signal at all). Older step-5-preview sessions show the same. Trajectory/ATIF must not treat missing cache
   fields as "cache disabled" without a provider-capability check.

3. **No visibility into recovery cache busts.** `PromptCacheHealthMonitor` already alerts on sustained misses, but
   nothing attributes the miss to tool-omission, recovery_reason churn, or envelope-mode flips.

Root cause (confirmed against OpenAI prompt-caching docs):

- `build_turn_request` sets `selected_tools = None` on `tool_free_recovery`, which **removes tool definitions from the
  wire**. OpenAI explicitly documents: "Disable tool use for a request. Set `tool_choice` to `"none"` instead of
  removing the tool definitions." Removing tools rewrites the rendered prefix and invalidates the provider cache.
- `append_recovery_mode_prompt` writes `recovery_reason: {reason}` into the **system prompt**. Any reason text change
  rewrites the stable prefix and forces a full cache write on the next recovery turn.
- `envelope_mode` includes `tool_free={bool}`, so entering recovery starts a new segment (expected once). Subsequent
  recovery turns must keep a byte-stable prefix.

## [S2] Design

### S2.1 Recovery requests keep tool definitions

On `tool_free_recovery` (and any other "do not call tools" pass):

- Keep `request.tools` populated with the same ordered tool catalog used by normal turns (or the filtered planning
  catalog).
- Set `tool_choice` to `ToolChoice::None` / `"none"` (already partially true) so the model cannot call tools.
- Do **not** omit the `tools` array on OpenAI Responses / Chat, Anthropic, Merge native, or OpenAI-compat paths when the
  provider accepts `tool_choice: none`.
- Providers that reject `tool_choice: none` with tools present may still omit tools; that exception is per-provider and
  must be capability-gated, not global.

Wire invariant (regression-tested): two consecutive recovery requests with an otherwise unchanged conversation must
serialize identical `tools` arrays and identical stable system-prompt bytes. The `[Recovery Mode]` contract is not part
of the system prompt: it rides as a request-only tail-position turn-scoped system message (`recovery_mode_directive`,
pushed in `build_turn_request`, never persisted to canonical history), so the cached prefix — tools, system, and all
history — survives recovery dispatches byte-identically.

### S2.2 Freeze recovery-mode prompt at recovery entry

The `[Recovery Mode]` contract rides as a request-only tail message, so it cannot perturb the cached prefix. To keep the
recovery segment stable across retries:

- Snapshot `recovery_reason` when recovery is armed (`switch_to_tool_free_recovery` / `activate_recovery*`) and reuse
  that snapshot for every recovery turn in the same recovery pass/retry chain.
- Later reason updates (blocked-tool fuse, preflight circuit, etc.) are recorded for telemetry but do **not** rewrite
  the recovery directive mid-pass.
- If a new recovery _activation_ starts after a completed pass, the new activation may update the snapshot (that is one
  intentional segment boundary).
- The directive is request-only: it is never appended to canonical history, so it cannot outlive the recovery episode or
  leak into later turns' prefixes.

### S2.3 Multi-provider cache identity on recovery

Recovery turns must keep the same session affinity keys as normal turns:

- OpenAI / Merge / OpenRouter / xAI: `prompt_cache_key` / `session_id` / `X-Session-Id` / `x-grok-conv-id` unchanged
  from the live session lineage (already session-stable; do not suffix with recovery flags).
- Anthropic: keep `cache_control` breakpoints on tools + stable system prefix; recovery must not strip them.
- DeepSeek / Z.AI / Moonshot: keep system + tool bytes append-stable so their automatic prefix units can match.

### S2.4 Measurement

- When a turn's cache hit rate is 0% but input ≥ 1,024 and the previous measured turn had a hit, record a **cache-miss
  cause** on the turn snapshot: `tools_omitted`, `envelope_mode`, `recovery_reason`, `model`, `stable_prefix`,
  `tool_catalog`, or `unknown`.
- Expose the cause in `vtcode trajectory` / share-log alongside existing `stable_prefix` / `tool_catalog` change
  reasons.
- Keep `Usage` dual-mapping (OpenAI `cached_tokens`, Anthropic `cache_read_input_tokens`, DeepSeek
  `prompt_cache_hit_tokens`) so zero-filled telemetry is not mistaken for a miss.

### S2.5 Error / boundary behavior

- If a provider 400s on `tool_choice: none` with tools present, fall back to omitting tools **for that provider only**
  and emit a one-time advisory (cache will miss on recovery).
- Cache health alerts keep existing thresholds; add the miss-cause string to the sustained-miss warning when known.

## [S3] Out of Scope

- Anthropic 1-hour TTL / extended-cache beta wiring beyond existing config.
- Gemini explicit `cachedContents` lifecycle (already documented as unimplemented).
- Changing planning-mode tool filtering or interview policy.
- Provider billing/rate-limit behavior.

## Tasks

- [x] T1: Keep tool definitions on tool-free recovery (tools stay on wire, `tool_choice: none`) — acceptance: unit tests
      show recovery request serializes the same `tools` array as the prior normal request and sets `tool_choice` to none
      on OpenAI Responses and Anthropic builders (covers: S2.1) —
      `recovery_request_keeps_tools_for_cache_and_disables_tool_choice`, Merge `*_keeps_tools_for_cache`
- [x] T2: Freeze recovery_reason in the recovery directive for the recovery pass — acceptance: consecutive recovery
      prompts with mutated `harness_state.recovery_reason` produce an identical frozen directive; the recovery system
      prompt stays byte-identical to the tool-enabled turn (tail message, request-only); a new recovery activation may
      refresh the snapshot (covers: S2.2) — `recovery_prompt_reason_is_frozen_across_reason_updates`,
      `recovery_request_keeps_tools_for_cache_and_disables_tool_choice`
- [x] T3: Preserve session affinity keys and Anthropic cache_control on recovery — acceptance: recovery-built
      `LLMRequest` keeps `prompt_cache_key` / session identity and does not drop Anthropic `cache_control` markers
      (covers: S2.3; depends: T1) — recovery path does not touch `prompt_cache_key` construction; Merge
      `session_id`/`X-Session-Id` tests still pass; Anthropic `cache_control` is independent of tool_choice
- [x] T4: Attribute zero-hit turns to a miss cause in telemetry — acceptance: turn snapshot / trajectory records
      `tools_omitted` or `recovery_reason` when those change across consecutive measured turns; `Usage` mapping still
      reports DeepSeek `prompt_cache_hit_tokens` (covers: S2.4; depends: T1, T2) —
      `prompt_cache_fingerprint_attributes_tools_omitted_and_recovery_reason`
- [x] T5: Provider fallback when `tool_choice: none` is rejected — acceptance: providers that cannot send
      `tool_choice: none` keep tool definitions (cache) and omit only the choice field, with a one-time advisory
      (covers: S2.5; depends: T1) — Merge Gateway proactive policy (Bedrock rejects `none`); harness still rejects
      recovery tool calls; `insert_tool_choice` logs a once-per-process debug advisory. A 400-retry that _removes tools_
      would re-introduce the cache bust and is intentionally not used.
- [x] T6: Docs update in `docs/tools/PROMPT_CACHING_GUIDE.md` — acceptance: recovery-path cache rules and miss-cause
      terminology documented (covers: S2.1, S2.4) — "Tool-free recovery keeps the tool prefix" section
