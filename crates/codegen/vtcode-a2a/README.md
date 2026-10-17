# vtcode-a2a

Agent2Agent (A2A) Protocol support for VT Code. Provides client and server implementations for the A2A protocol,
enabling inter-agent communication.

<!-- cargo-rdme start -->

Agent2Agent (A2A) Protocol support for VT Code.

<!-- cargo-rdme end -->

## Modules

| Module         | Purpose                                                 |
| -------------- | ------------------------------------------------------- |
| `agent_card`   | Agent discovery and capability advertisement            |
| `client`       | A2A protocol client                                     |
| `cli`          | CLI interface for A2A commands                          |
| `errors`       | A2A-specific error types                                |
| `rpc`          | JSON-RPC message types and protocol constants           |
| `server`       | Authenticated HTTP server (feature-gated: `a2a-server`) |
| `task_manager` | Task lifecycle management                               |
| `types`        | Core A2A protocol types (Message, Task, Part, etc.)     |
| `webhook`      | Push notification support                               |

## Features

| Feature      | Description                                              |
| ------------ | -------------------------------------------------------- |
| `a2a-server` | Enables the HTTP server module (axum, tower, tower-http) |

## Usage

```rust
use vtcode_a2a::{AgentCard, A2aClient, TaskManager};

let card = AgentCard::vtcode_default("http://localhost:8080");
let client = A2aClient::new("http://localhost:8080");
```

## Server security

The agent card endpoint is public for discovery. The `/a2a` and `/a2a/stream` endpoints require an
`Authorization: Bearer <token>` header. `A2aServerState::new` generates a random token; use `auth_token()` to pass it to
a client, or use `new_with_auth_token` to provide one explicitly. Cross-origin browser access is disabled by default.

Webhook destinations must use HTTPS, or HTTP to the exact `localhost` host or an IP loopback address. Webhook requests
do not follow redirects.

```rust
use vtcode_a2a::{A2aClient, AgentCard, TaskManager};
use vtcode_a2a::server::A2aServerState;

let state = A2aServerState::new(TaskManager::new(), AgentCard::vtcode_default("http://localhost:8080"));
let client = A2aClient::new("http://localhost:8080")?.with_bearer_token(state.auth_token());
```

The `vtcode a2a` CLI reads the token from `VTCODE_A2A_TOKEN`. Keeping the token in the environment avoids exposing it in
process arguments.

## API reference

<https://docs.rs/vtcode-a2a>
