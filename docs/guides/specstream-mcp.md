# SpecStream MCP

Add live web search and structured web data retrieval to VT Code through its existing Streamable HTTP MCP client.
[SpecStream](https://specstream.ai/docs/mcp) serves MCP at `https://api.specstream.ai/mcp` using a SpecStream API key
or OAuth. New accounts start with $10 of credit, with search priced from $1 per 1,000 fast-tier calls ($5 per 1,000
default calls) and Deep Search quoted per run before spending; model inference costs are separate.

## Configure the provider

Export your SpecStream API key (created under **Agents & API** at `https://specstream.ai/workspace`) in your shell
environment rather than committing it to workspace configuration:

```bash
export SPECSTREAM_API_KEY="your-specstream-api-key"
```

Merge the [runnable example](../examples/specstream-mcp.toml) into your trusted user-level `vtcode.toml`:

```toml
[mcp]
enabled = true
experimental_use_rmcp_client = true

[[mcp.providers]]
name = "specstream"
enabled = true
endpoint = "https://api.specstream.ai/mcp"
api_key_env = "SPECSTREAM_API_KEY"
http_headers = { "User-Agent" = "VTCode/specstream-mcp" }

[tools]
profile = "advanced_vtcode"
```

If `[mcp]` already exists, update its two flags and append the provider to the existing `[[mcp.providers]]` entries.
Keep your other providers, timeouts, allowlists, and model settings. VT Code reads `SPECSTREAM_API_KEY` from the
environment and sends it as an `Authorization: Bearer` header. The `User-Agent` header identifies VT Code on discovery
and tool requests. The default `vt_code` tool profile does not send MCP tools to the model, so the example selects
`advanced_vtcode`.

If you enforce endpoint requirements, append `https://api.specstream.ai/mcp` to your existing
`mcp.requirements.allowed_http_endpoints`. If you enforce tool allowlists, permit only the tools you need under
`[mcp.allowlist.providers.specstream]`, for example `search_web`:

```toml
[mcp.allowlist.providers.specstream]
tools = ["search_web"]
```

See the [MCP integration guide](mcp-integration.md#allowlist-behaviour) for the existing security and approval
controls.

## Verify and use

Check that VT Code has loaded the provider configuration:

```bash
vtcode mcp get specstream
vtcode mcp list
```

These commands inspect configuration; they do not open a network connection. When `SPECSTREAM_API_KEY` is exported,
start a new interactive VT Code session and run `/mcp tools` to verify discovery, then ask the agent to use
`specstream`, for example:

> Use specstream search_web to find the current Rust MSRV policy and cite the source URLs.

Key tools exposed by the server include:

| Tool | Use |
| --- | --- |
| `search_web` | Run a web search (`{"q": "<question>"}`) and return sourced results. |
| `compile` | Parse a plain-language web data request into structured fields. |
| `spec_options` | Inspect supported fields for a Deep Search request. |
| `quote_capacity_work` | Price a Deep Search request before authorizing spend. |
| `authorize_capacity_work` | Start a quoted run with an approved `confirm_max_cents` cap. |
| `list_find_rows` | Read rows from a run, each with its source URL and read time. |

The server discovers the full tool catalog via `tools/list` on connect, and schemas may evolve. Keep VT Code's normal
tool approvals enabled, especially for tools that quote or authorize spend, and treat retrieved web content as
untrusted evidence.

## Troubleshooting and removal

- Missing environment variable: confirm `SPECSTREAM_API_KEY` is exported in the shell that starts VT Code. When the
  provider is enabled and `api_key_env` is set, VT Code requires that variable to be present.
- Authentication error (`401 invalid_token`): verify the key is active in your SpecStream workspace and has the needed
  scopes and spending limit.
- No `specstream` tools for the model: check `[tools] profile`, the global `[mcp] enabled` flag, the provider
  `enabled` flag, endpoint requirements, and tool allowlists, then restart the session.
- Connection failure: confirm your network permits HTTPS to `api.specstream.ai`. For clients that require stdio
  instead of Streamable HTTP, SpecStream publishes `specstream-mcp-stdio` on PyPI.

To disable only this integration, set `enabled = false` in its provider entry or remove that entry. Leave global MCP
support enabled if your other providers need it.
