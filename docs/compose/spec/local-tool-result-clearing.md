---
feature: local-tool-result-clearing
status: delivered
updated: 2026-09-27
branch: feat/local-tool-result-clearing
commits: ace5ba394..fd05baef2
---

# Local Tool-Result Clearing (non-Anthropic)

## Report

**What was built** — Request-only local tool-result clearing for providers without Anthropic `context_management.edits`.
When estimated history tokens exceed `trigger_tokens`, `clear_old_tool_results` stubs every tool-result body except the
newest `keep_tool_uses`, preserving `tool_call_id` pairing and never mutating durable history or `ThreadEvent`s.
`clear_tool_inputs` replaces paired call arguments with a JSON placeholder. The request builder applies this only when
the wire will not carry native `clear_tool_uses`.

**Verification** — `./scripts/check-dev.sh` PASS;
`cargo nextest run -p vtcode-core -E 'test(clear_old_tool) or test(local_tool_result)'` PASS 7/7;
`RUSTFLAGS="-D warnings" cargo check --locked -p vtcode-core -p vtcode` PASS; request-path filter 18/18 PASS.
PRE-EXISTING: `cli_harness_failures::print_mode_requires_prompt_or_stdin` (env noise).

### Journey log

- First cut used `clear_at_least_tokens` as a stop ceiling; review correctly flagged that re-shaping full durable
  history then leaves a permanently growing tail. Floor semantics (stub all non-kept) is required.
- `clear_tool_inputs` must write valid JSON (`{"cleared":"tool_input"}`), not prose — OpenAI sends `function.arguments`
  verbatim.
- Clear order is oldest-first (Anthropic), not newest-of-old.

## [S1] Problem

`agent.harness.tool_result_clearing` defaults to enabled and `docs/development/EXECUTION_POLICY.md` claims old tool
results are stripped past `trigger_tokens`. The only implementation is Anthropic wire
`context_management.edits.clear_tool_uses_20250919` (`context_management.rs:34-36`). For every other provider (OpenAI,
Merge Gateway, ZAI/GLM, OpenRouter, xAI, …) the setting is a silent no-op: tool result bodies stay in the request
history until late compaction. Live trajectory evidence (`.vtcode/logs/trajectory.jsonl`, `zai/glm-5.3-flash`) shows
`message_history_tokens` growing 44k→50k with no reclaim while `tool_result_clearing.enabled` is true. Every subsequent
request pays the full accumulated tool-result text.

## [S2] Design

Request-only history shaping, same contract as `normalize_history_for_request`: never mutate durable session history or
`ThreadEvent`s.

**Pure function** (new, `vtcode-core::core::agent::state`):

```rust
pub fn clear_old_tool_results(
    messages: &[Message],
    trigger_tokens: u64,
    keep_tool_uses: u32,
    clear_at_least_tokens: u64,
    clear_tool_inputs: bool,
) -> Vec<Message>
```

Semantics (mirror Anthropic `clear_tool_uses_20250919`):

1. Estimate total history tokens (`Message::estimate_tokens`). If estimate < `trigger_tokens`, return
   `messages.to_vec()` unchanged.
2. Collect Tool-role message indices from the end. Keep the newest `keep_tool_uses` results untouched. Stub **every**
   older tool result (oldest-first). `clear_at_least_tokens` is a floor on reclaimed tokens, not a stop ceiling —
   stopping early would leave a permanently growing tail when the function re-shapes full durable history each request.
3. Preserve `role`, `tool_call_id`, `origin_tool`, and message order so provider tool-pairing validation still passes.
4. When `clear_tool_inputs`, also replace `tool_calls[].function.arguments` and freeform `text` on Assistant messages
   whose paired results were cleared, with the JSON placeholder `{"cleared":"tool_input"}` (never prose — providers send
   `arguments` verbatim).

**Stub shape** (stable, compact, actionable):

```json
{
  "cleared": "tool_result",
  "reason": "tool_result_clearing",
  "note": "Older tool result cleared to bound context growth. Full output remains in session logs; re-run the tool if raw bytes are needed.",
  "tool": "<origin_tool or omitted>",
  "tool_call_id": "<id>"
}
```

**Call site** (interactive `request_builder.rs` and headless `runner/execute.rs`, after
`normalize_history_for_request`):

Apply local clearing when `tool_result_clearing.enabled` AND the request will NOT carry Anthropic `clear_tool_uses`
(i.e. not `provider_name == "anthropic" && capabilities.context_edits`). Never both.

### Out of scope / non-goals

- No `ThreadEvent` or `vtcode-exec-events` changes.
- No persistent mutation of `AgentSessionState.messages`.
- No change to Anthropic wire `context_management` payload.
- No change to `prune_oversized_tool_outputs` (compaction-only).

## [S3] Out of Scope

- Cross-turn exploration digests (ranked #3).
- Duplicate-read stub re-injection (ranked #2).
- Safety deny-message improvements (ranked #5).

## Tasks

- [x] T1: Pure `clear_old_tool_results` in `vtcode-core` with unit tests — acceptance: keeps newest N results, stubs all
      older ones past trigger, preserves `tool_call_id`, floor on `clear_at_least_tokens`, no-op below trigger (covers:
      S2)
- [x] T2: Wire request_builder local path when native clear_tool_uses is absent — acceptance: gate helper tests +
      durable-history immutability test show request messages carry stubs while working_history is unchanged (covers:
      S2; depends: T1)
- [x] T3: Docs — EXECUTION_POLICY + CONFIG_FIELD_REFERENCE note provider split — acceptance: docs state local clearing
      for non-Anthropic and native edits for Anthropic (covers: S1, S2; depends: T2)
