# Context Engineering in VT Code

## Overview

VT Code implements context engineering principles based on [Anthropic's research](https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents) to manage the "attention budget" of large language models effectively. This document explains the strategies and features we use to prevent context rot and maintain agent coherence across long-horizon tasks.

## Context Engineering vs Prompt Engineering

### Single-Turn Prompt Engineering

Traditional prompt engineering focuses on crafting a single prompt for discrete tasks:

-   **Input**: System prompt + User message
-   **Output**: Assistant message
-   **Process**: One-shot, static

### Multi-Turn Context Engineering (Agents)

Context engineering is about **iterative curation** - deciding what context to pass to the model on each turn:

**Available Context:**

-   Documentation, tools, memory files
-   Comprehensive instructions, domain knowledge
-   Message history, previous tool results

↓ **Curation (happens each turn)** ↓

**Selected Context:**

-   System prompt
-   Relevant docs (not all docs)
-   Memory file summary
-   Relevant tools (not all tools)
-   User message
-   Recent message history (not full history)

→ [Model] → Assistant message → Tool call → Tool result → **Next turn curation**

**Key Insight:** Unlike prompt engineering where you craft a prompt once, context engineering is **iterative** - the curation phase happens each time we decide what to pass to the model.

## Core Principles

### 1. **Minimal Token Usage ("Right Altitude" Prompts)**

Our system prompts strike a balance between specificity and flexibility:

-   **Concise Instructions**: Clear guidance without prescriptive micromanagement
-   **Progressive Disclosure**: Load information layer-by-layer as needed
-   **Heuristics Over Rules**: Provide strong patterns rather than exhaustive edge cases

Example from our default prompt:

```
## Context Strategy
- Use search tools (rg, grep, ripgrep) to find relevant code before reading files
- Load file metadata (paths, sizes) as references; read content only when necessary
- Summarize tool outputs; avoid echoing large results
- Preserve recent decisions and errors in your working memory
```

### 2. **Just-in-Time Context Loading**

Instead of pre-loading everything, we use lightweight references:

-   **File Paths as Metadata**: List files first, read content only when relevant
-   **Search Before Read**: Use `grep_file` to identify relevant files
-   **Chunked Reading**: Auto-truncate large files (>2000 lines) to first/last portions
-   **Pagination**: Tools support `per_page` and `page` parameters for large results

### 3. **Token Budget Management**

We track token usage across the context window to prevent exceeding limits:

```rust
use vtcode_core::core::token_budget::{TokenBudgetManager, TokenBudgetConfig, ContextComponent};

// Initialize tracker - use latest models from docs/models.json
let config = TokenBudgetConfig::for_model("gpt-5-mini", 400_000);
let manager = TokenBudgetManager::new(config);

// Track token usage
let tokens = manager.count_tokens_for_component(
    text,
    ContextComponent::ToolResult,
    Some("file_read_1")
).await?;

// Check thresholds
if manager.is_alert_threshold_exceeded().await {
    // Issue alert/warning
}
```

**Token Budget Features:**

-   Real-time token counting using Hugging Face `tokenizers`
-   Component-level tracking (system prompt, user messages, tool results, etc.)
-   Configurable warning thresholds
-   Automatic deduction after context cleanup

### 4. **Decision Ledger (Structured Note-Taking)**

The decision tracker maintains persistent memory across turns:

```rust
use vtcode_core::core::decision_tracker::DecisionTracker;

let mut tracker = DecisionTracker::new();

// Record decisions
let decision_id = tracker.record_decision(
    "Reading config file to understand project structure".to_string(),
    Action::ToolCall {
        name: "read_file".to_string(),
        args: json!({"path": "vtcode.toml"}),
        expected_outcome: "Configuration loaded".to_string(),
    },
    Some(0.9), // confidence score
);

// Generate compact ledger for prompt injection
let ledger_summary = tracker.render_ledger_brief(12);
```

The ledger is automatically injected into the system prompt if configured:

```toml
[context.ledger]
enabled = true
max_entries = 12
include_in_prompt = true
preserve_in_compression = true
```

### 5. **Tool Result Clearing and Summarization**

To prevent context pollution from verbose tool outputs:

-   **Auto-Truncation**: Command outputs >10k lines show first 5k + last 5k
-   **Concise Formats**: Tools default to `response_format="concise"`

### 7. **Tool Design for Efficiency**

Our tools are designed with context efficiency in mind:

#### Search Tools

-   **grep_file**: Fast pattern matching with `max_results` limits
-   **grep_file**: Syntax-aware search with `max_results` and `context_lines`
-   Return metadata first (file paths, line numbers) before content

#### File Operations

-   **list_files**: Pagination support, metadata-only by default
-   **read_file**: Auto-chunking for large files
-   **edit_file**: Precise replacements avoid rewriting entire files

#### Command Execution

-   **run_pty_cmd**: Auto-truncation, timeout limits
-   Streaming mode for long-running commands

## Configuration

### Token Budget Settings

```toml
[context.token_budget]
enabled = true
# Model for tokenizer - use latest models from docs/models.json
# Examples: "gpt-5-mini", "gpt-5-nano", "claude-sonnet-4", "deepseek-chat"
model = "gpt-5-nano"
warning_threshold = 0.75  # Warn at 75% usage

detailed_tracking = false  # Enable for debugging
```

### Context Management

```toml
[context]
max_context_tokens = 128000
trim_to_percent = 80
preserve_recent_turns = 5

[context.ledger]
enabled = true
max_entries = 12
include_in_prompt = true
preserve_in_compression = true
```

## Best Practices

### For Users

1. **Start Broad, Drill Down**: Use search tools to explore before reading files
2. **Paginate Large Results**: Use `per_page=50` for directory listings
3. **Review Budget**: Check token usage with `/status` command
4. **Leverage Ledger**: Reference past decisions instead of re-explaining

### For Developers

1. **Tool Design**: Return lightweight metadata before full content
2. **Result Limits**: Always provide `max_results` parameters
3. **Format Options**: Offer `concise` vs `detailed` response formats
4. **Chunking**: Auto-chunk large outputs (files, logs, listings)
5. **Summarization**: Compress verbose outputs automatically

## Monitoring

### Token Budget Reports

```rust
let report = manager.generate_report().await;
println!("{}", report);
```

Output:

```
Token Budget Report
==================
Total Tokens: 45000/128000 (35.2%)
Remaining: 83000 tokens

Breakdown by Category:
- System Prompt: 2500 tokens
- User Messages: 8000 tokens
- Assistant Messages: 12000 tokens
- Tool Results: 20000 tokens
- Decision Ledger: 2500 tokens
```

### Component Tracking

Enable detailed tracking for debugging:

```toml
[context.token_budget]
detailed_tracking = true
```

Then inspect per-component usage:

```rust
let breakdown = manager.get_component_breakdown().await;
for (component, tokens) in breakdown {
    println!("{}: {} tokens", component, tokens);
}
```

## Performance Considerations

### Token Counting Overhead

-   Uses Hugging Face `tokenizers` with heuristic fallback when pretrained assets are unavailable
-   ~10μs per message for typical sizes
-   Caching minimizes repeated tokenization
-   Disable `detailed_tracking` in production for best performance

### Memory Efficiency

-   LRU caches for tokenizer instances
-   Incremental tracking (no full recount needed)
-   Deduplication of identical content

## Future Enhancements

### Planned Features

1. **Sub-Agent Architecture**: Specialized agents with focused context windows
2. **Semantic Chunking**: Content-aware splitting for better preservation
3. **Context Swapping**: Hot-swap between task-specific contexts
4. **Adaptive Thresholds**: Learn optimal warning points per task type
5. **Multi-Model Support**: Per-provider tokenizers (Claude, Gemini)

## References

-   [Anthropic: Effective Context Engineering for AI Agents](https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents)
-   [Hugging Face tokenizers Documentation](https://huggingface.co/docs/tokenizers/index)
-   [Context Rot Research (Chroma)](https://research.trychroma.com/context-rot)

## Related Documentation

-   [Configuration Guide](./config/README.md)
-   [Tool Development Guide](./tools/README.md)
-   [Decision Tracking](./features/decision_tracking.md)
-   [Performance Optimization](./performance/optimization.md)
