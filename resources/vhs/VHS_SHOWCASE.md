# VT Code VHS Showcase

This directory contains VHS (Video Hosting System) showcases for VT Code, demonstrating the key features and agentic capabilities of the VT Code terminal coding agent.

## Demo Files

### Basic Demos
- `demo.tape` - Basic features overview including version, help, and ask commands
- `demo.gif` - Generated GIF from basic demo

### Agentic Coding Demos
- `demo-agentic.tape` - Showcases VT Code's agentic coding capabilities (original version):
  - Directory and file operations
  - Project analysis with tree-sitter
  - File reading and creation
  - Tool usage demonstrations
- `demo-agentic.gif` - Generated GIF showing agentic coding features

- `demo-agentic-improved.tape` - Enhanced version with better timing for chat visibility:
  - Improved timing to show chat interactions properly
  - Better visibility of agent responses and tool usage
  - More detailed demonstrations of each capability
- `demo-agentic-improved.gif` - Generated GIF with enhanced chat visibility

### Tool Usage Demos
- `demo-tools.tape` - Focuses on tool usage and integration (original version):
  - Shell command execution (`pwd`, `ls`)
  - File operations (create, read)
  - Slash commands (`/command`, `/help`)
  - Tree-sitter code analysis
  - Verbose mode showing tool interactions
- `demo-tools.gif` - Generated GIF highlighting tool usage

- `demo-tools-improved.tape` - Enhanced version with better timing for chat visibility:
  - Improved timing to show chat interactions properly
  - Better visibility of tool execution and responses
  - More detailed demonstrations of tool usage
- `demo-tools-improved.gif` - Generated GIF with enhanced chat visibility

### TUI Demos
- `demo-tui.tape` - Showcases the advanced Terminal User Interface:
  - Interactive chat interface
  - Slash commands (`/help`, `/list-themes`)
  - Theme switching
  - Verbose mode
- `demo-tui.gif` - Generated GIF featuring the TUI experience

## What's Demonstrated

### Agentic Capabilities
- **Autonomous Navigation**: Using tools to discover current directory and file structure
- **File Operations**: Creating, reading, and modifying files autonomously
- **Command Execution**: Running shell commands via `run_terminal_cmd` and `/command`
- **Code Analysis**: Tree-sitter powered code structure analysis
- **Tool Integration**: Using built-in tools for various operations

### Core Features
- Interactive TUI with mouse support
- Multi-provider LLM support (OpenAI, Anthropic, Gemini, etc.)
- Tree-sitter code analysis
- Advanced context management
- Slash commands with `/exit`, `/help`, `/list-themes`, `/command`
- Real-time PTY integration

## Prerequisites

- [VHS](https://github.com/charmbracelet/vhs) installed on your system
- VT Code installed and available in your PATH
- Appropriate API keys for your chosen LLM provider

## How to Run

To regenerate any demo GIF:

```bash
# For agentic coding demo
vhs demo-agentic.tape

# For tool usage demo
vhs demo-tools.tape

# For TUI demo
vhs demo-tui.tape

# For basic demo
vhs demo.tape
```

To publish a demo to vhs.charm.sh:

```bash
vhs publish demo-agentic.gif
```

To modify a demo, edit the corresponding `.tape` file using VHS syntax, then regenerate the GIF.

## Customization

All demos use a custom theme with the following colors:
- Background: #262626
- Foreground: #BFB38F
- Selection: #D99A4E
- Cursor: #BF4545

## Troubleshooting

If the recording fails:
1. Ensure VT Code is installed and in your PATH
2. Check that you have the required API keys configured
3. Verify VHS is properly installed
4. Make sure the terminal has enough space to run the commands

For more information about VHS syntax, visit [github.com/charmbracelet/vhs](https://github.com/charmbracelet/vhs).

## Use Cases Showcased

### Agentic Coding Examples:
- "What directory are we in?" - Demonstrates directory discovery using tools
- "Can you list the files in this directory?" - Shows file listing capabilities
- "Create a simple hello world function in a new file called demo.rs" - File creation task
- "Can you show me the README.md file?" - File reading operation

### Tool Usage Examples:
- Running shell commands with `/command pwd` and `/command ls -la`
- Creating and reading files with content verification
- Using tree-sitter for code analysis
- Tool-based problem solving and verification

The demos showcase VT Code's ability to act as an autonomous coding agent that can navigate, analyze, create, and modify code projects using various integrated tools.