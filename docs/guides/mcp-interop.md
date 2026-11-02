# MCP Interop for Client and Server Maintainers

VT Code speaks the [Model Context Protocol](https://modelcontextprotocol.io/llms.txt) on both sides: it is an **MCP
client** that connects to your MCP server, and it exposes **MCP server surfaces** that your MCP client can connect to.
This page is written for maintainers of other MCP servers and MCP clients who want to interoperate with VT Code or
contribute interoperability fixes. For user-facing configuration walk-throughs, see the
[MCP Integration Guide](./mcp-integration.md).

## What VT Code implements

| Surface                            | Direction        | Transports                                                       | Source                                                       |
| ---------------------------------- | ---------------- | ---------------------------------------------------------------- | ------------------------------------------------------------ |
| MCP client (`[mcp.providers]`)     | VT Code → server | `stdio`, HTTP with server-sent events (Streamable HTTP)          | `crates/codegen/vtcode-mcp` (rmcp-based client)              |
| Embedded MCP server (`[mcp.server]`) | client → VT Code | SSE or HTTP on loopback, with an explicit tool allowlist        | `crates/codegen/vtcode-config/src/mcp.rs` (`McpServerConfig`) |
| Read-only remote MCP (`webmcp`)    | client → VT Code | Streamable HTTP at `/mcp`, legacy HTTP+SSE at `/sse/`            | `crates/codegen/vtcode-webmcp/src/remote_mcp.rs`             |

## Server maintainers: register your MCP server in VT Code

VT Code connects to servers over `stdio` and HTTP transports, with optional OAuth 2.1 authorization for hosted HTTP
endpoints (`vtcode mcp login`), per-provider allowlists, and tool discovery with prompt-caching-friendly deferral. The
`vtcode.toml` mapping is documented in the [MCP Integration Guide](./mcp-integration.md#configuring-mcp-providers) and
the schema lives in `crates/codegen/vtcode-config/src/mcp/transport.rs`.

If your server is a hosted Streamable HTTP endpoint, follow the existing partner integrations —
[Parallel Search](./parallel-search-mcp.md), [Cohesivity](./cohesivity-mcp.md), and
[Memcode](./memcode-mcp.md) are runnable examples with pinned endpoints and example TOML. A server that supports the
current MCP `protocol_version` and SSE streaming works without patches; pin the negotiated version explicitly with
`protocol_version` in the provider block.

## Client maintainers: connect your MCP client to VT Code

Two opt-in server surfaces accept external MCP clients.

### Read-only search/fetch surface (`vtcode webmcp serve --mcp`)

Exposes exactly two tools — `search` and `fetch` — over a workspace root. `/sse/` is the canonical public URL for
OpenAI-compatible clients; `/mcp` is the modern Streamable HTTP alias. The listener binds to loopback only; put a
TLS-terminating proxy in front of it that validates the external OAuth bearer and injects the internal token. VT Code
advertises `/.well-known/oauth-protected-resource` but does not implement OAuth or JWKS endpoints. Configuration,
limits (20 results, 256 scanned files, 16 MiB content by default), and the proxy contract are documented in the
[WebMCP guide → read-only remote MCP surface](../user-guide/webmcp.md#use-the-read-only-remote-mcp-surface).

### Embedded MCP server (`[mcp.server]`)

For a general-purpose surface, enable the `[mcp.server]` table to expose selected built-in tools over SSE or HTTP:

```toml
[mcp.server]
enabled = true
bind_address = "127.0.0.1"
port = 3030
transport = "sse"
exposed_tools = ["read_file"]
```

Only tool names listed in `exposed_tools` are reachable, and the `[mcp.security]` table adds authentication, rate
limits, and argument validation. See [MCP Integration Guide → Security and validation](./mcp-integration.md#security-and-validation).

## Interop conventions VT Code follows

These behaviors are deliberate contract, maintained by tests; interop fixes should preserve them:

- **Tool result fidelity.** Successful results keep `content`, public annotations, and `structuredContent` exactly as
  the server returned them, including an empty object or explicit null. Protocol `_meta` is omitted at the result,
  content-block, and embedded-resource envelope levels and never leaks into error text. See
  [MCP Integration Guide → Tool result boundary](./mcp-integration.md#tool-result-boundary).
- **Deterministic catalogs.** LLM-visible tool/resource/prompt lists are sorted; providers are iterated by name and the
  first provider by name wins on collisions. `list_changed` refreshes are burst-coalesced and token-bucket limited.
- **Untrusted resource content.** Discovered server text is treated as bounded untrusted data; host policy is enforced
  via allowlists (`[mcp.allowlist]`), trust checks, and tool registration policy, not via content rewriting.
- **No redirects with auth headers.** The HTTP client never follows redirects, so custom authorization headers cannot
  leak to cross-origin targets.
- **Protocol alignment.** Configuration naming mirrors the upstream
  [MCP specification](https://modelcontextprotocol.io/specification/2025-06-18/architecture/index.md), and the
  negotiation behavior (`legacy` vs `auto` handshake) is documented per-provider in the integration guide.

## Where to look in the source

| Area                                  | Location                                                        |
| ------------------------------------- | --------------------------------------------------------------- |
| Client and connection pooling         | `crates/codegen/vtcode-mcp/src/` (`client.rs`, `provider.rs`)   |
| Transports                            | `crates/codegen/vtcode-mcp/src/rmcp_transport.rs`               |
| Tool discovery and caching            | `crates/codegen/vtcode-mcp/src/tool_discovery.rs`               |
| Read-only server surface              | `crates/codegen/vtcode-webmcp/src/remote_mcp.rs`                |
| Config schema                         | `crates/codegen/vtcode-config/src/mcp.rs` and `mcp/transport.rs`|
| Crate-specific conventions            | `crates/codegen/vtcode-mcp/AGENTS.md`                           |

## Testing your integration

Run the MCP suites with `cargo nextest` (unit tests are in-module and do not need live servers):

```bash
cargo nextest run -p vtcode-mcp
cargo nextest run -p vtcode-webmcp remote_mcp
```

For an end-to-end client check against a live server, the Context7 smoke test spawns the official
`@upstash/context7-mcp` package on demand:

```bash
cargo nextest run -p vtcode-core -E 'test(context7_list_tools_smoke)' -- --ignored
```

Hex / shell based transport tests are Unix-only. Read `crates/codegen/vtcode-mcp/AGENTS.md` for the crate's testing
ground rules before touching transports.

## Proposing an interop fix or a provider guide

1. Open an issue describing the incompatibility, including MCP `protocol_version`, transport, and (when possible)
   `vtcode trajectory` output from the failing session.
2. Add or extend a regression test next to the affected surface (`vtcode-mcp` for client behavior,
   `vtcode-webmcp` for the remote server surface). Coalescing, determinism, and boundary behavior have existing test
   patterns to follow.
3. Patch your integration guide link as runnable TOML under `docs/examples/` when adding a hosted provider; partner
   guides live alongside [Parallel Search](./parallel-search-mcp.md) and the others listed above.
4. Submit with Conventional Commits and run the local gates per the
   [contribution guide](../CONTRIBUTING.md).

Interop reports from maintainer perspectives — including servers VT Code fails to connect to and clients that fail
against VT Code's surfaces — are as valuable as patches.
