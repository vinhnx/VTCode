# VT Code CLI Improvements from Claude CLI Reference

This document summarizes the CLI enhancements applied from Claude's CLI patterns to VT Code.

## Implemented Features

### 1. Multiple Workspace Support (`--add-dir`)

-   **Flag**: `--add-dir <PATH>`
-   **Description**: Add additional working directories for the agent to access
-   **Compatibility**: Repeats allowed, validates each path exists as a directory
-   **Example**: `vtcode --add-dir ../apps --add-dir ../libs chat`
-   **Implementation**: Added `additional_dirs: Vec<PathBuf>` to `Cli` struct and `StartupContext`

### 2. Enhanced Security Control

#### `--dangerously-skip-permissions`

-   **Alias**: `--skip-confirmations` (for backwards compatibility)
-   **Description**: Skip all permission prompts (use with extreme caution)
-   **Implementation**: Enhanced with clearer naming while maintaining backwards compatibility

#### `--permission-mode <MODE>`

-   **Description**: Begin in a specified permission mode
-   **Options**:
    -   `ask`: Prompt for every tool execution (default)
    -   `suggest`: Agent suggests tools but asks for approval
    -   `auto-approved`: Allowed tools run automatically
    -   `full-auto`: All tools run without prompts
-   **Example**: `vtcode --permission-mode suggest chat`
-   **Implementation**: Maps to security config and full_auto settings

### 3. IDE Integration

#### `--ide`

-   **Description**: Automatically connect to IDE on startup if exactly one valid IDE is available
-   **Implementation**: Auto-detects Zed IDE (currently only supported IDE) via environment variables
-   **Example**: `vtcode --ide chat`
-   **Auto-detection**: Checks for `ZED_CLI` or `VIMRUNTIME` environment variables

### 4. Tool Filtering (Framework Ready)

#### `--allowed-tools <TOOLS>`

-   **Description**: Tools that execute without prompting for permission
-   **Format**: Comma-separated list or multiple flags
-   **Example**: `vtcode --allowed-tools "Read,Edit,Grep" --allowed-tools "Bash(git:*)"`
-   **Framework**: CLI flags defined, ready for tool registry integration

#### `--disallowed-tools <TOOLS>`

-   **Description**: Tools that cannot be used by the agent
-   **Format**: Comma-separated list or multiple flags
-   **Example**: `vtcode --disallowed-tools "Bash(rm:*),Bash(sudo:*)"`
-   **Framework**: CLI flags defined, ready for tool registry integration

### 5. Platform Integration Flags

#### `--chrome` and `--no-chrome`

-   **Description**: Enable/disable Chrome browser integration for web automation
-   **Status**: CLI flags defined, ready for browser automation implementation
-   **Example**: `vtcode --chrome chat`

## Implementation Details

### Modified Files

1. **vtcode-core/src/cli/args.rs**

    - Added new CLI flags with proper clap attributes
    - Updated `Default` implementation for `Cli`
    - Added comprehensive documentation for each flag

2. **src/startup/mod.rs**

    - Enhanced `StartupContext` with `additional_dirs` field
    - Added `validate_additional_directories()` function
    - Added `apply_permission_mode_override()` function to map permission modes to config

3. **src/cli/mod.rs**

    - Added `set_additional_dirs_env()` function
    - Added PathBuf import

4. **src/main.rs**
    - Added `detect_available_ide()` function for IDE auto-detection
    - Integrated `--ide` flag handling
    - Added environment variable setup for additional directories

## Usage Examples

### Multiple Workspaces

```bash
# Work with multiple directories
vtcode --add-dir ../frontend --add-dir ../backend --add-dir ../shared chat
```

### Permission Modes

```bash
# Ask for every tool execution
vtcode --permission-mode ask chat

# Suggest tools but ask for approval
vtcode --permission-mode suggest chat

# Auto-run allowed tools only
vtcode --permission-mode auto-approved chat

# Full auto mode (dangerous!)
vtcode --permission-mode full-auto --dangerously-skip-permissions chat
```

### IDE Integration

```bash
# Auto-connect to available IDE
vtcode --ide chat

# Or explicitly use ACP
vtcode acp zed
```

### Security Levels

```bash
# Use new explicit flag (same as --skip-confirmations but clearer)
vtcode --dangerously-skip-permissions --print "explain this code"

# Old flag still works for backwards compatibility
vtcode --skip-confirmations --print "explain this code"
```

## Backwards Compatibility

All changes maintain backwards compatibility:

-   `--skip-confirmations` still works (aliased to `--dangerously-skip-permissions`)
-   Existing command structure unchanged
-   New flags are optional and don't affect default behavior

## Future Enhancements

The following flags are defined in CLI but not yet fully implemented:

-   `--allowed-tools` and `--disallowed-tools`: Framework ready for tool registry integration
-   `--chrome` and `--no-chrome`: Ready for browser automation implementation

## Benefits

1. **Better User Experience**: More intuitive flag names and clearer help text
2. **Enhanced Security**: Explicit `--dangerously-skip-permissions` makes risks clear
3. **Flexible Workflows**: Multiple workspace support enables monorepo development
4. **IDE Integration**: Seamless IDE integration with explicit `--ide` flag
5. **Permission Control**: Granular permission modes for different use cases
6. **Future-Ready**: Framework for advanced tool filtering and browser automation
