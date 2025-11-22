# VTCode Agent Message Chain & Reasoning Improvements

**Date**: November 2025  
**Status**: Completed  
**Impact**: Enhanced reasoning preservation and message chain coherence

---

## Problem Statement

The vtcode agent loop was discarding valuable reasoning information from LLM responses, leading to:
1. **Lost Context**: Reasoning traces from models like Claude 3.5 (extended thinking) were not preserved
2. **Fragmented Conversations**: Message chains lacked depth and internal decision-making visibility
3. **Reduced Traceability**: Downstream tool calls had no explanation of their reasoning
4. **Poor Auditability**: Users couldn't see why the agent chose certain actions

---

## Issues Fixed

### Issue 1: Reasoning Discarded in ToolCalls Path
**File**: `src/agent/runloop/unified/turn/turn_loop.rs` (line 281)

**Before**:
```rust
TurnProcessingResult::ToolCalls {
    tool_calls,
    assistant_text,
    reasoning: _,  // ❌ Reasoning discarded!
} => {
    if !assistant_text.trim().is_empty() {
        working_history.push(uni::Message::assistant(assistant_text));
    }
}
```

**After**:
```rust
TurnProcessingResult::ToolCalls {
    tool_calls,
    assistant_text,
    reasoning,  // ✅ Reasoning preserved!
} => {
    if !assistant_text.trim().is_empty() {
        let msg = uni::Message::assistant(assistant_text);
        let msg_with_reasoning = if let Some(reasoning_text) = reasoning {
            msg.with_reasoning(Some(reasoning_text))  // ✅ Attached to message
        } else {
            msg
        };
        working_history.push(msg_with_reasoning);
    } else if let Some(reasoning_text) = reasoning {
        // ✅ Even if no text, preserve reasoning in empty assistant message
        working_history.push(uni::Message::assistant(String::new())
            .with_reasoning(Some(reasoning_text)));
    }
}
```

**Impact**: Reasoning from tool-calling turns now preserved in message history.

---

### Issue 2: Reasoning Discarded in TextResponse Path
**File**: `src/agent/runloop/unified/turn/turn_loop.rs` (line 557-810)

**Before**:
```rust
TurnProcessingResult::TextResponse { text, reasoning: _ } => {
    // ...
    working_history.push(uni::Message::assistant(text));  // ❌ No reasoning!
}
```

**After**:
```rust
TurnProcessingResult::TextResponse { text, reasoning } => {
    // ...
    let msg = uni::Message::assistant(text);
    let msg_with_reasoning = if let Some(reasoning_text) = reasoning {
        msg.with_reasoning(Some(reasoning_text))
    } else {
        msg
    };
    working_history.push(msg_with_reasoning);  // ✅ Reasoning preserved!
}
```

**Impact**: Final text responses now carry their reasoning context.

---

### Issue 3: System Prompt Lacking Guidance on Message Chains
**File**: `vtcode-core/src/prompts/system.rs`

**Added Section IV: MESSAGE CHAINS & CONVERSATION FLOW**

Key additions:
- Explicit guidance on preserving context across turns
- Progressive refinement pattern (reasoning → search → discovery → action → verification)
- Cache-aware message building to avoid repetition
- Guidance on signaling transitions between stages

**Added Extended Thinking Support Guidance**:
```markdown
### Explicit Reasoning (Extended Thinking)
If the model supports reasoning (Claude 3.5+, GPT-4o with beta), leverage it:
- **Use for Complex Tasks**: When 3+ decision points exist, let the model reason through them.
- **Structure Reasoning**: Break down: problem decomposition → hypotheses → solution selection.
- **In Message Chain**: Reasoning appears as a separate message before actions, 
  giving the agent space to think deeply.
```

**Updated Multi-LLM Section (VI)**:
- Claude 3.5+: Supports extended thinking (reasoning)
- GPT-4o: Supports reasoning effort parameter
- Gemini 2.0+: Supports extended thinking in beta

---

## Technical Implementation

### How Reasoning is Preserved

