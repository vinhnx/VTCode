# Profiles, Operations, and Reference

_Part of [VT Code Configuration](./config.md)._

## Profiles and overrides

### profiles

A _profile_ is a collection of configuration values that can be set together. Multiple profiles can be defined in
`vtcode.toml` and you can specify the one you want to use depending on the project type or your current task.

Here is an example of a `vtcode.toml` that defines multiple profiles:

```toml
# Default settings
[agent]
provider = "openai"
default_model = "gpt-5"

[security]
human_in_the_loop = true
default_tool_policy = "ask"

# Profile for development work
[profiles.development]
[profiles.development.agent]
provider = "openai"
default_model = "gpt-5"

[profiles.development.security]
human_in_the_loop = true
default_tool_policy = "ask"

[profiles.development.participants]
default_participants = ["@workspace", "@code", "@git"]

# Profile for research work
[profiles.research]
[profiles.research.agent]
provider = "anthropic"
default_model = "claude-sonnet-5"

[profiles.research.tools.policies]
web_search = "allow"
code_search = "allow"
exec_command = "deny"

# Profile for local development with Ollama
[profiles.local]
[profiles.local.agent.provider_settings.ollama]
enabled = true

[profiles.local.agent]
provider = "ollama"
default_model = "llama3.1"

[profiles.local.security]
human_in_the_loop = false
default_tool_policy = "allow"
```

Users can specify config values at multiple levels. Values are merged from lowest to highest precedence as follows:

1. Built-in defaults
2. System-level `/etc/vtcode/vtcode.toml` and `XDG_CONFIG_DIRS` candidates (Unix)
3. Legacy user-level `$VTCODE_HOME/vtcode.toml` (default `~/.vtcode/vtcode.toml`)
4. Canonical user-level config-directory `vtcode.toml`
5. Project profile `.vtcode/projects/<project>/config/vtcode.toml`
6. Workspace fallback `.vtcode/vtcode.toml`
7. Workspace root `vtcode.toml`
8. Explicit config file (`VTCODE_CONFIG_PATH` or `--config path/to/file.toml`), retaining the global layers
9. Runtime overrides (`-c/--config key=value`) and explicit runtime flags

Merge semantics are layered: tables merge recursively, while scalar and array values are replaced by higher-precedence
layers.

### workspace-specific overrides

You can also define settings that only apply to specific workspace types:

```toml
# Settings for any workspace containing a package.json
[workspace.nodejs]
[workspace.nodejs.agent]
default_model = "gpt-5"

[workspace.nodejs.participants]
default_participants = ["@workspace", "@code", "@terminal"]

# Settings for any workspace containing a Cargo.toml
[workspace.rust]
[workspace.rust.agent]
default_model = "claude-sonnet-5"

[workspace.rust.participants]
default_participants = ["@workspace", "@code", "@terminal", "@git"]
```

## Observability and telemetry

### telemetry

VT Code can emit telemetry data about usage and performance:

```toml
[telemetry]
# Enable telemetry collection (disabled by default for privacy)
enabled = false

# Whether to include usage analytics
analytics = false

# Whether to report errors to the development team
report_errors = true

# Level of detail for telemetry data
# Options: "minimal", "basic", "detailed"
level = "minimal"
```

### logging

Configure logging behavior in VT Code:

```toml
[logging]
# Enable detailed logging (useful for debugging)
enabled = false

# Log level: "error", "warn", "info", "debug", "trace"
level = "info"

# Whether to include sensitive information in logs (never enabled by default)
include_sensitive = false

# Maximum size of log files before rotation (in bytes)
max_log_size = 10485760  # 10MB
```

## Authentication and authorization

### API keys

Each AI provider requires an API key configuration. These are typically managed through environment variables:

```bash
# Environment variables for API keys
OPENAI_API_KEY=your_openai_api_key
ANTHROPIC_API_KEY=your_anthropic_api_key
GOOGLE_GEMINI_API_KEY=your_google_api_key
DEEPSEEK_API_KEY=your_deepseek_api_key
META_API_KEY=your_meta_api_key
# Meta's documentation also names MODEL_API_KEY as the credential variable.
MODEL_API_KEY=your_meta_api_key
OPENROUTER_API_KEY=your_openrouter_api_key
AI_GATEWAY_API_KEY=your_vercel_ai_gateway_api_key
# Optional Vercel AI Gateway endpoint override:
# VERCEL_AI_GATEWAY_BASE_URL=https://ai-gateway.vercel.sh/v1
MIMO_API_KEY=your_mimo_api_key
EVOLINK_API_KEY=your_evolink_api_key
STEPFUN_API_KEY=your_stepfun_api_key
QWEN_API_KEY=your_qwen_api_key
NVIDIA_API_KEY=your_nvidia_api_key
MERGE_GATEWAY_API_KEY=your_merge_gateway_api_key
# Optional Merge Gateway endpoint override (native default):
# MERGE_GATEWAY_BASE_URL=https://api-gateway.merge.dev/v1
OLLAMA_HOST=http://localhost:11434  # For Ollama
```

