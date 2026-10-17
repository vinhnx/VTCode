# Anthropic Thinking in VT Code

Direct Anthropic thinking is driven by the per-model capability profiles in
`crates/codegen/vtcode-llm/src/providers/anthropic/capabilities.rs`. Every profiled model is a Claude 5.x model and runs
**adaptive** thinking; none of them accepts manual `budget_tokens`. Claude ids without a profile (Claude 4.x and older)
are sent without a `thinking` field.

## Compact Runtime Matrix

| Model               | Thinking                | Default effort | Default display       | `task_budget` | Forced `tool_choice` |
| ------------------- | ----------------------- | -------------- | --------------------- | ------------- | -------------------- |
| `claude-opus-5-5`   | Adaptive, always on     | `medium`       | `updates` (VT Code)   | Yes           | Rejected             |
| `claude-opus-5`     | Adaptive, on by default | `high`         | API default (omitted) | Yes           | Allowed              |
| `claude-fable-5-1`  | Adaptive, always on     | `high`         | API default (omitted) | Yes           | Rejected             |
| `claude-fable-5`    | Adaptive, always on     | `high`         | API default (omitted) | Yes           | Allowed              |
| `claude-sonnet-5-5` | Adaptive, on by default | `high`         | `updates` (VT Code)   | No            | Rejected             |
| `claude-sonnet-5`   | Adaptive, on by default | `high`         | API default (omitted) | No            | Allowed              |

All six models share these limits:

- Effort levels: `low`, `medium`, `high`, `xhigh` and `max`.
- A 64k default `max_tokens`.
- A 1M-token context window.
- Explicit `temperature`, `top_p` and `top_k` are rejected, so VT Code drops them.
- Server-side refusal fallbacks are available.

## Turning up-front thinking off

`claude-sonnet-5` and `claude-opus-5` (the latter at `high` effort or below) accept `thinking: { "type": "disabled" }`.
`claude-sonnet-5-5` rejects `disabled` with a 400 and takes `{ "type": "between_tools" }` as its lowest setting instead
— up-front thinking is off while the short progress notes written between tool calls still come back as `thinking`
blocks. It is itself rejected at `xhigh`/`max` effort, so VT Code omits the field and lets the model run adaptive
thinking there.

| Model                                                   | Lowest setting sent           | At `xhigh`/`max` effort  |
| ------------------------------------------------------- | ----------------------------- | ------------------------ |
| `claude-sonnet-5-5`                                     | `{ "type": "between_tools" }` | field omitted (adaptive) |
| `claude-sonnet-5`                                       | `{ "type": "disabled" }`      | `{ "type": "disabled" }` |
| `claude-opus-5`                                         | `{ "type": "disabled" }`      | field omitted (adaptive) |
| `claude-opus-5-5`, `claude-fable-5`, `claude-fable-5-1` | none                          | none                     |

Both settings are produced by `extended_thinking_enabled = false` and by a `thinking_mode = disabled` request override,
and rewritten per model for server-side fallback entries.

## Configuration

Configure Anthropic thinking in `vtcode.toml`:

```toml
[provider.anthropic]
effort = "xhigh"                 # optional; omit to use the model default
thinking_display = "summarized"  # optional: "summarized", "omitted", or "updates"
```

### Important defaults

- `effort` is unset by default, so each model uses its own default (`medium` on Claude Opus 5.5, `high` on the others).
  An explicit `agent.reasoning_effort` or `/model` picker effort takes precedence. A configured level the model does not
  support falls back to the model default.
- `task_budget_tokens` is only sent for Claude Fable 5/5.1, Opus 5, and Opus 5.5 (not Claude Sonnet 5 or Sonnet 5.5).
- `thinking_display` defaults to the model default when unset.
  - Claude Opus 5.5 and Claude Sonnet 5.5 request `updates` (beta `thinking-display-updates-2026-08-18`, sent only when
    used), so the text they write between tool calls stays visible while the reasoning itself stays hidden.
  - `updates` is accepted only by Sonnet 5.5, Opus 5.5 and Fable 5/5.1. On other models a configured `updates` is
    omitted, not sent.
- When unset, `max_tokens` defaults to 64k for every Claude 5.x model, with thinking on or off.

## Adaptive Thinking Behavior

VT Code sends:

```json
{
    "thinking": { "type": "adaptive" },
    "output_config": { "effort": "..." }
}
```

`output_config.effort` is included only when an effort is configured.

- An explicit `thinking_budget`, `MAX_THINKING_TOKENS`, or a manual-budget request override is served as adaptive
  thinking. The model does not receive `budget_tokens`, which every Claude 5.x model rejects with a 400.
- `extended_thinking_enabled = false` asks for the model's lowest thinking setting (`disabled`, or `between_tools` on
  Claude Sonnet 5.5). Adaptive-only models have none, so VT Code logs a warning and keeps the model default.
