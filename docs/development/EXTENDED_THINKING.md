# Anthropic Thinking in VT Code

VT Code currently splits direct Anthropic Claude thinking into two runtime paths:

- Adaptive by default: `claude-opus-4-7`, `claude-opus-4-6`, `claude-sonnet-4-6`, `claude-mythos-preview`
- Manual budget only: `claude-haiku-4-5`

## Compact Runtime Matrix

| Model                   | VT Code default | What VT Code emits                                                                          |
| ----------------------- | --------------- | ------------------------------------------------------------------------------------------- |
| `claude-opus-4-7`       | Adaptive        | `thinking: { type: "adaptive" }`, default `effort = xhigh`, optional `task_budget`          |
| `claude-mythos-preview` | Adaptive        | `thinking: { type: "adaptive" }`, default `effort = high`, disabled thinking is rejected    |
| `claude-opus-4-6`       | Adaptive        | `thinking: { type: "adaptive" }`, default `effort = high`, explicit `thinking_budget` falls back to manual mode |
| `claude-sonnet-4-6`     | Adaptive        | `thinking: { type: "adaptive" }`, default `effort = high`, explicit `thinking_budget` falls back to manual mode |
| `claude-haiku-4-5`      | Manual budget   | `thinking: { type: "enabled", budget_tokens: N }`                                           |

## Configuration

Configure Anthropic thinking in `vtcode.toml`:

```toml
[provider.anthropic]
extended_thinking_enabled = true
interleaved_thinking_budget_tokens = 12000
interleaved_thinking_beta = "interleaved-thinking-2025-05-14"
effort = "xhigh"
thinking_display = "summarized"
```

### Important defaults

- `effort` now defaults to `xhigh`; models that do not support `xhigh` fall back to their supported default, typically `high`
- `xhigh` is only valid for Claude Opus 4.7
- `task_budget_tokens` is only sent for Claude Opus 4.7
- `thinking_display` defaults to the Anthropic API default when unset
- Claude Opus 4.7 and Claude Mythos Preview default to omitted thinking at the API level

## Adaptive Thinking Behavior

For adaptive models, VT Code sends:

```json
{
    "thinking": { "type": "adaptive" },
    "output_config": { "effort": "..." }
}
```

### Adaptive model notes

- Claude Opus 4.7 is adaptive-only in VT Code
- Claude Mythos Preview is adaptive-only and cannot be used with disabled thinking
- Claude Opus 4.6 and Claude Sonnet 4.6 are adaptive by default, but still accept explicit manual budgets for backward compatibility
- `thinking_budget` is rejected on adaptive-only models and forces manual mode only on Claude Opus 4.6 / Sonnet 4.6
- `effort` is enabled on Claude Opus 4.7, Claude Opus 4.6, Claude Sonnet 4.6, and Claude Mythos Preview
- Claude Opus 4.7 supports `low`, `medium`, `high`, `xhigh`, and `max`
- Claude Opus 4.6 and Claude Sonnet 4.6 support `low`, `medium`, `high`, and `max`
- Claude Mythos Preview supports `low`, `medium`, `high`, and `max`

## Budgeted Thinking Behavior

For budgeted-thinking models, VT Code sends:

```json
{
    "thinking": {
        "type": "enabled",
        "budget_tokens": 12000
    }
}
```

### Budget selection order

1. Explicit `thinking_budget` on the request
2. `MAX_THINKING_TOKENS` from the environment
3. `reasoning_effort` mapped to a token budget
4. `provider.anthropic.interleaved_thinking_budget_tokens`

### Manual-mode notes

- Claude Haiku 4.5 stays on the budgeted path
- Claude Sonnet 4.6 still uses the interleaved-thinking beta header when it falls back to manual mode
- Claude Opus 4.6 can still use manual budgets, but VT Code does not enable interleaved manual thinking for it
- When interleaving is unavailable, `budget_tokens` must stay below `max_tokens`

## Feature Compatibility

When thinking is active, VT Code enforces or normalizes the following behavior:

- `tool_choice` is limited to `auto` or `none`
- assistant prefills are incompatible with adaptive-only Claude models
- `thinking_display = "summarized"` restores visible summarized thinking on models that default to omitted output
- Claude Opus 4.7 rejects explicit `temperature`, `top_p`, and `top_k`

## Disabling Thinking

To disable thinking where VT Code allows it:

```toml
[provider.anthropic]
extended_thinking_enabled = false
```

Current VT Code behavior:

- Disabled thinking is allowed for Claude Opus 4.7
- Disabled thinking is allowed for Claude Opus 4.6 and Claude Sonnet 4.6
- Disabled thinking is rejected for Claude Mythos Preview
- Budgeted models stop emitting `thinking` blocks when disabled

## Prompting Tips

### Use General Instructions First

Claude often performs better with high-level instructions rather than step-by-step prescriptive guidance. The model's creativity in approaching problems may exceed a human's ability to prescribe the optimal thinking process.

**Instead of:**

```
Think through this problem step by step:
1. First, identify the variables
2. Then, set up the equation
3. Next, solve for x
```

**Consider:**

```
Please think about this problem thoroughly and in great detail.
Consider multiple approaches and show your complete reasoning.
Try different methods if your first approach doesn't work.
```

### Multishot Prompting

Multishot prompting works well with extended thinking. When you provide examples of how to think through problems, Claude will follow similar reasoning patterns.

You can include few-shot examples using XML tags like `<thinking>` or `<scratchpad>` to indicate canonical patterns of extended thinking.

### Self-Verification

Ask Claude to verify its work for improved consistency and error handling:

```
Write a function to calculate the factorial of a number.
Before you finish, please verify your solution with test cases for:
- n=0
- n=1
- n=5
- n=10
And fix any issues you find.
```

### Best Practices

1. **Start small**: Begin with minimum budget (1024) and increase incrementally
2. **Use batch processing**: For budgets above 32K tokens to avoid networking issues
3. **Language**: Extended thinking performs best in English (outputs can be in any supported language)
4. **Clean responses**: Instruct Claude not to repeat its extended thinking if you want cleaner output
5. **Don't pass back thinking**: Passing Claude's extended thinking back in user text blocks doesn't improve performance

### What NOT to Do

- Don't prefill extended thinking blocks (explicitly not allowed)
- Don't manually modify output text that follows thinking blocks
- Don't pass thinking output back in user messages
- Don't use extended thinking for simple tasks where regular prompting suffices

## Budget Recommendations by Task Type

| Task Type               | Recommended Budget | Example                                  |
| ----------------------- | ------------------ | ---------------------------------------- |
| Simple calculations     | 1024-2048          | Basic math, simple lookups               |
| Standard analysis       | 2048-4096          | Code review, summarization               |
| Complex reasoning       | 4096-8192          | Multi-step problems, debugging           |
| Research synthesis      | 8192-16384         | Analyzing multiple sources               |
| Complex STEM problems   | 16384-32768        | 4D visualizations, physics simulations   |
| Constraint optimization | 16384-32768        | Multi-variable planning with constraints |

## References

- [Anthropic Extended Thinking Documentation](https://docs.anthropic.com/en/docs/build-with-claude/extended-thinking)
- [Extended Thinking Tips](https://docs.anthropic.com/en/docs/build-with-claude/prompt-engineering/extended-thinking-tips)
- [Anthropic API Reference](https://docs.anthropic.com/en/api/messages)
