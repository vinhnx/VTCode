# ACP Quick Reference

## Initialize ACP Client

```rust
use vtcode_acp_client::AcpClient;

let client = AcpClient::new("my-agent".to_string())?;
let registry = client.registry();
```

## Register Remote Agent

```rust
use vtcode_acp_client::AgentInfo;

let agent = AgentInfo {
    id: "remote-agent".to_string(),
    name: "Remote Agent".to_string(),
    base_url: "http://localhost:8081".to_string(),
    description: Some("Description".to_string()),
    capabilities: vec!["action1".to_string(), "action2".to_string()],
    metadata: Default::default(),
    online: true,
    last_seen: None,
};

registry.register(agent).await?;
```

## Discover Agents

```rust
// List all agents
let all = registry.list_all().await?;

// List online agents
let online = registry.list_online().await?;

// Find by capability
let agents = registry.find_by_capability("python").await?;

// Find by ID
let agent = registry.find("agent-id").await?;
```

## Call Remote Agent (Sync)

```rust
use serde_json::json;

let result = client.call_sync(
    "remote-agent",
    "execute".to_string(),
    json!({"param": "value"}),
).await?;

println!("Result: {}", result);
```

## Call Remote Agent (Async)

```rust
let message_id = client.call_async(
    "remote-agent",
    "long_task".to_string(),
    json!({"data": "..."}),
).await?;

// Store message_id for later polling
println!("Task queued: {}", message_id);
```

## Check Agent Health

```rust
let is_online = client.ping("remote-agent").await?;

if is_online {
    println!("Agent is reachable");
} else {
    println!("Agent is offline");
}
```

## Discover Agent Metadata

```rust
let agent_info = client.discover_agent("http://localhost:8081").await?;
println!("Agent: {} ({})", agent_info.name, agent_info.id);
println!("Capabilities: {:?}", agent_info.capabilities);
```

## Error Handling

```rust
use vtcode_acp_client::AcpError;

match client.call_sync("id", "action".to_string(), json!({})).await {
    Ok(result) => println!("Success: {}", result),
    Err(AcpError::AgentNotFound(id)) => println!("Agent {} not found", id),
    Err(AcpError::NetworkError(e)) => println!("Network: {}", e),
    Err(AcpError::Timeout(e)) => println!("Timeout: {}", e),
    Err(AcpError::RemoteError { agent_id, message, code }) => {
        println!("Remote error from {}: {} ({:?})", agent_id, message, code);
    }
    Err(e) => println!("Error: {}", e),
}
```

## Message Types

```rust
use vtcode_acp_client::AcpMessage;

// Create request
let msg = AcpMessage::request(
    "sender".to_string(),
    "recipient".to_string(),
    "action".to_string(),
    json!({"param": "value"}),
);

// Serialize
let json = msg.to_json()?;

// Deserialize
let msg = AcpMessage::from_json(&json)?;

// Create response
let response = AcpMessage::response(
    "agent-a".to_string(),
    "agent-b".to_string(),
    json!({"result": "data"}),
    "correlation-id".to_string(),
);

// Create error
let error = AcpMessage::error_response(
    "agent-a".to_string(),
    "agent-b".to_string(),
    "ERROR_CODE".to_string(),
    "Error message".to_string(),
    "correlation-id".to_string(),
);
```

## Update Agent Status

```rust
// Mark as offline
registry.update_status("agent-id", false).await?;

// Mark as online
registry.update_status("agent-id", true).await?;
```

## Registry Operations

```rust
// Count agents
let count = registry.count().await;

// Clear all agents
registry.clear().await;

// Unregister agent
registry.unregister("agent-id").await?;
```

## Custom Timeout

```rust
use std::time::Duration;
use vtcode_acp_client::AcpClientBuilder;

let client = AcpClientBuilder::new("local-agent".to_string())
    .with_timeout(Duration::from_secs(60))
    .build()?;
```

