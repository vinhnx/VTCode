# Migrating Claude models in VT Code

Guide for switching VT Code between Claude model versions. This covers both end-user config changes and developer-facing
API contract changes.

---

> [!NOTE]
> This guide is specific to VT Code. For the underlying Anthropic API changes, see the official
> [Migrating to Claude Opus 5](https://docs.anthropic.com/en/build-with-claude/migrating-to-claude-opus-5) guide. VT
> Code's `AnthropicProvider` handles most wire-level translations automatically; the items below are what you need to
> change in VT Code config or code.

## Quick navigation

| Current model                       | Target model                      | Section                                                                                                |
| ----------------------------------- | --------------------------------- | ------------------------------------------------------------------------------------------------------ |
| Claude Mythos Preview               | Claude Fable 5 or Claude Mythos 5 | [Mythos Preview → Fable/Mythos 5](#mythos-preview--fable-5--mythos-5)                                  |
| Claude Opus 5                       | Claude Fable 5 or Claude Mythos 5 | [Opus 5 → Fable/Mythos 5](#opus-5--fable-5--mythos-5)                                                  |
| Claude Opus 5                       | Claude Opus 5.5                   | [Opus 5 → Opus 5.5](/docs/build-with-claude/migrating-to-claude-opus-5-5.md#opus-5--opus-55)           |
| Claude Sonnet 5                     | Claude Sonnet 5.5                 | [Sonnet 5 → Sonnet 5.5](/docs/build-with-claude/migrating-to-claude-sonnet-5-5.md#sonnet-5--sonnet-55) |
| Claude Opus 4.8                     | Claude Fable 5 or Claude Mythos 5 | [Opus 4.8 → Fable/Mythos 5](#opus-48--fable-5--mythos-5)                                               |
| Claude Opus 4.8                     | Claude Opus 5                     | [Opus 4.8 → Opus 5](#opus-48--opus-5)                                                                  |
| Claude Opus 4.7                     | Claude Opus 5                     | [Opus 4.7 → Opus 5](#opus-47--opus-5)                                                                  |
| Claude Opus 4.6 or earlier Opus     | Claude Opus 5                     | [Opus 4.6 → Opus 5](#opus-46--opus-5)                                                                  |
| Claude Opus 4.5 or earlier Opus     | Claude Opus 5                     | [Opus 4.5+ → Opus 5 (cumulative)](#opus-45-or-earlier--opus-5-cumulative)                              |
| Claude Sonnet 5                     | Claude Opus 5                     | [Sonnet 5 → Opus 5](#sonnet-5--opus-5)                                                                 |
| Claude Sonnet 4.6                   | Claude Sonnet 5                   | [Sonnet 4.6 → Sonnet 5](#sonnet-46--sonnet-5)                                                          |
| Claude Sonnet 4.5 or earlier Sonnet | Claude Sonnet 5                   | [Sonnet 4.5+ → Sonnet 5](#sonnet-45-or-earlier--sonnet-5)                                              |
| Claude Haiku 4.5                    | Claude Sonnet 5                   | [Haiku 4.5 → Sonnet 5](#haiku-45--sonnet-5)                                                            |
| Claude Haiku 3.5 or earlier Haiku   | Claude Haiku 4.5                  | [Haiku 3.5 → Haiku 4.5](#haiku-35-or-earlier--haiku-45)                                                |

## Model comparison

Quick reference for Claude models in VT Code.

| Feature                      | Claude Fable 5       | Claude Mythos 5      | Claude Opus 5.5                        | Claude Opus 5             | Claude Sonnet 5.5                                                   | Claude Sonnet 5           | Claude Haiku 4.5 |
| ---------------------------- | -------------------- | -------------------- | -------------------------------------- | ------------------------- | ------------------------------------------------------------------- | ------------------------- | ---------------- |
| `ModelId` variant            | `ClaudeFable5`       | `ClaudeMythos5`      | `ClaudeOpus55`                         | `ClaudeOpus5`             | `ClaudeSonnet55`                                                    | `ClaudeSonnet5`           | `ClaudeSonnet5`  |
| Default model                | No                   | No                   | No                                     | No                        | No                                                                  | **Yes** (`[default]`)     | No               |
| Context window               | 1M                   | 1M                   | 1M                                     | 1M                        | 1M                                                                  | 1M                        | 200k             |
| Max output                   | 128k                 | 128k                 | 128k                                   | 128k                      | 128k                                                                | 128k                      | 64k              |
| Thinking mode                | Adaptive (always on) | Adaptive (always on) | Adaptive (always on)                   | Adaptive (always on)      | Adaptive (always on)                                                | Adaptive (always on)      | Manual extended  |
| Effort parameter             | —                    | —                    | low/medium/high/xhigh/max              | low/medium/high/xhigh/max | low/medium/high/xhigh/max                                           | low/medium/high/xhigh/max | —                |
| Manual extended thinking     | Not supported        | Not supported        | Not supported                          | Not supported             | Not supported                                                       | Not supported             | Supported        |
| Thinking can be disabled     | **No** (400)         | **No** (400)         | **No** (400)                           | Yes, effort ≤ high        | `between_tools` only                                                | Yes                       | Yes              |
| Prefill                      | Not supported        | Not supported        | Not supported                          | Not supported             | Not supported                                                       | Not supported             | Supported        |
| Sampling params              | Default only         | Default only         | Default only                           | Default only              | Default only                                                        | Default only              | temp OR top_p    |
| Data retention               | 30-day required      | 30-day required      | Standard                               | Standard                  | Standard                                                            | Standard                  | Standard         |
| Safety classifiers (refusal) | Yes                  | No                   | Yes (cyber, bio, reasoning_extraction) | Yes (cyber)               | Yes (cyber, bio, frontier_llm, reasoning_extraction, general_harms) | Yes (cyber)               | No               |
| Priority Tier                | Yes                  | No                   | —                                      | No                        | —                                                                   | No                        | Yes              |
| Effort default in VT Code    | `high`               | `high`               | `medium`                               | `high`                    | `high`                                                              | `high`                    | N/A              |
| `thinking_display` default   | `omitted`            | `omitted`            | `omitted`                              | `omitted`                 | `omitted`                                                           | `omitted`                 | `summarized`     |

---

## Where to make changes in VT Code

### End-user: `vtcode.toml`

The two places to update:

```toml
# 1. Model selection
agent.default_model = "claude-sonnet-5"   # before
agent.default_model = "claude-opus-5"     # after

# 2. Anthropic provider settings (if present)
[provider.anthropic]
effort = "xhigh"                          # may need adjustment
extended_thinking_enabled = true           # ignored on adaptive-only models
interleaved_thinking_budget_tokens = 31999  # ignored on adaptive-only models
thinking_display = "summarized"            # default changed on newer models
task_budget_tokens = 128000                # Opus 4.7+ only
```

### Developer: `ModelId` enum

If you are developing against `vtcode-config`, update the `ModelId` enum in
`crates/codegen/vtcode-config/src/models/model_id.rs` and its match arms (`as_str.rs`, `display.rs`, `description.rs`,
`parse.rs`, `provider.rs`, `capabilities.rs`, `collection.rs`). Use the
[adding-llm-providers](/docs/development/ADDING_MODELS.md) workflow.

### Developer: Request/response handling

If your code constructs `LLMRequest` directly or matches on `FinishReason`, review the changes below. The
`AnthropicProvider` handles most wire-level translations automatically.

---

## Migrating to Claude Fable 5 / Claude Mythos 5

Claude Fable 5 is generally available. Claude Mythos 5 is the same model without safety classifiers, available through
[Project Glasswing](https://anthropic.com/glasswing).

### Baseline behavior in VT Code

Both models map to adaptive-only thinking in VT Code's `AnthropicProvider`:

- `provider.anthropic.extended_thinking_enabled` and `interleaved_thinking_budget_tokens` are **ignored** (the request
  builder omits `thinking` entirely; adaptive is always on).
- `provider.anthropic.thinking_display` defaults to `"omitted"` on these models. Set it to `"summarized"` if your UI
  streams reasoning content.
- Prefill is not supported. VT Code's request builder omits any `prefill` field when the target model is adaptive-only.
- Sampling parameters (`temperature`, `top_p`, `top_k`) are forced to `None` by the request builder when thinking is
  active.
- Data retention: both models require 30-day retention. Requests from ZDR-configured organizations return
  `400 invalid_request_error`.

### Mythos Preview → Fable 5 / Mythos 5

#### Config changes

```toml
# Before
agent.default_model = "claude-mythos-preview"

[provider.anthropic]
effort = "high"
```

```toml
# After
agent.default_model = "claude-fable-5"   # or "claude-mythos-5"

[provider.anthropic]
effort = "high"
# Remove: extended_thinking_enabled, interleaved_thinking_budget_tokens
# Remove: thinking: {type: "enabled", budget_tokens: N} from any code constructing LLMRequest directly
```

#### What changed

**Breaking:**

1. **Extended thinking config is ignored.** `thinking: {type: "enabled", budget_tokens: N}` returns 400. VT Code's
   request builder drops `budget_tokens` for adaptive-only models. Remove `interleaved_thinking_budget_tokens` from your
   config to avoid confusion.

2. **`thinking: {type: "disabled"}` returns 400.** VT Code no longer sends a `thinking` field for these models. If you
   had logic to explicitly disable thinking, remove it.

3. **Prefill is not supported.** VT Code's request builder omits `prefill` for adaptive-only models. If you were using
   the prefill feature to steer output format, switch to `output_config.format` or system prompt instructions.

**Recommended:**

1. **Thinking display defaults to `"omitted"`.** If your product shows reasoning traces, set
   `thinking_display = "summarized"` in `[provider.anthropic]`. The raw chain of thought is never returned; summarized
   text is available when `display` is set.

2. **Re-baseline cost.** Pricing is $10/$50 per million input/output tokens. Token counts are roughly unchanged from
   Mythos Preview (same tokenizer).

#### Migration checklist

- [ ] `agent.default_model` updated to `claude-fable-5` or `claude-mythos-5`
- [ ] `provider.anthropic.extended_thinking_enabled` set to `false` or removed (ignored on these models)
- [ ] `provider.anthropic.interleaved_thinking_budget_tokens` set to `0` or removed
- [ ] `provider.anthropic.thinking_display` set to `"summarized"` if UI shows reasoning
- [ ] Prefill usage removed (structured outputs or system instructions instead)
- [ ] `stop_reason: "refusal"` handling added if using Claude Fable 5 (safety classifiers)
- [ ] Data retention verified (30-day required; ZDR not supported)
- [ ] Cost re-baselined at new pricing

---

### Opus 5 → Fable 5 / Mythos 5

#### Config changes

```toml
# Before
agent.default_model = "claude-opus-5"

[provider.anthropic]
effort = "xhigh"
thinking_display = "summarized"
```

```toml
# After
agent.default_model = "claude-fable-5"   # or "claude-mythos-5"

[provider.anthropic]
effort = "high"                          # Fable/Mythos 5 default
thinking_display = "summarized"
# Remove any logic that sets thinking: {type: "disabled"}
```

#### What changed

**Breaking:**

1. **Thinking can no longer be disabled.** On Opus 5, VT Code sends `thinking: {type: "disabled"}` when effort is `high`
   or below and the user opts out. On Fable/Mythos 5, that config returns 400. Remove the disable-thinking path; use
   lower effort (`medium`, `low`) to control token spend.

**Recommended:**

1. **Pricing:** $10/$50 vs Opus 5's $5/$25. Token counts are unchanged.

2. **Priority Tier:** Fable 5 supports it; Mythos 5 does not. Opus 5 does not support it, so no existing traffic is
   affected.

#### Migration checklist

- [ ] Model updated
- [ ] Thinking-disable logic removed
- [ ] Effort lowered from `xhigh` if cost is a concern
- [ ] Data retention confirmed

---

### Opus 4.8 → Fable 5 / Mythos 5

> [!NOTE]
> If you are on Opus 4.7 or earlier, apply the [Opus 4.7 → Opus 5](#opus-47--opus-5) changes first, then the
> remaining delta below.

#### Config changes

```toml
# Before
agent.default_model = "claude-opus-5"

[provider.anthropic]
effort = "xhigh"
thinking_display = "summarized"
```

```toml
# After
agent.default_model = "claude-fable-5"   # or "claude-mythos-5"

[provider.anthropic]
effort = "high"
thinking_display = "summarized"
# Remove: any thinking: {type: "disabled"} logic
# Remove: interleaved_thinking_budget_tokens (ignored)
```

#### What changed

**Breaking:**

1. **Adaptive thinking is always on.** On Opus 4.8, VT Code sends `thinking: {type: "adaptive"}` only when
   `extended_thinking_enabled = true`. On Fable/Mythos 5, the request builder omits `thinking` entirely and the API runs
   adaptive thinking by default. Requests without a `thinking` field that previously ran without thinking now run with
   adaptive thinking. Revisit `max_tokens` for workloads that relied on no-thinking behavior.

**Recommended:**

1. **Effort calibration.** Opus 4.8 recommended `xhigh` for coding work. Fable/Mythos 5 default to `high`; test `medium`
   or `low` for cost-sensitive workloads. Lower effort still performs well on these models.

2. **Prompt caching minimum dropped to 512 tokens.** Prompts that were too short to cache on Opus 4.8 now create cache
   entries automatically.

3. **Safety classifiers (Fable 5 only).** Handle `FinishReason::Refusal` and inspect `stop_details.category`. VT Code's
   response parser already maps `stop_reason: "refusal"` to `FinishReason::Refusal` and exposes `stop_details`.

#### Migration checklist

- [ ] Model updated
- [ ] `thinking: {type: "disabled"}` removed
- [ ] `max_tokens` revisited for workloads that ran without thinking
- [ ] Effort recalibrated (start at `high`)
- [ ] `thinking_display = "summarized"` set if UI shows reasoning
- [ ] `FinishReason::Refusal` handling verified
- [ ] Data retention confirmed

---

## Migrating to Claude Opus 5

Claude Opus 5 is a drop-in upgrade for Claude Opus 4.8 at the same $5/$25 pricing. In VT Code, the two breaking changes
are: thinking is on by default, and disabling thinking is capped at effort `high` or below.

### Opus 4.8 → Opus 5

#### Config changes

```toml
# Before
agent.default_model = "claude-opus-5"

[provider.anthropic]
effort = "xhigh"
# Optional: thinking_display = "omitted" (default on Opus 4.8+)
```

```toml
# After
agent.default_model = "claude-opus-5"

[provider.anthropic]
effort = "xhigh"
# Optional: thinking_display = "omitted" (default on Opus 4.7+)
```

No other config changes are required for this hop. VT Code's request builder automatically omits
`thinking: {type: "disabled"}` when effort is above `high` and sends adaptive thinking instead.

#### What changed

**Breaking:**

1. **Thinking on by default.** On Opus 4.8, VT Code sent no `thinking` field when `extended_thinking_enabled = false`,
   and the API ran without thinking. On Opus 5, the same config runs with adaptive thinking. If you had workloads that
   relied on no-thinking behavior, either:
   - Revisit `max_tokens` (it remains a hard limit on thinking + response text), or
   - Set `effort = "high"` (or `medium`/`low`) and VT Code will send `thinking: {type: "disabled"}` for you.

2. **Disabling thinking is capped at `high` effort.** VT Code validates this per-request. A config with
   `effort = "xhigh"` or `effort = "max"` combined with thinking-disabled will be rejected with 400. The request builder
   will log a clear error; lower effort or remove the disable-thinking path.

**Recommended:**

1. **Test `max` effort.** VT Code supports `low`, `medium`, `high`, `xhigh`, `max`. Test `max` for capability-critical
   work; raise `max_tokens` to at least 64k when using `xhigh` or `max`.

2. **Mid-conversation system messages.** Opus 5 accepts `role: "system"` in the `messages` array. VT Code's message
   builder can now inject mid-conversation system messages without rebuilding the full history, preserving prompt cache
   hits. When VT Code collapses or bounds tool output, it adds the fixed disclosure after the tool-result user message.
   Anthropic wire routes use `clear_at: "next_user_message"` and the `mid-conversation-system-clear-at-2026-08-21` beta
   only when the selected provider/model capability supports them; unsupported Anthropic models and non-Anthropic wires
   receive the same text through their ordinary system/history mapping. The typed marker remains in canonical history
   for replay and provider switching.

3. **Task budgets (beta).** If you use `task_budget_tokens` in `[provider.anthropic]`, it works unchanged on Opus 5. The
   minimum is 20,000 tokens.

4. **Fast mode (beta).** Set `VTCODE_ANTHROPIC_BETA=fast-mode-2026-02-01` or add it via the provider's beta header
   config; VT Code passes it through unchanged.

#### Migration checklist

- [ ] `agent.default_model = "claude-opus-5"`
- [ ] Workloads that ran without `thinking` field revisited (they now run with adaptive thinking)
- [ ] `max_tokens` raised for thinking-enabled workloads
- [ ] `effort` recalibrated; `thinking: {type: "disabled"}` removed or effort lowered to `high`/`medium`/`low`
- [ ] `max` effort tested with `max_tokens >= 64k`
- [ ] Mid-conversation system messages considered for prompt cache preservation
- [ ] `task_budget_tokens` reviewed if used
- [ ] `stop_reason: "refusal"` handling verified (VT Code maps to `FinishReason::Refusal`)
- [ ] Cost and latency re-baselined

---

### Opus 4.7 → Opus 5

> [!NOTE]
> If you are on Opus 4.6 or earlier, use the [Opus 4.6 → Opus 5](#opus-46--opus-5) section instead. It includes
> breaking changes (sampling parameters, prefill, tokenizer) that this hop does not cover.

The same config changes as the Opus 4.8 → Opus 5 hop apply. The only additional item: if you used
`betas=["interleaved-thinking-2025-05-14"]` manually, remove it — VT Code adds it automatically when needed, and
adaptive thinking enables interleaved thinking implicitly.

---

### Opus 4.6 → Opus 5

#### Config changes

```toml
# Before
agent.default_model = "claude-opus-4-6"

[provider.anthropic]
effort = "xhigh"
extended_thinking_enabled = true
interleaved_thinking_budget_tokens = 10000
interleaved_thinking_type_enabled = "enabled"
```

```toml
# After
agent.default_model = "claude-opus-5"

[provider.anthropic]
effort = "high"                          # recalibrated
thinking_display = "summarized"          # if you want visible reasoning
# Remove: extended_thinking_enabled (ignored on adaptive-only models)
# Remove: interleaved_thinking_budget_tokens (ignored)
# Remove: interleaved_thinking_type_enabled (ignored)
```

#### What changed

**Breaking:**

1. **Extended thinking removed.** `thinking: {type: "enabled", budget_tokens: N}` returns 400. VT Code's request builder
   now emits `ThinkingConfig::Adaptive` for Opus 5. Remove `extended_thinking_enabled` and
   `interleaved_thinking_budget_tokens` from config to avoid confusion.

2. **Thinking on by default.** Same as the 4.8 → 5 hop. Requests without a `thinking` field now run with adaptive
   thinking.

3. **Disabling thinking capped at `high` effort.** Same as above.

4. **Sampling parameters removed.** VT Code's request builder already forces `temperature = None` when thinking is
   active or the model is Opus 4.8+. On Opus 5, non-default `temperature`/`top_p`/`top_k` are rejected server-side
   regardless. Ensure no code path sets these explicitly.

5. **Thinking content omitted by default.** VT Code's default `thinking_display` for Opus 5 is `"omitted"` (matching the
   API). Set `thinking_display = "summarized"` in config if your UI displays reasoning.

6. **Tokenizer changed.** The new tokenizer uses roughly 1x–1.35x tokens compared to pre-Opus 4.7 models. VT Code's
   token counting (`count_tokens_enabled = true`) will return different values. Re-baseline `max_tokens` and cost
   expectations.

7. **Prefill removed.** VT Code's request builder omits `prefill` for Opus 5. Use `output_config.format` or system
   instructions instead.

**Recommended:**

1. **Advisor compatibility.** If you use the advisor feature, verify `validate_advisor_pair()` accepts your
   executor/advisor combination. Opus 5 has `advisor_tier = 6`; Fable 5 has `advisor_tier = 8`. Self-advising is only
   allowed for Fable 5 and Mythos 5.

2. **High-resolution images.** Maximum image resolution is 2,576px on the long edge (up from 1,568). Full-resolution
   images use up to ~3x more image tokens. VT Code passes images through unchanged; downsample before sending if
   fidelity is not needed.

#### Migration checklist

- [ ] Model updated
- [ ] `extended_thinking_enabled`, `interleaved_thinking_budget_tokens`, `interleaved_thinking_type_enabled` removed or
      set to defaults
- [ ] `temperature`, `top_p`, `top_k` removed from any direct `LLMRequest` construction
- [ ] `thinking_display` set to `"summarized"` if UI shows reasoning
- [ ] `max_tokens` raised for tokenizer change (~30% more tokens)
- [ ] Advisor pair validated if using advisor feature
- [ ] Image budgets revisited if using vision
- [ ] `FinishReason::Refusal` and `FinishReason::Length` (for `model_context_window_exceeded`) handled

---

### Opus 4.5 or earlier → Opus 5 (cumulative)

Apply all changes from the [Opus 4.6 → Opus 5](#opus-46--opus-5) section, plus:

**Additional breaking:**

1. **Prefill removal (from 4.6).** Already covered above.

2. **Tool parameter JSON escaping (from 4.6).** Claude 4.5+ preserves trailing newlines in tool call string parameters.
   VT Code's tool parser uses standard JSON parsing, so this is handled automatically. If you have custom string-based
   tool argument parsing, verify it handles trailing newlines.

**Additional recommended:**

1. **Beta header cleanup (from 4.7).** VT Code manages beta headers internally. If you passed
   `betas=["effort-2025-11-24"]`, `betas=["fine-grained-tool-streaming-2025-05-14"]`, or
   `betas=["interleaved-thinking-2025-05-14"]` manually, remove them — they are GA or auto-enabled.

2. **Tool versions (from 4.7).** VT Code's tool definitions are internal. If you reference tool types in prompts or
   code, update:
   - `text_editor_20250124` → `text_editor_20250728` (`str_replace_editor` → `str_replace_based_edit_tool`)
   - `code_execution_20250825` → `code_execution_20260521`

3. **Legacy beta headers (from 4.7).** Remove `token-efficient-tools-2025-02-19` and `output-128k-2025-02-19` if present
   in config.

#### Cumulative migration checklist

- [ ] All [Opus 4.6 → Opus 5](#opus-46--opus-5) items
- [ ] Prefill removed (if not already done upstream)
- [ ] Tool parameter JSON parsing uses standard parser
- [ ] Beta headers cleaned up (`effort-2025-11-24`, `fine-grained-tool-streaming-2025-05-14`,
      `interleaved-thinking-2025-05-14`)
- [ ] `output_format` migrated to `output_config.format` if using structured outputs
- [ ] Tool types updated (`text_editor_20250728`, `code_execution_20260521`)
- [ ] Legacy beta headers removed (`token-efficient-tools-2025-02-19`, `output-128k-2025-02-19`)
- [ ] Prompts updated for Claude 4+ communication style

---

### Sonnet 5 → Opus 5

#### Config changes

```toml
# Before
agent.default_model = "claude-sonnet-5"

[provider.anthropic]
effort = "high"
```

```toml
# After
agent.default_model = "claude-opus-5"

[provider.anthropic]
effort = "high"
# No other changes required for this hop
```

#### What changed

**Breaking:**

1. **Disabling thinking is capped at `high` effort.** On Sonnet 5, VT Code accepts `thinking: {type: "disabled"}` at any
   effort. On Opus 5, it is only allowed at `high` or below. Audit any code or config that disables thinking with
   `xhigh` or `max` effort.

**Recommended:**

1. **Pricing:** $5/$25 vs Sonnet 5's $2/$10 (intro) → $3/$15.

2. **Mid-conversation system messages.** Opus 5 supports them; Sonnet 5 does not. If you rebuild message history to
   update instructions, consider switching to mid-conversation system messages to preserve prompt cache hits.

3. **Web fetch.** The web fetch tool is available on Sonnet 5 but not on Opus 5. If your workload uses web fetch, plan
   an alternative.

#### Migration checklist

- [ ] Model updated
- [ ] Thinking-disable + high-effort combinations audited
- [ ] Web fetch alternatives planned if applicable
- [ ] Cost re-baselined

---

## Migrating to Claude Sonnet 5

Claude Sonnet 5 is VT Code's default model (`[default]`). It uses a new tokenizer (~30% more tokens), adaptive thinking
on by default, and requires the latest tool versions.

### Sonnet 4.6 → Sonnet 5

#### Config changes

```toml
# Before
agent.default_model = "claude-sonnet-5"

[provider.anthropic]
effort = "high"
extended_thinking_enabled = true
interleaved_thinking_budget_tokens = 10000
```

```toml
# After
agent.default_model = "claude-sonnet-5"

[provider.anthropic]
effort = "high"
thinking_display = "summarized"    # if UI shows reasoning; default is "omitted"
# Remove: extended_thinking_enabled (ignored on adaptive-only models)
# Remove: interleaved_thinking_budget_tokens (ignored)
```

#### What changed

**Breaking:**

1. **Adaptive thinking on by default.** Sonnet 4.6 ran without thinking when no `thinking` field was sent. Sonnet 5 runs
   with adaptive thinking. Revisit `max_tokens` for workloads that previously ran without thinking.

2. **Manual extended thinking removed.** `thinking: {type: "enabled", budget_tokens: N}` returns 400. VT Code's request
   builder drops budgeted thinking for Sonnet 5.

3. **New tokenizer.** Token counts increase by roughly 30%. VT Code's `count_tokens_enabled = true` will return
   different values. Re-baseline `max_tokens` and cost.

**Recommended:**

1. **Effort default is `high`.** Sonnet 4.6 had no effort parameter. Sonnet 5 defaults to `high` in VT Code, which may
   increase latency. Test `medium` or `low` for cost-sensitive workloads.

2. **Cybersecurity safeguards.** Sonnet 5 is the first Sonnet-tier model with real-time safety classifiers. VT Code maps
   `stop_reason: "refusal"` to `FinishReason::Refusal`. Add handling if your workload touches cybersecurity topics.

#### Migration checklist

- [ ] Model updated
- [ ] `extended_thinking_enabled` and `interleaved_thinking_budget_tokens` removed
- [ ] `thinking_display` set to `"summarized"` if UI shows reasoning
- [ ] `max_tokens` revisited for tokenizer change and thinking-on-by-default
- [ ] Effort tested (`medium`/`low` for cost savings)
- [ ] `FinishReason::Refusal` handling added
- [ ] Cost re-baselined

---

### Sonnet 4.5 or earlier → Sonnet 5

Apply the [Sonnet 4.6 → Sonnet 5](#sonnet-46--sonnet-5) changes, plus:

**Breaking:**

1. **Prefill removed (from 4.6).** VT Code's request builder omits `prefill` for Sonnet 5. Use `output_config.format` or
   system instructions.

2. **Tool JSON escaping (from 4.6).** VT Code's tool parser uses standard JSON; no action needed unless you have custom
   parsing.

3. **Effort parameter introduced.** Sonnet 4.5 had no effort. Sonnet 5 defaults to `high` in VT Code. Explicitly set
   `effort` if the default latency is too high.

#### Migration checklist

- [ ] All Sonnet 4.6 → Sonnet 5 items
- [ ] Prefill removed
- [ ] Tool JSON parsing verified
- [ ] Effort explicitly set if default `high` is too costly

---

### Haiku 4.5 → Sonnet 5

Haiku 4.5 uses manual extended thinking; Sonnet 5 uses adaptive thinking on by default. This is the largest API-level
gap between adjacent tiers in VT Code.

#### Config changes

```toml
# Before
agent.default_model = "claude-sonnet-5"

[provider.anthropic]
effort = "high"                          # not available on Haiku 4.5; ignored
extended_thinking_enabled = true
interleaved_thinking_budget_tokens = 10000
```

```toml
# After
agent.default_model = "claude-sonnet-5"

[provider.anthropic]
effort = "high"
thinking_display = "summarized"
# Remove: extended_thinking_enabled, interleaved_thinking_budget_tokens
```

#### What changed

1. **Thinking mode flipped.** Haiku 4.5 supports `thinking: {type: "enabled", budget_tokens: N}` and rejects
   `thinking: {type: "adaptive"}`. Sonnet 5 is the opposite: adaptive is on by default, manual extended thinking
   returns 400. Remove all budget-token thinking config.

2. **Effort parameter now available.** Haiku 4.5 did not support effort. Sonnet 5 defaults to `high` in VT Code. Test
   `medium`/`low` for cost control.

3. **Context window: 200k → 1M.** Sonnet 5 serves 1M tokens by default. Re-run token counting.

4. **Sampling parameters.** Haiku 4.5 accepts `temperature` OR `top_p` (one at a time). Sonnet 5 rejects non-default
   sampling parameters entirely. VT Code's request builder already strips these for adaptive-thinking models.

5. **Prefill removed.** Haiku 4.5 supports assistant prefill; Sonnet 5 does not.

6. **Cybersecurity safeguards.** Sonnet 5 adds safety classifiers. Handle `FinishReason::Refusal`.

#### Migration checklist

- [ ] Model updated
- [ ] All manual extended thinking config removed
- [ ] Effort set and tested
- [ ] `max_tokens` revisited for tokenizer + context window changes
- [ ] Sampling parameters removed
- [ ] Prefill removed
- [ ] `FinishReason::Refusal` handling added
- [ ] Cost re-baselined

---

## Migrating to Claude Haiku 4.5

Haiku 4.5 is the fastest model with near-frontier performance. It is the only current VT Code model that still supports
manual extended thinking.

### Haiku 3.5 or earlier → Haiku 4.5

#### Config changes

```toml
# Before
agent.default_model = "claude-3-5-haiku-20241022"

[provider.anthropic]
effort = "high"
```

```toml
# After
agent.default_model = "claude-sonnet-5"

[provider.anthropic]
extended_thinking_enabled = true
interleaved_thinking_budget_tokens = 10000
# Note: effort is not available on Haiku 4.5
```

#### What changed

**Breaking:**

1. **Sampling parameters.** Use only `temperature` OR `top_p`, not both. VT Code's request builder does not enforce this
   automatically for Haiku; verify your config.

2. **Tool versions.** Update to `text_editor_20250728` and `code_execution_20250825`. Remove `undo_edit` references.

3. **Refusal handling.** Haiku 4.5 can return `stop_reason: "refusal"`. VT Code maps it to `FinishReason::Refusal`.

#### Migration checklist

- [ ] Model updated
- [ ] Tool versions updated
- [ ] Sampling parameters set to `temperature` OR `top_p` (not both)
- [ ] `FinishReason::Refusal` handling added
- [ ] Rate limits reviewed (Haiku 4.5 has separate limits)
- [ ] Extended thinking enabled for coding/reasoning tasks if desired

---

## Consolidated VT Code migration checklist

Use this when migrating across any model generation.

- [ ] **Model ID updated:** `agent.default_model` set to target model string; or `ModelId` enum updated if developing
      against `vtcode-config`
- [ ] **Effort reviewed:** `provider.anthropic.effort` matches target model's supported levels; `xhigh`/`max` only used
      with `max_tokens >= 64k`
- [ ] **Thinking config cleaned:** `extended_thinking_enabled` and `interleaved_thinking_budget_tokens` removed for
      adaptive-only models (Fable 5, Mythos 5, Opus 5, Opus 5.5, Sonnet 5); kept only for Haiku 4.5
- [ ] **Thinking display set:** `thinking_display = "summarized"` if UI shows reasoning; defaults to `"omitted"` on Opus
      4.7+ and Sonnet 5
- [ ] **Thinking disable removed or capped:** Removed for Fable/Mythos 5 and Opus 5.5; `between_tools` for Sonnet 5.5;
      effort ≤ `high` for Opus 5 / Sonnet 5
- [ ] **Forced tool choice removed:** `tool_choice` reduced to `auto` for Sonnet 5.5, Opus 5.5 and Fable 5.1
- [ ] **`max_tokens` revisited:** Raised for workloads that now run with adaptive thinking; accounts for new tokenizer
      (~30% more on Sonnet 5, Opus 4.7+)
- [ ] **Prefill removed:** Replaced with structured outputs (`output_config.format`) or system prompt instructions
- [ ] **Sampling parameters removed:** `temperature`, `top_p`, `top_k` removed for Opus 4.7+, Sonnet 5, Fable 5, Mythos
      5
- [ ] **Advisor pair validated:** `validate_advisor_pair()` passes for executor + advisor combination (if using advisor
      feature)
- [ ] **Stop reasons handled:** `FinishReason::Refusal` and `FinishReason::Length` (`model_context_window_exceeded`)
      handled in response processing
- [ ] **Token counting re-baselined:** `count_tokens_enabled = true` used to verify new token counts; client-side
      estimates updated
- [ ] **Image budgets revisited:** Downsampled if using vision and high-resolution fidelity is not needed (up to 3x more
      tokens)
- [ ] **Tool JSON parsing verified:** Standard JSON parser used (trailing newlines preserved on Claude 4.5+)
- [ ] **Prompt caching reviewed:** Prompts ≥ 512 tokens now create cache entries on most models
- [ ] **Beta headers:** No manual beta headers needed; VT Code manages them internally
- [ ] **Task budgets evaluated:** `task_budget_tokens` set if using agentic workloads (min 20,000 tokens)
- [ ] **Data retention confirmed:** 30-day retention verified for Fable 5 and Mythos 5; ZDR eligibility checked
- [ ] **Priority Tier assessed:** Capacity planned separately if organization has commitment (not supported on Opus 5,
      Sonnet 5, Mythos 5)
- [ ] **Web fetch alternatives planned:** If using web fetch tool, alternative planned for Opus 5 (not available)
- [ ] **Prompts retuned:** Explicit conciseness/length instructions added; verification/self-check instructions removed
      (Opus 5)
- [ ] **Cost and latency re-baselined:** Measured on target model with chosen effort level

---

## Developer reference

### Key VT Code types

| Type                   | Location                                         | Role                                                               |
| ---------------------- | ------------------------------------------------ | ------------------------------------------------------------------ |
| `ModelId`              | `vtcode-config/src/models/model_id.rs`           | Canonical model enum; add new models here                          |
| `AnthropicConfig`      | `vtcode-config/src/core/provider.rs`             | All Anthropic-specific settings                                    |
| `ReasoningEffortLevel` | `vtcode-commons/src/reasoning.rs`                | Effort enum (`None`/`Minimal`/`Low`/`Medium`/`High`/`XHigh`/`Max`) |
| `LLMRequest`           | `vtcode-llm/src/provider/request.rs`             | Universal request struct                                           |
| `LLMResponse`          | `vtcode-commons/src/llm.rs`                      | Universal response struct                                          |
| `FinishReason`         | `vtcode-commons/src/llm.rs`                      | Stop reason enum                                                   |
| `AnthropicProvider`    | `vtcode-llm/src/providers/anthropic/provider.rs` | Request building, HTTP, response parsing                           |
| `ModelResolver`        | `vtcode-llm/src/model_resolver.rs`               | Resolves model string → provider + catalog                         |
| `ThinkingDisplayMode`  | `vtcode-config/src/core/provider.rs`             | `Summarized` / `Omitted` / `Unknown`                               |

### How VT Code translates config to API

1. `agent.default_model` → `ModelId` enum (or raw string for OpenRouter/custom)
2. `[provider.anthropic]` → `AnthropicConfig` struct
3. `LLMRequest::from_config()` merges user config with model defaults
4. `AnthropicProvider::convert_to_anthropic_format()` builds the wire payload:
   - Calls `build_thinking_config()` → adaptive, manual, or omitted per model
   - Resolves `effort` from `AnthropicConfig.effort` → model default
   - Builds `output_config { effort, task_budget, format }` when present
   - Forces `temperature = None` when thinking is active
   - Adds beta headers automatically
5. `AnthropicProvider::parse_response()` maps `stop_reason` → `FinishReason` and extracts `stop_details`

You rarely need to touch steps 3–5; the provider handles model differences internally.

---

## Get help

- [Migrating to Claude Opus 5.5](/docs/build-with-claude/migrating-to-claude-opus-5-5.md) — Opus 5 → Opus 5.5 hop
- [Migrating to Claude Sonnet 5.5](/docs/build-with-claude/migrating-to-claude-sonnet-5-5.md) — Sonnet 5 → Sonnet 5.5
  hop
- [VT Code config reference](/docs/config/CONFIG_FIELD_REFERENCE.md) — all `[provider.anthropic]` fields
- [Extended thinking in VT Code](/docs/development/EXTENDED_THINKING.md) — thinking matrix and budget selection
- [Adding models](/docs/development/ADDING_MODELS.md) — developer workflow for new model IDs
- [Anthropic migration guide](https://docs.anthropic.com/en/build-with-claude/migrating-to-claude-opus-5) — underlying
  API changes
