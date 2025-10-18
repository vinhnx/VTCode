# Configuring the inline status line

The inline UI can display a single-line status bar beneath the prompt. By default
VT Code shows the current git branch (with an asterisk when the working tree is
dirty) on the left and the active model with its reasoning effort on the right.
The `[ui.status_line]` table in `vtcode.toml` lets you override this behaviour or
turn the status line off entirely.

## Available modes

The `mode` key accepts three values:

- `auto` (default) keeps the built-in git and model summary.
- `command` runs a user-provided command and renders the first line of stdout.
- `hidden` disables the status line.

When `mode = "command"` the command must be provided via the optional
`command` key. The runtime executes it with `sh -c`, piping a JSON payload to
stdin and rendering the first line from stdout if it is not empty.

```toml
[ui.status_line]
mode = "command"
command = "~/.config/vtcode/statusline.sh"
refresh_interval_ms = 1000
command_timeout_ms = 200
```

- `refresh_interval_ms` throttles how often the command runs. A value of `0`
  refreshes on every render tick.
- `command_timeout_ms` aborts the command if it takes too long. The default is
  200 ms.
- If `command` is omitted or resolves to an empty string, VT Code falls back to
  `auto` mode.

## Command payload structure

The JSON payload written to stdin contains the following fields:

| Field | Description |
| --- | --- |
| `hook_event_name` | Always set to `"Status"`; VT Code reserves this value for status line updates. |
| `cwd` | Absolute path to the active workspace. |
| `workspace.current_dir` | Same as `cwd`. |
| `workspace.project_dir` | Same as `cwd`. |
| `model.id` | Raw model identifier from configuration. |
| `model.display_name` | Human-readable name when recognised. |
| `runtime.reasoning_effort` | Current reasoning effort level. |
| `git.branch` | Current branch name when inside a git repository. |
| `git.dirty` | Boolean indicating whether uncommitted changes exist. |
| `version` | VT Code package version. |

Scripts can parse this payload with any JSON-capable tool. Only the first line of
stdout is rendered, so keep the output concise and apply colour escape sequences
if desired.

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

Mark the script as executable and point `command` to its path. VT Code caches the
last successful output and reuses it until the command is refreshed.
