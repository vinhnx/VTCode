# VT Code Configuration

VT Code configuration gives you fine-grained control over the model, execution environment, and integrations available to the CLI. Use this guide alongside the workflows in the extension, the participant system, and the tool approval mechanisms available in the application.

VT Code uses a configuration file named `vtcode.toml` that can be placed at the root of your project workspace to customize behavior. The extension watches for changes to this file and will automatically update settings when it's modified.

## Quick navigation

- [Feature flags](#feature-flags)
- [Model selection](#model-selection)
- [Execution environment](#execution-environment)
- [MCP integration](#mcp-integration)
- [Security and approvals](#security-and-approvals)
- [Participant system](#participant-system)
- [Profiles and overrides](#profiles-and-overrides)
- [Reference table](#config-reference)
- [Generated field reference](./config/CONFIG_FIELD_REFERENCE.md)

VT Code supports several mechanisms for setting config values:

- The `$VTCODE_HOME/config.toml` configuration file where the `VTCODE_HOME` environment value defaults to `~/.vtcode`.
- The workspace-level `vtcode.toml` file that can be placed at the root of your project (similar to `AGENTS.md` in the OpenAI Codex).
- Environment variables that can override certain configuration options.

Both the workspace `vtcode.toml` and the main `config.toml` file support the following options:

## Feature flags

Optional and experimental capabilities are toggled via the `[features]` table in `vtcode.toml`. These allow you to customize the behavior of various VT Code features.

```toml
[features]
streaming = true           # enable streaming responses
human_in_the_loop = true   # enable human-in-the-loop tool approval
participant_context = true # include participant context in messages
terminal_integration = true # enable terminal integration features
```

Supported features:

| Key                    | Default | Description                                |
| ---------------------- | :-----: | ------------------------------------------ |
| `streaming`            |  true   | Enable streaming responses in the UI       |
| `human_in_the_loop`    |  true   | Enable tool approval prompts               |
| `participant_context`  |  true   | Include participant context in messages    |
| `terminal_integration` |  true   | Enable terminal integration features       |
| `mcp_enabled`          |  false  | Enable Model Context Protocol integrations |

## Model selection

### agent.provider

The AI provider that VT Code should use.

```toml
[agent]
provider = "anthropic"  # available: openai, anthropic, google, together, fireworks, ollama
default_model = "claude-sonnet-4-5"  # overrides the default model for the selected provider
```

### agent.provider_settings

This option lets you customize the settings for different AI providers.

For example, if you wanted to add custom API endpoints or settings for a provider:

```toml
[agent.provider_settings.openai]
name = "OpenAI"
base_url = "https://api.openai.com/v1"
env_key = "OPENAI_API_KEY"
# Extra query params that need to be added to the URL
query_params = {}

[agent.provider_settings.anthropic]
name = "Anthropic"
base_url = "https://api.anthropic.com/v1"
env_key = "ANTHROPIC_API_KEY"

[agent.provider_settings.google]
name = "Google Gemini"
base_url = "https://generativelanguage.googleapis.com/v1beta"
env_key = "GOOGLE_GEMINI_API_KEY"
# Note: Google's API uses a different format
query_params = { key = "$GOOGLE_GEMINI_API_KEY" }

[agent.provider_settings.ollama]
name = "Ollama"
base_url = "http://localhost:11434/v1"
# No API key required for local Ollama instance
```

Note this makes it possible to use VT Code with non-default models, so long as they are properly configured with the correct API endpoints and authentication.

Or a third-party provider (using a distinct environment variable for the API key):

```toml
[agent.provider_settings.mistral]
name = "Mistral"
base_url = "https://api.mistral.ai/v1"
env_key = "MISTRAL_API_KEY"
```

It is also possible to configure a provider to include extra HTTP headers with a request. These can be hardcoded values (`http_headers`) or values read from environment variables (`env_http_headers`):

```toml
[agent.provider_settings.example]
# name, base_url, ...
# This will add the HTTP header `X-Example-Header` with value `example-value`
# to each request to the model provider.
http_headers = { "X-Example-Header" = "example-value" }
# This will add the HTTP header `X-Example-Features` with the value of the
# `EXAMPLE_FEATURES` environment variable to each request to the model provider
# _if_ the environment variable is set and its value is non-empty.
env_http_headers = { "X-Example-Features" = "EXAMPLE_FEATURES" }
```

### Model-specific settings

You can also configure model-specific behavior:

```toml
[agent.model_settings]
context_window = 128000    # Context window size in tokens
max_output_tokens = 4096   # Maximum tokens for model output
temperature = 0.7          # Model temperature (0.0-2.0)
top_p = 0.9                # Top-P sampling parameter
```

## Execution environment

### workspace.settings

Controls various workspace-specific settings for VT Code execution:

```toml
[workspace]
# By default, VT Code will look for a vtcode.toml file in the root of your workspace
# This determines the behavior when multiple workspaces exist
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

## MCP integration

### mcp

You can configure VT Code to use [Model Context Protocol (MCP) servers](https://modelcontextprotocol.io/) to give VT Code access to external applications, resources, or services.

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
shell_exec = "ask"        # Options: "ask", "allow", "deny"
write_file = "ask"        # Options: "ask", "allow", "deny"
read_file = "allow"       # Options: "ask", "allow", "deny"
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
allowed_tools = ["read_file", "web_search", "shell_exec"]
```

## Participant system

### participants

Controls the behavior of the participant system that provides context augmentation:

```toml
[participants]
# Enable participant system for @mention support
enabled = true

# Default participants to always include
default_participants = ["@workspace", "@code"]

# Timeout for participant context resolution (in seconds)
timeout = 15

# Whether to cache participant context between messages
cache_context = true

# Maximum size of context that each participant can provide
max_context_size = 524288  # 512KB
```

### participant.settings

Individual settings for different participants:

```toml
[participants.workspace]
# Include file statistics in workspace context
include_file_stats = true

# Include git status in workspace context
include_git_status = true

# Maximum number of files to list
max_files_to_list = 100

[participants.code]
# Include syntax highlighting information
include_syntax_info = true

# Maximum file size to send for code context (in bytes)
max_file_size = 262144  # 256KB

[participants.terminal]
# Include recent terminal commands
include_recent_commands = true

# Number of recent commands to include
recent_commands_limit = 10

[participants.git]
# Include git repository information
include_repo_info = true

# Include git diff information
include_diff = false
```

## Profiles and overrides

### profiles

A _profile_ is a collection of configuration values that can be set together. Multiple profiles can be defined in `vtcode.toml` and you can specify the one you want to use depending on the project type or your current task.

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
default_model = "claude-haiku-4-5"

[profiles.research.tools.policies]
web_search = "allow"
read_file = "allow"
shell_exec = "deny"

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

Users can specify config values at multiple levels. Order of precedence is as follows:

1. Workspace-level `vtcode.toml` (highest precedence in the workspace)
2. User-level `$VTCODE_HOME/config.toml`
3. Extension defaults (lowest precedence)

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
default_model = "claude-haiku-4-5"

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
FIREWORKS_API_KEY=your_fireworks_api_key
TOGETHER_API_KEY=your_together_api_key
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

## VS Code Integration

### VS Code Commands for Configuration

VT Code VS Code extension provides several commands to help manage configuration:

- `VT Code: Open Configuration` - Opens the workspace `vtcode.toml` file if it exists
- `VT Code: Toggle Human-in-the-Loop` - Quickly toggle the human_in_the_loop setting
- `VT Code: Configure MCP Providers` - Helper command to manage MCP provider settings
- `VT Code: Open Tools Policy Configuration` - Opens the tools policy section of the config

### Command System Integration

The VT Code extension uses a command system that can be configured through the settings:

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

VT Code follows VS Code's workspace trust model. Some features are only available in trusted workspaces:

```toml
# This setting is respected by VS Code when determining workspace trust
[security]
trusted_workspace_mode = true
```

In untrusted workspaces, VT Code limits CLI automation capabilities to protect your system.

## Configuration Validation and Troubleshooting

### Validation

VT Code validates the configuration file on load. You can check for configuration errors by:

1. Looking at the VT Code output channel in VS Code
2. Using the `VT Code: Open Configuration` command which will highlight any parsing errors
3. Running `vtcode check-config` from the command line if you have the CLI installed

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
[`docs/config/CONFIG_FIELD_REFERENCE.md`](./config/CONFIG_FIELD_REFERENCE.md).

| Key                                     | Type / Values                                     | Notes                                                                   |
| --------------------------------------- | ------------------------------------------------- | ----------------------------------------------------------------------- |
| `agent.provider`                        | string                                            | Provider to use (e.g., `openai`, `anthropic`, `google`, `ollama`).      |
| `agent.default_model`                   | string                                            | Default model for the selected provider.                                |
| `agent.context_window`                  | number                                            | Context window tokens.                                                  |
| `agent.max_output_tokens`               | number                                            | Max output tokens.                                                      |
| `agent.temperature`                     | number                                            | Model temperature (0.0-2.0).                                            |
| `agent.top_p`                           | number                                            | Top-P sampling parameter (0.0-1.0).                                     |
| `context.semantic_compression`          | boolean                                           | Enable structural-aware context compression (default: false).           |
| `context.tool_aware_retention`          | boolean                                           | Extend retention for recent tool outputs (default: false).              |
| `context.max_structural_depth`          | number                                            | AST depth preserved when semantic compression is enabled (default: 3).  |
| `context.preserve_recent_tools`         | number                                            | Recent tool outputs to preserve when retention is enabled (default: 5). |
| `security.human_in_the_loop`            | boolean                                           | Enable tool approval prompts (default: true).                           |
| `security.default_tool_policy`          | `ask` \| `allow` \| `deny`                        | Default tool execution policy.                                          |
| `tools.policies.*`                      | `ask` \| `allow` \| `deny`                        | Policies for specific tools.                                            |
| `mcp.enabled`                           | boolean                                           | Enable MCP integration (default: false).                                |
| `mcp.providers[].name`                  | string                                            | MCP provider name.                                                      |
| `mcp.providers[].command`               | string                                            | MCP provider command to execute.                                        |
| `mcp.providers[].args`                  | array                                             | Arguments for the MCP command.                                          |
| `mcp.providers[].enabled`               | boolean                                           | Whether the provider is enabled.                                        |
| `participants.enabled`                  | boolean                                           | Enable participant system (default: true).                              |
| `participants.default_participants`     | array                                             | Default participants to include.                                        |
| `participants.timeout`                  | number                                            | Timeout for participant context (seconds).                              |
| `automation.full_auto.enabled`          | boolean                                           | Enable full automation mode.                                            |
| `automation.full_auto.allowed_tools`    | array                                             | Tools allowed in automation mode.                                       |
| `workspace.include_context`             | boolean                                           | Include workspace context.                                              |
| `workspace.max_context_size`            | number                                            | Max size of workspace context (bytes).                                  |
| `execution.tool_timeout`                | number                                            | Timeout for tool executions (seconds).                                  |
| `execution.api_timeout`                 | number                                            | Timeout for API calls (seconds).                                        |
| `telemetry.enabled`                     | boolean                                           | Enable telemetry (default: false).                                      |
| `telemetry.analytics`                   | boolean                                           | Enable usage analytics.                                                 |
| `logging.enabled`                       | boolean                                           | Enable detailed logging.                                                |
| `logging.level`                         | `error` \| `warn` \| `info` \| `debug` \| `trace` | Log level.                                                              |
| `auth.secure_storage`                   | boolean                                           | Store credentials securely (default: true).                             |
| `auth.validate_keys`                    | boolean                                           | Validate API keys on startup.                                           |
| `commands.ask_agent_enabled`            | boolean                                           | Enable the ask agent command.                                           |
| `commands.analyze_enabled`              | boolean                                           | Enable the analyze command.                                             |
| `commands.command_timeout`              | number                                            | Command execution timeout (seconds).                                    |
| `profiles.*.agent.provider`             | string                                            | Provider override for a profile.                                        |
| `profiles.*.security.human_in_the_loop` | boolean                                           | Security setting override for a profile.                                |
| `profiles.*.tools.policies.*`           | `ask` \| `allow` \| `deny`                        | Tool policy override for a profile.                                     |
