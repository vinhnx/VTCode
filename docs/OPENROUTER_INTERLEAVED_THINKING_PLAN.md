# OpenRouter Interleaved Thinking Implementation Plan

## Overview

This document outlines the implementation plan for enhanced support of OpenRouter's **Interleaved Thinking** feature, which allows models to reason between tool calls for more sophisticated multi-step workflows.

## Feature Description

**Interleaved Thinking** enables models to:

-   Reason about tool results before deciding the next action
-   Chain multiple tool calls with reasoning steps in between
-   Make nuanced decisions based on intermediate results
-   Provide transparent reasoning for tool selection

**Example Pattern**:

```
User: "Research the environmental impact of electric vehicles"

Assistant (Interleaved Thinking):
1. Reasoning: "I need to research EV environmental impact. Let me start with academic papers..."
2. Tool Call: search_academic_papers({"query": "electric vehicle lifecycle", ...})
3. [Tool Result: Papers showing mixed results on manufacturing]
4. Reasoning: "The papers show mixed results. I need current statistics..."
5. Tool Call: get_latest_statistics({"topic": "EV carbon footprint", ...})
6. [Tool Result: Latest statistics]
7. Reasoning: "Now I have both research and data. Let me search for manufacturing specifics..."
8. Tool Call: search_academic_papers({"query": "EV battery manufacturing", ...})
9. Final Analysis: [Comprehensive synthesis of all gathered information]
```

**Key Insight**: The model reasons between each tool call, not just before or after all tool calls.

---

## Current Implementation Analysis

### What Works Today

The current implementation handles:

```rust
// ✅ Reasoning extraction from responses
if let Some(reasoning_value) = message.get("reasoning") {
    // Process reasoning
}

// ✅ Tool calls extraction
if let Some(tool_calls) = message.get("tool_calls") {
    // Process tool calls
}

// ✅ Content array parsing
process_content_value(content_value, &mut aggregated_content, ...)
```

### What's Missing

1. **No preservation of reasoning-tool call ordering** within a single assistant message
2. **No structured representation** of interleaved thinking sequences
3. **No telemetry** for interleaved thinking patterns
4. **No high-level API** for managing interleaved thinking workflows

---

## Proposed Changes

### Phase 1: Data Structure Enhancement

#### 1.1 Add InterleavedContent Type

**File**: `vtcode-core/src/llm/provider.rs`

```rust
/// Represents a sequence of content that may include interleaved reasoning and tool calls
#[derive(Debug, Clone, PartialEq)]
pub enum InterleavedContent {
    /// Regular text content
    Text(String),
    /// Reasoning/thinking content
    Reasoning(String),
    /// A tool call request
    ToolCall(ToolCall),
}

/// A message with support for interleaved thinking
#[derive(Debug, Clone)]
pub struct InterleavedMessage {
    pub role: MessageRole,
    /// Ordered sequence of content, reasoning, and tool calls
    pub content_sequence: Vec<InterleavedContent>,
    pub tool_call_id: Option<String>,
}

impl InterleavedMessage {
    /// Convert to a flat Message (current format)
    pub fn flatten(&self) -> Message {
        let mut text_parts = Vec::new();
        let mut reasoning_parts = Vec::new();
        let mut tool_calls = Vec::new();

        for item in &self.content_sequence {
            match item {
                InterleavedContent::Text(t) => text_parts.push(t.clone()),
                InterleavedContent::Reasoning(r) => reasoning_parts.push(r.clone()),
                InterleavedContent::ToolCall(tc) => tool_calls.push(tc.clone()),
            }
        }

        Message {
            role: self.role.clone(),
            content: MessageContent::Text(text_parts.join("\n")),
            reasoning: if reasoning_parts.is_empty() {
                None
            } else {
                Some(reasoning_parts.join("\n"))
            },
            reasoning_details: None,
            tool_calls: if tool_calls.is_empty() { None } else { Some(tool_calls) },
            tool_call_id: self.tool_call_id.clone(),
        }
    }

    /// Check if this message exhibits interleaved thinking pattern
    pub fn has_interleaved_thinking(&self) -> bool {
        let mut has_reasoning = false;
        let mut has_tool_call = false;
        let mut transitions = 0;
        let mut last_was_reasoning = false;

        for item in &self.content_sequence {
            match item {
                InterleavedContent::Reasoning(_) => {
                    has_reasoning = true;
                    if has_tool_call && !last_was_reasoning {
                        transitions += 1;
                    }
                    last_was_reasoning = true;
                }
                InterleavedContent::ToolCall(_) => {
                    has_tool_call = true;
                    if last_was_reasoning {
                        transitions += 1;
                    }
                    last_was_reasoning = false;
                }
                _ => {
                    last_was_reasoning = false;
                }
            }
        }

        // Interleaved thinking requires reasoning, tool calls, and transitions
        has_reasoning && has_tool_call && transitions >= 2
    }
}
```

