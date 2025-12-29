# VT Code Zed Extension Features

This document outlines the features provided by the VT Code extension for Zed.

## Current Features

### 1. Language Support for vtcode.toml

-   **Syntax Highlighting**: TOML syntax highlighting specific to VT Code configuration files
-   **File Association**: Automatically detects and highlights `vtcode.toml` files
-   **Configuration Validation**: Basic TOML validation through Zed's language support

### 2. Integration with VT Code CLI

The extension provides seamless integration with the VT Code command-line agent:

-   Launches VT Code CLI commands from within Zed
-   Passes workspace context to the agent
-   Streams responses directly in the editor

### 3. Configuration Management

Users can edit their `vtcode.toml` configuration with:

-   Syntax highlighting for all configuration sections
-   Inline documentation for common settings
-   File validation for TOML syntax

## Planned Features

The following features are planned for future releases:

### Commands in Command Palette

-   **Ask the Agent**: Send arbitrary questions to VT Code agent
-   **Analyze Workspace**: Run VT Code's workspace analysis
-   **Launch Chat**: Open an interactive chat session
-   **Edit Configuration**: Quick access to vtcode.toml
-   **View Status**: Show VT Code CLI installation status

### Integration Features

-   **Selection Queries**: Ask about highlighted code snippets
-   **Code Analysis**: Get semantic analysis of workspace
-   **Refactoring Suggestions**: Receive AI-powered refactoring recommendations
-   **Context Awareness**: Automatic context passing to agent

### Editor Features

-   **Completions**: Code completions from VT Code agent
-   **Diagnostics**: Integration with VT Code's diagnostic tools
-   **Code Lens**: Display actionable information inline
-   **Quick Fixes**: VT Code agent-powered code fixes

### Settings and Configuration

-   **Custom VT Code Path**: Override default vtcode command path
-   **API Key Management**: Secure handling of AI provider API keys
-   **Output Channel**: Dedicated output channel for VT Code responses
-   **Logging Options**: Control verbosity of logging

## Architecture

```

       Zed Editor

   VT Code Extension (WASM Binary)

    • Language Support
    • Configuration Management
    • CLI Integration




                vtcode.toml (Configuration)

                VT Code CLI Binary
                   (Handles AI Logic)
```

## Configuration Structure

The extension expects the following `vtcode.toml` structure:

```toml
# AI Provider Configuration
[ai]
provider = "anthropic"  # or "openai", "gemini", etc.
model = "claude-4-5-sonnet"

# Workspace Settings
[workspace]
analyze_on_startup = false
max_context_tokens = 8000
ignore_patterns = ["node_modules", ".git"]

# Security Settings
[security]
human_in_the_loop = true
allowed_tools = ["read_file", "edit_file", "analyze_code"]
```

## Workflow Examples

### Example 1: Analyze Current File

1. Open a file in your workspace
2. Run "VT Code: Ask About Selection" (right-click on code)
3. Extension passes the selected code to VT Code agent
4. Response appears in output channel

### Example 2: Configure Workspace

1. Run "VT Code: Open Configuration"
2. Edit `vtcode.toml` with syntax highlighting
3. Save file - configuration is automatically picked up
4. Future agent calls use updated configuration

### Example 3: Ask General Questions

1. Run "VT Code: Ask the Agent"
2. Type your question
3. Agent processes with workspace context
4. Response streams to output channel

## Dependencies

-   **VT Code CLI**: Must be installed and in PATH
-   **Zed**: Version 0.150.0 or higher recommended
-   **Rust**: For building from source (development only)

## Compatibility

-   **Platforms**: macOS, Linux, Windows
-   **Zed Versions**: Latest stable release
-   **VT Code Versions**: 0.1.0 and later

## Performance Considerations

-   Extension runs in WebAssembly sandbox
-   All heavy computation delegated to VT Code CLI
-   Minimal memory footprint in editor process
-   Asynchronous command execution prevents UI blocking

## Security Model

-   Extension runs in isolated WASM environment
-   No direct file system access
-   All commands validated before execution
-   Respects Zed's trust model for workspaces

## Troubleshooting

### VT Code CLI Not Found

-   Ensure VT Code is installed: `cargo install vtcode`
-   Check PATH: `which vtcode`
-   Configure custom path in Zed settings

### Configuration Not Loading

-   Verify `vtcode.toml` exists in workspace root
-   Check TOML syntax validity
-   Review Zed logs for error details

### Commands Not Appearing

-   Reload extension: Close and reopen Zed
-   Check extension installation in Extensions panel
-   Verify extension.toml is valid

## Feedback and Support

Report issues or request features at:
[VT Code GitHub Issues](https://github.com/vinhnx/vtcode/issues)

## Related Documentation

-   [VT Code Main Documentation](https://github.com/vinhnx/vtcode)
-   [Zed Editor Documentation](https://zed.dev/docs)
-   [TOML Specification](https://toml.io/)
