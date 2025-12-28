# A2A Protocol Support for VT Code

VT Code now implements the [Agent2Agent (A2A) Protocol](https://a2a-protocol.org), an open standard enabling communication and interoperability between AI agents.

## Overview

The A2A Protocol enables:

- **Agent Discovery**: Via Agent Cards at `/.well-known/agent-card.json`
- **Task Lifecycle Management**: States like `submitted`, `working`, `completed`, `failed`
- **Real-time Streaming**: Via Server-Sent Events (SSE)
- **Rich Content Types**: Text, file, and structured data parts
- **Push Notifications**: Webhook-based async updates
- **JSON-RPC 2.0**: Over HTTP(S) for interoperability

## Architecture

VT Code's A2A implementation spans across modules:

```
vtcode-core/src/a2a/
├── mod.rs                  # Module organization & re-exports
├── types.rs                # Core data structures (Task, Message, Part, Artifact)
├── rpc.rs                  # JSON-RPC 2.0 protocol (requests, responses, methods)
├── errors.rs               # A2A and JSON-RPC error codes
├── agent_card.rs           # Agent discovery metadata
├── task_manager.rs         # In-memory task lifecycle management
├── server.rs               # Axum HTTP server (feature-gated: a2a-server)
├── client.rs               # HTTP client for remote agent communication
└── webhook.rs              # Webhook notifier for push events
```

## Core Types

### TaskState Enum

Represents the lifecycle of a task:

```rust
pub enum TaskState {
    Submitted,      // Task submitted, waiting to start
    Working,        // Task actively processing
    InputRequired,  // Awaiting user input
    Completed,      // Task finished successfully
    Failed,         // Task failed with error
    Canceled,       // Task canceled by request
    Rejected,       // Task rejected by agent
    AuthRequired,   // Requires authentication
    Unknown,        // Unknown state
}
```

### Message & Part Types

Messages contain rich content:

```rust
pub struct Message {
    pub role: MessageRole,                  // User or Agent
    pub parts: Vec<Part>,                   // Content parts
    pub message_id: Option<String>,         // Unique ID
    pub task_id: Option<String>,            // Associated task
    pub context_id: Option<String>,         // Conversation context
    pub reference_task_ids: Vec<String>,    // Prior task references
    pub metadata: Option<HashMap<String, serde_json::Value>>,
}

pub enum Part {
    Text { text: String },                  // Plain text
    File { file: FileContent },             // File URI or bytes
    Data { data: serde_json::Value },       // Structured data
}
```

### Task Structure

Represents a stateful unit of work:

```rust
pub struct Task {
    pub id: String,                         // Unique task ID
    pub context_id: Option<String>,         // Conversation context
    pub status: TaskStatus,                 // Current status & message
    pub artifacts: Vec<Artifact>,           // Generated outputs
    pub history: Vec<Message>,              // Conversation history
    pub metadata: Option<HashMap<String, serde_json::Value>>,
}
```

### Artifact Structure

Outputs produced by tasks:

```rust
pub struct Artifact {
    pub id: String,                         // Unique artifact ID
    pub name: Option<String>,               // Human-readable name
    pub description: Option<String>,        // Description
    pub parts: Vec<Part>,                   // Content parts
    pub index: Option<u32>,                 // Ordering index
    pub metadata: Option<HashMap<String, serde_json::Value>>,
}
```

## Task Manager API

The `TaskManager` provides in-memory task storage and lifecycle management:

```rust
impl TaskManager {
    /// Create a new task
    pub async fn create_task(&self, context_id: Option<String>) -> Task

    /// Update task status
    pub async fn update_status(
        &self,
        task_id: &str,
        state: TaskState,
        message: Option<Message>
    ) -> A2aResult<Task>

    /// Add an artifact to a task
    pub async fn add_artifact(&self, task_id: &str, artifact: Artifact) -> A2aResult<Task>

    /// Get a task by ID
    pub async fn get_task(&self, task_id: &str) -> Option<Task>

    /// List tasks with filtering
    pub async fn list_tasks(&self, params: ListTasksParams) -> ListTasksResult

    /// Cancel a task
    pub async fn cancel_task(&self, task_id: &str) -> A2aResult<Task>

    /// Configure webhooks for task events
    pub async fn set_webhook_config(&self, config: TaskPushNotificationConfig)

    /// Get webhook configuration
    pub async fn get_webhook_config(&self, task_id: &str) -> Option<TaskPushNotificationConfig>
}
```

## Server API (HTTP Endpoints)

The A2A HTTP server (enabled with `a2a-server` feature) exposes:

### Agent Discovery

```http
GET /.well-known/agent-card.json
```

Returns the agent's capability card:

```json
{
  "protocolVersion": "1.0",
  "name": "VT Code Agent",
  "description": "AI code assistant powered by VT Code",
  "version": "0.54.1",
  "url": "https://example.com",
  "capabilities": {
    "streaming": true,
    "pushNotifications": true,
    "stateTransitionHistory": true
  }
}
```

### JSON-RPC Endpoints

```http
POST /a2a
```

Send JSON-RPC requests for task management:

**Create Task & Send Message**:
```json
{
  "jsonrpc": "2.0",
  "method": "message/send",
  "params": {
    "message": {
      "role": "user",
      "parts": [{"type": "text", "text": "Help me refactor this code"}]
    }
  },
  "id": "req-123"
}
```

**Stream Messages**:
```json
{
  "jsonrpc": "2.0",
  "method": "message/stream",
  "params": {
    "taskId": "task-123",
    "contextId": "ctx-123"
  },
  "id": "req-124"
}
```

**Get Task**:
```json
{
  "jsonrpc": "2.0",
  "method": "tasks/get",
  "params": {
    "id": "task-123"
  },
  "id": "req-125"
}
```

**List Tasks**:
```json
{
  "jsonrpc": "2.0",
  "method": "tasks/list",
  "params": {
    "contextId": "ctx-123",
    "pageSize": 10
  },
  "id": "req-126"
}
```

**Cancel Task**:
```json
{
  "jsonrpc": "2.0",
  "method": "tasks/cancel",
  "params": {
    "taskId": "task-123"
  },
  "id": "req-127"
}
```

### Streaming Endpoint

```http
POST /a2a/stream
```

Establish Server-Sent Events stream for real-time updates:

```json
{
  "jsonrpc": "2.0",
  "method": "message/stream",
  "params": {
    "taskId": "task-123",
    "contextId": "ctx-123"
  },
  "id": "req-128"
}
```

Response stream:

```
data: {"event":{"message":{"role":"agent","parts":[...]},...}}
data: {"event":{"taskStatus":{...},"state":"completed",...}}
...
```

## Client API

Connect to remote A2A agents:

```rust
use vtcode_core::a2a::A2aClient;

let client = A2aClient::new("https://agent.example.com");

// Discover agent
let card = client.discover_agent().await?;

// Send message
let params = MessageSendParams::new(
    Message::user_text("What is your name?")
);
let response = client.send_message(params).await?;

// Stream messages
let mut stream = client.stream_messages(task_id, context_id).await?;
while let Some(event) = stream.next().await {
    println!("Event: {:?}", event);
}

// Get task
let task = client.get_task(task_id).await?;

// List tasks
let tasks = client.list_tasks(context_id).await?;

// Cancel task
let canceled = client.cancel_task(task_id).await?;
```

## Error Handling

A2A errors use both standard JSON-RPC and A2A-specific error codes:

```rust
pub enum A2aErrorCode {
    // Standard JSON-RPC errors (-32700 to -32603)
    JsonParseError,         // -32700
    InvalidRequest,         // -32600
    MethodNotFound,         // -32601
    InvalidParams,          // -32602
    InternalError,          // -32603
    
    // A2A-specific errors
    TaskNotFound,           // -32001
    TaskNotCancelable,      // -32002
    PushNotificationNotSupported,  // -32003
    UnsupportedOperation,   // -32004
    ContentTypeNotSupported,// -32005
}
```

## Configuration

Enable A2A server in Cargo.toml:

```toml
[features]
a2a-server = ["dep:axum", "dep:tower", "dep:tower-http", "dep:tokio-stream"]
```

## Examples

### Create and Query Task

```rust
use vtcode_core::a2a::TaskManager;

#[tokio::main]
async fn main() {
    let manager = TaskManager::new();
    
    // Create task
    let task = manager.create_task(Some("conversation-1".to_string())).await;
    println!("Created task: {}", task.id);
    
    // Update status
    manager.update_status(
        &task.id,
        TaskState::Working,
        Some(Message::agent_text("Processing your request..."))
    ).await.ok();
    
    // Add artifact
    let artifact = Artifact::text("result-1", "Refactored code");
    manager.add_artifact(&task.id, artifact).await.ok();
    
    // Get task
    let updated = manager.get_task(&task.id).await.unwrap();
    println!("Task state: {:?}", updated.state());
    println!("Artifacts: {}", updated.artifacts.len());
}
```

### Setup Server

```rust
use vtcode_core::a2a::{AgentCard, TaskManager};
use vtcode_core::a2a::server::A2aServerState;

#[tokio::main]
async fn main() {
    let agent_card = AgentCard::new("my-agent", "My AI Agent", "1.0.0");
    let task_manager = TaskManager::new();
    let server_state = A2aServerState::new(task_manager, agent_card);
    
    // Create router
    let router = vtcode_core::a2a::server::create_router(server_state);
    
    // Start listening
    let listener = tokio::net::TcpListener::bind("127.0.0.1:8080")
        .await
        .unwrap();
    
    axum::serve(listener, router).await.unwrap();
}
```

### Connect to Remote Agent

```rust
use vtcode_core::a2a::A2aClient;

#[tokio::main]
async fn main() {
    let client = A2aClient::new("https://agent.example.com");
    
    // Discover agent
    let card = client.discover_agent().await.unwrap();
    println!("Agent: {} ({})", card.name, card.version);
    
    // Send message
    use vtcode_core::a2a::{Message, MessageSendParams};
    let params = MessageSendParams::new(
        Message::user_text("What can you do?")
    );
    let response = client.send_message(params).await.unwrap();
    println!("Response: {:?}", response);
}
```

## Testing

Integration tests cover:

- Task lifecycle management
- Message handling
- Artifact management
- Concurrent operations
- State transitions
- Error handling
- Large message handling
- Streaming events

Run tests:

```bash
cargo test --test a2a_integration_tests
```

## Implementation Status

- ✅ **Phase 1**: Core types, task manager, server
- ✅ **Phase 2**: Integration tests, streaming, webhooks
- 🚧 **Phase 3**: Advanced client features, authentication
- 📋 **Phase 4**: Extended documentation, examples

## Dependencies

- **axum**: HTTP server framework
- **tokio**: Async runtime
- **tower-http**: HTTP middleware
- **serde_json**: JSON serialization
- **reqwest**: HTTP client
- **tokio-stream**: Async streams

## Performance

- In-memory task storage with RwLock concurrency
- Efficient artifact streaming
- Best-effort webhook delivery
- Broadcast channel for SSE streaming

## Security Notes

- Webhook authentication via headers
- Request validation per JSON-RPC spec
- Error code sanitization
- Protocol version compatibility checks

## References

- [A2A Protocol Specification](https://a2a-protocol.org)
- [JSON-RPC 2.0 Specification](https://www.jsonrpc.org/specification)
- [Server-Sent Events (SSE) Standard](https://html.spec.whatwg.org/multipage/server-sent-events.html)