#### 1.2 Extend LLMResponse

**File**: `vtcode-core/src/llm/provider.rs`

```rust
#[derive(Debug, Clone)]
pub struct LLMResponse {
    pub content: Option<String>,
    pub tool_calls: Option<Vec<ToolCall>>,
    pub usage: Option<Usage>,
    pub finish_reason: FinishReason,
    pub reasoning: Option<String>,
    pub reasoning_details: Option<Vec<Value>>,

    // NEW: Interleaved thinking support
    /// If present, contains the ordered sequence of reasoning and tool calls
    pub interleaved_content: Option<Vec<InterleavedContent>>,
}

impl LLMResponse {
    /// Check if this response uses interleaved thinking
    pub fn uses_interleaved_thinking(&self) -> bool {
        self.interleaved_content
            .as_ref()
            .map(|seq| {
                let has_reasoning = seq.iter().any(|c| matches!(c, InterleavedContent::Reasoning(_)));
                let has_tools = seq.iter().any(|c| matches!(c, InterleavedContent::ToolCall(_)));
                has_reasoning && has_tools
            })
            .unwrap_or(false)
    }

    /// Get metrics about interleaved thinking
    pub fn interleaved_thinking_metrics(&self) -> Option<InterleavedThinkingMetrics> {
        let seq = self.interleaved_content.as_ref()?;

        let reasoning_segments = seq.iter()
            .filter(|c| matches!(c, InterleavedContent::Reasoning(_)))
            .count();
        let tool_calls = seq.iter()
            .filter(|c| matches!(c, InterleavedContent::ToolCall(_)))
            .count();

        // Estimate reasoning tokens (rough approximation)
        let reasoning_tokens: usize = seq.iter()
            .filter_map(|c| match c {
                InterleavedContent::Reasoning(r) => Some(r.split_whitespace().count()),
                _ => None,
            })
            .sum();

        Some(InterleavedThinkingMetrics {
            reasoning_segments,
            tool_calls,
            estimated_reasoning_tokens: reasoning_tokens as u32,
            total_transitions: Self::count_transitions(seq),
        })
    }

    fn count_transitions(seq: &[InterleavedContent]) -> usize {
        let mut transitions = 0;
        let mut last_type = None;

        for item in seq {
            let current_type = match item {
                InterleavedContent::Reasoning(_) => "reasoning",
                InterleavedContent::ToolCall(_) => "tool",
                InterleavedContent::Text(_) => "text",
            };

            if let Some(last) = last_type {
                if last != current_type {
                    transitions += 1;
                }
            }
            last_type = Some(current_type);
        }

        transitions
    }
}

#[derive(Debug, Clone)]
pub struct InterleavedThinkingMetrics {
    pub reasoning_segments: usize,
    pub tool_calls: usize,
    pub estimated_reasoning_tokens: u32,
    pub total_transitions: usize,
}
```

---

### Phase 2: OpenRouter Provider Enhancement

#### 2.1 Update Content Processing

**File**: `vtcode-core/src/llm/providers/openrouter.rs`

