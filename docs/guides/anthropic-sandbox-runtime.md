# Anthropic Sandbox Runtime Integration Guide

VT Code includes integration with Anthropic's sandbox runtime (`srt`), providing enhanced security for command execution within the agent. The sandbox runtime creates a secure, isolated environment for running terminal commands with configurable permissions.

## Overview

The Anthropic sandbox runtime integration:
- Restricts filesystem access to your project workspace with configurable allow/deny rules
- Controls network access through domain-based allowlists
- Provides isolated execution environment preventing unauthorized system access
- Integrates seamlessly with VT Code's existing tool system

## Installation

First, install the Anthropic sandbox runtime:

```bash
npm install -g @anthropic-ai/sandbox-runtime
```

You can verify the installation by running:
```bash
srt --help
```

## Configuration

The sandbox runtime can be controlled through VT Code's slash command interface:

### Basic Commands
- `/sandbox` - Toggle sandboxing on or off
- `/sandbox enable` - Explicitly enable sandboxing
- `/sandbox disable` - Explicitly disable sandboxing
- `/sandbox status` - Show current sandbox configuration
- `/sandbox help` - Show available commands and usage

### Network Access Management
- `/sandbox allow-domain example.com` - Allow network access to a specific domain
- `/sandbox remove-domain example.com` - Remove a domain from the allowlist

## Usage Examples

### Enabling the Sandbox
```text
/sandbox enable
```
This will create sandbox settings in `.vtcode/sandbox/settings.json` and restrict command execution to the current workspace.

### Managing Network Access
```text
/sandbox allow-domain github.com
/sandbox allow-domain crates.io
```
Now terminal commands can access these domains for operations like `git clone` or `cargo add`.

### Checking Status
```text
/sandbox status
```
This shows:
- Current sandbox state (enabled/disabled)
- Settings file location
- Runtime binary location
- Network allowlist
- Default read restrictions

## Security Features

### Filesystem Permissions
- Read and write access limited to the project workspace
- Default deny rules prevent access to sensitive locations:
  - `~/.ssh` (SSH keys)
  - `/etc/ssh` (system SSH configuration)
  - `/root` (root user directory)
  - `/etc/shadow` (password hashes)

### Network Protection
- All network requests are blocked by default
- Only domains explicitly added to the allowlist can be accessed
- Supports both HTTP and HTTPS requests

### Isolated Execution
- Commands run in a restricted environment
- No access to system libraries or configuration outside the sandbox
- Process isolation prevents interference with system processes

## Integration with VT Code Tools

The sandbox integration works with VT Code's existing bash runner tool:

- `run_terminal_cmd` - Commands are executed in the sandboxed environment when enabled
- Network requests are controlled through the allowlist
- File operations are restricted to the workspace

## Configuration File

The sandbox settings are stored in `.vtcode/sandbox/settings.json`:

```json
{
  "sandbox": {
    "enabled": true
  },
  "permissions": {
    "allow": [
      "Edit(/path/to/workspace)",
      "Read(/path/to/workspace)",
      "Read(.)",
      "WebFetch(domain:github.com)"
    ],
    "deny": [
      "Read(~/.ssh)",
      "Read(/etc/ssh)",
      "Read(/root)",
      "Read(/etc/shadow)"
    ]
  }
}
```

## Environment Variables

- `SRT_PATH` - Override the path to the sandbox runtime binary if installed in a non-standard location

## Troubleshooting

### "srt was not found in PATH"
If you see the error "Anthropic sandbox runtime 'srt' was not found in PATH", ensure that:
1. The sandbox runtime is installed: `npm install -g @anthropic-ai/sandbox-runtime`
2. The npm global bin directory is in your PATH: `export PATH=$(npm config get prefix)/bin:$PATH`

### Network requests are blocked
If you need to access external resources (like for git operations or package management), add the domains to your allowlist:
```text
/sandbox allow-domain github.com
/sandbox allow-domain crates.io
```

### Sandbox settings not persisting
The sandbox settings are saved in `.vtcode/sandbox/settings.json` in your project root. This file should be git ignored by default but will persist sandbox configuration between VT Code sessions.

## Best Practices

1. **Start with tight permissions**: Enable sandboxing from the beginning of your session to ensure all access is properly configured.

2. **Manage network access proactively**: Add domains as needed rather than disabling sandboxing for network access.

3. **Monitor the allowlist**: Regularly review the network allowlist to ensure only necessary domains have access.

4. **Use with ACP integration**: The sandbox works well with Zed's Agent Client Protocol integration for enhanced security in your editor.

## Limitations

- Some complex terminal commands may not work in the sandboxed environment
- System-level operations that require access outside the workspace are restricted
- Performance may be slightly impacted due to the sandboxing overhead
- Interactive commands requiring TTY may have limited functionality

## Development and Testing

The sandbox integration is designed to work seamlessly with VT Code's existing testing infrastructure. When enabled, it provides an additional layer of safety during development and testing operations.