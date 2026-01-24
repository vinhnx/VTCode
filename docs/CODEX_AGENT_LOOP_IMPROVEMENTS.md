# VT Code Improvements Based on OpenAI Codex Agent Loop

**Source**: [Unrolling the Codex Agent Loop](https://openai.com/index/unrolling-the-codex-agent-loop/) (January 2026)

## Executive Summary

This document outlines improvements to VT Code based on learnings from the OpenAI Codex CLI architecture. The Codex agent loop provides battle-tested patterns for context management, timeout enforcement, and execution safety.

---

## Key Patterns to Implement

### 1. Deterministic Execution Boundaries (HIGH PRIORITY)

**Codex Pattern**: Always enforce timeout with fallback defaults and hard maximum.

```rust
// Current VT Code: TimeoutsConfig exists but isn't enforced consistently
// Proposed: Add deterministic timeout resolution

/// Resolve timeout with mandatory bounds (never returns 0 or unbounded)
pub fn resolve_timeout(user_timeout: Option<u64>) -> u64 {
    const DEFAULT_TIMEOUT_SECS: u64 = 600;  // 10 minutes
    const MAX_TIMEOUT_SECS: u64 = 3600;     // 1 hour
    
    match user_timeout {
        None => DEFAULT_TIMEOUT_SECS,
        Some(0) => DEFAULT_TIMEOUT_SECS,  // Invalid → default
        Some(t) if t > MAX_TIMEOUT_SECS => MAX_TIMEOUT_SECS,
        Some(t) => t,
    }
}
```

**Files to modify**:
- `vtcode-config/src/timeouts.rs` - Add `resolve_timeout()` function
- `src/agent/runloop/unified/driver.rs` - Wrap agent loop with `tokio::time::timeout`
- `vtcode-tools/src/executor.rs` - Apply timeout to all tool executions

### 2. Context Window Compaction (HIGH PRIORITY)

**Codex Pattern**: Replace conversation history with summarized version when nearing limit.

**Current State**: VT Code has `TokenBudgetStatus` but no actual compaction.

**Implementation**:

```rust
// Add to context_manager.rs

/// Compaction configuration
pub struct CompactionConfig {
    /// Threshold (0-1) at which to trigger compaction
    pub trigger_threshold: f64,  // 0.85 = 85%
    /// Target size after compaction (0-1)
    pub target_threshold: f64,   // 0.50 = 50%
    /// Summarization prompt template
    pub summary_prompt: String,
}

impl ContextManager {
    /// Compact conversation history when approaching context limit
    /// Returns new compressed history that fits within target threshold
    pub async fn compact_history(
        &mut self,
        history: &[Message],
        llm_client: &dyn LlmClient,
    ) -> Result<Vec<Message>> {
        // 1. Calculate current usage vs threshold
        let usage_ratio = self.get_usage_ratio();
        if usage_ratio < self.compaction_config.trigger_threshold {
            return Ok(history.to_vec());
        }
        
        // 2. Build summarization prompt
        let summary_prompt = format!(
            "{}\n\nConversation to summarize:\n{}",
            COMPACTION_SYSTEM_PROMPT,
            self.format_history_for_summary(history)
        );
        
        // 3. Query LLM for summary
        let summary = llm_client.complete(&summary_prompt).await?;
        
        // 4. Build new history with summary as first message
        let mut new_history = Vec::new();
        new_history.push(Message::system(format!(
            "Previous conversation summary:\n{}", 
            summary
        )));
        
        // 5. Keep last N messages for recency
        let keep_count = 10;
        if history.len() > keep_count {
            new_history.extend(history[history.len() - keep_count..].to_vec());
        }
        
        Ok(new_history)
    }
}
```

**Files to create/modify**:
- `vtcode-core/src/compaction/mod.rs` - New module for compaction logic
- `vtcode-core/src/compaction/summarizer.rs` - LLM-based summarization
- `src/agent/runloop/unified/context_manager.rs` - Integrate compaction

### 3. Multi-Level Output Truncation (MEDIUM PRIORITY)

**Codex Pattern**: Three independent limits to prevent different classes of OOM.

```rust
// Add to vtcode-config/src/constants.rs

pub mod output_limits {
    /// Maximum size for agent messages (primary reasoning output)
    pub const MAX_AGENT_MESSAGES_SIZE: usize = 10 * 1024 * 1024; // 10 MB
    
    /// Maximum size for all messages (full history)
    pub const MAX_ALL_MESSAGES_SIZE: usize = 50 * 1024 * 1024; // 50 MB
    
    /// Maximum size per line (prevent OOM on malformed output)
    pub const MAX_LINE_LENGTH: usize = 1024 * 1024; // 1 MB
    
    /// Default message count limit
    pub const DEFAULT_MESSAGE_LIMIT: usize = 10_000;
    
    /// Maximum message count limit
    pub const MAX_MESSAGE_LIMIT: usize = 50_000;
}
```

**Lazy Truncation Pattern**:
```rust
/// Mark truncated but continue draining stream (prevents pipe blocking)
fn collect_with_truncation(
    output: &mut String,
    new_content: &str,
    max_size: usize,
    truncated: &mut bool,
) {
    let new_size = output.len() + new_content.len();
    
    if new_size > max_size {
        if !*truncated {
            output.push_str("\n[... content truncated due to size limit ...]");
            *truncated = true;
        }
        // Continue draining but don't accumulate
        return;
    }
    
    output.push_str(new_content);
}
```

### 4. Session-Based Conversation Persistence (MEDIUM PRIORITY)

**Codex Pattern**: Store `thread_id` for multi-turn resumption.

```rust
// Add to vtcode-core/src/session/mod.rs

/// Session identifier for conversation persistence
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionId(pub String);

impl SessionId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }
    
    pub fn from_string(s: impl Into<String>) -> Self {
        Self(s.into())
    }
}

/// Session state that can be persisted and resumed
#[derive(Debug, Serialize, Deserialize)]
pub struct SessionState {
    pub id: SessionId,
    pub created_at: DateTime<Utc>,
    pub last_updated: DateTime<Utc>,
    /// Compacted conversation history
    pub history: Vec<Message>,
    /// Loaded skills
    pub active_skills: Vec<String>,
    /// Working directory
    pub working_dir: PathBuf,
}

impl SessionState {
    /// Save session to disk
    pub fn save(&self, path: &Path) -> Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }
    
    /// Load session from disk
    pub fn load(path: &Path) -> Result<Self> {
        let json = std::fs::read_to_string(path)?;
        Ok(serde_json::from_str(&json)?)
    }
}
```

### 5. Prompt Caching via Deterministic Prefix (HIGH PRIORITY)

**Codex Pattern**: Keep system prompt + tools in stable order for cache hits.

> "When we get cache hits, sampling the model is linear rather than quadratic."

**Current Issue**: VT Code may vary prompt order, reducing cache hits.

**Implementation**:
```rust
// Add to vtcode-core/src/prompts/cache_aware.rs

/// Build prompt with cache-optimized ordering
/// Order: 1. System prompt, 2. Tools (sorted), 3. Instructions, 4. History
pub fn build_cache_aware_prompt(
    system_prompt: &str,
    tools: &[ToolDefinition],
    instructions: &str,
    history: &[Message],
) -> Prompt {
    // Sort tools by name for deterministic ordering
    let mut sorted_tools = tools.to_vec();
    sorted_tools.sort_by(|a, b| a.name.cmp(&b.name));
    
    Prompt {
        system: system_prompt.to_string(),
        tools: sorted_tools,
        instructions: instructions.to_string(),
        messages: history.to_vec(),
    }
}
```

### 6. Bounded Line Reading for Streaming Output (LOW PRIORITY)

**Codex Pattern**: Size-limited line reading prevents memory spikes.

```rust
// Add to vtcode-bash-runner/src/stream.rs

/// Read line with size limit, preventing OOM on malformed output
pub async fn read_line_with_limit<R: AsyncBufReadExt + Unpin>(
    reader: &mut R,
    buf: &mut Vec<u8>,
    max_len: usize,
) -> io::Result<ReadLineResult> {
    buf.clear();
    let mut total_read = 0;
    let mut truncated = false;
    
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            return Ok(ReadLineResult::Eof);
        }
        
        // Find newline
        if let Some(pos) = available.iter().position(|&b| b == b'\n') {
            let to_read = pos + 1;
            if total_read + to_read <= max_len {
                buf.extend_from_slice(&available[..to_read]);
            } else {
                truncated = true;
            }
            reader.consume(to_read);
            return Ok(if truncated {
                ReadLineResult::Truncated(buf.clone())
            } else {
                ReadLineResult::Line(buf.clone())
            });
        }
        
        // No newline found, accumulate if under limit
        let len = available.len();
        if total_read + len <= max_len {
            buf.extend_from_slice(available);
            total_read += len;
        } else {
            truncated = true;
        }
        reader.consume(len);
    }
}

pub enum ReadLineResult {
    Line(Vec<u8>),
    Truncated(Vec<u8>),
    Eof,
}
```

### 7. Agent Harness Orchestration + Telemetry (HIGH PRIORITY)

**Codex Pattern**: Explicit phase boundaries, structured event stream, and per-turn budgets make the harness predictable under load.

**Current State**: Unified runloop spans multiple modules with implicit phase transitions and mixed logging.

**Implementation**:

- Define a `TurnPhase` state machine and `TurnRunId` / `TurnId` identifiers in the unified runloop context.
- Emit structured events for each phase boundary and tool execution using `vtcode-exec-events`.
- Enforce per-turn budgets (tool calls, wall clock, and tool retries) in the tool pipeline.

**Files to modify**:
- `src/agent/runloop/unified/turn/turn_loop.rs` - Centralize phase transitions and identifiers
- `src/agent/runloop/unified/run_loop_context.rs` - Store run/turn identifiers + budgets
- `src/agent/runloop/unified/tool_pipeline.rs` - Enforce budget and retry policy
- `src/agent/runloop/unified/inline_events/` - Emit structured harness events
- `vtcode-config/src/core/agent.rs` - Add harness config (budgets, retry policy, event output)
- `vtcode-core/src/config/constants.rs` - Defaults for budgets and retry limits

---

## Implementation Priority

| Priority | Feature | Effort | Impact |
|----------|---------|--------|--------|
| 1 | Deterministic Timeout Enforcement | Low | High |
| 2 | Context Window Compaction | High | Very High |
| 3 | Prompt Cache Optimization | Medium | High |
| 4 | Multi-Level Output Truncation | Medium | Medium |
| 5 | Session Persistence | Medium | Medium |
| 6 | Bounded Line Reading | Low | Low |
| 7 | Agent Harness Orchestration + Telemetry | High | High |

---

## Quick Wins (Implement First)

### 1. Add timeout resolution constant

```rust
// vtcode-config/src/constants.rs
pub mod execution {
    pub const DEFAULT_TIMEOUT_SECS: u64 = 600;
    pub const MAX_TIMEOUT_SECS: u64 = 3600;
    pub const MIN_TIMEOUT_SECS: u64 = 10;
}
```

### 2. Wrap agent loop with timeout

```rust
// src/agent/runloop/unified/driver.rs
let timeout = resolve_timeout(config.timeout_secs);
let result = tokio::time::timeout(
    Duration::from_secs(timeout),
    run_agent_loop(ctx)
).await;

match result {
    Ok(inner) => inner,
    Err(_) => Err(anyhow!("Agent loop timed out after {} seconds", timeout)),
}
```

### 3. Add compaction trigger to pre_request_check

```rust
// src/agent/runloop/unified/context_manager.rs
pub(crate) fn pre_request_check(&self, history: &[Message]) -> PreRequestAction {
    // ... existing turn limit checks ...
    
    // Add token-based compaction trigger
    let usage_ratio = self.cached_stats.total_token_usage as f64 
        / self.context_window_size as f64;
    
    if usage_ratio >= TOKEN_BUDGET_CRITICAL_THRESHOLD {
        return PreRequestAction::Compact(
            "Context window at 90%+. Compacting conversation history."
        );
    }
    
    // ... rest of function ...
}
```

---

## Testing Strategy

1. **Timeout Tests**: Verify execution stops at exact timeout boundary
2. **Compaction Tests**: Ensure summary preserves critical context
3. **Truncation Tests**: Verify OOM protection with large outputs
4. **Session Tests**: Round-trip persistence and resumption
5. **Cache Tests**: Measure cache hit rate with deterministic prefix
6. **Harness Tests**: Validate turn phase ordering, budgets, and event log output

## Agent Harness ExecPlan

See `.vtcode/plans/agent-harness-improvements.md` for the execution plan that implements the harness workstream end-to-end.

---

## References

- [Unrolling the Codex Agent Loop](https://openai.com/index/unrolling-the-codex-agent-loop/)
- [OpenAI Codex CLI Repository](https://github.com/openai/codex)
- [OpenAI Prompt Caching Guide](https://platform.openai.com/docs/guides/prompt-caching)
