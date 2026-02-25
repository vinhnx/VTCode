# ACP (Agent Communication Protocol) Integration Guide

## Overview

VT Code now supports Agent Communication Protocol (ACP) for inter-agent communication. This enables vtcode to act as an ACP client, discovering and communicating with other agents in a distributed system.

**Key Features:**

-   REST-based HTTP protocol (no special SDKs required)
-   Agent discovery (online and offline metadata)
-   Synchronous and asynchronous request handling
-   Health monitoring and agent registry management
-   Three MCP tools for main agent integration

## Architecture

```

            VT Code Main Agent
  (Primary decision-maker & orchestrator)



         Three MCP Tools:

         acp_call             Call remote agents
         acp_discover         Discover agents
         acp_health           Monitor health



          vtcode-acp-client
           HTTP Communication Layer
           Agent Registry
           Message Handling
           Connection Management






 Agent A     Agent B   ...  Agent N

```

## Module Structure

### `vtcode-acp-client` Library

The ACP client library is located in `vtcode-acp-client/` and provides:

#### Core Modules

1. **`client.rs`** - HTTP-based ACP communication

    - `AcpClient`: Main client for agent communication
    - `AcpClientBuilder`: Fluent builder for client configuration
    - Methods: `call_sync()`, `call_async()`, `ping()`, `discover_agent()`

2. **`discovery.rs`** - Agent registry and discovery

    - `AgentRegistry`: In-memory registry of available agents
    - `AgentInfo`: Metadata about a registered agent
    - Methods: `register()`, `find()`, `find_by_capability()`, `list_online()`

3. **`messages.rs`** - ACP message types

    - `AcpMessage`: Core message envelope
    - `AcpRequest`: Request structure
    - `AcpResponse`: Response structure
    - `MessageType`: Enum for message types (Request, Response, Error)
    - `ResponseStatus`: Status codes (Success, Failed, Timeout, Partial)

4. **`error.rs`** - Error types
    - `AcpError`: Comprehensive error handling
    - `AcpResult<T>`: Standard result type for ACP operations

### `vtcode-tools` Integration

Three MCP tools expose ACP functionality to the main agent:

1. **`AcpTool`** - Inter-agent RPC calls

    - Action: `acp_call`
    - Params: `remote_agent_id`, `action`, `args`, `method` (sync/async)

2. **`AcpDiscoveryTool`** - Agent discovery

    - Action: `acp_discover`
    - Modes: `list_all`, `list_online`, `by_capability`, `by_id`

3. **`AcpHealthTool`** - Health monitoring
    - Action: `acp_health`
    - Checks agent liveness via ping

## Usage Examples

### 1. Discovering Agents

```json
{
    "tool": "acp_discover",
    "input": {
        "mode": "list_online"
    }
}
```

Response:

```json
{
    "agents": [
        {
            "id": "data-processor",
            "name": "Data Processor",
            "base_url": "http://localhost:8081",
            "capabilities": ["bash", "python"],
            "online": true
        }
    ],
    "count": 1
}
```

### 2. Finding Agents by Capability

```json
{
    "tool": "acp_discover",
    "input": {
        "mode": "by_capability",
        "capability": "python"
    }
}
```

### 3. Calling a Remote Agent

```json
{
    "tool": "acp_call",
    "input": {
        "remote_agent_id": "data-processor",
        "action": "execute_script",
        "args": {
            "script": "import json; print(json.dumps({'status': 'ok'}))"
        },
        "method": "sync"
    }
}
```

### 4. Async Agent Call

```json
{
    "tool": "acp_call",
    "input": {
        "remote_agent_id": "long-runner",
        "action": "train_model",
        "args": {
            "epochs": 100
        },
        "method": "async"
    }
}
```

Returns immediately with `message_id`:

```json
{
    "message_id": "uuid-string",
    "status": "queued",
    "remote_agent_id": "long-runner",
    "action": "train_model"
}
```

### 5. Health Check

```json
{
    "tool": "acp_health",
    "input": {
        "agent_id": "data-processor"
    }
}
```

## Initialization

### In Rust Code

```rust
use vtcode_acp_client::{AcpClient, AgentInfo};

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
        capabilities: vec!["tool1".to_string(), "tool2".to_string()],
        metadata: Default::default(),
        online: true,
        last_seen: None,
    };

    client.registry().register(agent).await?;

    // Call the remote agent
    let result = client.call_sync(
        "remote-agent",
        "some_action".to_string(),
        serde_json::json!({"param": "value"}),
    ).await?;

    println!("Result: {}", result);
    Ok(())
}
```

