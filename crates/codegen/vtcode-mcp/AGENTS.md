# vtcode-mcp

[Root AGENTS.md](../AGENTS.md) | Model Context Protocol client, connection pooling, and tool discovery. Layer 1 crate -- depends on vtcode-config, vtcode-commons, vtcode-utility-tool-specs.

## Module Groups

| Area | Modules |
|---|---|
| Client | `client.rs`, `provider.rs`, `rmcp_client.rs` |
| Transport | `rmcp_transport.rs`, `connection_pool.rs` |
| Discovery | `tool_discovery.rs`, `tool_discovery_cache.rs`, `schema.rs` |
| Types | `types.rs`, `traits.rs`, `errors.rs`, `enhanced_config.rs` |
| Utils | `utils.rs` |

## Rules

- `cli.rs` stays in vtcode-core (depends on `crate::cli::input_hardening`).
- Re-export facade in vtcode-core (`mcp/mod.rs`) must stay in sync.
- `rmcp_client` is `pub(crate)` -- not part of the public API.
- `convert_to_rmcp()` is `pub(crate)` -- internal JSON bridge.

## Gotchas

- `enhanced_config.rs` uses `#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]`.
- `rmcp-reqwest` is a renamed `reqwest` with rustls features -- not the same as the workspace `reqwest`.
- `DEFAULT_ENV_VARS` is platform-conditional (`#[cfg(unix)]` / `#[cfg(windows)]`).
