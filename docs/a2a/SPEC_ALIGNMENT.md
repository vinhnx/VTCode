# A2A Protocol Specification Alignment

## Overview

This document tracks the VT Code A2A implementation against the official A2A Protocol specification at https://a2a-protocol.org.

## Specification vs Implementation Gap Analysis

### ✅ IMPLEMENTED

#### Core Data Types

-   ✅ `JSONRPCMessage` - Base structures for requests/responses
-   ✅ `JSONRPCRequest` and `JSONRPCResponse` - Standard JSON-RPC 2.0 format
-   ✅ `JSONRPCError` - Error responses with code, message, data
-   ✅ `Task` - Full task structure with id, contextId, status, artifacts, history, metadata, kind
-   ✅ `TaskStatus` - State, optional message, ISO-8601 timestamp
-   ✅ `TaskState` - All 9 states (submitted, working, input-required, completed, failed, canceled, rejected, auth-required, unknown)
-   ✅ `Message` - Role, parts, messageId, taskId, contextId, referenceTaskIds, metadata
-   ✅ `Part` union types - Text, File (URI + bytes), Data (JSON)
-   ✅ `FileContent` - URI-based and inline base64 representations
-   ✅ `Artifact` - id, name, description, parts, index, metadata
-   ✅ `AgentCard` - Full structure with protocol version, identity, provider, capabilities, skills, security schemes, signatures
-   ✅ `AgentCapabilities` - streaming, pushNotifications, stateTransitionHistory, extensions
-   ✅ `AgentSkill` - id, name, description, tags, examples, input/output modes
-   ✅ `AgentProvider` - organization, url

#### Error Codes

-   ✅ Standard JSON-RPC 2.0 error codes (-32700, -32600, -32601, -32602, -32603)
-   ✅ A2A-specific error codes (-32001 to -32007)
-   ✅ Type-safe error handling with `A2aErrorCode` enum
-   ✅ Rich error context with `A2aError` type

#### RPC Methods (Core)

-   ✅ `message/send` - Send message to initiate/continue task
-   ✅ `tasks/get` - Retrieve task state
-   ✅ `tasks/list` - List tasks with filtering and pagination
-   ✅ `tasks/cancel` - Cancel running task

#### Core Concepts

-   ✅ Agent discovery via `/.well-known/agent-card.json`
-   ✅ Task lifecycle management
-   ✅ Rich message content (text, files, structured data)
-   ✅ Task status tracking with timestamps
-   ✅ Artifact management

### ⚠️ PARTIALLY IMPLEMENTED

#### Streaming (`message/stream`)

-   ⚠️ Endpoint exists but implementation is placeholder
-   ⚠️ Not generating actual streaming events
-   ⚠️ Missing `SendStreamingMessageResponse` structure
-   ⚠️ Server-Sent Events (SSE) not fully implemented

#### Push Notifications

-   ⚠️ `PushNotificationConfig` defined but not used
-   ⚠️ `tasks/pushNotificationConfig/set` method missing
-   ⚠️ `tasks/pushNotificationConfig/get` method missing
-   ⚠️ Webhook validation and delivery not implemented

### ❌ NOT IMPLEMENTED

#### Advanced Streaming Events

-   ❌ `SendStreamingMessageResponse` - Streaming message wrapper
-   ❌ `TaskArtifactUpdateEvent` - Artifact update events with append/lastChunk
-   ❌ `tasks/resubscribe` - Reconnection handler for SSE
-   ❌ Streaming event types with `kind` discriminator

#### Authentication & Security

-   ❌ `agent/getAuthenticatedExtendedCard` - Extended card for authenticated users
-   ❌ `supportsAuthenticatedExtendedCard` capability handling
-   ❌ OpenAPI security scheme parsing and validation
-   ❌ JWS signature verification for `AgentCardSignature`
-   ❌ Authentication header validation
-   ❌ OAuth/API key credential handling

#### Push Notification Management

