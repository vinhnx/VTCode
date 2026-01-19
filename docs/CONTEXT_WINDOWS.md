# Context Windows in VT Code

This document describes how VT Code manages context windows based on Anthropic's context window documentation.

## Overview

The "context window" refers to the entirety of text a language model can look back on and reference when generating new text, plus the new text it generates. This is different from the training corpus - it represents the model's "working memory."

## Context Window Sizes

VT Code supports multiple context window configurations:

| Configuration | Size | Availability |
|--------------|------|--------------|
| Standard | 200K tokens | All Claude models |
| Enterprise | 500K tokens | Claude.ai Enterprise |
| Extended (Beta) | 1M tokens | Claude Sonnet 4, Sonnet 4.5 (tier 4) |

### Enabling 1M Context Window

The 1M token context window is available for:
- Organizations in usage tier 4
- Organizations with custom rate limits
- Claude Sonnet 4 and Sonnet 4.5 models only

To enable, VT Code automatically includes the beta header `context-1m-2025-08-07` for eligible models.

## Token Budget Thresholds

VT Code implements proactive context management using these thresholds:

| Threshold | Usage | Action |
|-----------|-------|--------|
| Warning | 70% | Start preparing for context handoff, update key artifacts |
| High | 85% | Active context management, summarize and persist state |
| Critical | 90% | Force context handoff or summary |

These thresholds are defined in `vtcode-config/src/constants.rs`:

```rust
pub const TOKEN_BUDGET_WARNING_THRESHOLD: f64 = 0.70;
pub const TOKEN_BUDGET_HIGH_THRESHOLD: f64 = 0.85;
pub const TOKEN_BUDGET_CRITICAL_THRESHOLD: f64 = 0.90;
```

## Context Awareness

Claude Sonnet 4.5 and Claude Haiku 4.5 feature **context awareness**, enabling these models to track their remaining context window throughout a conversation.

### How It Works

At the start of a conversation, Claude receives information about its total context window:

```xml
<budget:token_budget>200000</budget:token_budget>
```

After each tool call, Claude receives an update on remaining capacity:

```xml
<system_warning>Token usage: 35000/200000; 165000 remaining</system_warning>
```

### Benefits

Context awareness is particularly valuable for:
- Long-running agent sessions that require sustained focus
- Multi-context-window workflows where state transitions matter
- Complex tasks requiring careful token management

## Extended Thinking

When using extended thinking, all input and output tokens (including thinking tokens) count toward the context window limit, with important nuances:

### Key Points

1. **Thinking tokens are stripped from subsequent turns**: The Claude API automatically excludes thinking blocks from previous turns when passed back as conversation history.

2. **Token efficiency**: Thinking tokens are billed as output tokens only once, during their generation.

3. **Tool use requirement**: When posting tool results, the entire unmodified thinking block that accompanies that specific tool request must be included.

### Thinking Budget Configuration

```toml
[anthropic]
extended_thinking_enabled = true
thinking_budget = 16000  # Default: 16K tokens
```

Minimum: 1,024 tokens
Recommended: 10,000+ tokens for complex reasoning

## Implementation Details

### Context Manager

The `ContextManager` in VT Code tracks:
- Total token usage across the conversation
- Token budget status (Normal, Warning, High, Critical)
- Context window size for the current model

```rust
pub enum TokenBudgetStatus {
    Normal,   // Below 70%
    Warning,  // 70-85%
    High,     // 85-90%
    Critical, // Above 90%
}
```

### Relevant Files

- `vtcode-config/src/constants.rs`: Context window constants and thresholds
- `src/agent/runloop/unified/context_manager.rs`: Token budget tracking
- `src/agent/runloop/unified/incremental_system_prompt.rs`: Context awareness injection

## Best Practices

1. **Monitor token usage**: Track usage throughout long conversations
2. **Persist important state**: Update `.vtcode/` artifacts before hitting limits
3. **Use context awareness**: Enable for Claude 4.5 models to optimize execution
4. **Plan for handoffs**: At 70% usage, start preparing for context transitions

## References

- [Anthropic Context Windows Documentation](https://docs.anthropic.com/en/docs/build-with-claude/context-windows)
- [Extended Thinking Guide](https://docs.anthropic.com/en/docs/build-with-claude/extended-thinking)
- [Claude 4 Best Practices](https://docs.anthropic.com/en/docs/build-with-claude/prompt-engineering/claude-4-best-practices)
