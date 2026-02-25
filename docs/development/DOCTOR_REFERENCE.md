# Doctor Command Quick Reference

## Output Sections

### [Core Environment]

-   **Workspace**: Project root directory location
-   **CLI Version**: VT Code binary version

### [Configuration]

Active settings from `vtcode.toml`:

-   **Config File**: Location of configuration file
-   **Theme**: ANSI color theme (affects terminal output)
-   **Model**: Primary LLM + small model if enabled
-   **Max Turns**: Conversation turn limit before auto-stop
-   **Context Tokens**: Maximum tokens in memory
-   **Token Budget**: Usage tracking and limits
-   **Decision Ledger**: Decision history tracking
-   **Max Tool Loops**: Tool call iteration limit
-   **HITL Enabled**: Human-in-the-loop approval status
-   **Tool Policy**: Default tool execution policy
-   **PTY Enabled**: Pseudo-terminal support status

### [API & Providers]

-   **API Key**: Authentication status for active provider

### [Dependencies]

-   **Node.js**: JavaScript runtime
-   **npm**: Package manager
-   **Ripgrep**: Fast code search tool

### [External Services]

-   **MCP**: Model Context Protocol provider status

### [Workspace Links]

-   Indexed list of linked directories with paths

### [Skills]

-   Count of loaded skills in current session
-   Skill names with scope indicators (user/repo)

## Status Indicators

-   `✓` - Check passed (shown in Status color)
-   `✗` - Check failed (shown in Error color)

## What Gets Diagnosed

### Configuration Options

| Option                         | Purpose                  | Default       |
| ------------------------------ | ------------------------ | ------------- |
| `theme`                        | Terminal color scheme    | `ciapre-dark` |
| `model`                        | Primary LLM model        | `gpt-5-nano`  |
| `small_model.enabled`          | Use efficient model tier | `true`        |
| `max_conversation_turns`       | Turn limit               | `50`          |
| `context.max_context_tokens`   | Memory limit             | `90000`       |
| `context.token_budget.enabled` | Track token usage        | `false`       |
| `context.ledger.enabled`       | Track decisions          | `true`        |
| `tools.max_tool_loops`         | Iteration limit          | `20`          |
| `security.human_in_the_loop`   | Require approval         | `true`        |
| `tools.default_policy`         | Tool exec policy         | `prompt`      |
| `pty.enabled`                  | PTY support              | `true`        |

### Checked Tools

| Tool      | Status    | Purpose                                      |
| --------- | --------- | -------------------------------------------- |
| Workspace | Required  | Project directory accessible                 |
| Config    | Important | `vtcode.toml` loaded                         |
| API Key   | Required  | Provider authentication                      |
| Node.js   | Optional  | JavaScript runtime support                   |
| npm       | Optional  | Package management                           |
| Ripgrep   | Optional  | Fast code search (fallback to built-in grep) |
| MCP       | Optional  | External service providers                   |

## Quick Diagnosis

### Failure Suggestions

The doctor automatically provides contextual suggestions when checks fail:

**Missing API Key**

```
✗ API Key: Missing API key for 'openai': OPENAI_API_KEY not found
  → Set API key: export OPENAI_API_KEY=sk-... or similar for your provider.
```

**No Configuration File**

```
✓ Config File: Using runtime defaults (no vtcode.toml found)
```

(No failure - but if you want customization)
→ Copy `vtcode.toml.example` to `vtcode.toml`

**Ripgrep Not Installed**

```
✗ Ripgrep: Ripgrep not installed (searches will fall back to built-in grep)
  → Install Ripgrep: brew install ripgrep (macOS), apt install ripgrep (Linux), or cargo install ripgrep
```

**MCP Connection Error**

```
✗ MCP: Init error: Failed to start MCP server 'time'
  → Check MCP configuration in vtcode.toml: ensure servers are running and timeouts are reasonable.
```

### Recommended Next Actions

After running `/doctor`, look for:

1. **All Pass (✓)**: You're ready! Try `/status` for session details
2. **Some Failures (✗)**: Follow the suggestions (→) to resolve issues
3. **Need Details**: Use `/skills list`, `/status`, or `/context` for more info

See the [Recommended Next Actions] section at the end of doctor output for quick guidance.

## Related Commands

-   `/status` - Show current session status and token usage
-   `/context` - Display context usage breakdown
-   `/cost` - Show token usage summary
-   `/debug` - Show debug information
-   `/analyze` - Analyze agent behavior patterns
-   `/skills list` - List available skills
-   `/skills load <name>` - Load a skill into session
-   `/skills info <name>` - Show skill details

## Configuration Documentation

See these files for detailed configuration:

-   `docs/config/CONFIGURATION_PRECEDENCE.md` - How settings are loaded
-   `vtcode.toml.example` - Example configuration with all options
-   `docs/providers/PROVIDER_GUIDES.md` - Provider-specific setup
