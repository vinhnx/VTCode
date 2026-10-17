<!-- Compact maintainer rules retain the repository instruction line budget. -->
<!-- markdownlint-disable MD013 -->
# vtcode-acp

[Root AGENTS.md](../../../AGENTS.md) | Agent Client Protocol (Zed integration). Canonical ACP entrypoint.

## Modules

`zed/` canonical SACP agent (upstream `agent-client-protocol` stdio) | `tooling/` tool adapters | `tooling_provider.rs` registry→provider `ToolDefinition` bridge | `workspace/` workspace helpers | `permissions/` permission flow | `reports/` reporting | `error/` AcpError | `discovery/` agent registry | legacy HTTP stack (crate-internal, not spec transport): `capabilities/` date versions, `client.rs` (deprecated), `client_v2/` HTTP `/rpc`+SSE, `session/` custom updates, `messages.rs` (deprecated), `jsonrpc/`, `transport/`

`zed/agent/handlers.rs` is the canonical SACP handler wiring; `zed/connection.rs` wraps the SACP `ConnectionTo<Client>` handle.

## Rules

- `zed/` (`StandardAcpAdapter`/`ZedAcpAdapter`, `ZedAgent`) is the canonical spec surface. `AcpClient` (deprecated 0.60.x) and `AcpClientV2` (HTTP `/rpc`+SSE, crate-internal) are legacy and not ACP stdio transports — do not extend them. `register_acp_connection()` is a global `OnceLock<Arc<ConnectionHandle>>` — call once from the host protocol after the SACP `connect_with` closure receives the `cx`. `acp` module in `vtcode-core` is the compatibility facade; canonical code lives here. ACP 1.0.1 uses SACP builder + handlers, not the old `impl acp::Agent` trait. `handlers.rs` registers SACP request/notification handlers around `ZedAgent`. `ZedAgent` is `Send + Sync` (`Arc<Mutex<_>>` + `AtomicBool`) so it can be moved into SACP `cx.spawn` tasks. Tool execution RPCs (`fs/read_text_file`, `terminal/create`, `session/request_permission`) must be called from inside a `cx.spawn(...)` task — invoking them directly from an SACP request handler deadlocks the dispatch loop.
- `ZedAgent` carries the configured `AuthCredentialsStoreMode`; provider-key resolution must use that mode rather than the platform default.
- Session lifecycle covers `new|load|list|resume|close|delete` + `logout` (all advertised). Single-workspace: `new|load|resume` require absolute `cwd` (`invalid_cwd` otherwise); non-workspace absolute `cwd` warns and uses workspace. `resume` is live-only (`unknown_session` after `close`); `load` attaches archive history. `close`/`delete` drop the live handle and cancel work, unknown ids fail `unknown_session`.

## Gotchas

- Legacy `capabilities.rs` `PROTOCOL_VERSION`/`SUPPORTED_VERSIONS` date strings belong to the old HTTP stack — do not confuse with upstream `agent_client_protocol::schema::ProtocolVersion::V1`. Update both legacy constants only if touching that stack.
- `messages.rs` types are deprecated — use `jsonrpc/` module instead (both legacy HTTP stack).
- `ConnectionHandle` wraps `agent_client_protocol::ConnectionTo<Client>`. The `block_task()` future returned by `cx.send_request(...).block_task()` is **only safe in a `cx.spawn` task**; calling it from a request handler deadlocks.
- The `acp` module re-exports `agent_client_protocol::schema::v1::*` plus `ProtocolVersion` from `schema::*`. `Client` and `Agent` (role structs) are at the crate root.
- `SessionUpdateNotification` decodes through a direct wire shape for SSE performance; new update variants must update that shape and its regression tests.
- Permission-flow tests use the duplex SACP connection harness; preserve allow, deny, cancel, unknown-option, and request-failure coverage. Stdio EOF, timeout, and cancellation paths must clear pending calls; frames and stderr remain bounded and sanitized. Line reads delegate to commons `line_framing` with `ExcludeLf` (CR is retained); keep the local `BoundedLine` adapter.
