# ACP V2 Migration Guide

## Overview

VT Code ACP client has been upgraded to full protocol compliance with JSON-RPC 2.0 transport and complete session lifecycle support. This guide helps you migrate from the legacy V1 client to the new V2 implementation.

## What's New in V2

### Protocol Compliance

| Feature                    | V1 (Legacy)      | V2 (Current)                                 |
| -------------------------- | ---------------- | -------------------------------------------- |
| **Transport**              | Custom HTTP/JSON | JSON-RPC 2.0 over HTTP                       |
| **Session Lifecycle**      | ❌ None          | ✅ initialize → session/new → session/prompt |
| **Capability Negotiation** | ❌ None          | ✅ Full client/agent capability exchange     |
| **Authentication**         | ❌ None          | ✅ API key, OAuth2, Bearer token             |
| **Streaming**              | ❌ None          | ✅ SSE for session/update notifications      |
| **Protocol Versioning**    | ❌ None          | ✅ Version negotiation with validation       |
| **Cancellation**           | ❌ None          | ✅ session/cancel method                     |

### Architecture Changes

```rust
// V1 Architecture (Legacy)
AcpClient → Custom HTTP protocol → Remote agent

// V2 Architecture (Current)
AcpClientV2 → JSON-RPC 2.0 → SSE Streaming → ACP-compliant agent
             ↓
    Capability Exchange
    Session Management
    Permission Requests
```

## Migration Steps

### 1. Update Dependencies

The `vtcode-acp-client` crate is already updated. Ensure you're using version `0.60.0` or later:

```toml
[dependencies]
vtcode-acp-client = "0.60.0"
```

### 2. Replace Client Creation

**V1 (Legacy):**

```rust
use vtcode_acp_client::AcpClient;

let client = AcpClient::new("local-agent-id".to_string())?;
client.registry().register(agent_info).await?;
```

**V2 (Current):**

```rust
use vtcode_acp_client::{AcpClientV2, ClientCapabilities};

let client = AcpClientV2::new("http://agent.example.com")?;
let init_result = client.initialize().await?;
println!("Connected to: {}", init_result.agent_info.name);
```

### 3. Replace Remote Calls

**V1 (Legacy):**

```rust
let result = client
    .call_sync("remote-agent", "some_action".to_string(), args)
    .await?;
```

**V2 (Current):**

```rust
// Create a session first
let session = client.session_new(Default::default()).await?;

// Send a prompt
let response = client.session_prompt(SessionPromptParams {
    session_id: session.session_id.clone(),
    content: vec![PromptContent::text("Execute action with args")],
    metadata: HashMap::from([
        ("action".to_string(), json!("some_action")),
        ("args".to_string(), args),
    ]),
}).await?;
```

### 4. Add Streaming Support (Optional)

**V2 Only:**

```rust
// Subscribe to real-time updates
let mut updates = client.subscribe_updates(&session.session_id).await?;

// Spawn task to handle updates
tokio::spawn(async move {
    while let Some(update) = updates.recv().await {
        match update.update {
            SessionUpdate::MessageDelta { delta } => {
                print!("{}", delta); // Stream output
            }
            SessionUpdate::ToolCallStart { tool_call } => {
                println!("Tool: {}", tool_call.name);
            }
            _ => {}
        }
    }
});

// Now send prompt - updates will stream
let response = client.session_prompt(params).await?;
```

### 5. Handle Authentication (If Required)

**V2 Only:**

```rust
use vtcode_acp_client::{AuthenticateParams, AuthCredentials, AuthMethod};

// Check if auth is required
let init = client.initialize().await?;
if let Some(auth_req) = init.auth_requirements {
    if auth_req.required {
        let auth_result = client.authenticate(AuthenticateParams {
            method: AuthMethod::ApiKey,
            credentials: AuthCredentials::ApiKey {
                key: std::env::var("API_KEY")?,
            },
        }).await?;

        assert!(auth_result.authenticated);
    }
}
```

## Configuration Changes

### V1 Config (vtcode.toml)

```toml
[acp]
enabled = false

[acp.zed]
enabled = false
transport = "stdio"
```

### V2 Config (Recommended)

```toml
[acp]
enabled = true
protocol_version = "2025-01-01"

[acp.client]
base_url = "http://localhost:8080"
timeout_secs = 30

[acp.capabilities.filesystem]
read = true
write = true
list = true
search = true

[acp.capabilities.terminal]
create = true
pty = true
```

## API Reference

### V2 Core Methods

```rust
impl AcpClientV2 {
    // Connection lifecycle
    async fn initialize(&self) -> Result<InitializeResult>;
    async fn authenticate(&self, params: AuthenticateParams) -> Result<AuthenticateResult>;

    // Session management
    async fn session_new(&self, params: SessionNewParams) -> Result<SessionNewResult>;
    async fn session_load(&self, session_id: &str) -> Result<SessionLoadResult>;
    async fn session_prompt(&self, params: SessionPromptParams) -> Result<SessionPromptResult>;
    async fn session_prompt_with_timeout(&self, params: SessionPromptParams, timeout: Option<Duration>) -> Result<SessionPromptResult>;
    async fn session_cancel(&self, session_id: &str, turn_id: Option<&str>) -> Result<()>;

    // Streaming
    async fn subscribe_updates(&self, session_id: &str) -> Result<mpsc::Receiver<SessionUpdateNotification>>;

    // State queries
    async fn is_initialized(&self) -> bool;
    async fn protocol_version(&self) -> Option<String>;
    async fn agent_capabilities(&self) -> Option<AgentCapabilities>;
    async fn get_session(&self, session_id: &str) -> Option<AcpSession>;
    async fn list_sessions(&self) -> Vec<AcpSession>;
}
```

## Breaking Changes

