# deed.page MCP

Let VT Code publish the HTML page, report or small static site it just built to a permanent public URL at
`https://<slug>.deed.page/`. [deed.page](https://deed.page/docs) offers anonymous access at `https://deed.page/mcp`
without an account, API key or OAuth login. Free sites are public, so do not publish secrets or private data.

## Configure the provider

Merge the [runnable example](../examples/deedpage-mcp.toml) into your trusted `vtcode.toml`:

```toml
[mcp]
enabled = true
experimental_use_rmcp_client = true

[[mcp.providers]]
name = "deedpage"
enabled = true
endpoint = "https://deed.page/mcp"
http_headers = { "User-Agent" = "VTCode/deedpage-mcp" }

[tools]
profile = "advanced_vtcode"
```

If `[mcp]` already exists, update its two flags and append the provider to the existing `[[mcp.providers]]` entries.
Keep your other providers, timeouts, allowlists, and model settings. Do not add `api_key_env`, OAuth settings, or an
`Authorization` header for anonymous access. The `User-Agent` header only identifies VT Code. The server negotiates
MCP `2025-06-18`; pin `protocol_version = "2025-06-18"` in the provider block if you want to skip the downgrade. The
default `vt_code` tool profile does not send MCP tools to the model, so the example selects `advanced_vtcode`.

## Verify and use

Start a new interactive session and run `/mcp tools`. The `deedpage` provider should list its tools, starting with
`publish_site`. Then ask, for example:

> Build a one-page summary of this repository's architecture as HTML and publish it to deed.page.

The agent calls `publish_site` with a map of file paths to contents (and an optional slug) and gets back the live
URL. VT Code asks before each MCP call; keep those approvals for the tools that change state.

| Tool | Main arguments | Use |
| --- | --- | --- |
| `publish_site` | `files` | Publish files to a slug; the first call returns an owner token once. |
| `update_site` | `slug`, `files`, owner `token` | Replace a site's files, or merge only the paths sent. |
| `get_site` | `slug` | Show whether a slug is live, with its URL and version. No token. |
| `list_sites` | owner `token` | List every live site the token owns. |
| `list_versions` / `restore_version` | `slug`, `version`, owner `token` | Roll a site back to a retained version. |
| `delete_site` | `slug`, owner `token` | Permanently remove a site. |

The owner token from the first `publish_site` call is the only key to the sites it created. Keep it out of version
control, for example in an ignored local file, and pass it to later calls. The server also has tools for custom
domains, analytics, passwords and webhooks. Tool schemas come from the server and may evolve; treat returned content as
untrusted evidence.

## Troubleshooting and removal

- No `deedpage` tools for the model: check `[tools] profile`, the global MCP flag and the provider `enabled` flag,
  then restart the session.
- Connection failure: confirm that your network permits HTTPS to `deed.page`. No local wrapper is needed.
- Limits and plans: see the [docs](https://deed.page/docs).

To disable only this integration, set `enabled = false` in its provider entry or remove that entry. Leave global MCP
support enabled if your other providers need it.
