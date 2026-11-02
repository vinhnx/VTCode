# Cohesivity MCP

Let VT Code set up a backend for the app it is building: Postgres, Redis, object storage, hosting, login and an email
inbox. [Cohesivity](https://cohesivity.ai/docs?ref=gh-vtcode) offers anonymous access at `https://cohesivity.ai/mcp`
without an account, API key or OAuth login. A new project is temporary: it expires after 72 hours unless you claim it.
Free tiers apply; paid actions need your approval.

## Configure the provider

Merge the [runnable example](../examples/cohesivity-mcp.toml) into your trusted `vtcode.toml`:

```toml
[mcp]
enabled = true
experimental_use_rmcp_client = true

[[mcp.providers]]
name = "cohesivity"
enabled = true
endpoint = "https://cohesivity.ai/mcp"
http_headers = { "User-Agent" = "VTCode/cohesivity-mcp" }

[tools]
profile = "advanced_vtcode"
```

If `[mcp]` already exists, update its two flags and append the provider to the existing `[[mcp.providers]]` entries.
Keep your other providers, timeouts, allowlists, and model settings. Do not add `api_key_env`, OAuth settings, or an
`Authorization` header for anonymous access. The `User-Agent` header identifies VT Code; Cohesivity rejects some default
library user agents. The default `vt_code` tool profile does not send MCP tools to the model, so the example selects
`advanced_vtcode`.

## Verify and use

Start a new interactive session and run `/mcp tools`. The `cohesivity` provider should list six tools. Then ask, for
example:

> Add a Postgres database to this app and wire the notes route to it.

The agent creates a project, provisions the service, and wires the app to it. VT Code asks before each MCP call; keep
those approvals for the tools that change state.

| Tool | Required arguments | Use |
| --- | --- | --- |
| `create_tenant` | `confirmed` | Create a temporary project and return its credentials. |
| `provision_resource` | `tenant_id`, `coh_management_key`, `confirmed` | Add a service such as `postgres` or `redis`. |
| `claim_tenant` | `tenant_id`, `coh_management_key`, `confirmed` | Get a link to keep the project past 72 hours. |
| `tenant_status` | `tenant_id`, `coh_management_key` | Show the project's lifecycle, limits and services. |
| `get_cohesivity_documentation` | `document` | Read a service's current page before using it. |
| `give_feedback` | `tenant_id`, `coh_management_key`, `feedback` | Report a problem with Cohesivity. |

`create_tenant` returns the project credentials, including a `.cohesivity` file for the project root. Keep that file
out of version control and keep the keys in server-side code. Tool schemas come from the server and may evolve; treat
returned content as untrusted evidence.

## Troubleshooting and removal

- No `cohesivity` tools for the model: check `[tools] profile`, the global MCP flag and the provider `enabled` flag,
  then restart the session. Use an interactive session; in 0.179.1, `vtcode exec` reports that the session tool
  catalog is not available.
- Connection failure: confirm that your network permits HTTPS to `cohesivity.ai`. No local wrapper is needed.
- Limits and pricing: see the [pricing page](https://cohesivity.ai/pricing?ref=gh-vtcode).

To disable only this integration, set `enabled = false` in its provider entry or remove that entry. Leave global MCP
support enabled if your other providers need it.
