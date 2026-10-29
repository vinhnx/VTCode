# Execution, Integrations, and Security

_Part of [VT Code Configuration](./config.md)._

## Execution environment

### workspace.settings

Controls various workspace-specific settings for VT Code execution.

> **Note:** The `[workspace]` section is now implemented. When `use_root_config = true`, only the workspace root
> `vtcode.toml` is used as the active config layer; system, user, project, and dot-dir layers are discarded.

```toml
[workspace]
# When true, force workspace root vtcode.toml as the sole active config layer
# (system, user, project, and dot-dir layers are discarded)
use_root_config = true

# Controls whether to include workspace context in messages
include_context = true

# Maximum size of context to include (in bytes)
max_context_size = 1048576  # 1MB
```

### execution.timeout

Controls timeout settings for various operations:

```toml
[execution]
# Timeout for tool executions in seconds
tool_timeout = 300  # 5 minutes

# Timeout for API calls in seconds
api_timeout = 120   # 2 minutes

# Maximum time for participant context resolution
participant_timeout = 30  # 30 seconds
```

### Long-running command waits

The `write_stdin` and `unified_exec` tools support an explicit `wait` action for command sessions. A wait deadline
returns an in-progress session instead of terminating it; call `wait` again with the returned `session_id` to continue
observing the same process. Waits are excluded from the ordinary per-turn harness wall-clock budget, but cancellation
and the long-running ceiling still apply.

```toml
[timeouts]
# Hard upper bound for one explicit command wait (seconds).
long_running_command_ceiling_seconds = 3600
```

The requested `wait_timeout_seconds` is clamped to this ceiling. Command output is kept memory-bounded; the tool
response contains a bounded preview and a `spool_path` when the spool file is open and healthy. For an active session,
`spool_complete = false` identifies a readable partial snapshot. If the process has exited before draining finishes,
`spool_pending = true` indicates that a later wait can observe the completed spool.

## MCP integration

### mcp

You can configure VT Code to use [Model Context Protocol (MCP) servers](https://modelcontextprotocol.io/) to give VT
Code access to external applications, resources, or services.

#### Server configuration

MCP providers are configured as follows:

```toml
[mcp]
enabled = true  # Enable MCP integration

# List of MCP providers to use
[[mcp.providers]]
name = "context7"
command = "npx"
args = ["-y", "context7", "serve", "api"]
enabled = true

[[mcp.providers]]
name = "figma"
command = "figma-mcp-server"
args = ["--port", "4000"]
enabled = false  # Disabled by default

[[mcp.providers]]
name = "github"
command = "github-mcp-server"
enabled = true
```

#### Provider configuration options

Each MCP provider supports these options:

| Field     | Type    | Required | Description                                      |
| --------- | ------- | -------- | ------------------------------------------------ |
| `name`    | string  | Yes      | Unique identifier for the MCP provider           |
| `command` | string  | Yes      | Command to execute to start the MCP server       |
| `args`    | array   | No       | Arguments to pass to the command                 |
| `enabled` | boolean | No       | Whether this provider is enabled (default: true) |
| `env`     | table   | No       | Environment variables to pass to the server      |
| `cwd`     | string  | No       | Working directory for the command                |

## WebMCP browser bridge

WebMCP is a first-class, opt-in bridge for a browser editor. It is not an MCP provider and does not reuse `[mcp]`
settings. The browser receives no direct filesystem capability: it sends digest-checked proposals over an
origin-validated WebSocket, while VT Code or the headless full-auto policy remains the mutation authority. The bridge is
shipped in the main `vtcode` binary; the repository's Vite project is the WebMCP browser app.

```toml
[webmcp]
enabled = false
host = "127.0.0.1"
port = 0
allowed_origins = ["http://localhost:5173"]
allowed_roots = []
pairing_ttl_secs = 300
max_frame_bytes = 1048576
max_in_flight_requests = 8

[webmcp.remote_mcp]
enabled = false
public_url = "https://mcp.example.com/sse/"
authorization_server = "https://login.example.com"
proxy_token_env = "VTCODE_WEBMCP_MCP_PROXY_TOKEN"
allowed_origins = []
max_results = 20
max_scan_files = 256
max_scan_bytes = 16777216
session_ttl_secs = 300
```

`allowed_origins` must contain exact browser origins; wildcards are rejected. Loopback is the default bind host and
direct non-loopback binding is rejected. Remote access additionally requires explicit CLI opt-in, a `wss://` public URL,
and a TLS-terminating reverse proxy forwarding to the loopback listener. Pairing codes expire after five minutes by
default and are consumed once. Authenticated sessions use the same value as an inactivity lease and are refreshed by
authenticated browser requests. Tokens remain in memory only.

`webmcp.remote_mcp` is a separate, disabled-by-default read-only MCP surface. Its HTTPS `public_url` is the canonical
`/sse/` endpoint, while `/mcp` serves modern Streamable HTTP. The configured proxy token is read from `proxy_token_env`;
the external proxy validates OAuth and injects that internal bearer token. `allowed_origins` in the nested table is an
independent MCP Origin allowlist, and missing MCP `Origin` is accepted. See the
[WebMCP development guide](../development/webmcp.md) for the protocol and threat model.

## Security and approvals

### security

The security section defines how VT Code handles potentially dangerous operations:

```toml
[security]
# Enable human-in-the-loop approval for tool calls
human_in_the_loop = true

# Default policy for tool execution
# Options: "ask", "allow", "deny"
default_tool_policy = "ask"

# Whether trusted workspaces can bypass some security checks
trusted_workspace_mode = true
```

### tools.policies

Define specific policies for different tools:

```toml
[tools.policies]
# Policy for shell execution tools
exec_command = "ask"      # Options: "ask", "allow", "deny"
write_stdin = "ask"       # Options: "ask", "allow", "deny"
apply_patch = "ask"       # Options: "ask", "allow", "deny"
code_search = "allow"     # Options: "ask", "allow", "deny"
web_search = "ask"        # Options: "ask", "allow", "deny"

# Custom policies for specific tools
custom_tool_example = "deny"
```

### automation

Control automation behavior in VT Code:

```toml
[automation]
# Enable full automation mode (bypasses human approval)
full_auto = false

# Settings for automation when enabled
[automation.full_auto]
enabled = false
# List of tools that are allowed in full automation mode
allowed_tools = ["exec_command", "write_stdin", "apply_patch"]
# Grant tool-loop and session tool-call limit increases without prompting (same increments and caps as manual approvals)
auto_grant_tool_limits = true

[automation.scheduled_tasks]
enabled = false
```

`automation.scheduled_tasks.enabled` controls VT Code's internal scheduler surfaces:

- one-shot reminder interception such as `remind me at 3pm to ...`
- scheduler tool `cron` (actions: `create`, `list`, `delete`; legacy names `cron_create`, `cron_list`, `cron_delete`
  still route to it)
- durable `vtcode schedule ...` commands and the local scheduler daemon

This subsystem is opt-in. Set it to `true` when you want VT Code scheduling enabled.

Set `VTCODE_DISABLE_CRON=1` to disable the scheduler entirely, regardless of config.
