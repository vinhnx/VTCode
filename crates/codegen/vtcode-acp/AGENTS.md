# vtcode-acp

[Root AGENTS.md](../../../AGENTS.md) | Agent Client Protocol (Zed integration). Canonical ACP entrypoint.

## Modules

- `zed/` canonical SACP agent (`agent/handlers.rs` wiring, `connection.rs` wraps `ConnectionTo<Client>`).
- `tooling/`, `tooling_provider.rs` tool adapters and registry bridge; `permissions/`, `workspace/`, `reports/`,
  `error/`, `discovery/`.
- Legacy HTTP stack, crate-internal and not an ACP transport: `capabilities/`, `client.rs`, `client_v2/`,
  `session/`, `messages.rs`, `jsonrpc/`, `transport/`. Do not extend it.

## Rules

- `zed/` (`ZedAgent`, `StandardAcpAdapter`) is the spec surface; `vtcode-core::acp` is only a compatibility facade.
- ACP 1.0.1 uses the SACP builder plus handlers, not the old `impl acp::Agent` trait.
- `register_acp_connection()` is a global `OnceLock`: call it once, after `connect_with` receives `cx`.
- Client RPCs (`fs/read_text_file`, `session/request_permission`, any `block_task()`) run only inside a
  `cx.spawn(...)` task; calling them from a request handler deadlocks the dispatch loop.
- Command tools (`exec_command`, `run_pty_cmd`) always run through the local `ToolRegistry`, never the client's
  `terminal/*` RPCs. A non-zero `exit_code` reports `failed`, never `completed`.
- A granted permission calls `preapprove_granted_local_tool` before execution; never preapprove a configured `deny`.
- `admit_local_tool_call` re-checks the primary agent's policy per call; provider selection honors
  `providers_whitelist`, and key resolution uses the configured `AuthCredentialsStoreMode`.
- Session lifecycle `new|load|list|resume|close|delete|logout`: `new|load|resume` need an absolute `cwd`;
  `resume` is live-only; unknown ids fail `unknown_session`.

## Gotchas

- Legacy `PROTOCOL_VERSION` date strings are not upstream `ProtocolVersion::V1`.
- `SessionUpdateNotification` has a direct wire shape: new variants update it and its regression tests.
- Stdio EOF, timeout, and cancel must clear pending calls; keep frames and stderr bounded (`line_framing`
  with `ExcludeLf`).

Details: [maintainer notes](../../../docs/development/maintainer-notes/vtcode-acp.md) — read before changing a subsystem.