- The interleaved-thinking beta header is never sent for Claude 5.x. Adaptive thinking interleaves natively.

### Manual budgets (Anthropic-compatible backends only)

The budgeted `{"type": "enabled", "budget_tokens": N}` path remains for Anthropic-compatible backends that advertise
reasoning without a Claude profile (for example MiniMax). The budget comes from the first of these that is set:

1. Explicit `thinking_budget` on the request
2. `MAX_THINKING_TOKENS` from the environment
3. `reasoning_effort` mapped to a token budget
4. `provider.anthropic.interleaved_thinking_budget_tokens`

The budget is clamped below `max_tokens`.

## Feature Compatibility

- **Assistant prefill:** a trailing assistant turn returns 400 on Claude 4.6 and later, including every Claude 5.x
  model, so VT Code never sends one there.
- **Forced `tool_choice`:** `any` and `tool` are downgraded to `auto` when thinking is on, or when the model rejects
  forced tool use (Sonnet 5.5, Opus 5.5, Fable 5.1). The same check applies to every server-side fallback model.
- **Replayed thinking:** thinking blocks are replayed in their original position (`anthropic_block_order`). Sonnet 5.5,
  Opus 5.5 and Fable 5.1 bind each thinking signature to the exact prior prefix, so history stays append-only for them.
- **Refusals:**
  - A refusal's `stop_details` is surfaced on both the streaming and the non-streaming path.
  - Server-side fallbacks use the `"default"` form (beta `server-side-fallback-2026-07-01`).
  - Each fallback model gets its own sanitized thinking config.

## Disabling Thinking

The lowest thinking setting depends on the model:

| Model                               | Lowest setting sent                                     |
| ----------------------------------- | ------------------------------------------------------- |
| Claude Sonnet 5.5                   | `{ "type": "between_tools" }`; omitted at `xhigh`/`max` |
| Claude Opus 5.5, Fable 5, Fable 5.1 | None; validation errors, fallbacks rewrite to adaptive  |
| Claude Opus 5                       | `{ "type": "disabled" }` only at effort `high` or below |
| Claude Sonnet 5                     | `{ "type": "disabled" }`                                |

The per-request disabled override omits the field on models that reject it, so those models keep their default thinking.

## Prompting Tips

### Use General Instructions First

Claude often performs better with high-level instructions rather than step-by-step prescriptive guidance. The model's
creativity in approaching problems may exceed a human's ability to prescribe the optimal thinking process.

**Instead of:**

```text
Think through this problem step by step:
1. First, identify the variables
2. Then, set up the equation
3. Next, solve for x
```

**Consider:**

```text
Please think about this problem thoroughly and in great detail.
Consider multiple approaches and show your complete reasoning.
Try different methods if your first approach doesn't work.
```

### Multishot Prompting

Multishot prompting works well with extended thinking. When you provide examples of how to think through problems,
Claude will follow similar reasoning patterns.

Keep examples in plain prose. Visible reasoning tags such as `<thinking>` or `<scratchpad>` are unnecessary with native
adaptive thinking, and they can invite `reasoning_extraction` refusals on Claude Opus 5.5.

### Self-Verification

Ask Claude to verify its work for improved consistency and error handling:

```text
Write a function to calculate the factorial of a number.
Before you finish, please verify your solution with test cases for:
- n=0
- n=1
- n=5
- n=10
And fix any issues you find.
```

### Best Practices

1. **Start with effort, not budgets**: Claude 5.x ignores token budgets; begin at the model default effort and step up
   only on measured headroom
2. **Use batch processing**: For very long thinking runs to avoid networking issues
3. **Language**: Extended thinking performs best in English (outputs can be in any supported language)
4. **Clean responses**: Instruct Claude not to repeat its extended thinking if you want cleaner output
5. **Don't pass back thinking**: Passing Claude's extended thinking back in user text blocks doesn't improve performance

### What NOT to Do

- Don't prefill extended thinking blocks (explicitly not allowed)
- Don't manually modify output text that follows thinking blocks
- Don't pass thinking output back in user messages
- Don't use extended thinking for simple tasks where regular prompting suffices

## Budget Recommendations by Task Type

These apply only to the manual-budget path on Anthropic-compatible backends; Claude 5.x models take `effort` instead.

| Task Type               | Recommended Budget | Example                                  |
| ----------------------- | ------------------ | ---------------------------------------- |
| Simple calculations     | 1024-2048          | Basic math, simple lookups               |
| Standard analysis       | 2048-4096          | Code review, summarization               |
| Complex reasoning       | 4096-8192          | Multi-step problems, debugging           |
| Research synthesis      | 8192-16384         | Analyzing multiple sources               |
| Complex STEM problems   | 16384-32768        | 4D visualizations, physics simulations   |
| Constraint optimization | 16384-32768        | Multi-variable planning with constraints |

## Reasoning Effort `xhigh` and `max` Across Providers

