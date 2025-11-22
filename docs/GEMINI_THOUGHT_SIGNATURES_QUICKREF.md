# Gemini Thought Signatures - Quick Reference

## What Are Thought Signatures?

Encrypted strings that preserve Gemini's internal reasoning state across function calls. Required by Gemini 3 Pro for proper context continuity.

## Key Points

-   **Automatic**: Handled automatically by VT Code's Gemini provider
-   **Required**: Gemini 3 Pro returns 400 error if signatures missing
-   **Transparent**: No changes needed in tool execution code
-   **Opaque**: Treat as encrypted black-box strings

## How to Use (For Developers)

### Standard Usage (Automatic)

```rust
use vtcode_core::llm::provider::{LLMProvider, LLMRequest, Message};
use vtcode_core::llm::providers::GeminiProvider;

let provider = GeminiProvider::new("api_key".to_string());

let request = LLMRequest {
    messages: vec![Message::user("Check weather in London")],
    tools: Some(vec![weather_tool]),
    // ... other fields
};

// 1. Get response with function call (signature automatically extracted)
let response = provider.generate(request).await?;

if let Some(tool_calls) = response.tool_calls {
    // 2. Execute tools
    let results = execute_tools(tool_calls).await?;

    // 3. Add to history (signatures preserved automatically)
    messages.push(Message::assistant_with_tool_calls(tool_calls));
    messages.push(Message::tool_results(results));

    // 4. Continue conversation (signatures sent back automatically)
    let final_response = provider.generate(LLMRequest {
        messages,
        // ...
    }).await?;
}
```

**That's it!** Thought signatures are handled automatically in the standard conversation flow.

## When to Care About Thought Signatures

### ✅ You DON'T need to worry if:

-   Using standard conversation patterns
-   Using VT Code's Gemini provider
-   Just executing tools and adding to history
-   Working with Gemini 2.x models

### ⚠️ You DO need to be careful if:

-   **Manually constructing messages**: Include full tool call objects
-   **Pruning conversation history**: Keep assistant messages with tool calls intact
-   **Serializing/deserializing**: Preserve `thought_signature` field in JSON
-   **Custom message builders**: Don't strip unknown fields

## Common Mistakes

### ❌ Don't Do This

```rust
// Reconstructing tool calls without signature
let tool_call = ToolCall {
    id: call.id.clone(),
    call_type: "function".to_string(),
    function: Some(call.function.clone()),
    text: None,
    thought_signature: None,  // ← Lost!
};
```

### ✅ Do This Instead

```rust
// Preserve entire tool call object
messages.push(Message::assistant_with_tool_calls(
    response.tool_calls.unwrap()  // Keeps signatures
));
```

## Debugging

### Check if signatures are present

```rust
if let Some(tool_calls) = &message.tool_calls {
    for call in tool_calls {
        if let Some(sig) = &call.thought_signature {
            println!("✓ Has signature: {:.20}...", sig);
        } else {
            println!("✗ Missing signature for call: {}", call.id);
        }
    }
}
```

### Common errors

| Error                            | Cause                        | Solution                               |
| -------------------------------- | ---------------------------- | -------------------------------------- |
| `400: missing thought signature` | Signature lost between turns | Keep full tool call objects in history |
| Performance degradation          | Signatures omitted           | Verify signatures present in requests  |
| Function call looping            | Incorrect signatures         | Don't modify signature strings         |

## Model Compatibility

| Model              | Requirement  | Behavior if Missing       |
| ------------------ | ------------ | ------------------------- |
| Gemini 3 Pro       | **Required** | 400 error                 |
| Gemini 3 Pro Image | Recommended  | Performance degradation   |
| Gemini 2.5/2.0     | Optional     | Slight performance impact |
| Other providers    | Ignored      | No effect                 |

## Data Structure Reference

### ToolCall

```rust
pub struct ToolCall {
    pub id: String,
    pub call_type: String,
    pub function: Option<FunctionCall>,
    pub text: Option<String>,
    pub thought_signature: Option<String>,  // ← Gemini-specific
}
```

### Part (Gemini)

```rust
pub enum Part {
    Text {
        text: String,
        #[serde(rename = "thoughtSignature")]
        thought_signature: Option<String>,
    },
    FunctionCall {
        function_call: FunctionCall,
        #[serde(rename = "thoughtSignature")]
        thought_signature: Option<String>,
    },
    // ...
}
```

## API Examples

### Multi-Step Function Calling

```rust
// Turn 1, Step 1: Initial request
let response1 = provider.generate(LLMRequest {
    messages: vec![Message::user("Book taxi if flight delayed")],
    tools: Some(vec![check_flight_tool, book_taxi_tool]),
    // ...
}).await?;

// Response has tool call with signature
let tool_calls1 = response1.tool_calls.unwrap();
// tool_calls1[0].thought_signature = Some("SIG_A")

// Turn 1, Step 2: Send result
let result1 = execute_tool(&tool_calls1[0]).await?;
messages.push(Message::assistant_with_tool_calls(tool_calls1));  // Keeps SIG_A
messages.push(Message::tool_result(result1));

let response2 = provider.generate(LLMRequest {
    messages: messages.clone(),
    // ...
}).await?;

// Response has second tool call with new signature
let tool_calls2 = response2.tool_calls.unwrap();
// tool_calls2[0].thought_signature = Some("SIG_B")

// Turn 1, Step 3: Send second result
// Must include BOTH signatures in history (SIG_A and SIG_B)
let result2 = execute_tool(&tool_calls2[0]).await?;
messages.push(Message::assistant_with_tool_calls(tool_calls2));  // Keeps SIG_B
messages.push(Message::tool_result(result2));

let final_response = provider.generate(LLMRequest {
    messages,  // Contains both SIG_A and SIG_B
    // ...
}).await?;
```

## Further Reading

-   [Full Documentation](./GEMINI_THOUGHT_SIGNATURES.md)
-   [Implementation Details](./GEMINI_THOUGHT_SIGNATURES_IMPLEMENTATION.md)
-   [Google Cloud Docs](https://cloud.google.com/vertex-ai/generative-ai/docs/thought-signatures)
