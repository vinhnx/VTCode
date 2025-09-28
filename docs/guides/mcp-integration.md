# MCP Integration Guide

This guide outlines how VT Code connects to external MCP (Model Context Protocol) servers, how
allowlists control tool access, and how to troubleshoot common configuration issues.

## Configuring MCP Providers

1. Open `vtcode.toml` and ensure the global MCP section is enabled:

   ```toml
   [mcp]
   enabled = true
   max_concurrent_connections = 3
   request_timeout_seconds = 45
   retry_attempts = 2
   ```

2. Configure the MCP UI panel if desired:

   ```toml
   [mcp.ui]
   mode = "compact"
   max_events = 25
   show_provider_names = false
   ```

3. Define one or more providers. Stdio transports embed the command, arguments, and optional
   working directory directly in the provider table:

   ```toml
   [[mcp.providers]]
   name = "time"
   enabled = true
   command = "uvx"
   args = ["mcp-server-time"]
   max_concurrent_requests = 2
   ```

   For HTTP transports, specify the endpoint and headers in place of the stdio fields. The
   configuration loader automatically deserializes either transport variant.

## Allowlist Behaviour

MCP access is gated by pattern-based allowlists. The defaults apply to every provider unless the
provider supplies its own patterns. Provider-specific rules now fully override the defaults:

- When a provider defines `tools`, `resources`, `prompts`, or `logging` patterns, only matches in
  that provider block are accepted. Default rules are ignored for that provider.
- If a provider omits a rule set, VT Code falls back to the default patterns.
- Configuration permissions (`configuration` maps) continue to support provider overrides via an
  explicit match or by delegating to the default rules.

This behaviour avoids situations where a restrictive provider configuration is silently bypassed by
broader default patterns.

### Example

```toml
[mcp.allowlist.default]
resources = ["docs/*"]

[mcp.allowlist.providers.context7]
resources = ["journals/*"]
```

In this configuration:

- `context7` can access only `journals/*` resources.
- Other providers continue to match `docs/*` through the default rule.

## Testing the Integration

Run the MCP-focused test suite to verify configuration parsing, allowlist enforcement, and registry
wiring:

```bash
cargo test -p vtcode-core mcp -- --nocapture
```

The suite includes mocked clients and parsing tests so it does not require live MCP servers. For
end-to-end checks against real servers, temporarily enable the ignored `test_time_mcp_server_integration`
case after installing the required provider binary.

## Troubleshooting

- **Unexpected tool execution permissions** – confirm whether the provider defines its own
  allowlist. Provider rules now override defaults, so missing patterns may block tools that defaults
  would otherwise allow.
- **Stale configuration values** – ensure `max_concurrent_connections`, `request_timeout_seconds`,
  and `retry_attempts` appear under the `[mcp]` table *after* any nested `[mcp.ui]` section. TOML
  resets the table context when a new header appears.
- **HTTP transport issues** – VT Code currently performs capability probing for HTTP MCP servers but
  requires a streaming implementation to be fully functional. Use stdio transports when possible.

With these settings and checks in place, MCP providers and allowlists should behave predictably,
unlocking additional context-aware tooling in VT Code.
