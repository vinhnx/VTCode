# Getting Started with VT Code

Welcome to VT Code! This guide will help you get up and running with this Rust-based terminal coding agent that provides intelligent.

## What Makes VT Code Special

VT Code represents a modern approach to AI-powered software development, featuring:

-   **Multi-Provider LLM Support** - Gemini, OpenAI, Anthropic integration
-   **Enhanced Terminal Interface** - Modern TUI with mouse support and text selection
-   **Advanced Code Intelligence** - Tree-sitter parsers for 6+ programming languages
-   **Enterprise-Grade Safety** - Comprehensive security controls and path validation
-   **Flexible Configuration** - TOML-based configuration with granular policies
-   **Research-Preview Features** - Cutting-edge agent coordination and context engineering
-   **Workspace-First Operations** - Full read/write/command access within `WORKSPACE_DIR` and built-in project indexing

## Prerequisites

### System Requirements

-   **Rust**: 1.75+ (stable recommended)
    -   Install from [rustup.rs](https://rustup.rs/)
    -   Includes Cargo package manager
-   **Git**: For version control and cloning the repository
-   **Operating System**: macOS, Linux, or Windows (with WSL2)

### API Requirements

Choose one of the supported LLM providers:

-   **Gemini** (Primary): `export GEMINI_API_KEY=your_key_here` or `export GOOGLE_API_KEY=your_key_here`

    -   Get from [Google AI Studio](https://aistudio.google.com/app/apikey)

-   **OpenAI**: `export OPENAI_API_KEY=your_key_here`

    -   Get from [OpenAI Platform](https://platform.openai.com/api-keys)

-   **Anthropic**: `export ANTHROPIC_API_KEY=your_key_here`
    -   Get from [Anthropic Console](https://console.anthropic.com/)

## Installation

### Option 1: Direct Download (Recommended)

```bash
# Clone the repository
git clone https://github.com/vinhnx/vtcode.git
cd vtcode

# Build the project
cargo build --release

# The binary will be available at target/release/vtcode
```

### Option 2: Using Provided Scripts

```bash
# Clone and build using the production script
git clone https://github.com/vinhnx/vtcode.git
cd vtcode

# Build and run in production mode
./run.sh

# Or build and run in development mode
./run-debug.sh
```

### Option 3: Cargo Install (Future)

```bash
# When published to crates.io
cargo install vtcode
```

## Quick Start

### 1. Set Your API Key

```bash
# For Gemini (recommended)
export GEMINI_API_KEY=your_api_key_here

# For OpenAI
export OPENAI_API_KEY=your_api_key_here

# For Anthropic
export ANTHROPIC_API_KEY=your_api_key_here
```

### 2. Confirm Workspace Context

`WORKSPACE_DIR` tells VT Code which project folder to treat as its primary context. The CLI sets this automatically when you launch inside a directory, but you can override or verify it explicitly:

```bash
# Confirm the workspace root
echo "Workspace: $WORKSPACE_DIR"

# Override if you need to target a different project
export WORKSPACE_DIR="/path/to/your/project"

# One-off override when launching commands
vtcode chat --workspace-dir /path/to/your/project
vtcode /path/to/your/project
```

### 3. Initialize VT Code in Your Project

```bash
# Navigate to your project
cd /path/to/your/project

# Initialize VT Code (creates vtcode.toml and .vtcodegitignore)
../vtcode/target/release/vtcode init

# Or if using scripts
/path/to/vtcode/run.sh init
```

### 4. Start Your First Session

```bash
# Start interactive chat
../vtcode/target/release/vtcode chat

# Or with the script
/path/to/vtcode/run.sh chat
```

### Resume Previous Sessions

- Run `vtcode --resume` to open an interactive picker of recent sessions, or `vtcode --resume <SESSION_ID>` to jump directly to a known session.
- Use `vtcode --continue` to resume the most recent session automatically without picking from the list.
- These flags are global; do not append a subcommand (for example, `vtcode chat --resume` is rejected). Resume or continue first, then start a new command if needed.

## Enhanced Terminal Interface

VT Code now features a completely revamped terminal user interface with numerous enhancements:

### Visual Improvements
- Modern, clean interface with rounded borders and improved styling
- Customizable color themes with Catppuccin color palette integration
- Enhanced message formatting with distinct markers for different message types
- Color-coded tool banners highlight execution summaries, detailed output, and MCP status updates
- Real-time command output display with proper ANSI color support

### Interaction Features
- Full mouse support for scrolling and text selection
- Interactive text selection with click-and-drag functionality
- Improved input area with placeholder text ("Implement {feature}...")
- Slash command auto-suggestions for quick access to built-in commands

### Terminal Command Support
- Real-time PTY (pseudo-terminal) integration for running shell commands
- Dedicated terminal output panels with command summaries
- Streaming output display for long-running processes
- Proper handling of ANSI escape sequences and colors

### Navigation & Controls
- Smooth scrolling with mouse wheel and keyboard shortcuts
- Page navigation with Page Up/Down keys
- Dedicated status bar with contextual information
- Clear exit and cancel controls (Esc key)

## Configuration

### Quick Slash Commands

During chat you can lead with `/` to trigger built-in actions without leaving the session:

-   `/help` — list supported commands
-   `/list-themes` — show available ANSI themes
-   `/theme <id>` — switch the active theme
-   `/command <program> [args...]` — run a shell command via `run_terminal_cmd`

Slash commands execute immediately and respect the same tool policies configured in `vtcode.toml`.

VT Code uses a comprehensive TOML configuration system. The `init` command creates a `vtcode.toml` file with sensible defaults.

### Basic Configuration

```toml
# Agent settings
[agent]
model = "gemini-2.5-flash-lite"  # Your preferred model
max_conversation_turns = 1000
verbose_logging = false

# Security settings
[security]
human_in_the_loop = true
confirm_destructive_actions = true
max_file_size_mb = 50

# Tool policies
[tools]
default_policy = "prompt"

[tools.policies]
read_file = "allow"
write_file = "prompt"
run_terminal_cmd = "prompt"
```

### Advanced Configuration

```toml
# Command permissions
[commands]
allow_list = ["ls", "pwd", "cat", "git status", "cargo check"]
deny_list = ["rm -rf", "sudo rm", "shutdown"]

# PTY settings
[pty]
enabled = true
default_rows = 24
default_cols = 80
command_timeout_seconds = 300
```

### Session Onboarding

VT Code opens each chat with a workspace-specific orientation generated by the context engine. Configure it with the `[agent.onboarding]` section:

```toml
[agent.onboarding]
enabled = true
intro_text = "Welcome! I preloaded workspace context so you can focus on decisions."
include_project_overview = true
include_language_summary = true
include_guideline_highlights = true
guideline_highlight_limit = 4
usage_tips = [
    "Share the outcome you need or ask for a quick /status summary.",
    "Reference AGENTS.md expectations before changing files.",
    "Draft or refresh your TODOs with update_plan before editing.",
    "Prefer targeted reads (read_file, grep_search) before editing.",
]
recommended_actions = [
    "Outline a 3-6 step TODO plan via update_plan before coding.",
    "Request a workspace orientation or describe the task you want to tackle.",
    "Confirm priorities or blockers so I can suggest next steps.",
]
# chat_placeholder = "Describe the task you want to tackle next."
```

When enabled, VT Code prints the onboarding message before the first prompt and appends the same context block to the system prompt for the model. Provide `chat_placeholder` only if you want a custom hint above the initial `>` prompt; leaving it unset keeps the input empty by default.

## Usage Examples

### Basic Chat Session

```bash
vtcode chat
```

The agent will greet you and await your instructions. Try asking:

-   "Analyze this codebase"
-   "Add error handling to the user authentication"
-   "Refactor this function to be more readable"
-   "Create a new API endpoint for user registration"

### Project Analysis

```bash
# Comprehensive project analysis
vtcode analyze

# Get detailed information
vtcode info

# Generate project summary
vtcode summary
```

## Understanding the Agents

### Orchestrator Agent

-   **Role**: Strategic coordinator and task planner
-   **Capabilities**: Task decomposition, agent delegation, progress tracking
-   **Use for**: Complex multi-step tasks requiring coordination

### Explorer Agent

-   **Role**: Investigation and verification specialist
-   **Capabilities**: Code analysis, system exploration, testing
-   **Use for**: Understanding codebases, verifying implementations

### Coder Agent

-   **Role**: Implementation specialist
-   **Capabilities**: Code writing, refactoring, debugging
-   **Use for**: Making code changes, adding features, fixing bugs

## Advanced Features

### Context Engineering

VT Code automatically manages conversation context:

-   **Intelligent compression** of long conversations
-   **Persistent context store** across sessions
-   **Automatic summarization** of completed tasks
-   **Context-aware responses** based on full history

### Safety Features

Comprehensive security controls:

-   **Path validation** - Prevents access outside workspace
-   **Command policies** - Allow/deny lists for terminal commands
-   **Human-in-the-loop** - Confirmation for dangerous operations
-   **File size limits** - Prevents processing of large files
-   **API key masking** - Secure credential handling

### Tool Integration

Rich tool ecosystem:

-   **File Operations**: Read, write, edit files safely
-   **Search & Analysis**: Fast text search and AST-based analysis
-   **Terminal Commands**: Execute shell commands with PTY support
-   **Code Intelligence**: Tree-sitter powered syntax analysis
-   **Batch Operations**: Process multiple files efficiently

## Troubleshooting

### Common Issues

#### API Key Not Set

```bash
# Check if API key is set
echo $GEMINI_API_KEY

# Set the API key
export GEMINI_API_KEY=your_key_here
```

#### Configuration Issues

```bash
# Validate configuration
vtcode config --validate

# Reset to defaults
vtcode config --reset
```

#### Build Issues

```bash
# Clean and rebuild
cargo clean
cargo build --release

# Check Rust version
rustc --version
cargo --version
```

### Getting Help

-   **Documentation**: Comprehensive guides in `docs/` directory
-   **GitHub Issues**: Report bugs and request features
-   **GitHub Discussions**: Community support and discussions
-   **Configuration Examples**: Check `vtcode.toml.example`

## Next Steps

Now that you have VT Code running, explore:

1. **[Configuration Guide](../CONFIGURATION.md)** - Advanced configuration options
1. **[Architecture Guide](../ARCHITECTURE.md)** - System design and components
1. **[Provider Guides](../PROVIDER_GUIDES.md)** - LLM provider integration
   - [OpenRouter Integration](../providers/openrouter.md)

## Contributing

Interested in contributing? Check out:

-   **[Development Guide](../development/README.md)** - Development setup
-   **[Contributing Guide](../../CONTRIBUTING.md)** - Contribution guidelines
-   **[Code Standards](../development/code-style.md)** - Coding conventions

---

**Happy coding with VT Code!** 🚀
