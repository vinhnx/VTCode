# Migrating to Claude Opus 5.5 in VT Code

Guide for switching VT Code to Claude Opus 5.5 (`claude-opus-5-5`, `ModelId::ClaudeOpus55`). This covers end-user config
changes and developer-facing API contract changes.

---

> [!NOTE]
> This guide is specific to VT Code. For the underlying Anthropic API changes, see the official [Migrating to Claude
> Opus 5.5](https://platform.claude.com/docs/en/models/opus-5-5/migration-guide) guide. VT Code's `AnthropicProvider`
> handles most wire-level translations automatically; the items below are what you need to change in VT Code config or
> code.

## Quick navigation

| Current model   | Target model    | Section                                   |
| --------------- | --------------- | ----------------------------------------- |
| Claude Opus 5   | Claude Opus 5.5 | [Opus 5 → Opus 5.5](#opus-5--opus-55)     |
| Claude Opus 4.8 | Claude Opus 5.5 | [Opus 4.8 → Opus 5.5](#opus-48--opus-55)  |
| Claude Sonnet 5 | Claude Opus 5.5 | [Sonnet 5 → Opus 5.5](#sonnet-5--opus-55) |

## Model comparison

| Feature                               | Claude Opus 5.5                              | Claude Opus 5             |
| ------------------------------------- | -------------------------------------------- | ------------------------- |
| `ModelId` variant                     | `ClaudeOpus55`                               | `ClaudeOpus5`             |
| Context window                        | 1M                                           | 1M                        |
| Max output                            | 128k                                         | 128k                      |
| Pricing (input / output per MTok)     | $4 / $20                                     | $5 / $25                  |
| Cache read / 5m write / 1h write      | $0.20 / $5 / $8                              | —                         |
| Thinking mode                         | Adaptive (always on)                         | Adaptive (on by default)  |
| Effort parameter                      | low/medium/high/xhigh/max                    | low/medium/high/xhigh/max |
| Effort default in VT Code             | `medium`                                     | `high`                    |
| Thinking can be disabled              | **No** (400)                                 | Yes, effort ≤ high        |
| Forced tool choice (`any` / specific) | **No** (400)                                 | Only when thinking is off |
| Manual extended thinking              | Not supported                                | Not supported             |
| Prefill                               | Not supported                                | Not supported             |
| Sampling params                       | Default only                                 | Default only              |
| `thinking_display` default            | `updates` in VT Code (API default `omitted`) | `omitted`                 |
| Advisor tier                          | 7 (above Opus 5, below Fable 5)              | 6                         |

---

## Where to make changes in VT Code

### End-user: `vtcode.toml`

The two places to update:

```toml
# 1. Model selection
agent.default_model = "claude-opus-5"      # before
agent.default_model = "claude-opus-5-5"    # after

# 2. Anthropic provider settings (if present)
[provider.anthropic]
effort = "medium"                          # new default; was "high" on Opus 5
extended_thinking_enabled = true           # must stay true; thinking can't be disabled
thinking_display = "updates"              # VT Code default on Opus 5.5; "omitted" hides progress updates
task_budget_tokens = 128000                # unchanged; supported on Opus 5.5 (min 20000)
```

Gateway routes use the same model hop with the vendor prefix:

```toml
agent.default_model = "anthropic/claude-opus-5"    # before (merge-gateway / vercel)
agent.default_model = "anthropic/claude-opus-5-5"  # after
```

### Developer: `ModelId` enum

`ModelId::ClaudeOpus55` (`"claude-opus-5-5"`, provider `Anthropic`) is the canonical identifier. Gateway variants are
`ModelId::MergeGatewayAnthropicClaudeOpus55` and `ModelId::VercelAnthropicClaudeOpus55`.
`ModelId::default_orchestrator_for_provider(Provider::Anthropic)` now returns `ClaudeOpus55`.

---

## Opus 5 → Opus 5.5

Opus 5.5 costs less than Opus 5 ($4/$20 vs $5/$25) with the same 1M context and 128k output. The breaking delta is small
but absolute: thinking can't be disabled at any effort, and forced tool choice is rejected.

### Config changes

```toml
# Before
agent.default_model = "claude-opus-5"

[provider.anthropic]
effort = "high"
```

```toml
# After
agent.default_model = "claude-opus-5-5"

[provider.anthropic]
effort = "medium"                          # Opus 5.5 default; set explicitly so the hop is visible
# Remove: any thinking: {type: "disabled"} logic — returns 400 on Opus 5.5
# Remove: tool_choice any/specific — returns 400 while thinking is active (always, on Opus 5.5)
```

No other config changes are required. `extended_thinking_enabled` must stay `true` (or unset); VT Code rejects
disabled-thinking requests for Opus 5.5 before they reach the API.

#### What changed

**Breaking:**

1. **Thinking can't be disabled.** On Opus 5, VT Code sent `thinking: {type: "disabled"}` when effort was `high` or
   below and the user opted out. On Opus 5.5 that request returns 400
   (`"thinking.type.disabled" is not supported for this model`). VT Code validates this per-request: any
   disabled-thinking path targeting `claude-opus-5-5` is rejected with a clear error. Remove the opt-out; use a lower
   effort (`low`) to control token spend.

2. **Effort default is `medium`, not `high`.** A request that omits `effort` now runs at `medium` where it ran at `high`
   on Opus 5. Set `effort` explicitly during migration, then re-run your effort sweep: step down where quality holds,
   step up for the most demanding work.

3. **Forced tool choice is rejected.** `tool_choice: any` or a specific tool returns 400. VT Code already rejects forced
   tool choice whenever thinking is active, and thinking is always active on Opus 5.5 — so existing validation now fires
   on every forced-choice request. Use `auto` with strict tool schemas or structured outputs, and say in the prompt when
   the tool applies.

4. **`thinking: {type: "enabled", budget_tokens: N}` still returns 400.** Same as Opus 5. Effort is the only thinking
   control.

**Recommended:**

1. **Render progress updates from `thinking` blocks.** Text the model writes between tool calls now comes back as
   progress-update `thinking` blocks, empty at the default `omitted` display. If your UI streamed that text as progress
   updates, it goes quiet between tool calls. VT Code requests `thinking.display: "updates"` (beta
   `thinking-display-updates-2026-08-18`) on Opus 5.5 when `thinking_display` is unset, so those blocks carry their text
   and stream into the reasoning view; the reasoning itself stays hidden. Render non-empty `thinking` blocks ahead of
   the `tool_use` block they precede and pass the blocks back unchanged with the rest of the assistant turn. Set
   `thinking_display = "omitted"` to opt out, or `"summarized"` to also get summarized reasoning. A configured
   `"updates"` is ignored on models that reject it (Opus 5, Sonnet 5).

2. **Handle `stop_reason: "refusal"`.** Opus 5.5 classifiers cover a broader set of categories than Opus 5's — expect
   values such as `bio` and `reasoning_extraction` in addition to `cyber`. VT Code maps `stop_reason: "refusal"` to
   `FinishReason::Refusal`; verify your handling surfaces the category.

3. **Advisor pairs.** Opus 5.5 has `advisor_tier = 7`: it can advise Opus 5 and Sonnet 5 executors, and Fable 5 / Fable
   5.1 can advise it. `validate_advisor_pair()` enforces this; the default advisor for an Opus 5.5 executor is Opus 5.5
   itself.

4. **No computer-use impact.** VT Code does not implement the `computer_20251124` tool, so that breaking change requires
   no VT Code action.

#### Migration checklist

- [ ] `agent.default_model = "claude-opus-5-5"` (or the `anthropic/`-prefixed route on gateways)
- [ ] `effort` set explicitly (`medium` default; re-sweep per workload)
- [ ] `thinking: {type: "disabled"}` paths removed (400 at any effort)
- [ ] `thinking: {type: "enabled", budget_tokens: N}` paths removed (still 400)
- [ ] `tool_choice` `any`/specific replaced with `auto` + strict tools or structured outputs
- [ ] `extended_thinking_enabled` left `true` (or unset)
- [ ] `thinking_display` left unset (`updates`) or set explicitly if your UI should not render text between tool calls
- [ ] `FinishReason::Refusal` handling verified for new categories (`bio`, `reasoning_extraction`)
- [ ] Advisor pairs re-validated if using the advisor feature
- [ ] Cost and latency re-baselined at the chosen effort level

---

## Opus 4.8 → Opus 5.5

> [!NOTE]
> Work through [Opus 4.8 → Opus 5](/docs/build-with-claude/migrating-to-claude-opus-5.md#opus-48--opus-5) first: it
> covers thinking on by default and the response-shape changes that come with it. Then apply [Opus 5 → Opus
> 5.5](#opus-5--opus-55). The Opus 5 escape hatch (thinking can be disabled at `high` effort or below) doesn't carry over
> — on Opus 5.5 thinking can't be disabled at all.

---

## Sonnet 5 → Opus 5.5

Apply [Sonnet 5 → Opus 5](/docs/build-with-claude/migrating-to-claude-opus-5.md#sonnet-5--opus-5), then apply
[Opus 5 → Opus 5.5](#opus-5--opus-55). Two deltas stack: Sonnet 5 allows disabling thinking at any effort and lacks
mid-conversation system messages, while Opus 5.5 allows neither disablement nor (unlike Opus 5) any thinking opt-out —
audit thinking-disable paths twice, once per hop.

---

## Get help

- [Migrating Claude models in VT Code](/docs/build-with-claude/migrating-to-claude-opus-5.md) — Fable/Sonnet/Haiku hops
  and the consolidated checklist
- [VT Code config reference](/docs/config/CONFIG_FIELD_REFERENCE.md) — all `[provider.anthropic]` fields
- [Extended thinking in VT Code](/docs/development/EXTENDED_THINKING.md) — thinking matrix and budget selection
- [Anthropic migration guide](https://platform.claude.com/docs/en/models/opus-5-5/migration-guide) — underlying API
  changes
