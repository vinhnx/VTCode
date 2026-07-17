# vtcode-a2a

[Root AGENTS.md](../AGENTS.md) | Agent2Agent (A2A) Protocol support. Layer 0 crate — zero internal vtcode dependencies.

## Module Groups

| Area | Modules |
|---|---|
| Agent Card | `agent_card` — agent discovery and capability advertisement |
| Client | `client` — A2A protocol client |
| CLI | `cli` — CLI interface for A2A commands |
| Errors | `errors` — A2A-specific error types |
| RPC | `rpc` — JSON-RPC message types and protocol constants |
| Server | `server` — HTTP server (feature-gated: `a2a-server`) |
| Task Manager | `task_manager` — task lifecycle management |
| Types | `types` — core A2A protocol types (Message, Task, Part, etc.) |
| Webhook | `webhook` — push notification support |

## Rules

- The `server` module is feature-gated behind `a2a-server` — never import unconditionally.
- `shutdown_signal_logged()` is defined in lib.rs (not a separate module) — used by server.rs.
- Re-export facade in vtcode-core (`a2a/mod.rs`) must stay in sync with feature gates.

## Gotchas

- `server.rs` uses `crate::shutdown_signal_logged` (not vtcode-core's shutdown) — local function.
- Feature flag chain: vtcode binary `a2a-server` -> vtcode-core `a2a-server` -> vtcode-a2a `a2a-server`.
- `WebhookNotifier` is always available (not feature-gated) — only the HTTP server is gated.
