# Interactive Mode Reference

The VT Code terminal UI includes an interactive mode that combines keyboard-first navigation with quick commands for agent control. This page consolidates the shortcuts, input modes, and background execution behaviors available while you are connected to a session.

## Keyboard Shortcuts

> Keyboard shortcuts may vary slightly by platform and terminal emulator. Press `?` on an empty input line while VT Code is running to open a quick shortcut overlay.

### General Controls

| Shortcut | Description | Context |
| :-- | :-- | :-- |
| `Ctrl+C` | Cancel the current generation or command. Press twice to terminate the session. | Works during prompts, tool execution, and streaming replies. |
| `Ctrl+D` | Exit VT Code interactive mode. | Sends EOF to the shell integration. |
| `Ctrl+L` | Clear the terminal screen while keeping the conversation history. | Useful for refreshing when output is cluttered. |
| `Ctrl+O` | Toggle verbose tool output and diagnostics. | Reveals detailed tool invocation logs. |
| `Ctrl+A` | Move cursor to start of input line. | UNIX/readline-style editing. |
| `Ctrl+E` | Move cursor to end of input line (or open external editor when input is empty). | Uses `tools.editor` config, then `VISUAL`/`EDITOR`. |
| `Ctrl+W` | Delete the previous word. | UNIX/readline-style editing. |
| `Ctrl+U` | Delete from cursor to line start. | UNIX/readline-style editing. |
| `Ctrl+K` | Delete from cursor to line end. | UNIX/readline-style editing. |
| `Alt+Left/Right` | Move cursor by word. | UNIX/readline-style navigation. |
| `Ctrl+R` | Reverse search the command history. | Matches previous prompts and bash commands. |
| `Ctrl+V` (macOS/Linux) or `Alt+V` (Windows) | Paste an image from the clipboard. | Works with image-enabled sessions. |
| `Ctrl+Z` (Unix) | Suspend VT Code to the shell; run `fg` to resume. | Job-control support for terminal workflows. |
| `Up/Down arrows` | Navigate through command history. | Recall previous prompts or commands. |
| `Esc` + `Esc` | Rewind the conversation and code to the latest checkpoint. | Idle context only (while no task/PTY is running). |
| `Enter` | Queue the current input. | Plain input box only. |
| `Tab` | Queue the current input. | Plain input box only; list and slash UIs keep their existing tab behavior. |
| `Ctrl+Enter` | Process now or steer now. | Idle: runs the current draft, or the newest queued message if the draft is empty. Active: steers the current turn with the current draft. |
| `Shift+Tab` or `Alt+M` | Cycle permission modes. | Switches Auto-Accept Mode, Plan Mode, and normal mode. |

### Multiline Input

| Method | Shortcut | Context |
| :-- | :-- | :-- |
| Quick escape | `\` + `Enter` | Works across supported terminals. |
| macOS default | `Option+Enter` | Default multiline binding on macOS terminals. |
| Native or configured | `Shift+Enter` | Works natively in some terminals and is available after `/terminal-setup` in supported terminals. |
| Control sequence | `Ctrl+J` | Inserts a line feed for multiline editing. |
| Paste mode | Paste directly | Ideal for code blocks or long transcripts. |

> Tip: `Shift+Enter` works natively in `Ghostty`, `Kitty`, `WezTerm`, `iTerm2`, and `Warp`. Run `/terminal-setup` in supported terminals such as `VS Code`, `Alacritty`, or `Zed` when you want VT Code's guided setup flow.

### Quick Commands

| Shortcut | Description | Notes |
| :-- | :-- | :-- |
| `#` at start of input | Access custom prompts. | Opens quick picker to select and run custom prompts directly from input bar. |
| `/` at start of input | Issue a slash command. | Run `/help` or `/slash-commands` in a session to list everything available. |
| `!` at start of input | Enter Bash mode. | Runs shell commands directly and streams their output. |
| `@` within input | Open file picker. | Triggers file path autocomplete and picker to quickly reference files in your message. |

## Vim Mode

VT Code supports an optional Vim-style prompt editor.

- Set `ui.vim_mode = true` to enable it by default for new sessions.
- Use `/vim`, `/vim on`, and `/vim off` to change the current session only.
- Supported modes are `INSERT` and `NORMAL`.
- Supported subset includes motions, change/delete/yank operators, `f/F/t/T`, text objects, `p/P`, `J`, and repeat with `.`.
- VT Code does not implement visual mode, macros, or multiple registers; yanks reuse the single session clipboard.
- VT Code-specific prompt controls still win when relevant, including `Enter`, `Tab`, `Ctrl+Enter`, `/`, `@`, and `!`.

## Active Run Steering

When a task is already running, VT Code keeps the active turn alive and lets you steer it:

- `Enter` and `Tab` queue the current input for later processing.
- `Ctrl+Enter` sends the current draft to the active run as steering text.
- `/pause` pauses the active run at the next model/tool/approval boundary.
- `/resume` resumes a paused run while it is active. When idle, `/resume` still opens archived sessions.
- `/stop` still cancels the active run immediately.

## Plan Mode Notes

- Plan Mode is strict read-only (except optional writes under `.vtcode/plans/` for plan artifacts).
- The agent emits planning output in `<proposed_plan>...</proposed_plan>` blocks.
- `task_tracker` works in Plan Mode and mirrors checklist state with plan sidecars; `plan_task_tracker` remains as a compatibility alias.
- After a plan is emitted, VT Code shows an implementation choice: switch to Edit mode and execute, or continue planning.
- If automatic Plan->Edit switching fails, manually switch with `/plan off` or `/mode`, or use `Shift+Tab`/`Alt+M`.

## Command History

VT Code keeps a command history scoped to the working directory. The history resets when you clear it manually or start a new directory session.

* Cleared with the `/clear` command.
* Use the arrow keys to navigate between entries.
* History expansion via `!` is disabled by default to prevent accidental execution.

### Reverse Search with `Ctrl+R`

1. Press `Ctrl+R` to start the reverse history search.
2. Type a query to highlight matching entries.
3. Press `Ctrl+R` again to cycle through older matches.
4. Accept the current match with `Tab`, `Esc`, or `Enter` to execute immediately.
5. Cancel the search with `Ctrl+C` or `Backspace` on an empty query.

## Background Bash Commands

The Bash integration can run long commands asynchronously while you continue working with the agent.

### Running in the Background

* Ask VT Code to run a command in the background, or
* Press `Ctrl+B` while a command runs to move it to the background (press twice if your terminal uses tmux with the same prefix).

Background tasks return immediately with an ID. VT Code keeps streaming updates via the BashOutput tool, and tasks are automatically cleaned up when the session ends.

Common backgrounded commands include:

* Build systems (e.g., webpack, vite, make)
* Package managers (npm, yarn, pnpm)
* Test runners (jest, pytest)
* Development servers and other long-running processes

### Bash Mode with `!`

Prefix input with `!` to run commands directly without agent interpretation:

```bash
! npm test
! git status
! ls -la
```

Bash mode streams the command and its output into the chat, supports backgrounding via `Ctrl+B`, and is ideal for quick shell operations while keeping a shared context with the agent.

## Additional Resources

* [User guide overview](../README.md)
* [Getting started walkthrough](../user-guide/getting-started.md)
* [Advanced features](../ADVANCED_FEATURES_IMPLEMENTATION.md)
