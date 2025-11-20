# VTCode MCP Client Overview

## Current Positioning

VTCode ships a lean Model Context Protocol (MCP) client that focuses on outbound
connections to third-party MCP providers. The implementation is built on top of
the [`rmcp` crate](https://docs.rs/rmcp/latest/rmcp/) and follows the
[official protocol guidance](https://modelcontextprotocol.io/docs/getting-started/intro).
The legacy in-process MCP server, circuit breaker layer, and `EnhancedMcpClient`
wrapper have been removed. All integration points now flow through a single
`McpClient` type located in `vtcode-core/src/mcp_client.rs`.

Key capabilities that remain available:

-   Connect to multiple providers (stdio or experimental HTTP) using rmcp client
    transports.
-   Index exposed tools, resources, and prompts with allow-list enforcement.
-   Execute tools via the `McpToolExecutor` trait used by the VTCode tool registry.
-   Surface provider status, metadata, and resource contents to the TUI.

## Configuration Summary

All settings live in `vtcode.toml` under the `[mcp.client]` namespace. The
important fields are:

```toml
[mcp.client]
enabled = true
experimental_use_rmcp_client = true
tool_timeout_seconds = 30

[[mcp.client.providers]]
name = "my-toolchain"
enabled = true
transport = { type = "stdio", command = "node", args = ["mcp-server.js"] }
startup_timeout_ms = 10000
max_concurrent_requests = 4

[mcp.client.allowlist]
tools = ["my-toolchain.read", "my-toolchain.write"]
resources = ["my-toolchain://*"]
prompts = []

[mcp.client.security.validation]
max_argument_size = 65536
path_traversal_protection = true
```

Settings for the removed MCP server are ignored; they can be deleted whenever it
is convenient without affecting the runtime.

## Initialization Flow

1. `McpClient::new` clones the allow-list and prepares caches but does not start
   any transports.
2. `McpClient::initialize` iterates over enabled providers, skipping those that
   require the HTTP transport when the experimental flag is off.
3. Each provider is connected with `McpProvider::connect` and then initialized
   using a negotiated timeout. On success the client pulls tool metadata and
   records provider mappings for quick lookup.
4. Allow-list or transport failures are logged with context and the provider is
   skipped, keeping the rest of the session responsive.

Initialization happens sequentially today. Providers typically finish within
millisecond-to-second timeframes; if concurrency becomes a hotspot we can safely
wrap the connect/initialize block with `FuturesUnordered` because the rest of the
client only uses `Arc` and `RwLock` guarded state.

## Tool Execution Path

-   The tool registry calls `McpToolExecutor::execute_mcp_tool`, routed directly to
    `McpClient::execute_tool_with_validation`.
-   Arguments are serialized and checked against `max_argument_size` plus basic
    path-traversal heuristics.
-   The provider is resolved from an index (refreshing on cache miss) and
    `call_tool` is invoked with allow-list snapshots and configured timeouts.
-   Results are normalized into a provider/tool tagged JSON object that the tool
    registry can forward to higher-level workflows.

The client no longer retries failed calls automatically. Any `rmcp` or provider
errors bubble up to the caller with `anyhow::Context`, letting the UI decide how
to surface the failure.

## Status and Discovery APIs

-   `list_tools`, `list_resources`, and `list_prompts` optionally refresh on demand
    to keep UI panels in sync with remote state.
-   `read_resource` downloads the full MCP resource payload, preserving provider
    metadata and MIME hints for downstream rendering.
-   `get_status` reports whether the client is enabled, how many providers are
    active, and exposes the cached allow-list snapshot for debugging.

## Error Handling Approach

-   `anyhow::Result` is used throughout; every transport operation adds contextual
    messaging so failures are actionable.
-   Timeouts rely on provider-specific overrides with a global fallback from the
    configuration (`tool_timeout_seconds` and `request_timeout_seconds`).
-   Environment inheritance for stdio transports is funneled through
    `create_env_for_mcp_server`, which populates a curated list of safe variables
    (PATH, HOME, TZ, etc.) combined with provider overrides.

## Migration Notes for Legacy Code

-   Delete imports of `mcp_client_enhanced`, `mcp_integration`, `mcp_server`, and
    `circuit_breaker` modules. Their functionality is no longer present.
-   Replace `EnhancedMcpClient::initialize_async()` with `McpClient::initialize()`.
    The return type changes from a report struct to plain `Result<()>`.
-   Remove references to `InitializationReport`, `McpIntegrationWrapper`, and the
    old circuit breaker controls. They are not compiled anymore.
-   Configuration clean-up: `[mcp.server]` tables, circuit-breaker sections, and
    enhanced client toggles may be pruned from `vtcode.toml`.

## Troubleshooting Checklist

-   Enable tracing (`RUST_LOG=vtcode_core::mcp_client=debug`) to follow the rmcp
    handshake and tool-fetch lifecycle.
-   Ensure provider binaries are on the PATH inherited by the VTCode process; the
    client intentionally starts transports with a minimal environment.
-   When HTTP transports are required, set
    `experimental_use_rmcp_client = true` and confirm TLS or bearer token settings
    match provider expectations.
-   Verify the allow-list admits the tool/resource names you expect; denied items
    are silently skipped during indexing for safety.

This document should be the starting point before diving into the code. For
deeper protocol details consult the upstream rmcp documentation and the Model
Context Protocol specification linked above.