### Via MCP Tools (Recommended)

The main agent simply uses the three MCP tools (`acp_call`, `acp_discover`, `acp_health`) when calling remote agents. The ACP client is initialized automatically during agent startup.

## Message Protocol

### Request Format

```json
{
    "id": "uuid",
    "type": "request",
    "sender": "vtcode",
    "recipient": "remote-agent",
    "content": {
        "action": "execute_tool",
        "args": {
            /* tool-specific args */
        },
        "sync": true,
        "timeout_secs": 30
    },
    "timestamp": "2024-01-01T12:00:00Z",
    "correlation_id": null
}
```

### Response Format

```json
{
    "id": "uuid",
    "type": "response",
    "sender": "remote-agent",
    "recipient": "vtcode",
    "content": {
        "status": "success",
        "result": {
            /* execution result */
        },
        "execution_time_ms": 245
    },
    "timestamp": "2024-01-01T12:00:00Z",
    "correlation_id": "original-request-id"
}
```

### Error Response

```json
{
    "id": "uuid",
    "type": "error",
    "sender": "remote-agent",
    "recipient": "vtcode",
    "content": {
        "code": "INVALID_ACTION",
        "message": "Unknown action: invalid_tool",
        "details": null
    },
    "timestamp": "2024-01-01T12:00:00Z",
    "correlation_id": "original-request-id"
}
```

## HTTP Endpoints (Remote Agent Requirements)

For an agent to be discoverable and callable via ACP, it must implement:

### POST `/messages`

Receive and process ACP requests.

**Request:**

```json
{
    /* ACP message */
}
```

**Response:**

```json
{
    /* ACP response */
}
```

### GET `/metadata`

Return agent metadata for discovery.

**Response:**

```json
{
    "id": "agent-id",
    "name": "Agent Name",
    "base_url": "http://localhost:8080",
    "description": "Agent description",
    "capabilities": ["action1", "action2"],
    "metadata": {},
    "online": true,
    "last_seen": "2024-01-01T12:00:00Z"
}
```

### GET `/health`

Health check endpoint.

**Response:**

```json
{
    "status": "ok",
    "timestamp": "2024-01-01T12:00:00Z"
}
```

## Configuration

Agent registry can be configured via `vtcode.toml`:

```toml
[acp]
local_agent_id = "vtcode-instance-1"
timeout_secs = 30

# Pre-registered agents
[[acp.agents]]
id = "data-processor"
name = "Data Processor"
base_url = "http://localhost:8081"
capabilities = ["bash", "python"]

[[acp.agents]]
id = "model-trainer"
name = "Model Trainer"
base_url = "http://localhost:8082"
capabilities = ["tensorflow", "pytorch"]
```

## Performance Considerations

### Synchronous Calls

-   Blocks main agent until response received
-   Best for short-running tasks (<5 seconds)
-   Recommended for control flow decisions

### Asynchronous Calls

-   Returns immediately with `message_id`
-   Main agent continues processing
-   Best for long-running tasks (>5 seconds)
-   Main agent must poll or subscribe for updates

### Timeout Handling

-   Default timeout: 30 seconds
-   Configurable per request
-   Async calls may timeout gracefully

### Registry Caching

-   Agent registry is in-memory
-   Agents stay registered until explicitly unregistered
-   Status updates via `update_status()` method
-   Health check marks agents online/offline

## Error Handling

Common error scenarios:

```rust
// Agent not found
AcpError::AgentNotFound("agent-id".to_string())

// Network/connection failure
AcpError::NetworkError("Connection refused".to_string())

// Remote agent returned error
AcpError::RemoteError {
    agent_id: "remote-agent".to_string(),
    message: "Action not supported".to_string(),
    code: Some(400),
}

// Request timeout
AcpError::Timeout("Request exceeded 30s timeout".to_string())

// Message serialization failed
AcpError::SerializationError("Invalid JSON".to_string())
```

## Roadmap

Planned enhancements:

-   [ ] Agent authentication (JWT/mutual TLS)
-   [ ] Message encryption
-   [ ] Decentralized agent discovery
-   [ ] Agent service mesh integration
-   [ ] Distributed tracing with OpenTelemetry
-   [ ] Agent metrics collection
-   [ ] Message queuing for resilience
-   [ ] Retry policies and circuit breakers

## See Also

-   [ACP Official Spec](https://agentcommunicationprotocol.dev/)
-   [MCP Integration Guide](./MCP.md)
-   [vtcode Configuration](./CONFIG.md)
