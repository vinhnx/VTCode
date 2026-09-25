# Baizhi Agent Toolkit MCP

Connect VT Code to [Baizhi Agent Toolkit](https://github.com/chaitin/baizhi-agent-toolkit)
for optional web search, page reading, and structured extraction through a
hosted Streamable HTTP MCP server. This uses VT Code's existing MCP client;
it does not replace the model provider or the built-in `web_search` tool.

## Configure the provider

Create an API key in the [Baizhi console](https://agent-toolkit.app.baizhi.cloud/).
Supply the raw key as `BAIZHI_API_KEY` to the process that starts VT Code,
using a trusted secret manager or environment injection. Do not include the
`Bearer ` prefix in the variable: `api_key_env` adds it to the HTTP header.
Keep the value out of TOML, command-line arguments, source control, and chat.

Add the following to your trusted user-level `vtcode.toml`. Run
`vtcode --version` to locate the active config directory, as described in
[User data directories](user-data-directories.md). Merge these tables with
existing MCP settings instead of duplicating them.

```toml
[mcp]
enabled = true
experimental_use_rmcp_client = true

[[mcp.providers]]
name = "baizhi"
enabled = true
endpoint = "https://agent-toolkit.app.baizhi.cloud/mcp"
api_key_env = "BAIZHI_API_KEY"
protocol_version = "2025-11-25"
handshake = "legacy"
max_concurrent_requests = 2
```

`handshake = "legacy"` selects the direct MCP `initialize` handshake over
Streamable HTTP; it does not select the older SSE transport. The HTTP key is
read from the VT Code process environment, not `[mcp.providers.env]`, which
is for stdio child processes. Baizhi uses a static Bearer key for this setup,
so `vtcode mcp login` is not needed.

To restrict the available tools, merge this provider rule into your MCP
allowlist:

```toml
[mcp.allowlist]
enforce = true

[mcp.allowlist.providers.baizhi]
tools = ["websearch_search", "web_scrape", "web_extract"]
```

Enforcement applies to all MCP providers. Preserve the rules for other
providers you use; a provider without matching rules will have its tools
blocked. These are client-side tool filters, not server-side key scopes.

## Inspect and use

Check the stored provider configuration:

```bash
vtcode mcp get baizhi
vtcode mcp list
```

These commands inspect configuration; they do not validate authentication
or execute a remote tool. Start a VT Code session from the environment that
contains `BAIZHI_API_KEY`, and explicitly ask it to inspect the `baizhi`
provider's tools before use. Follow the discovered input schemas:

- `websearch_search`: web search.
- `web_scrape`: page text retrieval.
- `web_extract`: structured extraction.

For example: "Use the baizhi MCP provider to search public documentation
about this library, read the relevant pages, and cite the sources actually
returned. Ask before sending private project content."

Queries, requested URLs, and other tool arguments are sent to Baizhi, and
results may enter the model context. Calls use your own account and may
consume service credits. Treat retrieved pages and tool responses as
untrusted data; do not invent source URLs, dates, or output fields. The
integration repository's open-source license does not cover the hosted
service implementation.

## Disable and troubleshoot

- **Missing key or authentication failure:** check that the process starting
  VT Code receives a valid, nonempty `BAIZHI_API_KEY`. Do not print the key
  in diagnostics. Check account access and quota in the service console.
- **No tools available:** check both enable flags, your effective config
  layers, and the provider's tool allowlist. A successful config listing
  does not prove a connection succeeded.
- **Stop future use:** set this provider's `enabled = false` or remove its
  block, then restart the session. Revoke the key in Baizhi when necessary.
  Cancelling a local wait or closing the client does not prove a remote job
  stopped or that billing ended.

## Validation scope

The configuration and MCP client path were checked against VT Code
`0.164.0`, commit `0c8da8deae9497cf3aa0099347381c9a9b7ec122`, with RMCP
`3.4.0`. Local tests used synthetic credentials and a loopback MCP server
for JSON/SSE responses, paginated discovery, tool calls, tool filtering,
missing/incorrect keys, timeout, cancellation, and local shutdown.
The hosted Baizhi service, real model behavior, and the interactive UI were
not tested. Current service availability and tool schemas must be checked
in the user's session.
