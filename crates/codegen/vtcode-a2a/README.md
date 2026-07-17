# vtcode-a2a

Agent2Agent (A2A) Protocol support for VT Code. Provides client and server
implementations for the A2A protocol, enabling inter-agent communication.

<!-- cargo-rdme start -->

Agent2Agent (A2A) Protocol support for VT Code.

<!-- cargo-rdme end -->

## Modules

| Module | Purpose |
|---|---|
| `agent_card` | Agent discovery and capability advertisement |
| `client` | A2A protocol client |
| `cli` | CLI interface for A2A commands |
| `errors` | A2A-specific error types |
| `rpc` | JSON-RPC message types and protocol constants |
| `server` | HTTP server (feature-gated: `a2a-server`) |
| `task_manager` | Task lifecycle management |
| `types` | Core A2A protocol types (Message, Task, Part, etc.) |
| `webhook` | Push notification support |

## Features

| Feature | Description |
|---|---|
| `a2a-server` | Enables the HTTP server module (axum, tower, tower-http) |

## Usage

```rust
use vtcode_a2a::{AgentCard, A2aClient, TaskManager};

let card = AgentCard::vtcode_default("http://localhost:8080");
let client = A2aClient::new("http://localhost:8080");
```

## API reference

<https://docs.rs/vtcode-a2a>
