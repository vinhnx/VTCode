# VT Code Terminal Optimization Guide

This guide covers the terminal optimization features available in VT Code, designed to enhance your terminal coding experience with advanced capabilities similar to Claude Code.

## Table of Contents

-   [Theme and Appearance](#theme-and-appearance)
-   [Line Break Options](#line-break-options)
-   [Vim Mode](#vim-mode)
-   [Notification Setup](#notification-setup)
-   [Handling Large Inputs](#handling-large-inputs)

## Theme and Appearance

VT Code supports configurable themes for optimal visual experience. You can configure the theme in your `vtcode.toml` configuration file:

```toml
[agent]
theme = "vitesse-dark"  # or other available themes
```

The theme configuration affects the terminal interface, syntax highlighting, and overall visual appearance of the UI.

## Line Break Options

VT Code provides multiple options for handling line breaks in the terminal:

### Backslash + Enter (Quick Escape)

-   Type `\` followed by `Enter` to create a newline without submitting the input
-   This is useful for multi-line input without triggering command execution
-   Example: When writing multi-line code snippets or complex commands

### Shift+Enter

-   Press `Shift+Enter` to insert a newline character in the input field
-   This allows for multi-line input while staying in the same input context
-   Works in most terminal emulators including iTerm2, VS Code Terminal, Kitty, and others

### Keyboard Shortcuts

-   `Enter` = Submit the current input
-   `Ctrl+Enter` or `Cmd+Enter` = Queue the input for later processing
-   `Shift+Enter` = Insert newline without submitting
-   `Esc` = Cancel current input or close modals

## Vim Mode

VT Code includes a Vim mode with a subset of Vim keybindings for efficient text editing:

### Mode Switching

-   `i`, `a`, `o`, `O`, `A`, `I` → Enter Insert mode
-   `Esc` → Return to Normal mode
-   `/vim` → Toggle Vim mode on/off via slash command

### Navigation (Normal Mode)

-   `h`, `j`, `k`, `l` → Move left/down/up/right
-   `w` → Move to next word start
-   `e` → Move to end of current word
-   `b` → Move to previous word start
-   `0` → Move to start of line
-   `$` → Move to end of line
-   `^` → Move to first non-blank character of line
-   `gg` → Move to top of transcript
-   `G` → Move to bottom of transcript

### Editing (Normal Mode)

-   `x` → Delete character under cursor
-   `dd` → Delete current line
-   `dw` → Delete word
-   `de` → Delete to end of word
-   `db` → Delete to start of word
-   `D` or `d$` → Delete to end of line
-   `cc` → Change current line
-   `cw` → Change word
-   `ce` → Change to end of word
-   `cb` → Change to start of word
-   `C` or `c$` → Change to end of line
-   `.` → Repeat last command

## Notification Setup

VT Code provides comprehensive notification support for task completion and important events:

### Task Completion Notifications

-   Configure task completion hooks in your `vtcode.toml`:

```toml
[hooks.lifecycle]
task_completion = [
  {
    matcher = ".*",  # Match all tasks
    hooks = [
      {
        command = "echo 'Task completed: $VT_HOOK_EVENT' >> /tmp/vtcode_notifications.log",
        timeout_seconds = 10
      }
    ]
  }
]
```

### System Notifications

-   VT Code supports terminal bell notifications for important events
-   Configure in security settings:

```toml
[security]
hitl_notification_bell = true  # Enable bell for human-in-the-loop prompts
```

### iTerm2 Notifications

For iTerm2 users, you can configure system notifications:

1. Open iTerm2 Preferences
2. Navigate to Profiles → Terminal
3. Enable "Silence bell" and "Send escape sequence-generated alerts"
4. Set your preferred notification delay

### Custom Notification Hooks

You can create custom notification hooks for various lifecycle events:

-   `session_start` - When a session begins
-   `session_end` - When a session ends
-   `user_prompt_submit` - When user submits a prompt
-   `pre_tool_use` - Before tools execute
-   `post_tool_use` - After tools execute successfully
-   `task_completion` - When tasks complete (new feature)

## Handling Large Inputs

VT Code provides several methods for handling large inputs efficiently:

### File-Based Workflows

Use the `@` symbol to reference files directly in your input:

-   `@filename.txt` - Include content from filename.txt
-   `@"file with spaces.txt"` - Include content from files with spaces
-   `@'single quoted file.txt'` - Include content from files with special characters

This allows you to reference large files without pasting content directly into the terminal.

### Piped Input

You can pipe content directly to VT Code:

```bash
cat large_file.txt | vtcode ask "Analyze this content"
```

### Large Output Handling

VT Code automatically handles large tool outputs by:

-   Spooling large outputs to log files when they exceed configured thresholds
-   Providing compact output display modes
-   Supporting streaming for real-time processing

Configure large output handling in your `vtcode.toml`:

```toml
[ui]
tool_output_mode = "compact"
tool_output_max_lines = 50
tool_output_spool_bytes = 200000  # 200KB threshold for spooling
```

## Configuration Example

Here's a complete example configuration in `vtcode.toml` with optimization features enabled:

```toml
[agent]
provider = "openai"
default_model = "gpt-4o"
theme = "vitesse-dark"

[security]
human_in_the_loop = true
hitl_notification_bell = true

[ui]
tool_output_mode = "compact"
tool_output_max_lines = 600

[ui.status_line]
mode = "auto"

[pty]
enabled = true
default_rows = 24
default_cols = 120

# Lifecycle hooks for notifications
[hooks.lifecycle]
task_completion = [
  {
    matcher = ".*",
    hooks = [
      {
        command = "osascript -e 'display notification \"VT Code task completed\" with title \"VT Code\"' 2>/dev/null || true",
        timeout_seconds = 5
      }
    ]
  }
]

session_start = [
  {
    hooks = [
      {
        command = "echo 'VT Code session started at $(date)' >> ~/.vtcode/session.log",
        timeout_seconds = 5
      }
    ]
  }
]
```

## Troubleshooting

### Vim Mode Issues

-   If Vim mode feels unresponsive, try using `/vim` to toggle it off and on
-   Some key combinations may conflict with terminal shortcuts; adjust as needed

### Notification Issues

-   On macOS, ensure terminal has notification permissions
-   For iTerm2, check Terminal profile settings for bell configuration
-   Test hooks with simple commands before using complex notification scripts

### Large Input Issues

-   If file references aren't working, ensure the file exists and is accessible
-   Check that the `@` symbol is followed by a valid file path
-   Use quotes for file paths containing spaces or special characters

## Performance Tips

1. Use file references (`@filename`) instead of pasting large content
2. Configure appropriate output limits to prevent terminal flooding
3. Use Vim mode for efficient text editing in the terminal
4. Set up notification hooks for important events to stay informed
5. Configure terminal-specific settings (Shift+Enter, etc.) for optimal experience

This guide provides the foundation for optimizing your VT Code terminal experience. For more detailed information about specific features, refer to the relevant documentation sections.
