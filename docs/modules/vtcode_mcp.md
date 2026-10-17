# vtcode-mcp

Model Context Protocol (MCP) client, connection pooling, and tool discovery for VT Code.

## Overview

Layer 1 crate that provides MCP integration, enabling VT Code to connect to MCP servers for extended tool capabilities.
Includes connection pooling, tool discovery, and schema validation.

## Module Groups

| Area      | Modules                                                     | Description                               |
| --------- | ----------------------------------------------------------- | ----------------------------------------- |
| Client    | `client.rs`, `provider.rs`, `rmcp_client.rs`                | MCP client implementations                |
| Transport | `rmcp_transport.rs`, `connection_pool.rs`                   | Transport layer and connection management |
| Discovery | `tool_discovery.rs`, `tool_discovery_cache.rs`, `schema.rs` | Tool discovery and caching                |
| Types     | `types.rs`, `traits.rs`, `errors.rs`, `enhanced_config.rs`  | Type definitions and configuration        |
| Utils     | `utils.rs`                                                  | Utility functions                         |

## Key Components

### Client

The MCP client handles communication with MCP servers, including request/response handling and protocol negotiation.

### Connection Pool

Connection pooling manages multiple MCP server connections efficiently, reducing overhead for repeated requests.

### Tool Discovery

Tool discovery automatically finds and catalogs available tools from MCP servers:

- `tool_discovery.rs` — discovers tools from connected servers
- `tool_discovery_cache.rs` — caches discovered tools for performance
- `schema.rs` — tool schema validation

## Architecture Notes

- `cli.rs` stays in vtcode-core (depends on `crate::cli::input_hardening`)
- `rmcp_client` is `pub(crate)` — not part of the public API
- `convert_to_rmcp()` is `pub(crate)` — internal JSON bridge
- `rmcp-reqwest` is a renamed `reqwest` with rustls features — not the same as the workspace `reqwest`

## Configuration

MCP servers are configured through `vtcode.toml`:

```toml
[mcp]
servers = [
  { name = "example", url = "http://localhost:3000" }
]
```

Environment variables:

- `DEFAULT_ENV_VARS` is platform-conditional (`#[cfg(unix)]` / `#[cfg(windows)]`)

## Dependencies

- `vtcode-config` — MCP configuration
- `vtcode-commons` — shared utilities
- `vtcode-utility-tool-specs` — tool specifications

## See Also

- [MCP Integration Guide](../mcp/MCP_INTEGRATION_GUIDE.md) — setup and usage
- [Tool Specifications](../tools/TOOL_SPECS.md) — tool definitions
- [Security Model](../security/SECURITY_MODEL.md) — MCP security considerations
