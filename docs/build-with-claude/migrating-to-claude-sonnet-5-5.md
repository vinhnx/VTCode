# Migrating to Claude Sonnet 5.5 in VT Code

Guide for switching VT Code to Claude Sonnet 5.5 (`claude-sonnet-5-5`, `ModelId::ClaudeSonnet55`). This covers end-user
config changes and developer-facing API contract changes.

---

> [!NOTE]
> This guide is specific to VT Code. For the underlying Anthropic API changes, see the official [Migrating to Claude
> Sonnet 5.5](https://platform.claude.com/docs/en/models/sonnet-5-5/migration-guide) guide. VT Code's `AnthropicProvider`
> handles most wire-level translations automatically; the items below are what you need to change in VT Code config or
> code.

## Quick navigation

| Current model     | Target model      | Section                                          |
| ----------------- | ----------------- | ------------------------------------------------ |
| Claude Sonnet 5   | Claude Sonnet 5.5 | [Sonnet 5 → Sonnet 5.5](#sonnet-5--sonnet-55)    |
| Claude Sonnet 4.6 | Claude Sonnet 5.5 | [Sonnet 4.6 → Sonnet 5.5](#sonnet-46--sonnet-55) |
| Claude Haiku 4.5  | Claude Sonnet 5.5 | [Haiku 4.5 → Sonnet 5.5](#haiku-45--sonnet-55)   |

## Model comparison

| Feature                               | Claude Sonnet 5.5                | Claude Sonnet 5              |
| ------------------------------------- | -------------------------------- | ---------------------------- |
| `ModelId` variant                     | `ClaudeSonnet55`                 | `ClaudeSonnet5`              |
| Context window                        | 1M                               | 1M                           |
| Max output                            | 128k                             | 128k                         |
| Pricing (input / output per MTok)     | $2 / $10                         | $2 / $10                     |
| Cache read / 5m write / 1h write      | $0.20 / $2.50 / $4               | —                            |
| Thinking mode                         | Adaptive (on by default)         | Adaptive (on by default)     |
| Effort parameter                      | low/medium/high/xhigh/max        | low/medium/high/xhigh/max    |
| Effort default in VT Code             | `high`                           | `high`                       |
| Thinking can be disabled              | **Only** via `between_tools`     | Yes (`disabled`, any effort) |
| Forced tool choice (`any` / specific) | **No** (400)                     | Only when thinking is off    |
| Manual extended thinking              | Not supported                    | Not supported                |
| Prefill                               | Not supported                    | Not supported                |
| Sampling params                       | Default only                     | Default only                 |
| `thinking_display` default            | `updates` (VT Code)              | `omitted`                    |
| Advisor tier                          | 4 (above Sonnet 5, below Opus 5) | 3                            |

---

## Where to make changes in VT Code

### End-user: `vtcode.toml`

The two places to update:

```toml
# 1. Model selection
agent.default_model = "claude-sonnet-5"      # before
agent.default_model = "claude-sonnet-5-5"    # after

# 2. Anthropic provider settings (if present)
[provider.anthropic]
effort = "high"                              # unchanged default; re-sweep, levels are recalibrated
extended_thinking_enabled = false            # now sends thinking: {type: "between_tools"}, not "disabled"
thinking_display = "summarized"              # optional; unset requests "updates" on Sonnet 5.5
```

Gateway routes use the same model hop with the vendor prefix:

```toml
agent.default_model = "anthropic/claude-sonnet-5"    # before (merge-gateway)
agent.default_model = "anthropic/claude-sonnet-5-5"  # after
```

### Developer: `ModelId` enum

`ModelId::ClaudeSonnet55` (`"claude-sonnet-5-5"`, provider `Anthropic`) is the canonical identifier. The gateway variant
is `ModelId::MergeGatewayAnthropicClaudeSonnet55` (`"anthropic/claude-sonnet-5-5"`, provider `MergeGateway`).
`ModelId::default_orchestrator_for_provider(Provider::Anthropic)` still returns `ClaudeOpus55`; Sonnet 5.5 does not
change any default.

---

## Sonnet 5 → Sonnet 5.5

Sonnet 5.5 costs the same as Sonnet 5 ($2/$10) with the same 1M context and 128k output. The breaking deltas are all in
the request shape: `disabled` thinking, forced tool choice, and how between-tool text is returned.

### Config changes

```toml
# Before
agent.default_model = "claude-sonnet-5"

[provider.anthropic]
extended_thinking_enabled = false
```

```toml
# After
agent.default_model = "claude-sonnet-5-5"

[provider.anthropic]
# No change needed: VT Code sends thinking: {type: "between_tools"} here,
# because Sonnet 5.5 rejects "disabled" with a 400.
# Remove: tool_choice any/specific — returns 400 on Sonnet 5.5
```

No other config changes are required.

#### What changed

**Breaking:**

1. **`thinking: {"type": "disabled"}` returns a 400.** On Sonnet 5, VT Code sent `disabled` when effort was `high` or
   below and the user opted out. Sonnet 5.5's lowest setting is `between_tools`: up-front thinking is off, and the short
   progress notes written between tool calls still come back as `thinking` blocks. VT Code translates every "thinking
   off" path for you — `extended_thinking_enabled = false` and a `thinking_mode = disabled` request override both
   produce `between_tools`, and so does the rewrite applied to server-side fallback entries. `between_tools` is rejected
   at `xhigh`/`max` effort, so at those levels VT Code omits the field and the model runs adaptive thinking.

2. **Forced tool use is rejected.** `tool_choice: any` or a specific tool returns a 400 on Sonnet 5.5 at any effort. VT
   Code's existing forced-tool-choice guard now fires for every such request to Sonnet 5.5, downgrading it to `auto`.
   Move the "when to use this tool" instruction into the prompt, or use structured outputs.

3. **Thinking blocks are bound to the conversation.** Sonnet 5.5 signs each thinking block over the `system` prompt, the
   `tools` set, and every earlier message. On accounts created on or after 2026-08-31, replaying a block after an edit
   to earlier history returns a 400; on older accounts the block is silently dropped. VT Code already keeps history
   append-only for prefix-bound models (Opus 5.5, Fable 5.1) and now treats Sonnet 5.5 the same way, so no config change
   is needed — but a mid-conversation edit of the system prompt or tool list will not carry earlier reasoning forward.

4. **Sampling parameters are still rejected.** Same as Sonnet 5: `temperature`, `top_p`, and `top_k` must be omitted. VT
   Code drops them for the whole Claude 5.x family.

**Recommended:**

1. **Between-tool text now arrives in `thinking` blocks.** Notes longer than a sentence or two that the model writes
   between tool calls come back as progress-update `thinking` blocks, empty at the API's default `omitted` display, so
   an interface that streamed that text as progress goes quiet. VT Code requests `thinking.display: "updates"` (beta
   `thinking-display-updates-2026-08-18`) on Sonnet 5.5 when `thinking_display` is unset, so those blocks carry their
   text and stream into the reasoning view while the reasoning itself stays hidden. Set `thinking_display = "omitted"`
   to opt out, or `"summarized"` to also get summarized reasoning. A configured `"updates"` is ignored on models that
   reject it (Sonnet 5, Opus 5).

2. **Re-run your effort sweep.** Sonnet 5.5's five effort levels are recalibrated: a level does not produce the same
   amount of thinking as on Sonnet 5. Anthropic's own guidance is `high` by default, `medium` for agentic coding and
   multistep tool use, and `medium`/`low` for latency-sensitive chat.

3. **Handle the new refusal categories.** Sonnet 5.5 declines in five `stop_details` categories (`cyber`, `bio`,
   `frontier_llm`, `reasoning_extraction`, `general_harms`), broader than Sonnet 5's. VT Code maps
   `stop_reason: "refusal"` to `FinishReason::Refusal`; verify your handling surfaces the category. Server-side
   fallbacks (`fallbacks: "default"`) are supported on Sonnet 5.5 and retry `cyber` and `frontier_llm` declines on
   Claude Sonnet 5.

4. **Advisor pairs.** Sonnet 5.5 has `advisor_tier = 4`: a Sonnet 5 advisor is rejected with a 400, so use Claude Opus
   5, Opus 5.5, Fable 5, Fable 5.1, Mythos 5, Mythos 5.1, or Sonnet 5.5 itself. `validate_advisor_pair()` enforces this;
   `default_advisor_model()` returns `claude-opus-5`.

5. **No computer-use impact.** VT Code does not implement the `computer_20251124` tool, so that breaking change requires
   no VT Code action.

#### Migration checklist

- [ ] `agent.default_model = "claude-sonnet-5-5"` (or the `anthropic/`-prefixed route on gateways)
- [ ] `thinking: {"type": "disabled"}` paths replaced — VT Code sends `between_tools` for Sonnet 5.5 automatically
- [ ] `tool_choice` `any`/specific replaced with `auto`, or the "when to call" instruction moved into the prompt
- [ ] History kept append-only if the session edits its system prompt or tool list mid-conversation
- [ ] `thinking_display` reviewed if the UI renders between-tool text
- [ ] `FinishReason::Refusal` handling verified for the new categories
- [ ] Advisor pairs re-validated if using the advisor feature
- [ ] Effort sweep re-run and cost re-baselined

---

## Sonnet 4.6 → Sonnet 5.5

> [!NOTE]
> Work through [Sonnet 4.6 → Sonnet 5](/docs/build-with-claude/migrating-to-claude-opus-5.md#sonnet-46--sonnet-5) first:
> it covers thinking running on requests that omit the `thinking` field and the response-shape changes that come with it.
> Then apply [Sonnet 5 → Sonnet 5.5](#sonnet-5--sonnet-55). Two deltas stack: Sonnet 4.6 turns thinking off with
> `disabled` at any effort, and Sonnet 5.5 does not accept `disabled` at all — audit thinking-off paths twice, once per
> hop.

Also apply the Sonnet-4.6→5 deltas on top:

- **Thinking budgets return a 400.** Sonnet 4.6's deprecated `{"type": "enabled", "budget_tokens": N}` is not accepted.
  Use an effort level instead.
- **Sampling parameters return a 400** when set to a non-default value.
- **~30% more tokens** for the same text against Sonnet 4.6. Recount and revisit `max_tokens` (VT Code defaults to 64k
  on every Claude 5.x model).
- **High-resolution images.** Sonnet 5.5 uses the high-resolution image tier (up to 2576px long edge, 4,784 visual
  tokens per image); a 2000×1500 image costs roughly 2.5× the tokens it did on Sonnet 4.6.

---

## Haiku 4.5 → Sonnet 5.5

> [!NOTE]
> Work through [Haiku 4.5 → Sonnet 5](/docs/build-with-claude/migrating-to-claude-opus-5.md#haiku-45--sonnet-5) first,
> then apply [Sonnet 5 → Sonnet 5.5](#sonnet-5--sonnet-55).

Also apply the Haiku-specific deltas on top:

- **Model ID:** replace `claude-haiku-4-5-20251001` or the `claude-haiku-4-5` alias with `claude-sonnet-5-5`.
- **Cost:** the price per token is higher ($2/$10 vs $1/$5), and the same text produces more tokens. Recount and
  re-baseline cost.
- **Prompt caching:** the minimum cacheable prompt drops from 4,096 tokens to 512.
- **Thinking:** Haiku 4.5's manual `budget_tokens` mode is gone — replace it with an effort level.
- **Output limit:** Haiku 4.5's 64k output becomes Sonnet 5.5's 128k; VT Code still defaults to a 64k `max_tokens` that
  covers thinking plus text together.
- **Routing:** Sonnet 5.5 reads Haiku 4.5 thinking blocks, so a conversation that moves up keeps its reasoning.

---

## Get help

- [Migrating Claude models in VT Code](/docs/build-with-claude/migrating-to-claude-opus-5.md) — Fable/Sonnet/Haiku hops
  and the consolidated checklist
- [Migrating to Claude Opus 5.5](/docs/build-with-claude/migrating-to-claude-opus-5-5.md) — the Opus-tier successor hop
- [VT Code config reference](/docs/config/CONFIG_FIELD_REFERENCE.md) — all `[provider.anthropic]` fields
- [Extended thinking in VT Code](/docs/development/EXTENDED_THINKING.md) — thinking matrix, `between_tools`, and budget
  selection
- [Merge Gateway integration](/docs/providers/merge-gateway.md) — the `anthropic/`-prefixed route form
- [Anthropic migration guide](https://platform.claude.com/docs/en/models/sonnet-5-5/migration-guide) — underlying API
  changes