-   ❌ `tasks/pushNotificationConfig/set` - Configure webhooks
-   ❌ `tasks/pushNotificationConfig/get` - Query webhook config
-   ❌ Webhook URL validation (SSRF protection)
-   ❌ Async notification delivery
-   ❌ Signature verification for incoming webhooks

## Refinement Plan

### Phase 2.1: Complete Streaming Implementation

#### Add Streaming Event Structures

**File**: `vtcode-core/src/a2a/rpc.rs`

```rust
/// Streaming response from message/send and message/stream
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum StreamingEvent {
    /// Message from agent (may be partial)
    Message {
        message: Message,
        #[serde(skip_serializing_if = "Option::is_none")]
        context_id: Option<String>,
        #[serde(default)]
        final_: bool,
    },
    /// Task status update
    TaskStatus {
        task_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        context_id: Option<String>,
        status: TaskStatus,
        #[serde(default)]
        final_: bool,
    },
    /// Artifact update with append/lastChunk support
    TaskArtifact {
        task_id: String,
        artifact: Artifact,
        #[serde(default)]
        append: bool,
        #[serde(default)]
        last_chunk: bool,
        #[serde(default)]
        final_: bool,
    },
}
```

#### Implement SSE Streaming Handler

**File**: `vtcode-core/src/a2a/server.rs`

```rust
async fn handle_stream_impl(
    state: &A2aServerState,
    params: MessageSendParams,
) -> Result<impl Stream<Item = String>, A2aError> {
    // Create task and send initial event
    // Use tokio::sync::broadcast for multi-subscriber streaming
    // Yield events as JSON
}
```

### Phase 2.2: Push Notification Support

#### Add Push Configuration Methods

**File**: `vtcode-core/src/a2a/rpc.rs`

```rust
pub const METHOD_TASKS_PUSH_CONFIG_SET: &str = "tasks/pushNotificationConfig/set";
pub const METHOD_TASKS_PUSH_CONFIG_GET: &str = "tasks/pushNotificationConfig/get";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskPushNotificationConfig {
    pub task_id: String,
    pub config: PushNotificationConfig,
}
```

#### Implement Webhook Delivery

Create new module `vtcode-core/src/a2a/webhook.rs`:

```rust
pub struct WebhookNotifier {
    client: hyper::Client<...>,
    max_retries: u32,
}

impl WebhookNotifier {
    pub async fn send_notification(
        &self,
        url: &str,
        event: &StreamingEvent,
    ) -> Result<(), WebhookError>;
}
```

### Phase 2.3: Authentication & Security

#### Add Security Scheme Support

**File**: `vtcode-core/src/a2a/security.rs` (new)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SecurityScheme {
    ApiKey {
        name: String,
        #[serde(rename = "in")]
        location: String,
    },
    BearerToken {
        format: String,
    },
    OAuth2 {
        flows: serde_json::Value,
    },
}

pub struct SecurityValidator {
    schemes: HashMap<String, SecurityScheme>,
}

