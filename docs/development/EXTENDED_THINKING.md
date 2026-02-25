# Extended Thinking for Anthropic Models

Extended thinking gives Claude enhanced reasoning capabilities for complex tasks, providing transparency into its step-by-step thought process before delivering its final answer.

## Supported Models

Extended thinking is supported in the following Claude models:

- Claude Sonnet 4.5 (`claude-sonnet-4-5-20250929`)
- Claude Haiku 4.5 (`claude-haiku-4-5-20251001`)
- Claude Opus 4.5 (`claude-opus-4-5-20251101`)
- Claude Opus 4.1 (`claude-opus-4-1-20250805`)
- Claude Sonnet 4 (`claude-sonnet-4-20250514`)
- Claude Opus 4 (`claude-opus-4-20250514`)
- Claude Sonnet 3.7 (`claude-3-7-sonnet-20250219`)

## Configuration

Configure extended thinking in your `vtcode.toml`:

```toml
[provider.anthropic]
# Enable/disable extended thinking (default: true)
extended_thinking_enabled = true

# Budget tokens for extended thinking (minimum: 1024, recommended: 10000+)
# Larger budgets enable more thorough analysis for complex problems.
interleaved_thinking_budget_tokens = 12000

# Beta header for interleaved thinking (allows thinking between tool calls)
interleaved_thinking_beta = "interleaved-thinking-2025-05-14"

# Type value for enabling thinking mode
interleaved_thinking_type_enabled = "enabled"
```

## How It Works

When extended thinking is enabled:

1. Claude creates `thinking` content blocks where it outputs its internal reasoning
2. Claude incorporates insights from this reasoning before crafting a final response
3. The API response includes `thinking` content blocks, followed by `text` content blocks

## Token Budget

The `interleaved_thinking_budget_tokens` parameter determines the maximum number of tokens Claude is allowed to use for its internal reasoning process:

- **Minimum**: 1024 tokens
- **Recommended**: 10000+ tokens for complex tasks
- Claude may not use the entire budget allocated

### Budget Guidelines by Task Complexity

| Task Type | Recommended Budget |
|-----------|-------------------|
| Simple analysis | 1024-4096 |
| Code review | 4096-8192 |
| Complex reasoning | 8192-16384 |
| Architecture planning | 16384-32768 |

## Interleaved Thinking

With interleaved thinking enabled (via the beta header), Claude can think between tool calls, enabling:

- Reasoning about tool results before deciding what to do next
- Chaining multiple tool calls with reasoning steps in between
- More nuanced decisions based on intermediate results

## Feature Compatibility

When extended thinking is enabled, the following features are affected:

- **Temperature**: Automatically set to `None` (temperature modification is not compatible with thinking)
- **Top-k**: Not compatible with thinking
- **Top-p**: When thinking is enabled, can only be set between 0.95 and 1.0
- **Pre-fill**: Cannot pre-fill responses when thinking is enabled
- **Tool choice**: Only `auto` and `none` are supported (not `any` or specific tool forcing)
- **Structured output**: Cannot force structured output tool when thinking is enabled
- **Budget limit**: `budget_tokens` must be less than `max_tokens` (except with interleaved thinking)

## Extended Thinking with Tool Use

Extended thinking can be used alongside tool use, but with some limitations:

### Tool Choice Restrictions

When thinking is enabled, only these tool choices are supported:
- `auto` (default) - Claude decides when to use tools
- `none` - Disable tool use

The following are **not supported** with thinking:
- `any` - Forces Claude to use some tool
- Specific tool forcing - Forces a particular tool

If an incompatible tool choice is detected, VT Code automatically falls back to `auto`.

### Preserving Thinking Blocks

When using tools with thinking, thinking blocks must be preserved and passed back with tool results. VT Code handles this automatically in multi-turn conversations.

### Interleaved Thinking

With the `interleaved-thinking-2025-05-14` beta header (enabled by default), Claude can think between tool calls:
- Reason about tool results before deciding what to do next
- Chain multiple tool calls with reasoning steps in between
- Make more nuanced decisions based on intermediate results

## Summarized Thinking

For Claude 4 models, the API returns a summary of Claude's full thinking process. Key points:

- You're charged for the full thinking tokens generated, not the summary tokens
- The billed output token count will **not match** the visible token count
- Summarization preserves key ideas with minimal added latency

## Error Handling

Common errors when using extended thinking:

### Budget Too Small
```
Error code: 400 - thinking.enabled.budget_tokens: Input should be greater than or equal to 1024
```
**Solution**: Increase `interleaved_thinking_budget_tokens` to at least 1024.

### Temperature with Thinking
```
Error code: 400 - `temperature` may only be set to 1 when thinking is enabled
```
**Solution**: VT Code automatically handles this by setting temperature to `None` when thinking is enabled.

### Context Window Exceeded
```
Error code: 400 - prompt is too long: 214315 tokens > 204798 maximum
```
**Solution**: Reduce input size or thinking budget. Consider using batch processing for large thinking budgets.

## Disabling Extended Thinking

To disable extended thinking:

```toml
[provider.anthropic]
extended_thinking_enabled = false
```

When disabled:
- The interleaved-thinking beta header is not sent
- No thinking blocks are generated
- Temperature controls work normally
- Fallback to `reasoning` parameter for older models if `reasoning_effort` is set

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

| Task Type | Recommended Budget | Example |
|-----------|-------------------|---------|
| Simple calculations | 1024-2048 | Basic math, simple lookups |
| Standard analysis | 2048-4096 | Code review, summarization |
| Complex reasoning | 4096-8192 | Multi-step problems, debugging |
| Research synthesis | 8192-16384 | Analyzing multiple sources |
| Complex STEM problems | 16384-32768 | 4D visualizations, physics simulations |
| Constraint optimization | 16384-32768 | Multi-variable planning with constraints |

## References

- [Anthropic Extended Thinking Documentation](https://docs.anthropic.com/en/docs/build-with-claude/extended-thinking)
- [Extended Thinking Tips](https://docs.anthropic.com/en/docs/build-with-claude/prompt-engineering/extended-thinking-tips)
- [Anthropic API Reference](https://docs.anthropic.com/en/api/messages)