1. **LLM Response Capture**: `execute_llm_request()` receives `LLMResponse` with optional reasoning
2. **Processing**: `process_llm_response()` extracts reasoning into `TurnProcessingResult`
3. **Message Chain**: `run_turn_loop()` attaches reasoning to assistant messages via `.with_reasoning()`
4. **Archival**: Reasoning stored in `Message.reasoning` field without affecting API payloads

### Message Structure Example

```rust
// Before (reasoning lost):
Message::assistant("Tool X found the issue")
→ No context for future turns

// After (reasoning preserved):
Message::assistant("Tool X found the issue")
    .with_reasoning(Some("Analyzed 3 hypotheses: Y failed because..., Z because..., X matches because..."))
→ Full decision-making context preserved for next turn
```

---

## Benefits

### 1. Enhanced Traceability
- Users can see **why** the agent chose specific tools
- Audit trail includes internal decision-making
- Easier to debug agent behavior

### 2. Better Continuation
- Subsequent turns have full context of prior reasoning
- Agent can reference earlier hypotheses without re-analyzing
- Fewer redundant searches

### 3. Improved Multi-Turn Coherence
- Message chains now tell a complete story (problem → thinking → action → result)
- Extended thinking models (Claude 3.5+) properly leveraged
- Reasoning helps prevent circular loops

### 4. Future-Proofing
- Ready for reasoning-based model features
- Supports structured thinking (ReAct patterns)
- Enables reasoning-aware prompt optimization

---

## Testing Recommendations

### Manual Testing
1. Run vtcode with Claude 3.5+ model in session
2. Observe thinking messages in conversation history
3. Verify reasoning flows through tool calls
4. Check multi-turn coherence: agent references prior reasoning

### Automated Testing
```rust
#[test]
fn test_reasoning_preserved_in_tool_calls() {
    let result = TurnProcessingResult::ToolCalls {
        tool_calls: vec![...],
        assistant_text: "Executing search".to_string(),
        reasoning: Some("Hypothesized X, need to verify with grep".to_string()),
    };
    // Assert that working_history contains message with reasoning
}

#[test]
fn test_reasoning_preserved_in_text_response() {
    let result = TurnProcessingResult::TextResponse {
        text: "Found the issue".to_string(),
        reasoning: Some("Analyzed logs, narrowed to component Y".to_string()),
    };
    // Assert message carries reasoning
}
```

---

## Breaking Changes

**None**. Changes are backward-compatible:
- Models without reasoning support: `reasoning` field is `None`, messages work as before
- Models with reasoning: Reasoning field populated, preserved in history
- Existing conversation formats unchanged

---

## Configuration

No additional configuration required. Automatic behavior:
1. **If model provides reasoning**: Automatically attached to messages
2. **If model doesn't support reasoning**: Gracefully handled with `Option<String>`
3. **Message serialization**: Reasoning stored separately, doesn't affect API payloads

---

## Files Modified

1. **src/agent/runloop/unified/turn/turn_loop.rs**
   - Lines 277-296: ToolCalls reasoning preservation
   - Lines 562-817: TextResponse reasoning preservation

2. **vtcode-core/src/prompts/system.rs**
   - Lines 128-166: New "MESSAGE CHAINS & CONVERSATION FLOW" section
   - Lines 168-181: Updated "MULTI-LLM COMPATIBILITY" section

---

## Related Files (Not Modified, But Relevant)

- `src/agent/runloop/unified/turn/turn_processing.rs`: Already captures reasoning in `TurnProcessingResult`
- `vtcode-core/src/llm/provider.rs`: `Message::with_reasoning()` API used for attachment
- `docs/SYSTEM_PROMPT_*.md`: Reference documentation

---

## Future Enhancements

1. **Reasoning Analytics**: Track reasoning quality per model/task
2. **Structured Reasoning**: Formalize reasoning blocks (problem → hypotheses → solution)
3. **Reasoning Replay**: Ability to inspect agent reasoning in UI
4. **Cost Attribution**: Separate token counting for reasoning vs. action
5. **Reasoning-Guided Search**: Use reasoning to optimize tool choice in next turn

---

## References

- Claude 3.5 Sonnet: Extended thinking documentation
- OpenAI GPT-4o: Reasoning effort parameter
- VTCode System Prompt v4.2: `vtcode-core/src/prompts/system.rs`