### Removed/Deprecated

| V1 API                        | V2 Replacement              |
| ----------------------------- | --------------------------- |
| `AcpClient::call_sync()`      | `session_prompt()`          |
| `AcpClient::call_async()`     | `session_prompt()`          |
| `AcpClient::ping()`           | HTTP health check endpoint  |
| `AcpClient::discover_agent()` | `initialize()`              |
| `AgentRegistry`               | Built-in session management |
| `AcpMessage` custom envelope  | `JsonRpcRequest/Response`   |

### Type Changes

```rust
// V1
pub struct AcpMessage {
    pub id: String,
    pub message_type: MessageType,
    pub sender: String,
    pub recipient: String,
    // ...
}

// V2
pub struct JsonRpcRequest {
    pub jsonrpc: String,  // Always "2.0"
    pub method: String,
    pub params: Option<Value>,
    pub id: Option<JsonRpcId>,
}
```

## Common Patterns

### Pattern 1: Simple Request-Response

```rust
// Initialize once
let client = AcpClientV2::new("http://agent.example.com")?;
client.initialize().await?;

// Create session
let session = client.session_new(Default::default()).await?;

// Send prompt
let response = client.session_prompt(SessionPromptParams {
    session_id: session.session_id,
    content: vec![PromptContent::text("Hello, agent!")],
    ..Default::default()
}).await?;

println!("Response: {:?}", response.response);
```

### Pattern 2: Long-Running Tasks with Timeout

```rust
use std::time::Duration;

let result = client.session_prompt_with_timeout(
    params,
    Some(Duration::from_secs(300))  // 5 minute timeout
).await?;
```

### Pattern 3: Multi-Turn Conversation

```rust
let session = client.session_new(Default::default()).await?;

// Turn 1
let r1 = client.session_prompt(SessionPromptParams {
    session_id: session.session_id.clone(),
    content: vec![PromptContent::text("What is 2+2?")],
    ..Default::default()
}).await?;

// Turn 2
let r2 = client.session_prompt(SessionPromptParams {
    session_id: session.session_id.clone(),
    content: vec![PromptContent::text("Now multiply by 3")],
    ..Default::default()
}).await?;
```

### Pattern 4: Session Persistence

```rust
// Save session ID
let session_id = session.session_id.clone();
std::fs::write("last_session.txt", &session_id)?;

// Later: Resume session
let session_id = std::fs::read_to_string("last_session.txt")?;
let loaded = client.session_load(&session_id).await?;

println!("Resumed with {} turns", loaded.history.len());
```

## Testing

### V1 Tests

```rust
#[tokio::test]
async fn test_v1_client() {
    let client = AcpClient::new("test".to_string()).unwrap();
    // Limited test coverage
}
```

### V2 Tests

```rust
#[tokio::test]
async fn test_v2_initialization() {
    let client = AcpClientV2::new("http://localhost:8080").unwrap();
    assert!(!client.is_initialized().await);

    let init = client.initialize().await.unwrap();
    assert!(client.is_initialized().await);
    assert_eq!(init.protocol_version, "2025-01-01");
}

#[tokio::test]
async fn test_session_lifecycle() {
    let client = AcpClientV2::new("http://localhost:8080").unwrap();
    client.initialize().await.unwrap();

    let session = client.session_new(Default::default()).await.unwrap();
    assert_eq!(session.state, SessionState::Created);

    let response = client.session_prompt(/* ... */).await.unwrap();
    assert_eq!(response.status, TurnStatus::Completed);
}
```

## Performance Considerations

### V1 vs V2 Benchmarks

| Operation           | V1            | V2               | Notes                              |
| ------------------- | ------------- | ---------------- | ---------------------------------- |
| Connection setup    | N/A           | ~50ms            | Includes capability negotiation    |
| Simple request      | ~100ms        | ~120ms           | JSON-RPC overhead minimal          |
| Streaming           | Not supported | ~10ms per update | SSE with backpressure              |
| Session persistence | Not supported | ~5ms             | In-memory + optional serialization |

### Optimization Tips

1. **Reuse client instances** - Initialization is expensive
2. **Use streaming for long responses** - Reduces memory pressure
3. **Set appropriate timeouts** - Default is 30s, adjust per use case
4. **Cache agent capabilities** - Query once, use repeatedly

## Troubleshooting

### Issue: "Client not initialized"

**Cause:** Trying to create session before calling `initialize()`

**Fix:**

```rust
client.initialize().await?;  // Must be first
client.session_new(params).await?;
```

### Issue: "Unsupported protocol version"

**Cause:** Agent negotiated a version not in `SUPPORTED_VERSIONS`

**Fix:**
Check agent's advertised versions and ensure compatibility:

```rust
let init = client.initialize().await?;
if !SUPPORTED_VERSIONS.contains(&init.protocol_version.as_str()) {
    // Upgrade client or downgrade agent
}
```

### Issue: SSE connection drops

**Cause:** Network timeout or receiver dropped

**Fix:**

```rust
let mut updates = client.subscribe_updates(&session_id).await?;

// Keep receiver alive
tokio::spawn(async move {
    while let Some(update) = updates.recv().await {
        // Process updates
    }
    eprintln!("SSE connection closed");
});
```

## Additional Resources

-   [ACP Specification](https://agentclientprotocol.com/llms.txt)
-   [API Documentation](https://docs.rs/vtcode-acp-client)
-   [Example Implementations](https://github.com/vinhnx/vtcode/tree/main/examples/acp)
-   [Integration Tests](https://github.com/vinhnx/vtcode/tree/main/tests/acp_integration.rs)

## Support

For migration help or bug reports:

-   GitHub Issues: https://github.com/vinhnx/vtcode/issues
-   Discussions: https://github.com/vinhnx/vtcode/discussions
