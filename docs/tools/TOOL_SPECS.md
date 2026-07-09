# VT Code Tool Specifications

This document describes the public tool surface exposed to VT Code models after
the Codex-style tool migration.

## Public Profiles

| Profile | Tools | Use |
|---|---|---|
| Default | `exec_command`, `write_stdin`, `apply_patch` | Normal repository work: shell inspection, validation, interactive sessions, and patch edits. |
| Advanced VT Code | Default tools plus `code_search` | Syntax-aware search through ast-grep structural queries and Tree-sitter outlines. |

No `unified_*` schema, alias, hidden public tool, or compatibility profile is
available after this migration. Those names are legacy external schema names
only and must not be used in new prompts, config examples, evals, or tool calls.

## Default Tools

### `exec_command`

Runs a shell command through the active shell profile and the usual command
policy, sandbox, approval, and output-limit checks.

Required:

- `cmd`: command text.

Common optional fields:

- `workdir`: working directory.
- `tty`: allocate a PTY when the command needs an interactive terminal.
- `yield_time_ms`: how long to wait before returning output.
- `max_output_tokens`: output budget for the response.

Unix-like example:

```json
{"cmd":"rg -n \"ToolProfile\" vtcode-core","workdir":"/repo"}
```

PowerShell example:

```json
{"cmd":"Select-String -Path vtcode-core/**/*.rs -Pattern ToolProfile","workdir":"C:\\repo"}
```

### `write_stdin`

Sends input to a live session created by `exec_command`.

Required:

- `session_id`: identifier returned by a still-running command.
- `chars`: bytes or text to send.

Example:

```json
{"session_id":42,"chars":"q"}
```

### `apply_patch`

Applies a freeform patch through VT Code's workspace-boundary and edit-safety
checks. Use it for file edits, additions, moves, and deletions when the model has
the patch tool.

Example:

```diff
*** Begin Patch
*** Update File: README.md
@@
-old text
+new text
*** End Patch
```

## Advanced Semantic Search

`code_search` is available only when the advanced VT Code profile is enabled.
It preserves VT Code's semantic search features without making text search a
separate default function tool.

Actions:

- `outline`: Tree-sitter symbol maps for a file or path set.
- `structural`: ast-grep pattern search over syntax trees.

Examples:

```json
{"action":"outline","path":"vtcode-core/src/tools/registry","lang":"rust","view":"names"}
```

```json
{"action":"structural","path":"vtcode-core","lang":"rust","pattern":"ToolProfile::$NAME"}
```

Use shell `rg` or `grep` through `exec_command.cmd` for plain text, filenames,
and prose search. Use `code_search` when the query depends on syntax, nesting,
or symbol structure.

## File Inspection

The default profile has no public `read_file` or `write_file` tools. This
matches the Codex core finding: file inspection uses shell commands, internal
filesystem affordances, or MCP tools when present, and edits use `apply_patch`.

Examples:

```json
{"cmd":"sed -n '1,120p' docs/tools/TOOL_SPECS.md"}
```

```json
{"cmd":"rg --files docs | sort"}
```

Separately named non-default file tools may be added later only if a concrete
use case justifies them. They must not reuse a legacy `unified_*` name.

## Platform Profiles

VT Code selects the model-facing shell guidance from `agent.shell_prompt_profile`.

| Platform | Default profile | Guidance |
|---|---|---|
| Linux | `unix_like` | Use Unix-like shell commands in `exec_command.cmd`. |
| macOS | `unix_like` | Use BSD-compatible flags where BSD tools differ. |
| WSL | `unix_like` | Recommended route for Unix-like workflows on Windows. |
| Native Windows | `powershell` | Use native PowerShell syntax. |

The setting accepts `auto`, `unix_like`, or `powershell`. It controls prompt
examples and expected command syntax only. VT Code does not translate GNU flags
for macOS BSD tools, and it does not translate Unix commands to PowerShell.

## Migration From Removed Schemas

External users of the removed legacy schemas must update their calls directly:

| Removed legacy schema | Replacement |
|---|---|
| `unified_exec` run | `exec_command` |
| `unified_exec` session input | `write_stdin` |
| `unified_file` patch or edit | `apply_patch` |
| `unified_file` read or write | Shell commands through `exec_command.cmd` by default, or separately named non-default tools if added later. |
| `unified_search` text search | `rg` or `grep` through `exec_command.cmd` |
| `unified_search` semantic search | `code_search` in the advanced profile |
| `unified_search` web, skills, errors, discovery | Separate tools only where those affordances are retained. |

Short replacements:

```json
{"cmd":"rg -n \"struct ToolRegistration\" vtcode-core"}
```

```json
{"action":"outline","path":"vtcode-core/src/tools","lang":"rust","view":"names"}
```

## Related Docs

- [Tool Registry Guide](../guides/tool_registry.md)
- [Execution Policy](../development/EXECUTION_POLICY.md)
- [Grep Tool Guide](../development/grep-tool-guide.md)
- [AI Tool Surface Migration](../development/ai-tool-surface-migration.md)