## MCP Tool Usage (From Main Agent)

### Discover Agents

```json
{
  "tool": "acp_discover",
  "input": {
    "mode": "list_online"
  }
}
```

### Find by Capability

```json
{
  "tool": "acp_discover",
  "input": {
    "mode": "by_capability",
    "capability": "python"
  }
}
```

### Call Remote Agent (Sync)

```json
{
  "tool": "acp_call",
  "input": {
    "remote_agent_id": "data-processor",
    "action": "process",
    "args": {"input": "data"},
    "method": "sync"
  }
}
```

### Call Remote Agent (Async)

```json
{
  "tool": "acp_call",
  "input": {
    "remote_agent_id": "trainer",
    "action": "train_model",
    "args": {"epochs": 100},
    "method": "async"
  }
}
```

### Check Agent Health

```json
{
  "tool": "acp_health",
  "input": {
    "agent_id": "data-processor"
  }
}
```

## HTTP Endpoints (Remote Agent Must Implement)

### POST /messages
```
Request: AcpMessage
Response: AcpResponse
```

### GET /metadata
```
Response: AgentInfo
```

### GET /health
```
Response: "OK" or JSON status
```

## Environment Variables

```bash
# Enable tracing
RUST_LOG=vtcode_acp_client=trace

# Or with all details
RUST_LOG=debug
```

## Common Patterns

### Master-Worker

```rust
// Master discovers worker agents
let workers = registry.find_by_capability("worker").await?;

// Distribute work across workers
for (i, worker) in workers.iter().enumerate() {
    let result = client.call_sync(
        &worker.id,
        "process".to_string(),
        json!({"data": format!("batch_{}", i)}),
    ).await?;
    println!("Worker {} result: {}", worker.id, result);
}
```

### Pipelined Processing

```rust
// Data processor
let data = client.call_sync(
    "processor",
    "clean".to_string(),
    json!({"raw": raw_data}),
).await?;

// Model trainer
let model = client.call_sync(
    "trainer",
    "train".to_string(),
    json!({"data": data}),
).await?;

// Report generator
let report = client.call_sync(
    "reporter",
    "generate".to_string(),
    json!({"model": model}),
).await?;
```

### Async Batch Processing

```rust
// Queue multiple jobs
let mut job_ids = vec![];

for batch in batches {
    let msg_id = client.call_async(
        "processor",
        "process_batch".to_string(),
        json!({"batch": batch}),
    ).await?;
    job_ids.push(msg_id);
}

// Later, poll for results
for job_id in job_ids {
    println!("Job {}: queued", job_id);
}
```

## Testing

```bash
# Run all ACP client tests
cargo test -p vtcode-acp-client

# Run specific test
cargo test -p vtcode-acp-client test_agent_registry

# Run with output
cargo test -p vtcode-acp-client -- --nocapture

# Run example
cargo run --example acp_distributed_workflow
```

## Troubleshooting

### Agent Not Found
```
Error: Agent not found: agent-id
→ Check if agent was registered
→ Check registry.list_all() to see what's registered
```

### Network Error
```
Error: Network error: Connection refused
→ Check if remote agent is running
→ Verify base_url is correct
→ Check firewall/network connectivity
```

### Timeout
```
Error: Request exceeded 30s timeout
→ Increase timeout with AcpClientBuilder.with_timeout()
→ Check if remote agent is responding slowly
→ Consider using async call for long tasks
```

### Serialization Error
```
Error: Failed to parse response
→ Check remote agent is returning valid JSON
→ Verify message format matches ACP spec
```

## Documentation Links

- [Full ACP Integration Guide](./ACP_INTEGRATION.md)
- [Client Library README](../vtcode-acp-client/README.md)
- [ACP Specification](https://agentcommunicationprotocol.dev/)
- [Example Code](../examples/acp_distributed_workflow.rs)
- [Implementation Summary](./IMPLEMENTATION_SUMMARY.md)
