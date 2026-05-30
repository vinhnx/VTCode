# vtcode-acp

[Root AGENTS.md](../AGENTS.md) | Agent Client Protocol (Zed integration). Canonical ACP entrypoint.

## Modules

`capabilities/` protocol negotiation | `client/` legacy client (deprecated) | `client_v2/` current ACP client | `discovery/` agent registry | `session/` session lifecycle | `transport/` StdioTransport | `jsonrpc/` JSON-RPC types | `tooling/` tool adapters | `zed/` Zed-specific adapter | `workspace/` workspace helpers | `permissions/` permission flow | `reports/` reporting | `error/` AcpError

## Rules

- `AcpClientV2` is the current API. `AcpClient` is deprecated since 0.60.0.
- `StandardAcpAdapter` / `ZedAcpAdapter` in `zed/` bridge protocol to Zed.
- `register_acp_connection()` is a global `OnceLock` — call once from host protocol.
- `acp` module in `vtcode-core` is the compatibility facade; canonical code lives here.

## Gotchas

- `PROTOCOL_VERSION` + `SUPPORTED_VERSIONS` control negotiation — update both when protocol changes.
- `messages/` module types are deprecated — use `jsonrpc/` module instead.
