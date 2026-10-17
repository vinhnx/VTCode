# vtcode-a2a

Agent2Agent (A2A) Protocol support for VT Code.

## Overview

Layer 0 crate with zero internal vtcode dependencies. Provides the A2A protocol implementation for agent-to-agent
communication, including agent discovery, task management, and webhook notifications.

## Module Groups

| Area         | Modules        | Description                                         |
| ------------ | -------------- | --------------------------------------------------- |
| Agent Card   | `agent_card`   | Agent discovery and capability advertisement        |
| Client       | `client`       | A2A protocol client                                 |
| CLI          | `cli`          | CLI interface for A2A commands                      |
| Errors       | `errors`       | A2A-specific error types                            |
| RPC          | `rpc`          | JSON-RPC message types and protocol constants       |
| Server       | `server`       | HTTP server (feature-gated: `a2a-server`)           |
| Task Manager | `task_manager` | Task lifecycle management                           |
| Types        | `types`        | Core A2A protocol types (Message, Task, Part, etc.) |
| Webhook      | `webhook`      | Push notification support                           |

## Key Concepts

### Agent Card

Agent cards describe agent identity, version, capabilities, and security requirements. They enable agents to discover
and communicate with each other.

### Task Management

The task manager handles the lifecycle of A2A tasks, including creation, execution, and completion tracking.

### Webhook Notifications

`WebhookNotifier` is always available (not feature-gated) — only the HTTP server is gated behind `a2a-server`.

### Security Boundaries

The agent card is public for discovery; `/a2a` and `/a2a/stream` require the server bearer token. Browser cross-origin
access is disabled by default. Webhook destinations require HTTPS or exact localhost/loopback HTTP, and redirects are
not followed.

## Feature Flags

- `a2a-server` — enables the HTTP server module
- Feature chain: vtcode binary `a2a-server` → vtcode-core `a2a-server` → vtcode-a2a `a2a-server`

## Rules

- The `server` module is feature-gated behind `a2a-server` — never import unconditionally
- `shutdown_signal_logged()` is defined in lib.rs (not a separate module)
- Re-export facade in vtcode-core (`a2a/mod.rs`) must stay in sync with feature gates

## See Also

- [A2A Documentation](../a2a/) — protocol guides and examples
- [Architecture Guide](../ARCHITECTURE.md) — explicit delegation model
