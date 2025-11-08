# VTCode ACP Client

HTTP-based Agent Communication Protocol (ACP) client library for inter-agent communication in distributed agent systems.

## Features

✅ **REST-based Communication** - Standard HTTP protocol, no special SDKs required
✅ **Agent Discovery** - Find agents by capability or ID
✅ **Sync & Async** - Both synchronous and asynchronous request handling
✅ **Health Monitoring** - Ping agents to check status
✅ **Message Serialization** - Type-safe ACP message handling
✅ **Registry Management** - In-memory agent registry with lifecycle management
✅ **Error Handling** - Comprehensive error types for debugging

## Quick Start

### Add to Cargo.toml

```toml
[dependencies]
vtcode-acp-client = { path = "../vtcode-acp-client" }
```

### Basic Usage

```rust
use vtcode_acp_client::{AcpClient, AgentInfo};
use serde_json::json;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Create client
    let client = AcpClient::new("my-agent".to_string())?;
    
    // Register a remote agent
    let agent = AgentInfo {
        id: "remote-agent".to_string(),
        name: "Remote Agent".to_string(),
        base_url: "http://localhost:8081".to_string(),
        description: Some("A sample remote agent".to_string()),
        capabilities: vec!["bash".to_string()],
        metadata: Default::default(),
        online: true,
        last_seen: None,
    };
    
    client.registry().register(agent).await?;
    
    // Call the remote agent
    let result = client.call_sync(
        "remote-agent",
        "execute".to_string(),
        json!({"cmd": "echo hello"}),
    ).await?;
    
    println!("Result: {}", result);
    Ok(())
}
```

## Module Overview

### Core Components

#### `AcpClient`
Main client for agent communication.

```rust
// Create a new client
let client = AcpClient::new("local-agent-id".to_string())?;

// Synchronous call (waits for response)
let result = client.call_sync("remote-id", "action".to_string(), args).await?;

// Asynchronous call (returns message_id immediately)
let msg_id = client.call_async("remote-id", "action".to_string(), args).await?;

// Health check
let is_online = client.ping("remote-id").await?;

// Discover remote agent
let agent_info = client.discover_agent("http://remote:8080").await?;
```

#### `AgentRegistry`
In-memory registry of available agents.

```rust
let registry = client.registry();

// Register agent
registry.register(agent).await?;

// Find agent
let agent = registry.find("agent-id").await?;

// Find by capability
let agents = registry.find_by_capability("python").await?;

// List all/online agents
let all = registry.list_all().await?;
let online = registry.list_online().await?;

// Update status
registry.update_status("agent-id", false).await?;
```

#### `AcpMessage`
Type-safe message handling.

```rust
// Create request
let msg = AcpMessage::request(
    "sender".to_string(),
    "recipient".to_string(),
    "action".to_string(),
    json!({}),
);

// Serialize to JSON
let json = msg.to_json()?;

// Deserialize from JSON
let msg = AcpMessage::from_json(&json)?;
```

### Error Handling

```rust
use vtcode_acp_client::AcpError;

match client.call_sync("agent", "action".to_string(), args).await {
    Ok(result) => println!("Success: {}", result),
    Err(AcpError::AgentNotFound(id)) => println!("Agent {} not found", id),
    Err(AcpError::NetworkError(e)) => println!("Network error: {}", e),
    Err(AcpError::Timeout(e)) => println!("Timeout: {}", e),
    Err(AcpError::RemoteError { agent_id, message, code }) => {
        println!("Remote error from {}: {} (code: {:?})", agent_id, message, code);
    }
    Err(e) => println!("Error: {}", e),
}
```

## Architecture

```
User Code
   │
   └─► AcpClient
        ├─► HTTP Communication (reqwest)
        ├─► Message Serialization (serde_json)
        └─► AgentRegistry
             └─► In-memory HashMap<String, AgentInfo>
```

## Message Protocol

### Request
```json
{
  "id": "uuid",
  "type": "request",
  "sender": "local-agent",
  "recipient": "remote-agent",
  "content": {
    "action": "execute_tool",
    "args": { "param": "value" },
    "sync": true,
    "timeout_secs": 30
  },
  "timestamp": "2024-01-01T12:00:00Z",
  "correlation_id": null
}
```

### Response
```json
{
  "id": "uuid",
  "type": "response",
  "sender": "remote-agent",
  "recipient": "local-agent",
  "content": {
    "status": "success",
    "result": { "output": "data" },
    "execution_time_ms": 245
  },
  "timestamp": "2024-01-01T12:00:00Z",
  "correlation_id": "request-id"
}
```

## Remote Agent Requirements

For an agent to be callable, it must implement:

### 1. POST `/messages`
Handle ACP requests and return responses.

```rust
app.post("/messages", |msg: AcpMessage| async {
    // Process message
    // Return AcpResponse
})
```

### 2. GET `/metadata`
Return agent discovery information.

```rust
app.get("/metadata", || async {
    AgentInfo {
        id: "agent-id".to_string(),
        name: "Agent Name".to_string(),
        base_url: "http://localhost:8080".to_string(),
        // ... other fields
    }
})
```

### 3. GET `/health`
Simple health check endpoint.

```rust
app.get("/health", || "OK")
```

## Configuration

Build client with custom timeout:

```rust
use std::time::Duration;
use vtcode_acp_client::AcpClientBuilder;

let client = AcpClientBuilder::new("local-agent".to_string())
    .with_timeout(Duration::from_secs(60))
    .build()?;
```

## Testing

Run tests:

```bash
cargo test -p vtcode-acp-client
```

Run example:

```bash
cargo run --example acp_distributed_workflow
```

## Performance

- **Message serialization:** <1ms
- **Local registry lookup:** O(1)
- **HTTP timeout:** Configurable (default 30s)
- **Async overhead:** Minimal (uses tokio)

## Security Considerations

⚠️ **Current Implementation:**
- HTTP (not HTTPS) by default
- No authentication/authorization
- No message encryption
- Messages logged with tracing

🔒 **Recommended for Production:**
- Use HTTPS with certificate pinning
- Implement JWT or mTLS authentication
- Add message signing and encryption
- Implement rate limiting
- Add audit logging
- Use VPN/private networks

## Roadmap

- [ ] HTTPS/TLS support
- [ ] Authentication plugins (JWT, mTLS)
- [ ] Message encryption
- [ ] Automatic retries with exponential backoff
- [ ] Circuit breaker pattern
- [ ] Message queuing
- [ ] OpenTelemetry integration
- [ ] Metrics collection

## Integration with VTCode

The ACP client is exposed to the main agent through three MCP tools:

1. **acp_call** - Call remote agents
2. **acp_discover** - Discover agents
3. **acp_health** - Check agent health

See [ACP_INTEGRATION.md](../docs/ACP_INTEGRATION.md) for integration details.

## References

- [ACP Official Specification](https://agentcommunicationprotocol.dev/)
- [VTCode Documentation](../docs/)
- [Examples](../examples/)

## License

MIT
