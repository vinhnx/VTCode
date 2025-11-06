# VTCode Chat Sidebar - Quick Start Guide

## Installation

### 1. Prerequisites

-   VS Code 1.87.0 or higher
-   Node.js 18+ (for development)
-   vtcode CLI installed and accessible in PATH

### 2. Setup Extension

```bash
cd vscode-extension

# Install dependencies
npm install

# Compile TypeScript
npm run compile

# Optional: Watch mode for development
npm run watch
```

### 3. Launch Extension

Press `F5` in VS Code to launch the Extension Development Host with the chat sidebar enabled.

## Usage

### Opening the Chat

1. Click the VTCode icon in the activity bar (left sidebar)
2. The chat panel will open showing an empty conversation
3. Type your message or command in the input box at the bottom

### Basic Chat

```
Hello! Can you help me with some code?
```

The agent will respond through the vtcode CLI backend.

### System Commands

Clear the transcript:

```
/clear
```

Show help:

```
/help
```

Export conversation:

```
/export
```

View statistics:

```
/stats
```

### Agent Commands

Analyze selected code:

```
@analyze
```

_Note: Select code in the editor first_

Explain code:

```
@explain
```

Get refactoring suggestions:

```
@refactor
```

Generate unit tests:

```
@test
```

### Tool Commands

Run a shell command:

```
#run command="ls -la"
```

Read a file:

```
#read path="./src/main.rs"
```

Write to a file:

```
#write path="./output.txt" content="Hello World"
```

## Features Demo

### 1. Simple Query

```
What is the best way to handle errors in Rust?
```

Response includes reasoning and detailed explanation.

### 2. Code Analysis

1. Open a Rust file
2. Select a function
3. Type: `@analyze`

The agent analyzes the selected code and provides insights.

### 3. Tool Execution with Approval

```
#run command="cargo build"
```

A confirmation dialog appears:

-   ✓ Approve - Executes the command
-   ✗ Reject - Cancels the operation

### 4. Multi-Turn Conversation

```
User: Can you explain async/await in Rust?
Agent: [Detailed explanation...]
User: Show me an example
Agent: [Code example with explanation...]
User: How do I handle errors in async code?
Agent: [Error handling patterns...]
```

All messages are preserved in the transcript.

## Configuration

### VS Code Settings

Open `settings.json` and add:

```json
{
    "vtcode.cli.path": "vtcode",
    "vtcode.chat.autoApproveTools": false,
    "vtcode.chat.maxHistoryLength": 100,
    "vtcode.chat.enableStreaming": true,
    "vtcode.chat.showTimestamps": true,
    "vtcode.chat.defaultModel": "gemini-2.5-flash-lite"
}
```

### Environment Variables

```bash
# Set API keys
export GEMINI_API_KEY="your-key-here"
export OPENAI_API_KEY="your-key-here"

# Enable debug logging
export RUST_LOG="debug"

# Set default model
export VTCODE_MODEL="gemini-2.5-flash-lite"
```

## Keyboard Shortcuts

-   `Enter` - Send message
-   `Shift+Enter` - New line in input
-   `Ctrl+C` (in chat) - Cancel current operation
-   `Ctrl+L` - Clear transcript (when focus is in chat)

## Troubleshooting

### CLI Not Found

**Error**: `vtcode CLI not found at: vtcode`

**Solution**:

1. Ensure vtcode is installed: `cargo install --path .`
2. Add to PATH or configure full path in settings
3. Restart VS Code

### Tool Execution Fails

**Error**: `Tool execution failed: permission denied`

**Solution**:

1. Check file permissions
2. Verify workspace trust settings
3. Enable tool in vtcode.toml configuration

### No Response from Agent

**Issue**: Message sent but no response

**Solution**:

1. Check Output panel (View → Output → VTCode)
2. Verify API key is set correctly
3. Check network connectivity
4. Try a different model

### Webview Not Loading

**Issue**: Chat panel is blank

**Solution**:

1. Open Developer Tools (Help → Toggle Developer Tools)
2. Check console for errors
3. Reload window (Ctrl+R)
4. Reinstall extension

## Tips & Best Practices

### 1. Context Management

Keep conversations focused. Use `/clear` to start fresh when switching topics.

### 2. Tool Approval

Always review tool arguments before approval, especially for:

-   File system operations
-   Shell command execution
-   API calls

### 3. Model Selection

-   Use `gemini-2.5-flash-lite` for quick responses
-   Use `gemini-2.5-thinking` for complex reasoning
-   Configure in settings or vtcode.toml

### 4. Transcript Export

Export important conversations:

```
/export
```

Saves as JSON file for later reference or analysis.

### 5. Multi-File Operations

For operations spanning multiple files:

1. Use agent commands (`@analyze`, `@refactor`)
2. Review suggested changes
3. Apply incrementally

## Advanced Usage

### Custom System Prompts

Modify `vtcode.toml`:

```toml
[agent]
system_prompt_additions = """
You are an expert Rust developer specializing in performance optimization.
Always consider memory safety and zero-cost abstractions.
"""
```

### Tool Policies

Configure tool access in `vtcode.toml`:

```toml
[tools]
allow_list = ["read_file", "write_file", "run_command"]
deny_list = ["delete_file"]
require_approval = true
```

### MCP Integration

Enable MCP tools for extended capabilities:

```toml
[[mcp.providers]]
name = "github"
command = "mcp-server-github"
```

## Next Steps

1. Explore the [CHAT_EXTENSION.md](./CHAT_EXTENSION.md) for architecture details
2. Review [extension.ts](./src/extension.ts) for integration examples
3. Check [vtcodeBackend.ts](./src/vtcodeBackend.ts) for CLI communication
4. Customize UI in [chat-view.css](./media/chat-view.css)
5. Extend functionality by adding new tools

## Support

-   GitHub Issues: https://github.com/vinhnx/vtcode/issues
-   Documentation: https://github.com/vinhnx/vtcode/tree/main/docs
-   Discord: [Join our community]

## License

Same as main vtcode project - see LICENSE file.
