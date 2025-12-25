# MiniMax-M2 Usage Examples

This document provides practical examples of using MiniMax-M2 with VTCode.

## Prerequisites

1. Get your MiniMax API key from [MiniMax](https://www.minimax.chat/)
2. Set it as your Anthropic API key:
    ```bash
    export ANTHROPIC_API_KEY=your_minimax_api_key_here
    ```

## Example 1: Basic Configuration

Create or update your `vtcode.toml`:

```toml
[agent]
provider = "anthropic"
api_key_env = "ANTHROPIC_API_KEY"
default_model = "MiniMax-M2"
```

## Example 2: Simple Query

```bash
# Start VTCode
vtcode

# Or use the ask command directly
vtcode ask "What are the key features of Rust?"
```

## Example 3: Code Analysis

```bash
vtcode ask "Analyze the code in src/main.rs and suggest improvements"
```

## Example 4: With Reasoning

MiniMax-M2 supports reasoning (thinking blocks). Configure reasoning effort:

```toml
[agent]
provider = "anthropic"
default_model = "MiniMax-M2"
reasoning_effort = "high"
```

Then use it:

```bash
vtcode ask "Design a scalable microservices architecture for an e-commerce platform"
```

## Example 5: Tool Calling

MiniMax-M2 supports function calling. VTCode's built-in tools work seamlessly:

```bash
vtcode ask "Read the README.md file and create a summary in summary.txt"
```

The model will automatically use the `read_file` and `write_file` tools.

## Example 6: Streaming Responses

Streaming is enabled by default. You'll see responses appear in real-time as the model generates them.

## Example 7: Custom Temperature

If you need to adjust temperature (must be in range 0.0 < temp <= 1.0):

```bash
# Note: Temperature is typically set in the request, not in vtcode.toml
# The default temperature should work fine for most use cases
```

## Example 8: Multi-turn Conversation

```bash
vtcode

# First turn
> "Create a simple Python web server"

# Second turn (context is preserved)
> "Now add error handling to it"

# Third turn
> "Add logging as well"
```

## Example 9: Using with MCP Tools

If you have MCP tools configured, they work with MiniMax-M2:

```toml
[mcp]
enabled = true

[[mcp.providers]]
name = "time"
command = "uvx"
args = ["mcp-server-time"]
enabled = true
```

Then:

```bash
vtcode ask "What time is it in Tokyo?"
```

## Example 10: Switching Between Models

You can easily switch between MiniMax and other models:

```bash
# Use MiniMax-M2
vtcode --model MiniMax-M2 ask "Explain async/await"

# Use Claude
vtcode --model claude-sonnet-4-5 ask "Explain async/await"

# Use GPT-5
vtcode --model gpt-5 ask "Explain async/await"
```

## Example 11: Environment Variable Override

Override the base URL if needed:

```bash
export ANTHROPIC_BASE_URL=https://your-proxy.com/anthropic/v1
vtcode
```

## Example 12: Full Auto Mode

Use MiniMax-M2 in full automation mode (use with caution):

```toml
[agent]
provider = "anthropic"
default_model = "MiniMax-M2"

[automation.full_auto]
enabled = true
max_turns = 30
allowed_tools = [
    "write_file",
    "read_file",
    "list_files",
    "grep_file",
]
```

## Tips

1. **Temperature**: MiniMax requires temperature in (0.0, 1.0]. The default works well.
2. **Context**: MiniMax-M2 has a 200K token context window, similar to Claude models.
3. **Reasoning**: Enable higher reasoning effort for complex tasks.
4. **Tool Calling**: Works seamlessly with all VT Code built-in tools.
5. **Streaming**: Enabled by default for better UX.

## Troubleshooting

### Issue: "Invalid temperature"

**Solution**: Ensure temperature is in range (0.0, 1.0]. Don't set it to 0.0 or above 1.0.

### Issue: "Model not found"

**Solution**: Use exact model name: `MiniMax-M2` (case-sensitive)

### Issue: "Authentication failed"

**Solution**: Verify your API key is set correctly:

```bash
echo $ANTHROPIC_API_KEY
```

### Issue: "Image input not supported"

**Solution**: MiniMax-M2 currently only supports text input, not images or documents.

## Performance Comparison

Based on typical usage:

| Task Type        | MiniMax-M2  | Claude Sonnet 4.5 | Notes                                |
| ---------------- | ----------- | ----------------- | ------------------------------------ |
| Code Generation  | Fast        | Fast              | Similar performance                  |
| Reasoning        | Good        | Excellent         | Claude has edge on complex reasoning |
| Tool Calling     | Excellent   | Excellent         | Both work well                       |
| Context Handling | 200K tokens | 200K tokens       | Same capacity                        |

## Next Steps

-   Read the [MiniMax Integration Guide](../guides/minimax-integration.md)
-   Check the [Tool Calling Guide](../guides/tool-calling.md)
-   Explore [Configuration Options](../config/CONFIGURATION_PRECEDENCE.md)
