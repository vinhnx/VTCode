# VT Code Terminal Optimization Guide

This guide covers the terminal-specific settings that matter most when using VT Code interactively.

## Table of Contents

- [Theme and Appearance](#theme-and-appearance)
- [Line Break Options](#line-break-options)
- [Notification Setup](#notification-setup)
- [Handling Large Inputs](#handling-large-inputs)

## Theme and Appearance

VT Code can match its own interface to the way you work in your terminal, but it does not control the terminal application's theme directly.

- Use `/config` to adjust VT Code appearance and related interactive settings.
- Use `/vim` to toggle session-local Vim prompt editing, or persist it with `ui.vim_mode = true`.
- Use `/statusline` to generate a custom status-line script in the user or workspace config layer.
- Use `[ui.status_line]` in `vtcode.toml` to customize the bottom status bar.
- Keep terminal colors and fonts in your terminal app's own settings.

Example status-line configuration:

```toml
[ui.status_line]
mode = "command"
command = "~/.config/vtcode/statusline.sh"
refresh_interval_ms = 1000
command_timeout_ms = 200
```

See [status-line.md](./status-line.md) for the full status-line payload and examples.

## Line Break Options

VT Code supports several multiline input paths:

### Quick escape

- Type `\` followed by `Enter` to insert a newline without submitting.
- This works across VT Code terminal sessions without terminal-specific setup.

### Option+Enter on macOS

- `Option+Enter` is the default multiline fallback on macOS terminals.
- In Terminal.app, enable `Use Option as Meta Key` in Settings -> Profiles -> Keyboard.
- In iTerm2 or the VS Code terminal, set the left/right Option key to `Esc+` if you rely on Option-based shortcuts.

### Shift+Enter

- Native terminals: `Ghostty`, `Kitty`, `WezTerm`, `iTerm2`, and `Warp` already handle multiline input without VT Code editing terminal config.
- Guided setup terminals: run `/terminal-setup` in `VS Code`, `Alacritty`, or `Zed` if you want VT Code's terminal-specific setup flow.
- Manual terminals: `Terminal.app`, `xterm`, and unknown terminals require terminal-specific keybinding changes outside VT Code.

### Core input shortcuts

- `Enter` queues the current draft.
- `Ctrl+Enter` runs the draft immediately, or steers the active turn.
- `Ctrl+J` inserts a literal line feed.
- `Esc` cancels the current input or closes an active modal.

### Vim mode

- Set `ui.vim_mode = true` in `vtcode.toml` to enable Vim-style prompt editing by default.
- Use `/vim`, `/vim on`, or `/vim off` to change Vim mode for the current session only.
- VT Code currently supports a focused subset with `INSERT` and `NORMAL` modes only.
- VT Code-specific controls such as `Enter`, `Tab`, `Ctrl+Enter`, `/`, `@`, and `!` still keep their existing behavior.

## Notification Setup

VT Code has two separate notification paths: terminal-native alerts and lifecycle hooks.

### Terminal-native alerts

- VT Code can emit terminal bell and terminal-notification escape sequences when supported.
- Configure VT Code-side notification behavior in `vtcode.toml`:

```toml
[security]
hitl_notification_bell = true

[ui.notifications]
enabled = true
completion_failure = true
completion_success = false
```

- Some terminals surface these alerts directly:
    - `Ghostty` and `Kitty` support native alert flows well.
    - `iTerm2` can show Notification Center alerts after enabling the relevant profile settings.
    - Other terminals may only expose bell-based notifications.

### Lifecycle hook notifications

If your terminal does not surface alerts the way you want, use lifecycle hooks to run your own notification command.

```toml
[hooks.lifecycle]
task_completion = [
  { hooks = [ { command = "osascript -e 'display notification \"VT Code task completed\" with title \"VT Code\"'", timeout_seconds = 5 } ] }
]
```

See [lifecycle-hooks.md](./lifecycle-hooks.md) for event payloads, blocking semantics, and more examples.

VT Code also supports `hooks.lifecycle.notification` for notifications that survive runtime gating. Matchers are evaluated against `permission_prompt` and `idle_prompt`.

## Handling Large Inputs

Large pasted inputs are harder to manage than file-based workflows. Prefer referencing files or piping data into VT Code.

### File-based workflows

- Use `@path/to/file` inside interactive mode to attach files from the workspace.
- Quote paths with spaces, for example `@"docs/design notes.md"`.

### Piped input

```bash
cat large_file.txt | vtcode ask "Analyze this content"
```

### Output limits

VT Code can compact, truncate, or spool large tool output depending on your config:

```toml
[ui]
tool_output_mode = "compact"
tool_output_max_lines = 50
tool_output_spool_bytes = 200000
```

## Troubleshooting

### Multiline input

- If `Shift+Enter` does nothing, check whether your current terminal is one of VT Code's guided setup terminals.
- If you are on macOS, try `Option+Enter` before changing terminal bindings.
- If your terminal is not covered by `/terminal-setup`, configure the binding in the terminal app itself.

### Notifications

- Confirm your terminal has OS notification permissions where applicable.
- Test bell-based alerts with `printf '\\a'`.
- Validate hook commands separately before relying on them in `hooks.lifecycle`.

### Large input handling

- Prefer `@file` references over pasting long transcripts into the terminal.
- In the VS Code terminal, long pastes are more likely to be truncated than file-based input.

## Performance Tips

1. Use file references instead of pasting long documents into the terminal.
2. Keep `tool_output_mode` compact if you work with verbose builds or test output.
3. Configure notifications with hooks when your terminal does not surface native alerts well.
4. Use `/config` and `[ui.status_line]` to tune the interactive surface instead of terminal theme hacks.
