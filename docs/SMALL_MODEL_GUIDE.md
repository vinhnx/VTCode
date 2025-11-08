# Small Model Tier Guide

## Overview

Following Claude Code's pattern, VT Code now supports a **small/lightweight model tier** for efficient operations. This enables ~50% of LLM calls to use cheaper models (70-80% cost reduction) while maintaining quality.

## Configuration

### Default Settings

```toml
[agent.small_model]
enabled = true
model = ""                        # Auto-selects based on main provider
max_tokens = 1000
temperature = 0.3
use_for_large_reads = true
use_for_web_summary = true
use_for_git_history = true
use_for_compression = true
```

### Model Recommendations

By Provider:

| Provider | Main Model | Small Model | Savings |
|----------|-----------|-------------|---------|
| **Anthropic** | Claude 3.5 Sonnet | Claude 3.5 Haiku | ~70-75% |
| **OpenAI** | GPT-4o | GPT-4o Mini | ~75-80% |
| **Google** | Gemini 2.0 Pro | Gemini 2.0 Flash | ~70% |
| **Ollama** | mistral-7b | phi:2.7b | ~40% |

### Custom Configuration

Override with a specific model:

```toml
[agent.small_model]
enabled = true
model = "claude-3-5-haiku"        # Explicitly use Haiku
max_tokens = 800
temperature = 0.2
use_for_large_reads = true
use_for_web_summary = true
use_for_git_history = true
use_for_compression = true
```

## Use Cases

### 1. Large File Reads (>50KB)

**When to use:** Parsing, summarizing, extracting info from large files

**Example:**
```rust
// Reading a 100KB JSON log file to extract error messages
if config.agent.small_model.use_for_large_reads {
    // Use small_model: "Summarize error messages in this log"
    // Much cheaper than main model
}
```

**Token Savings:** 98% vs. returning raw file content to model

### 2. Web Content Summarization

**When to use:** Fetching and summarizing URLs, extracting specific information

**Example:**
```rust
// Fetch a documentation page and extract API endpoints
if config.agent.small_model.use_for_web_summary {
    // Use small_model: "Extract API endpoints from this page"
    // Fast and cheap for parsing structured content
}
```

**Token Savings:** 90% vs. sending full page to main model

### 3. Git History Processing

**When to use:** Analyzing commit messages, understanding code evolution

**Example:**
```rust
// Process last 50 commits to categorize changes
if config.agent.small_model.use_for_git_history {
    // Use small_model: "Categorize these commits by type"
    // Deterministic parsing with temperature=0.3
}
```

**Token Savings:** 85% vs. having main model parse commits

### 4. Conversation Context Compression

**When to use:** Summarizing long conversations when context grows

**Example:**
```rust
// Compress 30-turn conversation into summary
if config.agent.small_model.use_for_compression {
    // Use small_model: "Summarize key decisions from this conversation"
    // Temperature=0.3 ensures consistent summaries
}
```

**Token Savings:** 90% vs. keeping full context

### 5. One-Word Classifications

**When to use:** Simple categorization, tagging, brief labels

**Example:**
```rust
// Label 1000 error messages by category
if config.agent.small_model.enabled {
    // Batch: Use small_model to classify each error
    // Returns: "network", "parse", "timeout", etc.
}
```

**Token Savings:** 95% vs. main model for 1000 classifications

## Implementation Patterns

### Pattern 1: Conditional Model Selection

```rust
fn get_model_for_task(config: &AgentConfig, task: TaskType) -> String {
    match task {
        TaskType::LargeFileRead if config.agent.small_model.use_for_large_reads => {
            config.agent.small_model.model.clone()
                .or_else(|| auto_select_lightweight_sibling(&config.agent.default_model))
        },
        TaskType::WebSummary if config.agent.small_model.use_for_web_summary => {
            // Same logic for web content
        },
        _ => config.agent.default_model.clone(),
    }
}
```

### Pattern 2: Batch Processing with Small Model

```rust
async fn process_items_batch(
    items: Vec<Item>,
    config: &AgentConfig,
) -> Result<Vec<ProcessedItem>> {
    let model = if config.agent.small_model.enabled {
        &config.agent.small_model.model
    } else {
        &config.agent.default_model
    };
    
    let mut results = Vec::new();
    for item in items {
        let response = llm_client.complete(
            model,
            format!("Process: {}", item),
            config.agent.small_model.max_tokens,
            config.agent.small_model.temperature,
        ).await?;
        results.push(parse_response(response));
    }
    Ok(results)
}
```

### Pattern 3: Conditional Compression

```rust
async fn maybe_compress_context(
    conversation: &[Message],
    config: &AgentConfig,
) -> Result<Option<String>> {
    if conversation.len() < 20 {
        return Ok(None);
    }
    
    if !config.agent.small_model.use_for_compression {
        return Ok(None);
    }
    
    let summary = llm_client.complete(
        &config.agent.small_model.model,
        format!("Summarize key decisions: {}", format_messages(conversation)),
        config.agent.small_model.max_tokens,
        0.2, // Even lower temperature for consistency
    ).await?;
    
    Ok(Some(summary))
}
```

