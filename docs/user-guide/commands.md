# Command Reference

This guide summarizes common actions and how to invoke them with vtcode. The agent exposes a suite of tools to the LLM; you interact with them via chat. When you ask to search, read, or edit files, the agent chooses an appropriate tool.

## rp_search (ripgrep-like)

High-speed code search with glob filters, context lines, and optional literal/regex matching.
VTCode routes searches through the custom `rp_search` tool. It calls the system `rg` binary when available, and falls back to the embedded [perg](https://crates.io/crates/perg)
engine so downstream tools receive the same JSON response format. Prefer `rp_search` instead of invoking shell `rg`/`grep` yourself.

- Input fields:
  - `pattern` (string, required): Search pattern. Treated as regex unless `literal=true`.
  - `path` (string, default: `.`): Base directory to search from.
  - `case_sensitive` (bool, default: true): Case-sensitive when true.
  - `literal` (bool, default: false): Treat pattern as literal text when true.
  - `glob_pattern` (string, optional): Filter files by glob (e.g., `**/*.rs`).
  - `context_lines` (integer, default: 0): Lines before/after each hit.
  - `include_hidden` (bool, default: false): Include dotfiles when true.
  - `max_results` (integer, default: 1000): Cap results to avoid large payloads.

- Output fields:
  - `matches[]`: `{ path, line, column, line_text, before[], after[] }`
  - `total_matches`, `total_files_scanned`, `truncated`

### Examples

- Find TODO/FIXME with 2 lines of context in Rust files only:

```
Ask: Search for TODO|FIXME across the repo with 2 lines of context in .rs files
(Agent uses rp_search with)
{
  "pattern": "TODO|FIXME",
  "path": ".",
  "case_sensitive": false,
  "glob_pattern": "**/*.rs",
  "context_lines": 2
}
```

- Literal search for `unsafe {` anywhere (hidden files ignored):
```
{
  "pattern": "unsafe {",
  "literal": true,
  "context_lines": 1
}
```

- Search JavaScript files for a function name, case-insensitive:
```
{
  "pattern": "doSomethingImportant",
  "case_sensitive": false,
  "glob_pattern": "**/*.js"
}
```

## File operations

- `list_files(path, max_items?, include_hidden?)`
- `read_file(path, max_bytes?)`
- `write_file(path, content, mode?)` — mode: `overwrite`, `append`, or `skip_if_exists`
- `edit_file(path, old_str, new_str)` — tolerant to whitespace differences and detects rename conflicts

## stats (session metrics)

Display current configuration, available tools, and live performance metrics for the running
session. Use `--format` to choose `text`, `json`, or `html` output and `--detailed` to list each
tool.

## Tips

- The agent respects `.vtcodegitignore` to exclude files from search and I/O.
- Prefer `rp_search` for fast, focused searches with glob filters and context.
- Ask for “N lines of context” when searching to understand usage in-place.
- Shell commands are filtered by allow/deny lists and can be extended via `VTCODE_<AGENT>_COMMANDS_*` environment variables.