```rust
/// Enhanced content processing that preserves ordering
fn process_content_with_interleaving(
    content_value: &Value,
) -> Vec<InterleavedContent> {
    let mut sequence = Vec::new();

    match content_value {
        Value::Array(parts) => {
            for part in parts {
                if let Some(map) = part.as_object() {
                    if let Some(content_type) = map.get("type").and_then(|v| v.as_str()) {
                        match content_type {
                            "reasoning" | "thinking" | "analysis" => {
                                if let Some(text) = map.get("text").and_then(|v| v.as_str()) {
                                    sequence.push(InterleavedContent::Reasoning(text.to_string()));
                                }
                            }
                            "tool_call" => {
                                if let Some(tool_call) = parse_tool_call_from_content(map) {
                                    sequence.push(InterleavedContent::ToolCall(tool_call));
                                }
                            }
                            "text" | "output_text" => {
                                if let Some(text) = map.get("text").and_then(|v| v.as_str()) {
                                    sequence.push(InterleavedContent::Text(text.to_string()));
                                }
                            }
                            _ => {}
                        }
                    }
                } else if let Some(text) = part.as_str() {
                    sequence.push(InterleavedContent::Text(text.to_string()));
                }
            }
        }
        Value::String(text) => {
            sequence.push(InterleavedContent::Text(text.clone()));
        }
        _ => {}
    }

    sequence
}

fn parse_tool_call_from_content(map: &Map<String, Value>) -> Option<ToolCall> {
    let id = map.get("id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())?;

    let name = map.get("name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())?;

    let arguments = map.get("arguments")
        .map(|v| {
            if let Some(s) = v.as_str() {
                s.to_string()
            } else {
                v.to_string()
            }
        })
        .unwrap_or_else(|| "{}".to_string());

    Some(ToolCall::function(id, name, arguments))
}
```

#### 2.2 Update Response Parsing

**File**: `vtcode-core/src/llm/providers/openrouter.rs`

```rust
fn parse_openrouter_response(&self, response_json: Value) -> Result<LLMResponse, LLMError> {
    // ... existing code ...

    // NEW: Extract interleaved content if present
    let interleaved_content = if let Some(content_value) = message.get("content") {
        if content_value.is_array() {
            let seq = process_content_with_interleaving(content_value);
            if !seq.is_empty() {
                Some(seq)
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    // ... rest of response construction ...

    Ok(LLMResponse {
        content,
        tool_calls,
        usage,
        finish_reason,
        reasoning,
        reasoning_details,
        interleaved_content, // NEW
    })
}
```

#### 2.3 Update Streaming Support

**File**: `vtcode-core/src/llm/providers/openrouter.rs`

```rust
async fn stream(&self, request: LLMRequest) -> Result<LLMStream, LLMError> {
    let response = self.send_with_tool_fallback(&request, Some(true)).await?;

    let stream = try_stream! {
        let mut body_stream = response.bytes_stream();
        let mut buffer = String::new();
        let mut aggregated_content = String::new();
        let mut tool_call_builders: Vec<ToolCallBuilder> = Vec::new();
        let mut reasoning = ReasoningBuffer::default();
        let mut usage: Option<Usage> = None;
        let mut finish_reason = FinishReason::Stop;
        let mut done = false;
        let telemetry = OpenRouterStreamTelemetry::default();

        // NEW: Track interleaved sequence
        let mut interleaved_sequence: Vec<InterleavedContent> = Vec::new();
        let mut last_content_type: Option<&str> = None;

        while let Some(chunk_result) = body_stream.next().await {
            // ... existing streaming logic ...

            // NEW: When we emit content/reasoning, also track in sequence
            for fragment in delta.into_fragments() {
                match fragment {
                    StreamFragment::Content(text) if !text.is_empty() => {
                        // Track transition
                        if last_content_type != Some("text") {
                            interleaved_sequence.push(InterleavedContent::Text(String::new()));
                            last_content_type = Some("text");
                        }
                        // Append to last text item
                        if let Some(InterleavedContent::Text(ref mut t)) = interleaved_sequence.last_mut() {
                            t.push_str(&text);
                        }

                        yield LLMStreamEvent::Token { delta: text };
                    }
                    StreamFragment::Reasoning(text) if !text.is_empty() => {
                        // Track transition
                        if last_content_type != Some("reasoning") {
                            interleaved_sequence.push(InterleavedContent::Reasoning(String::new()));
                            last_content_type = Some("reasoning");
                        }
                        // Append to last reasoning item
                        if let Some(InterleavedContent::Reasoning(ref mut r)) = interleaved_sequence.last_mut() {
                            r.push_str(&text);
                        }

                        yield LLMStreamEvent::Reasoning { delta: text };
                    }
                    _ => {}
                }
            }
        }

        // Finalize tool calls and add to sequence
        let finalized_tool_calls = finalize_tool_calls(tool_call_builders);
        if let Some(ref tools) = finalized_tool_calls {
            for tool in tools {
                interleaved_sequence.push(InterleavedContent::ToolCall(tool.clone()));
            }
        }

        let mut response = finalize_stream_response(
            aggregated_content,
            vec![], // tool_call_builders already finalized
            usage,
            finish_reason,
            reasoning,
        );

        // Add interleaved content if any transitions occurred
        if interleaved_sequence.len() > 1 {
            response.interleaved_content = Some(interleaved_sequence);
        }
        response.tool_calls = finalized_tool_calls;

        yield LLMStreamEvent::Completed { response };
    };

    Ok(Box::pin(stream))
}
```