### auth.settings

Authentication settings for VT Code:

```toml
[auth]
# Whether to store credentials securely in the OS keychain
secure_storage = true

# Whether to validate API keys on startup
validate_keys = true

# Timeout for authentication requests
timeout = 30  # seconds
```

## CLI Configuration

### CLI Commands for Configuration

Manage configuration from the terminal:

- Open the workspace `vtcode.toml` file directly to edit settings
- Toggle the `human_in_the_loop` setting in `vtcode.toml`
- Manage MCP provider settings under the MCP configuration section
- Edit the tools policy section of the config directly

### Command System Integration

VT Code uses a command system that can be configured through the settings:

```toml
# Configure which commands are available
[commands]
# Whether to enable the ask agent command
ask_agent_enabled = true

# Whether to enable the analyze workspace command
analyze_enabled = true

# Timeout for command execution (in seconds)
command_timeout = 300
```

### Workspace Trust

Some features are only available in trusted workspaces:

```toml
# Workspace trust gates CLI automation
[security]
trusted_workspace_mode = true
```

In untrusted workspaces, VT Code limits CLI automation capabilities to protect your system.

## Configuration Validation and Troubleshooting

### Validation

VT Code validates the configuration file on load. You can check for configuration errors by:

1. Running `vtcode check-config` from the command line
2. Opening the workspace `vtcode.toml` directly to inspect parsing errors

Common configuration errors include:

- Invalid TOML syntax
- Missing required API keys for selected providers
- Invalid provider names

### Troubleshooting

If VT Code is not behaving as expected with your configuration:

1. First, verify the configuration file parses correctly:

   ```toml
   # Make sure all tables close properly
   [agent]
   provider = "openai"
   default_model = "gpt-5"
   # No missing closing brackets
   ```

2. Check that required environment variables are set:

   ```bash
   # Verify API keys are available
   echo $OPENAI_API_KEY
   ```

3. Enable logging temporarily to see what's happening:

   ```toml
   [logging]
   enabled = true
   level = "debug"
   ```

## Config reference

For complete field coverage generated from the live `vtcode-config` schema, use
[`docs/config/CONFIG_FIELD_REFERENCE.md`](./CONFIG_FIELD_REFERENCE.md).

For harness behavior, read `agent.harness`, `automation.full_auto`, and `context.dynamic` together: they jointly define
continuation, turn limits, and context reuse for long-running exec sessions.