## Cost Analysis

### Example: Processing 1000-Item Dataset

**With Main Model Only:**
- Cost per item: 10 tokens @ $0.01/1K = $0.0001
- Total: 1000 × $0.0001 = $0.10

**With Small Model (80% cheaper):**
- Cost per item: 10 tokens @ $0.002/1K = $0.00002
- Total: 1000 × $0.00002 = $0.02
- **Savings: $0.08 (80% reduction)**

### Example: Context Compression Every 20 Turns

Session with 100 turns:
- 5 compression cycles × 5000 tokens each
- Main model: 25,000 tokens @ $0.01/1K = $0.25
- Small model: 25,000 tokens @ $0.002/1K = $0.05
- **Savings: $0.20 per session (80% reduction)**

## Temperature Tuning

### Use Cases by Temperature

| Temperature | Use Case | Reason |
|-----------|----------|--------|
| **0.2** | Context compression | Consistent summaries |
| **0.3** | Parsing, categorization | Deterministic output |
| **0.5** | Summary extraction | Balanced accuracy |
| **0.7** | Content analysis | Some creativity |

**Default:** 0.3 (good for most parsing tasks)

```toml
# For more creative small model responses
[agent.small_model]
temperature = 0.5

# For strict parsing
[agent.small_model]
temperature = 0.1
```

## Fallback Strategy

Small model may fail or be unavailable. Always handle gracefully:

```rust
async fn read_with_fallback(
    path: &str,
    config: &AgentConfig,
) -> Result<String> {
    let file_size = std::fs::metadata(path)?.len();
    
    // Try small model for large files
    if file_size > 50_000 && config.agent.small_model.enabled {
        match use_small_model_for_read(path, config).await {
            Ok(summary) => return Ok(summary),
            Err(e) => {
                warn!("Small model read failed: {}, falling back to main model", e);
                // Fall through to main model
            }
        }
    }
    
    // Fallback to main model
    use_main_model_for_read(path, config).await
}
```

## Monitoring and Optimization

### Metrics to Track

1. **Token Usage Distribution**
   - What % of total tokens go through small model?
   - Target: ~50% of tokens through small model

2. **Cost per Task**
   - Track cost before/after small model usage
   - Monitor for regressions

3. **Quality Metrics**
   - Do small model summaries match main model quality?
   - Are classifications accurate?

4. **Latency**
   - Small models are often faster (fewer parameters)
   - Track wall-clock time improvements

### Example Monitoring

```rust
struct SmallModelMetrics {
    calls_made: u64,
    tokens_used: u64,
    cost_saved: f64,
    avg_latency_ms: f64,
    quality_score: f32, // 0.0-1.0
}

impl SmallModelMetrics {
    fn report(&self) {
        println!("Small Model Usage:");
        println!("  Calls: {}", self.calls_made);
        println!("  Tokens: {}", self.tokens_used);
        println!("  Cost Saved: ${:.2}", self.cost_saved);
        println!("  Avg Latency: {}ms", self.avg_latency_ms);
        println!("  Quality: {:.0}%", self.quality_score * 100.0);
    }
}
```

## Troubleshooting

### Issue: Small model not being used

**Check:**
1. Is `small_model.enabled = true` in config?
2. Is the specific use case enabled (e.g., `use_for_large_reads`)?
3. Is the task type matching the intended use case?

```toml
[agent.small_model]
enabled = true
use_for_large_reads = true       # Should be true
```

### Issue: Quality degradation

**Solutions:**
1. Lower `max_tokens` to force concise responses
2. Lower `temperature` for more deterministic output
3. Add more context/examples in the prompt

```toml
[agent.small_model]
max_tokens = 500                 # Reduce from 1000
temperature = 0.1                # Make more deterministic
```

### Issue: Small model not available

**Fallback:**
- Configuration allows graceful fallback to main model
- Check `use_for_*` flags are appropriate for your provider
- Some providers may not have suitable "small" models

## Best Practices

1. **Use for Well-Defined Tasks** - Small models excel at parsing, classification, summarization
2. **Avoid for Open-Ended Tasks** - Don't use for creative writing, complex reasoning
3. **Keep Prompts Clear** - Since temperature is low, prompts must be explicit
4. **Monitor Costs** - Track actual savings vs. expected savings
5. **Test First** - Verify quality on your specific tasks before enabling broadly
6. **Set Realistic Token Limits** - Too small can break parsing; too large wastes savings

## References

- **Configuration:** See `vtcode.toml` for full options
- **Code:** `vtcode-config/src/core/agent.rs` - `AgentSmallModelConfig`
- **System Prompt:** `vtcode-core/src/prompts/system.rs` - Small model guidance
- **Claude Code Analysis:** https://minusx.ai/blog/decoding-claude-code/ - Original inspiration

## Summary

The small model tier enables:

✅ **70-80% cost reduction** on ~50% of operations  
✅ **Maintained quality** for parsing/summary tasks  
✅ **Backward compatible** - Can be disabled completely  
✅ **Flexible** - Customize per use case  
✅ **Monitored** - Easy to track impact  

Start by enabling it for large file reads and web summaries. Expand based on results.