---

### Phase 3: High-Level Agentic Loop API

#### 3.1 AgenticLoopRunner

**File**: `vtcode-core/src/llm/providers/openrouter.rs`

```rust
pub struct AgenticLoopConfig {
    pub max_iterations: usize,
    pub enable_interleaved_thinking: bool,
    pub parallel_tool_calls: bool,
    pub preserve_tools: bool,
}

impl Default for AgenticLoopConfig {
    fn default() -> Self {
        Self {
            max_iterations: 10,
            enable_interleaved_thinking: true,
            parallel_tool_calls: true,
            preserve_tools: true,
        }
    }
}

pub type ToolExecutor = Box<dyn Fn(ToolCall) -> Result<String, LLMError> + Send + Sync>;

impl OpenRouterProvider {
    /// Runs an agentic loop following OpenRouter's best practices
    ///
    /// This implements the pattern shown in OpenRouter's documentation:
    /// 1. Send initial request with tools
    /// 2. If model responds with tool_calls, execute them
    /// 3. Add tool results to messages
    /// 4. Repeat until model returns a final answer or max iterations reached
    ///
    /// When `enable_interleaved_thinking` is true, the model can reason
    /// between tool calls for more sophisticated decision-making.
    pub async fn run_agentic_loop<F>(
        &self,
        mut request: LLMRequest,
        config: AgenticLoopConfig,
        tool_executor: F,
    ) -> Result<AgenticLoopResult, LLMError>
    where
        F: Fn(ToolCall) -> Result<String, LLMError> + Send + Sync,
    {
        let original_tools = request.tools.clone();
        let mut iteration_count = 0;
        let mut all_responses = Vec::new();

        // Apply config
        request.parallel_tool_calls = Some(config.parallel_tool_calls);

        while iteration_count < config.max_iterations {
            iteration_count += 1;

            // Ensure tools are preserved (OpenRouter requirement)
            if config.preserve_tools && request.tools.is_none() {
                request.tools = original_tools.clone();
            }

            // Generate response
            let response = self.generate(request.clone()).await?;
            all_responses.push(response.clone());

            // Add assistant message to conversation
            let assistant_message = Message {
                role: MessageRole::Assistant,
                content: MessageContent::Text(response.content.clone().unwrap_or_default()),
                reasoning: response.reasoning.clone(),
                reasoning_details: response.reasoning_details.clone(),
                tool_calls: response.tool_calls.clone(),
                tool_call_id: None,
            };
            request.messages.push(assistant_message);

            // Check if we're done
            if response.tool_calls.is_none() {
                return Ok(AgenticLoopResult {
                    final_response: response,
                    iterations: iteration_count,
                    all_responses,
                    termination_reason: TerminationReason::Completed,
                });
            }

            // Execute tools
            let tool_calls = response.tool_calls.unwrap();
            for tool_call in tool_calls {
                let result = tool_executor(tool_call.clone())?;

                let tool_message = Message {
                    role: MessageRole::Tool,
                    content: MessageContent::Text(result),
                    reasoning: None,
                    reasoning_details: None,
                    tool_calls: None,
                    tool_call_id: Some(tool_call.id),
                };
                request.messages.push(tool_message);
            }
        }

        Err(LLMError::Provider(format!(
            "Agentic loop exceeded maximum iterations ({})",
            config.max_iterations
        )))
    }
}

#[derive(Debug, Clone)]
pub struct AgenticLoopResult {
    pub final_response: LLMResponse,
    pub iterations: usize,
    pub all_responses: Vec<LLMResponse>,
    pub termination_reason: TerminationReason,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TerminationReason {
    Completed,
    MaxIterationsReached,
    Error(String),
}

impl AgenticLoopResult {
    /// Get total interleaved thinking metrics across all iterations
    pub fn total_interleaved_metrics(&self) -> InterleavedThinkingMetrics {
        let mut total_reasoning_segments = 0;
        let mut total_tool_calls = 0;
        let mut total_reasoning_tokens = 0;
        let mut total_transitions = 0;

        for response in &self.all_responses {
            if let Some(metrics) = response.interleaved_thinking_metrics() {
                total_reasoning_segments += metrics.reasoning_segments;
                total_tool_calls += metrics.tool_calls;
                total_reasoning_tokens += metrics.estimated_reasoning_tokens;
                total_transitions += metrics.total_transitions;
            }
        }

        InterleavedThinkingMetrics {
            reasoning_segments: total_reasoning_segments,
            tool_calls: total_tool_calls,
            estimated_reasoning_tokens: total_reasoning_tokens,
            total_transitions: total_transitions,
        }
    }
}
```

