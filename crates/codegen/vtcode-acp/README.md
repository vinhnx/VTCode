# vtcode-acp

`vtcode-acp` is the canonical ACP crate for VT Code.

Canonical surface is the SACP stdio agent in `src/zed/` (`StandardAcpAdapter`,
`ZedAcpAdapter`, `ZedAgent` on upstream `agent-client-protocol`).

Legacy crate-internal HTTP stack (`capabilities`, `client`, `client_v2`, `session`,
`messages`, `jsonrpc`, `transport` HTTP `/rpc`+SSE types with date versions like
`2025-01-01`) is not the ACP stdio transport and must not be extended for new
protocol work. See `AGENTS.md` for the module map.

<!-- cargo-rdme start -->

ACP (Agent Communication Protocol) support for VT Code.

This crate exposes both the ACP client library and the VT Code Zed bridge. Downstream crates should treat this as the
canonical ACP entrypoint.

<!-- cargo-rdme end -->

## Public entrypoints

- `StandardAcpAdapter` and `ZedAcpAdapter` for launching VT Code over ACP stdio (canonical)
- Legacy HTTP helpers (`AcpClientV2`, `AcpClient`) are crate-internal and not ACP stdio transports

## API reference

See [docs.rs/vtcode-acp](https://docs.rs/vtcode-acp).

## Related docs

- [ACP integration guide](../docs/acp/ACP_INTEGRATION.md)
- [ACP quick reference](../docs/acp/ACP_QUICK_REFERENCE.md)