VT Code exposes a portable effort ladder (`none`, `minimal`, `low`, `medium`, `high`, `xhigh`, `max`) via
`agent.reasoning_effort`, the `/model` picker, and the `/model` picker. `xhigh` and `max` are only offered for models
that natively support them; other models hide those levels instead of aliasing silently.

| Provider / models                                                                                                          | `xhigh`                     | `max`                                                                      | Wire shape                                                     |
| -------------------------------------------------------------------------------------------------------------------------- | --------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------- |
| OpenAI GPT-5.6 family (`gpt-5.6`, `-sol`, `-terra`, `-luna`), `gpt-6-astra`                                                | Native                      | Native (5.6+ only; `minimal` dropped on 5.6+)                              | `reasoning: { effort, summary: "auto" }`                       |
| OpenAI GPT-5 Codex / 5.2 Codex, GPT-5.1-mini, `gpt-oss-*`                                                                  | Codex only                  | Not supported                                                              | Same Responses shape                                           |
| Anthropic Claude 5.x (`claude-sonnet-5`, `claude-sonnet-5-5`, `claude-fable-5`/`-5-1`, `claude-opus-5`, `claude-opus-5-5`) | Native                      | Native                                                                     | `thinking: { type: "adaptive" }` + `output_config: { effort }` |
| xAI `grok-4.6+`                                                                                                            | Native                      | Clamped to `xhigh` (no native `max`; older models treat `xhigh` as `high`) | `reasoning_effort`                                             |
| Meta Muse Spark 1.1–1.3                                                                                                    | Native                      | Aliased to `xhigh` (`max` ships after additional safety testing)           | `reasoning_effort`                                             |
| DeepSeek V4 Pro / Flash                                                                                                    | Alias to `high`             | Native (`low` also native; `medium` maps to `high`)                        | `thinking: { type: "enabled" }` + `reasoning_effort`           |
| Moonshot Kimi K3                                                                                                           | Alias to `max`              | Native (default; `minimal` maps to `low`, `medium`/`high` map to `high`)   | Top-level `reasoning_effort`                                   |
| ZAI GLM-5.3 / 5.3-Flash / 5.3-FlashX / 5.2                                                                                 | Alias to `max` (5.2)        | Native (`low`/`high`/`max`; `medium` maps to `high`)                       | `thinking: { type: "enabled" }` + `reasoning_effort`           |
| StepFun (`step-3.7-flash`, `step-5-preview`)                                                                               | Hidden (collapse to `high`) | Hidden (collapse to `high`)                                                | `reasoning: { effort }` (native `low`/`medium`/`high`)         |
| Gemini 3.x, Ollama, LlamaCpp, Evolink, HuggingFace, Mistral                                                                | Hidden (collapse to `high`) | Hidden (collapse to `high`)                                                | `thinking_level` / omitted                                     |

### How to use

```toml
# vtcode.toml
[agent]
reasoning_effort = "xhigh"  # or "max" where natively supported
```

- `/model` picker effort (`xhigh`/`max`) validates against the active model's preset; unsupported levels are rejected
  with the supported list. A configured effort the active route does not support (for example after switching models) is
  omitted for that request with a warning instead of failing the turn.
- The `/model` picker only lists `xhigh`/`max` when the selected model advertises them (`supports_xhigh/max_reasoning` +
  `builtin_model_presets`).
- Harness config (`agent.harness`, `automation.full_auto`) intentionally has no effort field: effort flows through the
  existing model configuration (`agent.reasoning_effort`, per-subagent `reasoning_effort`, `provider.anthropic.effort`)
  into every turn, including full-auto runs.

### Limitations and considerations

- `max` is uncapped and has the highest token cost and latency. Reserve it for tasks where a wrong answer costs more
  than the extra inference spend; start at `high`, step up to `xhigh`, then `max` only on measured headroom.
- Anthropic at `xhigh`/`max`: set `max_tokens` to at least 64k so the model has room to think across subagents and tool
  calls (the Claude 5.x default is 64k); sampling parameters are rejected on every Claude 5.x model.
- DeepSeek thinking mode ignores `temperature`, `top_p`, `presence_penalty`, and `frequency_penalty`; Kimi K3 fixes
  `temperature` to `1.0` and switching effort mid-conversation invalidates prefix-cache hits.
- `reasoning.mode: pro`, `reasoning.context`, and multi-agent `ultra` are separate OpenAI axes and are not controlled by
  `xhigh`/`max`.
- ZAI GLM models always reason; `none` disables thinking only where the provider allows it, and preserved
  `reasoning_content` must round-trip unmodified for multi-turn coherence.

## References

- [Anthropic Extended Thinking Documentation](https://docs.anthropic.com/en/docs/build-with-claude/extended-thinking)
- [Extended Thinking Tips](https://docs.anthropic.com/en/docs/build-with-claude/prompt-engineering/extended-thinking-tips)
- [Anthropic API Reference](https://docs.anthropic.com/en/api/messages)