---

### Phase 4: Testing

#### 4.1 Unit Tests

**File**: `vtcode-core/src/llm/providers/openrouter.rs` (test module)

```rust
#[cfg(test)]
mod interleaved_thinking_tests {
    use super::*;

    #[test]
    fn test_interleaved_content_detection() {
        let msg = InterleavedMessage {
            role: MessageRole::Assistant,
            content_sequence: vec![
                InterleavedContent::Reasoning("Let me search...".to_string()),
                InterleavedContent::ToolCall(ToolCall::function(
                    "1".to_string(),
                    "search".to_string(),
                    "{}".to_string(),
                )),
                InterleavedContent::Reasoning("Based on results...".to_string()),
                InterleavedContent::ToolCall(ToolCall::function(
                    "2".to_string(),
                    "analyze".to_string(),
                    "{}".to_string(),
                )),
            ],
            tool_call_id: None,
        };

        assert!(msg.has_interleaved_thinking());
    }

    #[test]
    fn test_not_interleaved() {
        let msg = InterleavedMessage {
            role: MessageRole::Assistant,
            content_sequence: vec![
                InterleavedContent::Reasoning("Thinking...".to_string()),
                InterleavedContent::Text("Answer".to_string()),
            ],
            tool_call_id: None,
        };

        assert!(!msg.has_interleaved_thinking());
    }

    #[test]
    fn test_interleaved_metrics() {
        let response = LLMResponse {
            content: Some("Final answer".to_string()),
            tool_calls: None,
            usage: None,
            finish_reason: FinishReason::Stop,
            reasoning: None,
            reasoning_details: None,
            interleaved_content: Some(vec![
                InterleavedContent::Reasoning("Let me search for papers".to_string()),
                InterleavedContent::ToolCall(ToolCall::function("1".to_string(), "search".to_string(), "{}".to_string())),
                InterleavedContent::Reasoning("Based on the papers I found".to_string()),
                InterleavedContent::ToolCall(ToolCall::function("2".to_string(), "analyze".to_string(), "{}".to_string())),
                InterleavedContent::Text("Final analysis".to_string()),
            ]),
        };

        let metrics = response.interleaved_thinking_metrics().unwrap();
        assert_eq!(metrics.reasoning_segments, 2);
        assert_eq!(metrics.tool_calls, 2);
        assert!(metrics.total_transitions >= 3); // reasoning→tool→reasoning→tool→text
    }
}
```

#### 4.2 Integration Test

**File**: `vtcode-core/tests/openrouter_interleaved_thinking.rs`

