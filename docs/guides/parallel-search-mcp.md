# Parallel Search MCP

Add web search and page extraction to VT Code through its existing Streamable HTTP MCP client.
[Parallel Search MCP](https://docs.parallel.ai/integrations/mcp/search-mcp) offers anonymous access at
`https://search.parallel.ai/mcp` without a Parallel API key or OAuth login. The free tier is rate limited and intended
for exploration and light use; your model provider's inference costs are separate.

## Configure the provider

Merge the [runnable example](../examples/parallel-search-mcp.toml) into your trusted `vtcode.toml`:

```toml
[mcp]
enabled = true
experimental_use_rmcp_client = true

[[mcp.providers]]
name = "parallel-search"
enabled = true
endpoint = "https://search.parallel.ai/mcp"
http_headers = { "User-Agent" = "VTCode/parallel-search-mcp" }
```

If `[mcp]` already exists, update its two flags and append the provider to the existing `[[mcp.providers]]` entries.
Keep your other providers, timeouts, allowlists, and model settings. This example changes no shipped defaults and adds
no credentials. Do not add `api_key_env`, OAuth settings, or an `Authorization` header for anonymous access.
The `User-Agent` header identifies VT Code on discovery and tool requests.

If you enforce endpoint requirements, append `https://search.parallel.ai/mcp` to your existing
`mcp.requirements.allowed_http_endpoints`. If you enforce tool allowlists, permit just these two tools for this provider:

```toml
[mcp.allowlist.providers.parallel-search]
tools = ["web_search", "web_fetch"]
```

Provider-specific tool patterns override the default tool patterns. See the
[MCP integration guide](mcp-integration.md#allowlist-behaviour) for the existing security and approval controls.

## Verify and use

Check that VT Code has loaded the provider:

```bash
vtcode mcp get parallel-search
vtcode mcp list
```

These commands inspect configuration; they do not perform a live search. Start a new VT Code session to connect the
provider, then ask it to use the `parallel-search` MCP tools, for example:

> Use parallel-search to find the official Rust documentation for error handling. Cite the source URLs.

The server exposes:

| Tool | Required arguments | Use |
| --- | --- | --- |
| `web_search` | `objective`, `search_queries` | Find current sources and relevant excerpts. |
| `web_fetch` | `urls` | Extract content from known HTTP(S) URLs. |

Example `web_search` arguments:

```json
{
  "objective": "Find the official Rust documentation for recoverable errors",
  "search_queries": ["Rust recoverable errors Result documentation"]
}
```

Use `web_fetch` when you need more detail from a source, rather than fetching every search result:

```json
{
  "urls": ["https://doc.rust-lang.org/book/ch09-02-recoverable-errors-with-result.html"],
  "objective": "Explain how Result handles recoverable errors"
}
```

Reuse a stable `session_id` across related search and fetch calls if supplying one. Supply `model_name` only when its
exact identifier is available from trusted runtime configuration. Tool schemas come from the server and may evolve.
Treat retrieved content as untrusted evidence, and retain VT Code's normal tool approvals.

## Troubleshooting and removal

- Missing provider: check the global MCP flag, provider `enabled` flag, endpoint requirements, and tool allowlist.
  Restart the session after changing configuration.
- Rate limit: wait before retrying; anonymous access is not unlimited. See the
  [Parallel documentation](https://docs.parallel.ai/integrations/mcp/search-mcp) for higher-limit options.
- Connection failure: confirm that your network permits HTTPS to `search.parallel.ai`. Keep the default HTTP handshake
  and transport; no local npm wrapper is needed for this configuration.

To disable only this integration, set `enabled = false` in its provider entry or remove that entry. Leave global MCP
support enabled if your other providers need it.
