# Open Responses Specification Compliance

VT Code implements the [Open Responses](https://www.openresponses.org/) specification, an open, vendor-neutral standard for large language model APIs. This enables interoperable LLM workflows across different providers.

## Overview

The Open Responses implementation is located in `vtcode-core/src/open_responses/` and provides:

- **Unified Item Types**: State machine-based items with defined lifecycle states
- **Semantic Streaming Events**: Meaningful events (not raw token deltas) for predictable streaming
- **Response Objects**: Standardized structure per the specification
- **Error Handling**: Structured errors with type, code, param, and message fields
- **Extension Points**: Support for VT Code-specific item types and events

## Core Concepts

### Items as State Machines

All items follow a state machine model with defined lifecycle states:

```
┌─────────────┐
│ in_progress │──────────────────────────────────┐
└──────┬──────┘                                  │
       │                                         │
       ▼                                         ▼
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│ incomplete  │     │  completed  │     │   failed    │
└─────────────┘     └─────────────┘     └─────────────┘
  (terminal)          (terminal)          (terminal)
```

```rust
use vtcode_core::ItemStatus;

let status = ItemStatus::InProgress;
assert!(!status.is_terminal());

let completed = ItemStatus::Completed;
assert!(completed.is_terminal());
assert!(completed.is_success());
```

### Semantic Streaming Events

Streaming is modeled as semantic events, not raw text deltas:

| Event Type | Description |
|------------|-------------|
| `response.created` | Initial response creation |
| `response.in_progress` | Response processing started |
| `response.output_item.added` | New output item added |
| `response.output_text.delta` | Text content delta |
| `response.output_text.done` | Text content complete |
| `response.output_item.done` | Output item complete |
| `response.completed` | Response finished successfully |
| `response.failed` | Response failed with error |

```rust
use vtcode_core::{ResponseStreamEvent, Response, VecStreamEmitter, StreamEventEmitter};

let mut emitter = VecStreamEmitter::new();
let response = Response::new("resp_123", "gpt-4");

emitter.response_created(response.clone());
emitter.output_text_delta("resp_123", "item_0", 0, 0, "Hello, ");
emitter.output_text_delta("resp_123", "item_0", 0, 0, "world!");

let events = emitter.into_events();
assert_eq!(events.len(), 3);
```

### Output Item Types

VT Code supports these Open Responses item types:

| Type | Description |
|------|-------------|
| `message` | User/assistant/system messages |
| `reasoning` | Model's internal thought process |
| `function_call` | Tool/function invocation request |
| `function_call_output` | Tool execution result |
| `custom` | VT Code-specific extensions |

```rust
use vtcode_core::{OutputItem, MessageRole, ContentPart, ItemStatus};

// Create an assistant message
let message = OutputItem::message(
    "msg_1",
    MessageRole::Assistant,
    vec![ContentPart::output_text("Hello, world!")],
);

// Create a function call
let function_call = OutputItem::function_call(
    "fc_1",
    "read_file",
    serde_json::json!({"path": "/etc/passwd"}),
);
```

## Response Object Structure

The `Response` object follows the Open Responses specification:

```rust
use vtcode_core::{OpenResponse as Response, ResponseStatus};

let mut response = Response::new("resp_123", "claude-3-sonnet");
assert_eq!(response.status, ResponseStatus::InProgress);
assert_eq!(response.object, "response");

// Mark as completed
response.complete();
assert_eq!(response.status, ResponseStatus::Completed);
assert!(response.completed_at.is_some());
```

Key fields:
- `id`: Unique response identifier
- `object`: Always `"response"`
- `created_at`: Unix timestamp (seconds)
- `completed_at`: Completion timestamp (if applicable)
- `status`: Current response status
- `model`: Model that generated the response
- `output`: Vector of output items
- `usage`: Token usage statistics
- `error`: Error details (if failed)

## Bridging VT Code Events

The `ResponseBuilder` bridges VT Code's internal `ThreadEvent` system to Open Responses:

```rust
use vtcode_core::{ResponseBuilder, VecStreamEmitter, StreamEventEmitter};
use vtcode_exec_events::{ThreadEvent, ThreadStartedEvent, TurnCompletedEvent, Usage};

let mut builder = ResponseBuilder::new("gpt-4");
let mut emitter = VecStreamEmitter::new();

// Process VT Code events
builder.process_event(
    &ThreadEvent::ThreadStarted(ThreadStartedEvent {
        thread_id: "thread_1".to_string(),
    }),
    &mut emitter,
);

// Get the Open Responses response
let response = builder.response();
```

### Event Mapping

| VT Code Event | Open Responses Event |
|---------------|---------------------|
| `ThreadStarted` | `response.created` + `response.in_progress` |
| `TurnCompleted` | `response.completed` |
| `TurnFailed` | `response.failed` |
| `ItemStarted` | `response.output_item.added` |
| `ItemUpdated` | `response.output_text.delta` / `response.reasoning.delta` |
| `ItemCompleted` | `response.output_item.done` |

### Item Type Mapping

| VT Code Item | Open Responses Type |
|--------------|---------------------|
| `AgentMessageItem` | `message` (role: assistant) |
| `ReasoningItem` | `reasoning` |
| `CommandExecutionItem` | `function_call` (name: `vtcode.run_command`) |
| `McpToolCallItem` | `function_call` |
| `FileChangeItem` | `custom` (type: `vtcode:file_change`) |
| `WebSearchItem` | `custom` (type: `vtcode:web_search`) |
| `ErrorItem` | `custom` (type: `vtcode:error`) |

## Error Handling

Structured errors follow the Open Responses specification:

```rust
use vtcode_core::{OpenResponseError, OpenResponseErrorType, OpenResponseErrorCode};

// Create an error with type and message
let error = OpenResponseError::invalid_param("model", "Invalid model ID")
    .with_code(OpenResponseErrorCode::InvalidModel);

assert_eq!(error.error_type, OpenResponseErrorType::InvalidRequest);
assert_eq!(error.param, Some("model".to_string()));
```

Error types:
- `server_error`: Internal server error
- `invalid_request`: Invalid request parameters
- `not_found`: Resource not found
- `model_error`: Model-specific error
- `too_many_requests`: Rate limit exceeded

## Extension Points

VT Code-specific extensions use the `vtcode:` prefix:

```rust
use vtcode_core::open_responses::{CustomItem, is_valid_extension_type};

// Validate extension type naming
assert!(is_valid_extension_type("vtcode:file_change"));
assert!(is_valid_extension_type("acme:search_result"));
assert!(!is_valid_extension_type("file_change")); // Missing prefix

// Create a custom item
let custom = CustomItem::vtcode(
    "custom_1",
    "file_change",
    serde_json::json!({
        "path": "src/main.rs",
        "kind": "update",
    }),
);
assert_eq!(custom.custom_type, "vtcode:file_change");
```

## Usage Statistics

Token usage follows the Open Responses format:

```rust
use vtcode_core::{OpenUsage, InputTokensDetails};

let usage = OpenUsage {
    input_tokens: 1000,
    output_tokens: 200,
    total_tokens: 1200,
    input_tokens_details: Some(InputTokensDetails {
        cached_tokens: Some(500),
        audio_tokens: None,
        text_tokens: None,
    }),
    output_tokens_details: None,
};

// Convert from VT Code's internal usage
use vtcode_exec_events::Usage as ExecUsage;
let exec_usage = ExecUsage {
    input_tokens: 1000,
    cached_input_tokens: 500,
    output_tokens: 200,
};
let open_usage = OpenUsage::from_exec_usage(&exec_usage);
```

## Streaming Flow

The correct streaming event flow per the specification:

```
response.created
  └─> response.in_progress
        └─> response.output_item.added
              └─> response.content_part.added
                    └─> response.output_text.delta (repeated)
                          └─> response.output_text.done
                                └─> response.content_part.done
                                      └─> response.output_item.done
  └─> response.completed (or response.failed)
```

## Configuration

Enable Open Responses in your `vtcode.toml`:

```toml
[agent.open_responses]
# Enable Open Responses specification compliance layer
# Default: false (opt-in feature)
enabled = true

# Emit Open Responses events to the event sink
# (response.created, response.output_item.added, response.output_text.delta, etc.)
emit_events = true

# Include VT Code extension items (vtcode:file_change, vtcode:web_search, etc.)
include_extensions = true

# Map internal tool calls to Open Responses function_call items
map_tool_calls = true

# Include reasoning items in Open Responses output
include_reasoning = true
```

### Configuration Options

| Option | Default | Description |
|--------|---------|-------------|
| `enabled` | `false` | Enable the Open Responses layer (opt-in) |
| `emit_events` | `true` | Emit semantic streaming events |
| `include_extensions` | `true` | Include VT Code-specific extension items |
| `map_tool_calls` | `true` | Map tool calls to `function_call` items |
| `include_reasoning` | `true` | Include reasoning/thinking items |

### Programmatic Integration

```rust
use vtcode_core::{OpenResponsesIntegration, OpenResponsesCallback};
use vtcode_config::OpenResponsesConfig;
use std::sync::{Arc, Mutex};

// Create integration with config
let config = OpenResponsesConfig {
    enabled: true,
    ..Default::default()
};
let mut integration = OpenResponsesIntegration::new(config);

// Set up a callback for events
let callback: OpenResponsesCallback = Arc::new(Mutex::new(Box::new(|event| {
    println!("Open Responses event: {:?}", event.event_type());
})));
integration.set_callback(callback);

// Start a response session
integration.start_response("gpt-4");

// Process VT Code events (automatically converts to Open Responses)
// integration.process_event(&thread_event);

// Get the final response
if let Some(response) = integration.finish_response() {
    println!("Response completed: {}", response.id);
}
```

## References

- [Open Responses Specification](https://www.openresponses.org/specification)
- [Open Responses API Reference](https://www.openresponses.org/reference)
- [Open Responses Governance](https://www.openresponses.org/governance)
