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

## Agent teams

Agent teams are an experimental feature for coordinating multiple subagents in a single session.

- Enable `[agent_teams] enabled = true` and `[subagents] enabled = true` in `vtcode.toml`.
- Use `/team start` to create a team, then `/team task add` and `/team assign` to delegate tasks.
- Use `/team model` to set a default model for team subagents.
- Use `/subagent model` to set a default model for all subagents.
- See `docs/agent-teams.md` for full usage and limitations.

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

## stats (session metrics)

Display current configuration, available tools, and live performance metrics for the running
session. Use `--format` to choose `text`, `json`, or `html` output and `--detailed` to list each
tool.

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

## Tips

-   The agent respects `.vtcodegitignore` to exclude files from search and I/O.
-   Prefer `grep_file` for fast, focused searches with glob filters and context.
-   Ask for “N lines of context” when searching to understand usage in-place.
-   Shell commands are filtered by allow/deny lists and can be extended via `VTCODE_<AGENT>_COMMANDS_*` environment variables.
-   Use `vtcode update --check` regularly to stay informed about new features and security updates.
