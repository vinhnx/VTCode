# OpenAI Responses API Implementation Fixes

## Overview
Updated the OpenAI provider's Responses API implementation to correctly align with the official OpenAI Responses API specification (https://platform.openai.com/docs/api-reference/responses).

## Key Changes

### 1. Response Payload Parsing (`parse_responses_payload`)
**File**: `vtcode-core/src/llm/providers/openai.rs`

**What was fixed**:
- Removed fallback to `choices` field - Responses API only uses `output` array
- Fixed content item type parsing to match spec exactly
- Added support for all content item types per spec:
  - `text` / `output_text` - Regular text responses
  - `reasoning` - Thinking/reasoning model outputs
  - `function_call` - Tool/function calls with proper structure
  - `refusal` - Model refusals to respond

**Before**:
```rust
"output_text" | "text" => { ... }
"reasoning" => { ... }
"tool_call" => { ... }  // Wrong field name per spec
```

**After**:
```rust
"text" | "output_text" => { ... }
"reasoning" => { ... }
"function_call" => { ... }  // Correct per spec
"refusal" => { ... }  // Now supported
```

### 2. Tool Call Structure
**What was fixed**:
- Per spec, function calls are in `output` array items with structure:
  ```json
  { "type": "function_call", "id": "...", "function": {"name": "...", "arguments": "..."} }
  ```
- Fixed parsing to correctly extract from nested `function` object

**Before**:
```rust
let (name_value, arguments_value) = if let Some(function) = entry.get("function") { ... }
else { entry.get("name"), entry.get("arguments") }
```

**After**:
```rust
let function_obj = entry.get("function").and_then(|value| value.as_object());
let name = function_obj.and_then(|f| f.get("name").and_then(|n| n.as_str()));
```

### 3. Streaming Event Handling
**File**: `vtcode-core/src/llm/providers/openai.rs` (lines 2961-3050)

**What was fixed**:
- Added comprehensive event type handling per spec
- Improved error event parsing from `response.failed` and `response.incomplete` events
- Added support for additional event types:

| Event Type | Purpose | Action |
|---|---|---|
| `response.output_text.delta` | Text content streaming | Emit Token events |
| `response.refusal.delta` | Model refusal text | Accumulate content |
| `response.reasoning_text.delta` | Thinking/reasoning content | Emit Reasoning events |
| `response.reasoning_summary_text.delta` | Reasoning summary | Emit Reasoning events |
| `response.function_call_arguments.delta` | Tool arguments | Accumulate |
| `response.completed` | Response finished | Set final response |
| `response.failed` / `response.incomplete` | Error states | Parse and return error |

**Before**:
```rust
"response.reasoning_text.delta"
| "response.reasoning_summary_text.delta"
| "response.reasoning_content.delta" => { ... }  // Grouped together
```

**After**:
```rust
"response.reasoning_text.delta" => { ... }
"response.reasoning_summary_text.delta" => { ... }  // Separate handling
"response.function_call_arguments.delta" => { ... }  // New event type support
```

### 4. Response Completion
**What was fixed**:
- Removed incorrect `stop_reason` field lookup (not in Responses API)
- Simplified finish reason determination to check for tool calls
- Per spec: finish reason should be based on content type, not a separate field

**Before**:
```rust
let stop_reason = response_json
    .get("stop_reason")
    .or_else(|| output.iter().find_map(...))
    .unwrap_or("stop");
let finish_reason = match stop_reason { ... }
```

**After**:
```rust
let finish_reason = if !tool_calls_vec.is_empty() {
    FinishReason::ToolCalls
} else {
    FinishReason::Stop
};
```

### 5. Content Joining
**What was fixed**:
- Changed from iterator collection to string joining for proper content aggregation
- Content fragments should be concatenated as strings, not collected as Vec

**Before**:
```rust
Some(content_fragments.into_iter().collect())
```

**After**:
```rust
Some(content_fragments.join(""))
```

## Compliance with Spec

### Responses API Endpoint
- ✅ Uses `/responses` endpoint with `-beta` header
- ✅ Supports streaming with `stream: true` parameter
- ✅ Handles non-streaming responses via `response.completed` event

### Response Object
- ✅ Parses `output` array (message items with `content` array)
- ✅ Extracts `usage` information with token counts
- ✅ Handles `status` field (completed status verified)
- ✅ Supports all content item types

### Streaming Events
- ✅ `response.output_text.delta` - Text streaming
- ✅ `response.reasoning_text.delta` - Reasoning streaming  
- ✅ `response.reasoning_summary_text.delta` - Summary streaming
- ✅ `response.refusal.delta` - Refusal streaming
- ✅ `response.function_call_arguments.delta` - Tool arguments
- ✅ `response.completed` - Completion event
- ✅ `response.failed` / `response.incomplete` - Error handling

### Error Handling
- ✅ Properly parses error messages from `response.failed` / `response.incomplete` events
- ✅ Extracts error details from `error` field in response object

## Testing Recommendations

1. **Streaming responses**: Test with models supporting streaming (GPT-5.1, GPT-5.2)
2. **Reasoning models**: Verify with `o-series` models and reasoning_effort settings
3. **Tool calls**: Test function calling with multiple tools
4. **Error cases**: Verify error event parsing and user-friendly messages
5. **Refusals**: Test safety refusals are properly captured

## Files Modified
- `vtcode-core/src/llm/providers/openai.rs`
  - `parse_responses_payload()` - Complete rewrite to spec
  - Streaming event handler - Enhanced event type support
  - Comments/documentation - Added spec references

## Backward Compatibility
- No breaking changes to public API
- Internal implementation details updated to match spec
- Existing tests should continue to pass with corrected behavior
