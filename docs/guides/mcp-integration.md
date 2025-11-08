# MCP Integration Guide

This guide outlines how VT Code connects to external MCP (Model Context Protocol) servers, how
allowlists control tool access, and how to troubleshoot common configuration issues. The
configuration surface documented below maps directly to the serde structures in
`vtcode-config/src/mcp.rs`, so the examples here stay aligned with the loader that powers the
CLI and the reusable `vtcode-config` crate. For the canonical protocol behaviour and the latest
server/client expectations, consult the upstream MCP reference index at
`https://modelcontextprotocol.io/llms.txt`. That index links to the full architecture, transport,
authorisation, logging, and resource specification documents maintained by the MCP community.

## MCP Specification Map

The `llms.txt` index enumerates every normative reference you may need while configuring VT Code.
When you need deeper protocol detail, jump straight from this guide to the most relevant
specification chapter:

| Topic | Primary reference |
| ----- | ----------------- |
| Architecture, lifecycle, and key changes | [Architecture overview](https://modelcontextprotocol.io/specification/2025-06-18/architecture/index.md), [Lifecycle](https://modelcontextprotocol.io/specification/2025-06-18/basic/lifecycle.md), [Changelog](https://modelcontextprotocol.io/specification/2025-06-18/changelog.md) |
| Transports and cancellation | [Transports](https://modelcontextprotocol.io/specification/2025-06-18/basic/transports.md), [Cancellation](https://modelcontextprotocol.io/specification/2025-06-18/basic/utilities/cancellation.md), [Progress](https://modelcontextprotocol.io/specification/2025-06-18/basic/utilities/progress.md) |
| Server-side concepts | [Server overview](https://modelcontextprotocol.io/specification/2025-06-18/server/index.md), [Tools](https://modelcontextprotocol.io/specification/2025-06-18/server/tools.md), [Resources](https://modelcontextprotocol.io/specification/2025-06-18/server/resources.md), [Prompts](https://modelcontextprotocol.io/specification/2025-06-18/server/prompts.md), [Logging](https://modelcontextprotocol.io/specification/2025-06-18/server/utilities/logging.md), [Pagination](https://modelcontextprotocol.io/specification/2025-06-18/server/utilities/pagination.md) |
| Client responsibilities | [Client overview](https://modelcontextprotocol.io/specification/2025-06-18/client/roots.md), [Sampling](https://modelcontextprotocol.io/specification/2025-06-18/client/sampling.md), [Elicitation](https://modelcontextprotocol.io/specification/2025-06-18/client/elicitation.md) |
| Security and governance | [Security best practices](https://modelcontextprotocol.io/specification/2025-06-18/basic/security_best_practices.md), [Authorization](https://modelcontextprotocol.io/specification/2025-06-18/basic/authorization.md), [Understanding authorization](https://modelcontextprotocol.io/docs/tutorials/security/authorization.md), [Governance](https://modelcontextprotocol.io/community/governance.md) |
| Tooling and debugging | [MCP Inspector](https://modelcontextprotocol.io/docs/tools/inspector.md), [Example clients](https://modelcontextprotocol.io/clients.md), [Example servers](https://modelcontextprotocol.io/examples.md) |

Keep the specification open while editing `vtcode.toml`; VT Code intentionally mirrors the naming
and behavioural guarantees defined there.

## Configuring MCP Providers

### Core `[mcp]` table

Open `vtcode.toml` and ensure the global MCP section is enabled. The top-level table mirrors the
`McpClientConfig` defaults, letting you tune concurrency, timeout, and transport behaviour in one
place. All values fall back to the defaults compiled into `vtcode-config`, summarised in the
following table:

| Key | Default | Purpose |
| --- | ------- | ------- |
| `enabled` | `false` | Turns all MCP wiring on or off at once. |
| `max_concurrent_connections` | `5` | Global connection pool shared across providers. |
| `request_timeout_seconds` | `30` | Envelope timeout for lifecycle requests (mirrors MCP [Lifecycle](https://modelcontextprotocol.io/specification/2025-06-18/basic/lifecycle.md)). |
| `retry_attempts` | `3` | Retry budget for transport failures before surfacing an error. |
| `startup_timeout_seconds` | `null` | Optional provider start handshake timeout; inherits `request_timeout_seconds` when unset. |
| `tool_timeout_seconds` | `null` | Optional per-tool guard for long-running operations. |
| `experimental_use_rmcp_client` | `true` | Enables the Rust MCP client so HTTP/streamable transports are available. |
| `ui` | `mode = "compact"`, `max_events = 50`, `show_provider_names = true` | Controls event rendering inside the TUI. |

Configure overrides as needed:

```toml
[mcp]
enabled = true
max_concurrent_connections = 3
request_timeout_seconds = 45
retry_attempts = 2
startup_timeout_seconds = 90   # optional: provider startup handshake window
tool_timeout_seconds = 45      # optional: per-tool execution deadline
experimental_use_rmcp_client = true
```

`startup_timeout_seconds` defaults to `None`, which falls back to the global `request_timeout_seconds`
value. Setting it to `0` disables the startup timeout entirely—handy for first-run npm downloads.
`tool_timeout_seconds` behaves the same way for individual tool calls.

### UI configuration

Configure the MCP UI panel if desired. Renderer profiles let you pick tailored output formats for
specific providers or tool prefixes. Renderer identifiers correspond to the enums defined in
`McpRendererProfile`, so the strings in your TOML match the canonical variants used in code:

```toml
[mcp.ui]
mode = "compact"
max_events = 25
show_provider_names = false

[mcp.ui.renderers]
"context7" = "context7"
"sequential-thinker" = "sequential-thinking"
```

### Provider transports

Define one or more providers. Stdio transports embed the command, arguments, and optional working
directory directly in the provider table. VT Code forwards every value here into the MCP lifecycle
as described in the [stdio transport spec](https://modelcontextprotocol.io/specification/2025-06-18/basic/transports.md):

```toml
[[mcp.providers]]
name = "time"
enabled = true
command = "uvx"
args = ["mcp-server-time"]
working_directory = "./servers/time"
max_concurrent_requests = 2
startup_timeout_ms = 15_000

[mcp.providers.env]
PYTHONUNBUFFERED = "1"
```

For HTTP transports, specify the endpoint and headers in place of the stdio fields. The
configuration loader automatically deserializes either transport variant and populates provider
metadata used by the MCP client registry:

```toml
[[mcp.providers]]
name = "figma"
enabled = true
max_concurrent_requests = 4
endpoint = "https://mcp.figma.com/mcp"
api_key_env = "FIGMA_MCP_TOKEN"
protocol_version = "2025-06-18"
headers = { "X-Client" = "vtcode", "X-MCP-App" = "vtcode" }
```

Environment variables defined under `[mcp.providers.env]` are forwarded to stdio transports and
composed with the curated whitelist the loader already exposes. Use `working_directory` to stage
local binaries, credentials, or fixtures that the provider expects on disk. For HTTP transports,
`protocol_version` determines which MCP schema the client negotiates (the default matches
`vtcode-config`'s `2024-11-05`, but you can adopt the `2025-06-18` release or later when providers
publish compatible endpoints). Custom `headers` values help satisfy hosted provider requirements
for client identification—check the server's docs for required `Authorization` formats per the MCP
[authorization guidance](https://modelcontextprotocol.io/specification/2025-06-18/basic/authorization.md).
The `max_concurrent_requests` guard prevents a single provider from starving the global pool
configured in `[mcp]`.

> **Note:** Streamable HTTP support is still evolving. The client negotiates the declared
> `protocol_version`, but servers must expose Server-Sent Events per the transport spec. If an HTTP
> provider lacks streaming, fall back to a stdio wrapper until the server adopts the reference
> implementation.

## Security and validation

VT Code exposes additional security gates through the `[mcp.security]` table. Enable authentication
and tighten rate limits or validation rules when running sensitive providers. These settings mirror
the guidance in the MCP [security best practices](https://modelcontextprotocol.io/specification/2025-06-18/basic/security_best_practices.md)
chapter:

```toml
[mcp.security]
auth_enabled = true
api_key_env = "VT_MCP_API_KEY"

[mcp.security.rate_limit]
requests_per_minute = 120
concurrent_requests = 6

[mcp.security.validation]
schema_validation_enabled = true
path_traversal_protection = true
max_argument_size = 262144
```

The same structure powers the optional embedded MCP server (`vtcode` as a provider). Combine the
security block with the `[mcp.server]` table to expose curated tools over SSE or HTTP. Ensure your
tool list lines up with the [server tools contract](https://modelcontextprotocol.io/specification/2025-06-18/server/tools.md):

```toml
[mcp.server]
enabled = true
bind_address = "127.0.0.1"
port = 3030
transport = "sse"
exposed_tools = ["read_file"]
```

`vtcode-config` re-exports serde schemas for these tables, making it straightforward to validate
configuration files in automation or IDE tooling.

## Allowlist Behaviour

MCP access is gated by pattern-based allowlists. The defaults apply to every provider unless the
provider supplies its own patterns. Provider-specific rules now fully override the defaults:

- When a provider defines `tools`, `resources`, `prompts`, or `logging` patterns, only matches in
  that provider block are accepted. Default rules are ignored for that provider.
- If a provider omits a rule set, VT Code falls back to the default patterns.
- Configuration permissions (`configuration` maps) continue to support provider overrides via an
  explicit match or by delegating to the default rules.

Each allowlist key maps directly to the Model Context Protocol concepts described in the official
specification (all cited in `llms.txt`):

- `tools` correspond to [tool definitions](https://modelcontextprotocol.io/specification/2025-06-18/server/tools.md),
  letting you scope remote execution entry points.
- `resources` align with [resource handles](https://modelcontextprotocol.io/specification/2025-06-18/server/resources.md)
  exposed by a server.
- `prompts` constrain [server-authored prompt templates](https://modelcontextprotocol.io/specification/2025-06-18/server/prompts.md).
- `logging` mirrors [logging channels](https://modelcontextprotocol.io/specification/2025-06-18/server/utilities/logging.md)
  surfaced by compliant servers.

This behaviour avoids situations where a restrictive provider configuration is silently bypassed by
broader default patterns.

### Example

```toml
[mcp.allowlist]
enforce = true

[mcp.allowlist.default]
resources = ["docs/*"]

[mcp.allowlist.providers.context7]
resources = ["journals/*"]
```

In this configuration:

- `context7` can access only `journals/*` resources.
- Other providers continue to match `docs/*` through the default rule.

Enable `enforce = true` when you want the rules to be mandatory; leaving it unset keeps legacy
behaviour where allowlist checks are advisory only.

## Testing the Integration

Run the MCP-focused test suite to verify configuration parsing, allowlist enforcement, and registry
wiring. These tests exercise the serde types documented above and confirm compatibility with the
[schema reference](https://modelcontextprotocol.io/specification/2025-06-18/schema.md):

```bash
cargo test -p vtcode-core mcp -- --nocapture
```

The suite includes mocked clients and parsing tests so it does not require live MCP servers. For an
end-to-end check against the Context7 MCP server, invoke the ignored smoke test which spawns the
official `@upstash/context7-mcp` package on demand. This mirrors the [example servers](https://modelcontextprotocol.io/examples.md)
highlighted in the MCP documentation:

```bash
cargo test -p vtcode-core --test mcp_context7_manual context7_list_tools_smoke -- --ignored --nocapture
```

Expect the test to take a little longer on the first run while `npx` downloads the server bundle.

## Troubleshooting

- **Unexpected tool execution permissions** – confirm whether the provider defines its own
  allowlist. Provider rules now override defaults, so missing patterns may block tools that defaults
  would otherwise allow.
- **Provider handshake visibility** – VT Code now sends explicit MCP client metadata and
  normalizes structured tool responses. Context7 results surface as plain JSON objects in the
  tool panel so downstream renderers can display status, metadata, and message lists without
  additional post-processing.
- **Stale configuration values** – ensure `max_concurrent_connections`, `request_timeout_seconds`,
  and `retry_attempts` appear under the `[mcp]` table *after* any nested `[mcp.ui]` section. TOML
  resets the table context when a new header appears.
- **HTTP transport issues** – VT Code currently performs capability probing for HTTP MCP servers but
  requires a streaming implementation to be fully functional. Use stdio transports when possible.

With these settings and checks in place, MCP providers and allowlists should behave predictably,
unlocking additional context-aware tooling in VT Code.