| Key                                     | Type / Values                                                          | Notes                                                                                                                                                                                               |
| --------------------------------------- | ---------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `agent.provider`                        | string                                                                 | Provider to use (e.g., `openai`, `anthropic`, `google`, `meta`, `nvidia`, `merge-gateway`, `ollama`).                                                                                               |
| `agent.default_model`                   | string                                                                 | Default model for the selected provider.                                                                                                                                                            |
| `agent.context_window`                  | number                                                                 | Context window tokens.                                                                                                                                                                              |
| `agent.max_output_tokens`               | number                                                                 | Max output tokens.                                                                                                                                                                                  |
| `agent.temperature`                     | number                                                                 | Model temperature (0.0-2.0).                                                                                                                                                                        |
| `agent.top_p`                           | number                                                                 | Top-P sampling parameter (0.0-1.0).                                                                                                                                                                 |
| `context.semantic_compression`          | boolean                                                                | Enable structural-aware context compression (default: false).                                                                                                                                       |
| `context.tool_aware_retention`          | boolean                                                                | Extend retention for recent tool outputs (default: false).                                                                                                                                          |
| `context.max_structural_depth`          | number                                                                 | AST depth preserved when semantic compression is enabled (default: 3).                                                                                                                              |
| `context.preserve_recent_tools`         | number                                                                 | Recent tool outputs to preserve when retention is enabled (default: 5).                                                                                                                             |
| `security.human_in_the_loop`            | boolean                                                                | Enable tool approval prompts (default: true).                                                                                                                                                       |
| `security.default_tool_policy`          | `ask` \| `allow` \| `deny`                                             | Default tool execution policy.                                                                                                                                                                      |
| `tools.policies.*`                      | `ask` \| `allow` \| `deny`                                             | Policies for specific tools.                                                                                                                                                                        |
| `mcp.enabled`                           | boolean                                                                | Enable MCP integration (default: false).                                                                                                                                                            |
| `mcp.providers[].name`                  | string                                                                 | MCP provider name.                                                                                                                                                                                  |
| `mcp.providers[].command`               | string                                                                 | MCP provider command to execute.                                                                                                                                                                    |
| `mcp.providers[].args`                  | array                                                                  | Arguments for the MCP command.                                                                                                                                                                      |
| `mcp.providers[].enabled`               | boolean                                                                | Whether the provider is enabled.                                                                                                                                                                    |
| `participants.enabled`                  | boolean                                                                | Enable participant system (default: true).                                                                                                                                                          |
| `participants.default_participants`     | array                                                                  | Default participants to include.                                                                                                                                                                    |
| `participants.timeout`                  | number                                                                 | Timeout for participant context (seconds).                                                                                                                                                          |
| `automation.full_auto.enabled`          | boolean                                                                | Enable full automation.                                                                                                                                                                             |
| `automation.full_auto.allowed_tools`    | array                                                                  | Tools allowed during full automation. Workflow-coordination tools (`task_tracker`, `start_planning`, `request_user_input`) stay available in every mode even when omitted.                          |
| `automation.full_auto.max_turns`        | integer                                                                | Upper bound for autonomous turns before exec pauses.                                                                                                                                                |
| `automation.scheduled_tasks.enabled`    | boolean                                                                | Enable VT Code's internal scheduler for reminders, cron tools, and `vtcode schedule`. Can still be force-disabled with `VTCODE_DISABLE_CRON=1`.                                                     |
| `agent.harness.continuation_policy`     | `off` \| `exec_only` \| `all`                                          | Controls when the harness may auto-continue after a completion attempt. Default: `all` in interactive and exec sessions; use `exec_only` to keep interactive sessions manual.                       |
| `agent.harness.event_log_path`          | string \| null                                                         | Optional compatibility/export JSONL sink for harness events. Canonical events always live at `<workspace>/.vtcode/sessions/<session_id>/events.jsonl`; unset does not create a global harness file. |
| `sandbox.default_policy`                | `read_only` \| `workspace_write` \| `danger_full_access` \| `external` | Default sandbox policy.                                                                                                                                                                             |
| `workspace.use_root_config`             | boolean                                                                | When true, force workspace root vtcode.toml as the sole active config layer (system, user, project, and dot-dir layers discarded).                                                                  |
| `workspace.include_context`             | boolean                                                                | Include workspace context.                                                                                                                                                                          |
| `workspace.max_context_size`            | number                                                                 | Max size of workspace context (bytes).                                                                                                                                                              |
| `execution.tool_timeout`                | number                                                                 | Timeout for tool executions (seconds).                                                                                                                                                              |
| `execution.api_timeout`                 | number                                                                 | Timeout for API calls (seconds).                                                                                                                                                                    |
| `telemetry.enabled`                     | boolean                                                                | Enable telemetry (default: false).                                                                                                                                                                  |
| `telemetry.analytics`                   | boolean                                                                | Enable usage analytics.                                                                                                                                                                             |
| `logging.enabled`                       | boolean                                                                | Enable detailed logging.                                                                                                                                                                            |
| `logging.level`                         | `error` \| `warn` \| `info` \| `debug` \| `trace`                      | Log level.                                                                                                                                                                                          |
| `auth.secure_storage`                   | boolean                                                                | Store credentials securely (default: true).                                                                                                                                                         |
| `auth.validate_keys`                    | boolean                                                                | Validate API keys on startup.                                                                                                                                                                       |
| `commands.ask_agent_enabled`            | boolean                                                                | Enable the ask agent command.                                                                                                                                                                       |
| `commands.analyze_enabled`              | boolean                                                                | Enable the analyze command.                                                                                                                                                                         |
| `commands.command_timeout`              | number                                                                 | Command execution timeout (seconds).                                                                                                                                                                |
| `profiles.*.agent.provider`             | string                                                                 | Provider override for a profile.                                                                                                                                                                    |
| `profiles.*.security.human_in_the_loop` | boolean                                                                | Security setting override for a profile.                                                                                                                                                            |
| `profiles.*.tools.policies.*`           | `ask` \| `allow` \| `deny`                                             | Tool policy override for a profile.                                                                                                                                                                 |
| `providers.<name>.models`               | array                                                                  | Additional model identifiers to add to a built-in provider's model picker.                                                                                                                          |
| `providers.<name>.base_url`             | string                                                                 | Override the provider's default API endpoint.                                                                                                                                                       |
| `providers.<name>.api_key_env`          | string                                                                 | Override the provider's default API key environment variable.                                                                                                                                       |
