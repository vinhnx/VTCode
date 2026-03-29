# Command Reference

This guide summarizes common actions and how to invoke them with vtcode. The agent exposes a suite of tools to the LLM; you interact with them via chat. When you ask to search, read, or edit files, the agent chooses an appropriate tool.

## grep_file (ripgrep-like)

High-speed code search with glob filters, context lines, and optional literal/regex matching.
VT Code routes searches through the custom `grep_file` tool. It calls the system `rg` binary when available, and falls back to the embedded [perg](https://crates.io/crates/perg)
engine so downstream tools receive the same JSON response format. Prefer `grep_file` instead of invoking shell `rg`/`grep` yourself.

-   Input fields:

    -   `pattern` (string, required): Search pattern. Treated as regex unless `literal=true`.
    -   `path` (string, default: `.`): Base directory to search from.
    -   `case_sensitive` (bool, default: true): Case-sensitive when true.
    -   `literal` (bool, default: false): Treat pattern as literal text when true.
    -   `glob_pattern` (string, optional): Filter files by glob (e.g., `**/*.rs`).
    -   `context_lines` (integer, default: 0): Lines before/after each hit.
    -   `include_hidden` (bool, default: false): Include dotfiles when true.
    -   `max_results` (integer, default: 1000): Cap results to avoid large payloads.

-   Output fields:
    -   `matches[]`: `{ path, line, column, line_text, before[], after[] }`
    -   `total_matches`, `total_files_scanned`, `truncated`

### Examples

-   Find TODO/FIXME with 2 lines of context in Rust files only:

```
Ask: Search for TODO|FIXME across the repo with 2 lines of context in .rs files
(Agent uses grep_file with)
{
  "pattern": "TODO|FIXME",
  "path": ".",
  "case_sensitive": false,
  "glob_pattern": "**/*.rs",
  "context_lines": 2
}
```

-   Literal search for `unsafe {` anywhere (hidden files ignored):

```
{
  "pattern": "unsafe {",
  "literal": true,
  "context_lines": 1
}
```

-   Search JavaScript files for a function name, case-insensitive:

```
{
  "pattern": "doSomethingImportant",
  "case_sensitive": false,
  "glob_pattern": "**/*.js"
}
```

## File operations

-   `list_files(path, max_items?, include_hidden?)`
-   `read_file(path, max_bytes?)`
-   `write_file(path, content, mode?)` — mode: `overwrite`, `append`, or `skip_if_exists`
-   `edit_file(path, old_str, new_str)` — tolerant to whitespace differences and detects rename conflicts

## Session resume and forks

VT Code can reopen archived sessions, continue the latest one, or fork a previous session into a new archive.

### Resume the latest or a specific session

```bash
vtcode --continue
vtcode --resume session-123
vtcode --resume          # interactive picker
```

### Fork from an archived session

```bash
vtcode --fork-session session-123
vtcode --fork-session session-123 --session-id bugfix-branch
vtcode --resume session-123 --session-id bugfix-branch
vtcode --continue --session-id followup-branch
```

Notes:

- `--session-id` turns `--resume ...` or `--continue` into a fork instead of an in-place resume.
- `--resume` with no ID plus `--session-id ...` opens the interactive picker and then forks the selected session.
- `--all` expands the picker/search scope across workspaces for resume and fork flows.

### Start a summarized fork

```bash
vtcode --fork-session session-123 --summarize
vtcode --resume session-123 --session-id handoff --summarize
vtcode --resume --session-id handoff --summarize   # interactive picker, summarized fork
```

Summarized forks do not copy the full transcript. VT Code starts the child session from:

- one structured conversation summary
- retained recent real user messages
- the session memory envelope

Normal forks keep the full archived transcript unchanged.

## Quick Actions in Chat Input

VT Code provides several quick actions directly in the chat input for faster workflow:

-   **File Picker (`@`)** — Type `@` anywhere in your input to open the file picker and select files to reference in your message. This allows you to quickly mention files without typing full paths.
-   **Slash Commands (`/`)** — Type `/` at the start of input to access all available slash commands including `/files`, `/stats`, and many more.

### `/code-ide` (VS Code integration)

Use the `/code-ide` slash command to trigger IDE-specific actions from within a VT Code chat session or the integrated terminal. When the VS Code extension is installed:

-   Run `/code-ide` in the VT Code terminal session to synchronize with the sidebar views and refresh context-aware data.
-   Use the **VT Code: Send /code-ide Slash Command** command palette entry or the Quick Actions panel to dispatch the slash command directly to the active VT Code terminal.
-   The command will emit IDE events back to the extension host, keeping the Agent Loop timeline, status indicators, and MCP configuration summaries in sync.

Configure the behaviour under **Settings › Extensions › VT Code**:

-   `vtcode.terminal.autoRunChat` — Automatically run `vtcode chat` when the managed terminal opens.
-   `vtcode.terminal.allowMultipleInstances` — Opt-in to creating new terminal sessions instead of reusing the shared VT Code terminal.
-   `vtcode.agentTimeline.refreshDebounceMs` — Control how quickly the Agent Loop timeline reacts to incoming terminal output.

### Slash-command notes

- `/resume` opens archived sessions when the current run is idle.
- `/fork` opens the session picker and then lets you choose between a full-copy fork and a summarized fork.
- `/compact` manually compacts the current conversation context. On the local fallback path, VT Code keeps a structured summary plus retained user prompts instead of a mixed recent tail.
- `/loop` schedules a session-scoped prompt that re-enters the chat only when the current turn is idle.
- `/schedule` manages durable scheduled tasks. They are polled while VT Code is open, and the local scheduler daemon keeps them running in the background.

## Scheduled tasks

Use `/loop` inside interactive chat for quick polling, and use `vtcode schedule` when the task should survive restarts.

```bash
vtcode schedule create --prompt "check the deployment" --every 10m
vtcode schedule create --prompt "review the nightly build" --cron "0 9 * * 1-5"
vtcode schedule create --reminder "push the release branch" --at "15:00"
vtcode schedule list
vtcode schedule delete 1a2b3c4d
vtcode schedule serve
```

See [Scheduled Tasks](./scheduled-tasks.md) for session reminders, durable daemon behavior, and service installation details.

## stats (session metrics)

Display current configuration, available tools, and live performance metrics for the running
session. Use `--format` to choose `text`, `json`, or `html` output and `--detailed` to list each
tool.

## schema (runtime tool introspection)

Inspect VT Code's built-in tool schemas at runtime so automation can discover exact tool names and
input parameters without relying on stale docs.

### Usage

```bash
# Full JSON document (default)
vtcode schema tools

# Compact schema descriptions for tighter context windows
vtcode schema tools --mode minimal

# NDJSON output for streaming parsers
vtcode schema tools --format ndjson

# Filter to specific tools
vtcode schema tools --name unified_search --name unified_file
```

### Options

- `--mode` — `minimal`, `progressive` (default), or `full`
- `--format` — `json` (default) or `ndjson`
- `--name` — repeatable exact tool-name filter

## update (binary updates)

Check for and install binary updates of VT Code from GitHub Releases. Updates are downloaded and verified against checksums for security.

### Usage

```bash
# Check for available updates without installing
vtcode update --check

# Check for updates (same as above, default behavior)
vtcode update
```

### Options

- `--check` — Check for updates and display release notes without installing
- `--force` — Force update even if already on the latest version

### How it works

1. The command checks the GitHub API for the latest VT Code release
2. It compares the remote version with your current version
3. If a new version is available, it shows release notes and download information
4. Interactive TUI sessions automatically check for updates on launch (short cached interval)
5. Managed installs (Homebrew/cargo/npm) show package-manager-specific update guidance

### Examples

- Check for updates:
  ```bash
  vtcode update --check
  ```

- Check for updates and show if you're on the latest version:
  ```bash
  vtcode update
  ```

## dependencies

Manage optional VT Code dependencies such as ripgrep and ast-grep.

### Usage

```bash
# Install both optional search tools in one step
vtcode dependencies install search-tools

# Check whether VT Code can resolve the optional search tools
vtcode dependencies status search-tools

# Install ripgrep using a supported system installer
vtcode dependencies install ripgrep

# Install the managed ast-grep binary into ~/.vtcode/bin
vtcode dependencies install ast-grep

# Check whether VT Code can resolve ast-grep
vtcode dependencies status ast-grep
```

### Notes

- `vtcode deps ...` is a short alias for `vtcode dependencies ...`
- `vtcode dependencies install search-tools` bundles the recommended `ripgrep` + `ast-grep` setup after any install method
- `vtcode dependencies install ripgrep` installs `rg` through a supported system installer and keeps startup non-blocking when you skip it
- VT Code does not auto-edit your shell profile; add `export PATH="$HOME/.vtcode/bin:$PATH"` yourself if you want the managed binary outside VT Code
- On Linux, prefer `ast-grep` over `sg`
- The curl installer includes the search-tools bundle by default; use `--without-search-tools` to skip it

## pods

Manage remote GPU-backed model pods over SSH. See the full feature guide in
[GPU Pod Manager](../features/GPU_POD_MANAGER.md).

### Usage

```bash
# Start a pod-backed model
vtcode pods start --name llama \
  --model meta-llama/Llama-3.1-8B-Instruct \
  --ssh "ssh root@gpu.example.com" \
  --gpu 0:A100 --gpu 1:A100 \
  --gpus 2

# Inspect tracked pods
vtcode pods list

# Stream logs for one model
vtcode pods logs --name llama
```

### Commands

- `vtcode pods start` - Launch a model on the active pod
- `vtcode pods stop` - Stop one tracked model
- `vtcode pods stop-all` - Stop every tracked model on the active pod
- `vtcode pods list` - Show tracked model status
- `vtcode pods logs` - Stream the remote log for a model
- `vtcode pods known-models` - Show compatible and incompatible profiles

## Tips

-   The agent respects `.vtcodegitignore` to exclude files from search and I/O.
-   Prefer `grep_file` for fast, focused searches with glob filters and context.
-   Ask for “N lines of context” when searching to understand usage in-place.
-   Shell commands are filtered by allow/deny lists and can be extended via `VTCODE_<AGENT>_COMMANDS_*` environment variables.
-   Use `vtcode update --check` regularly to stay informed about new features and security updates.