```rust
#[tokio::test]
#[ignore] // Requires API key
async fn test_interleaved_thinking_workflow() {
    let api_key = std::env::var("OPENROUTER_API_KEY").expect("OPENROUTER_API_KEY not set");
    let provider = OpenRouterProvider::with_model(
        api_key,
        "anthropic/claude-3.5-sonnet".to_string(),
    );

    let tools = vec![
        ToolDefinition::function(
            "search_papers".to_string(),
            "Search for academic papers".to_string(),
            json!({"type": "object", "properties": {}}),
        ),
        ToolDefinition::function(
            "get_statistics".to_string(),
            "Get latest statistics".to_string(),
            json!({"type": "object", "properties": {}}),
        ),
    ];

    let request = LLMRequest {
        messages: vec![Message::user(
            "Research the environmental impact of electric vehicles".to_string(),
        )],
        system_prompt: None,
        tools: Some(tools.clone()),
        model: "anthropic/claude-3.5-sonnet".to_string(),
        max_tokens: Some(4000),
        temperature: None,
        stream: false,
        tool_choice: None,
        parallel_tool_calls: Some(false), // Sequential for clear interleaving
        parallel_tool_config: None,
        reasoning_effort: None,
    };

    let config = AgenticLoopConfig {
        max_iterations: 5,
        enable_interleaved_thinking: true,
        parallel_tool_calls: false,
        preserve_tools: true,
    };

    let tool_executor = |tool_call: ToolCall| -> Result<String, LLMError> {
        // Mock tool execution
        Ok(format!("Result for {}: mock data", tool_call.function.name))
    };

    let result = provider
        .run_agentic_loop(request, config, tool_executor)
        .await
        .expect("Agentic loop failed");

    println!("Completed in {} iterations", result.iterations);

    let metrics = result.total_interleaved_metrics();
    println!("Interleaved thinking metrics: {:#?}", metrics);

    // Verify interleaved thinking occurred
    assert!(result.iterations > 1, "Should have multiple iterations");
    assert!(metrics.reasoning_segments > 0, "Should have reasoning");
    assert!(metrics.tool_calls > 0, "Should have tool calls");
}
```

---

### Phase 5: Documentation

#### 5.1 User Guide

**File**: `docs/OPENROUTER_INTERLEAVED_THINKING_GUIDE.md`

Create comprehensive guide with:

-   What is interleaved thinking
-   When to use it
-   Cost implications
-   Code examples
-   Best practices

#### 5.2 API Documentation

Add rustdoc comments to all new types and functions explaining:

-   Purpose
-   Usage
-   Examples
-   Performance considerations

---

## Migration Strategy

### Backwards Compatibility

All changes are **100% backwards compatible**:

1. New fields are `Option<T>` types
2. Existing `Message` and `LLMResponse` work unchanged
3. New `InterleavedContent` is opt-in
4. Agentic loop API is new (doesn't replace anything)

### Gradual Adoption

Users can adopt incrementally:

1. Continue using existing API (no changes needed)
2. Start checking `interleaved_content` in responses
3. Use `interleaved_thinking_metrics()` for monitoring
4. Switch to `run_agentic_loop()` when ready

---

## Implementation Timeline

### Week 1: Core Data Structures

-   [ ] Add `InterleavedContent` type
-   [ ] Extend `LLMResponse` with `interleaved_content` field
-   [ ] Add `interleaved_thinking_metrics()` method
-   [ ] Unit tests for new types

### Week 2: OpenRouter Provider Updates

-   [ ] Update `process_content_with_interleaving()`
-   [ ] Update `parse_openrouter_response()`
-   [ ] Update streaming support
-   [ ] Provider-level tests

### Week 3: Agentic Loop API

-   [ ] Implement `AgenticLoopConfig`
-   [ ] Implement `run_agentic_loop()`
-   [ ] Implement `AgenticLoopResult` with metrics
-   [ ] Integration tests

### Week 4: Documentation & Polish

-   [ ] Write user guide
-   [ ] Add rustdoc comments
-   [ ] Add examples
-   [ ] Performance testing
-   [ ] Final review

---

## Success Criteria

-   [ ] All existing tests pass
-   [ ] New tests added with >90% coverage
-   [ ] Backwards compatibility maintained
-   [ ] Documentation complete
-   [ ] Performance impact <5% for non-interleaved cases
-   [ ] Successfully demonstrates multi-step reasoning with real OpenRouter API
-   [ ] Clear metrics for token cost tracking

---

## Future Enhancements

### Phase 2 (Future)

-   **Visual representation** of interleaved thinking sequences in TUI
-   **Cost estimation** before running agentic loops
-   **Automatic loop optimization** based on metrics
-   **Export interleaved thinking** to structured formats (JSON, Markdown)

### Phase 3 (Future)

-   **Learning from patterns**: Suggest when to use interleaved thinking
-   **Custom reasoning strategies**: Allow users to define reasoning patterns
-   **Multi-provider support**: Extend to other providers that support similar patterns

---

## References

-   [OpenRouter Tool Calling Documentation](https://openrouter.ai/docs/features/tool-calling#interleaved-thinking)
-   Current implementation: `vtcode-core/src/llm/providers/openrouter.rs`
-   Review document: `docs/OPENROUTER_API_REVIEW.md`