impl SecurityValidator {
    pub fn validate_request(&self, headers: &HeaderMap) -> Result<(), SecurityError>;
}
```

#### Implement Extended Card Endpoint

**File**: `vtcode-core/src/a2a/server.rs`

```rust
pub async fn get_authenticated_agent_card(
    State(state): State<A2aServerState>,
    headers: HeaderMap,
) -> Result<Json<AgentCard>, A2aErrorResponse> {
    // Validate authentication
    // Return extended card if supported
}
```

## Detailed Refinements Needed

### 1. Event Stream Types

**Current Gap**: `StreamingEvent` partially defined but not used

**Refinement**:

-   Add `SendStreamingMessageResponse` with `kind: "streaming-response"` discriminator
-   Add `TaskArtifactUpdateEvent` with `append` and `lastChunk` boolean fields
-   Implement proper streaming with `tokio_stream` and `futures::Stream`

**Testing**:

-   Test streaming message events
-   Test task status updates during streaming
-   Test artifact updates with partial chunks

### 2. Push Notification Handlers

**Current Gap**: Methods not implemented

**Refinement**:

-   Add `handle_push_config_set` to set webhook URL for task
-   Add `handle_push_config_get` to query webhook config
-   Implement webhook validation (URL scheme, SSRF protection)
-   Implement async notification queue

**Testing**:

-   Test setting webhook URL
-   Test getting webhook config
-   Test webhook payload validation

### 3. Task Resubscribe Method

**Current Gap**: Not implemented

**Refinement**:

-   Add `tasks/resubscribe` RPC method
-   Support reconnection to streaming updates
-   Implement event history replay

### 4. Authentication Handling

**Current Gap**: No authentication validation

**Refinement**:

-   Parse `securitySchemes` from agent card
-   Implement header-based auth (API key, Bearer token)
-   Validate incoming requests against declared schemes
-   Support OAuth2 flows

**Testing**:

-   Test API key validation
-   Test Bearer token validation
-   Test unauthorized requests

### 5. Card Signatures

**Current Gap**: `AgentCardSignature` defined but not used

**Refinement**:

-   Implement JWS signature verification
-   Support multiple signatures
-   Validate card integrity

### 6. Configuration Extension

**File Changes**:

**`vtcode-core/src/a2a/config.rs` (new)**

```rust
pub struct A2aConfig {
    pub enabled: bool,
    pub max_streaming_events_per_second: u32,
    pub webhook_timeout_secs: u32,
    pub webhook_max_retries: u32,
    pub supported_mime_types: Vec<String>,
    pub max_message_size_bytes: usize,
}
```

## Compliance Checklist

### Spec Compliance Status

| Feature            | Status | Notes                                 |
| ------------------ | ------ | ------------------------------------- |
| Agent Card         | ✅     | Full implementation                   |
| Message Types      | ✅     | All parts supported                   |
| Task Lifecycle     | ✅     | All 9 states                          |
| JSON-RPC 2.0       | ✅     | Full compliance                       |
| Error Codes        | ✅     | All standard + A2A                    |
| Basic Methods      | ✅     | send, get, list, cancel               |
| Streaming Events   | ⚠️     | Structure defined, handler incomplete |
| Push Notifications | ❌     | Not implemented                       |
| Extended Card      | ❌     | Not implemented                       |
| Security Schemes   | ❌     | Not implemented                       |
| Signatures         | ❌     | Not implemented                       |
| Resubscribe        | ❌     | Not implemented                       |

### Priority for Phase 3

1. **HIGH** - Complete streaming implementation
2. **HIGH** - Add push notification support
3. **MEDIUM** - Implement security scheme validation
4. **MEDIUM** - Add authenticated extended card
5. **LOW** - Implement card signature verification

## Migration Path

### For Users

No breaking changes. All new features are:

-   Behind feature flags where applicable
-   Optional in configuration
-   Backward compatible with Phase 1 implementation

### For Developers

1. Existing code continues to work unchanged
2. New methods available via handler dispatch
3. Optional features can be enabled per-agent

## Testing Strategy

### Unit Tests to Add

1. Streaming event serialization/deserialization
2. Push notification configuration validation
3. Security scheme parsing
4. Authentication header validation
5. Card signature verification

### Integration Tests to Add

1. Complete streaming workflow with events
2. Multi-event streaming session
3. Webhook delivery with retries
4. Authenticated agent card retrieval
5. Security scheme enforcement

## Documentation Updates

1. Update README with streaming examples
2. Add push notification guide
3. Add security configuration guide
4. Add authentication examples
5. Update API reference

## References

-   [A2A Specification](https://a2a-protocol.org)
-   [JSON-RPC 2.0](https://www.jsonrpc.org/specification)
-   [OpenAPI Security Schemes](https://spec.openapis.org/oas/v3.0.3#security-scheme-object)
