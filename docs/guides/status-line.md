# Configuring the inline status line

The inline UI can display a single-line status bar beneath the prompt. By default VT Code shows the current git branch
(with an asterisk when the working tree is dirty) on the left and the active model with its reasoning effort on the
right, with the current clock time (24-hour `HH:MM:SS`) rightmost. The `[ui.status_line]` table in `vtcode.toml` lets
you override this behaviour or turn the status line off entirely.

If you want VT Code to scaffold the command setup for you, run `/statusline` in an interactive session. VT Code asks
whether to persist the script in the user config layer (`~/.config/vtcode/statusline.sh`) or the current workspace
(`.vtcode/statusline.sh`), then lets you configure mode, command, script scaffolding, and timing directly in the inline
modal.

## Available modes

The `mode` key accepts three values:

- `auto` (default) keeps the built-in git and model summary, with the clock time appended on the right.
- `command` runs a user-provided command and renders the first line of stdout.
- `hidden` disables the status line.

When `mode = "command"` the command must be provided via the optional `command` key. The runtime executes it with
`sh -c`, piping a JSON payload to stdin and rendering the first line from stdout if it is not empty.
Commands may ignore the JSON input; a closed stdin pipe does not discard successful output. Nonzero exit statuses,
timeouts, and other I/O failures remain errors.

During response progress, the transcript owns the loading row and the footer keeps this configured content:
automatic context, your command's output, or no configured text in hidden mode. Live configuration changes still
apply, including switching modes, changing the command, and hiding the clock. Mode and configured right-side content
keep their slots across loading phases. Narrow layouts retain mode first, then right-side content, context, and optional
hints; each region is truncated separately. A bounded loading fallback appears after context only when space remains.
Background work leaves the bottom line while anything is busy (loading row, foreground command, or in-flight turn):
the live count rides the transcript loading row (`· N bg`, static, no shimmer) and a running foreground command
appends `· Ctrl+B background` there (static, rebound-aware) so the one-click entry point stays visible inline.
A short header badge (`• N bg` live, `✓ N done` retained) stays always visible. The `Running N background tasks...`
copy and drawer hint
return to the bottom line only once everything is idle, so the composer line never blinks as turn phases, the progress
fallback budget, or the foreground-command counter toggle underneath. There is no bottom-line exception for the
foreground command: the `Ctrl+B background` hint never appears after the branch status while busy. Background a
running command with `Ctrl+B`, open the drawer with `Alt+S`, press empty-Enter for `/jobs`, or run `/subprocesses`;
the header suggestions line keeps showing `Ctrl+B background` while a foreground command runs.

```toml
[ui.status_line]
mode = "command"
command = "~/.config/vtcode/statusline.sh"
refresh_interval_ms = 1000
command_timeout_ms = 200
```

- `refresh_interval_ms` throttles how often the command runs. A value of `0` refreshes on every render tick.
- `command_timeout_ms` aborts the command if it takes too long. The default is 200 ms.
- If `command` is omitted or resolves to an empty string, VT Code falls back to `auto` mode.
- `show_clock` (default `true`) controls whether the current time (`HH:MM:SS`, 24-hour clock) is appended to the right
  edge of the status line in `auto` mode. Set it to `false` to hide the clock.

## Command payload structure

The JSON payload written to stdin contains the following fields:

| Field                      | Description                                                                    |
| -------------------------- | ------------------------------------------------------------------------------ |
| `hook_event_name`          | Always set to `"Status"`; VT Code reserves this value for status line updates. |
| `cwd`                      | Absolute path to the active workspace.                                         |
| `workspace.current_dir`    | Same as `cwd`.                                                                 |
| `workspace.project_dir`    | Same as `cwd`.                                                                 |
| `model.id`                 | Raw model identifier from configuration.                                       |
| `model.display_name`       | Human-readable name when recognised.                                           |
| `runtime.reasoning_effort` | Current reasoning effort level.                                                |
| `git.branch`               | Current branch name when inside a git repository.                              |
| `git.dirty`                | Boolean indicating whether uncommitted changes exist.                          |
| `version`                  | VT Code package version.                                                       |

Scripts can parse this payload with any JSON-capable tool. Only the first line of stdout is rendered, so keep the output
concise and apply colour escape sequences if desired.

## Example script

```bash
#!/bin/bash
input=$(cat)
branch=$(echo "$input" | jq -r '.git.branch // ""')
model=$(echo "$input" | jq -r '.model.display_name // .model.id // ""')
reasoning=$(echo "$input" | jq -r '.runtime.reasoning_effort // ""')
status="$model"
if [ -n "$reasoning" ]; then
  status="$status ($reasoning)"
fi
if [ -n "$branch" ]; then
  echo "$branch | $status"
else
  echo "$status"
fi
```

Mark the script as executable and point `command` to its path. VT Code caches the last successful output and reuses it
until the command is refreshed.
