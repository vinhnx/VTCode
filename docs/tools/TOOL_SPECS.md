# VT Code Tool Specifications

This document describes the public tool surface exposed to VT Code models after
the Codex-style tool migration.

## Public Profiles

| Profile | Tools | Use |
|---|---|---|
| Default | `exec_command`, `write_stdin`, `apply_patch` | Normal repository work: shell inspection, validation, interactive sessions, and patch edits. |
| Advanced VT Code | Default tools plus `code_search` | Bounded workspace search for definitions, syntactic usages, literal text, and matching paths. |

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

## Advanced Code Search

`code_search` is available only when the advanced VT Code profile is enabled.
It accepts exactly five inputs:

- `query` is required and searched literally. A wholly lower-case query is
  case-insensitive; a query containing an upper-case character is
  case-sensitive.
- `path` optionally limits the search to one workspace file or directory.
- `file_types` optionally limits results by language name or common extension.
- `result_types` optionally selects `definition`, `usage`, `text`, or `path`.
- `max_results` optionally sets the returned limit from 1 to 100. It defaults
  to 20.

Definitions are recognised declarations with an exact matching name. Usages
are exact syntactic identifiers outside recognised declaration names. They are
not resolved references, so an unrelated identifier with the same spelling may
appear. Text results cover comments, strings, prose, configuration, and other
unclassified content. Path results match existing filenames or paths.

Omit `result_types` to search all four categories. Results are ordered by
category, then source location. `truncated: true` means the bounded search may
have more candidates; it does not report an exact repository-wide total.
Narrow `path`, `file_types`, or `result_types` in another call.

Example:

```json
{"query":"ToolProfile","path":"vtcode-core","file_types":["rust"],"result_types":["definition","usage"],"max_results":20}
```

Use `exec_command` or the specialised ast-grep skill for arbitrary structural
patterns.

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
| `unified_search` search | `code_search` in the advanced profile, using its five query-led inputs |
| `unified_search` web, skills, errors, discovery | Separate tools only where those affordances are retained. |

Short replacements:

```json
{"cmd":"rg -n \"struct ToolRegistration\" vtcode-core"}
```

```json
{"query":"ToolRegistration","path":"vtcode-core/src/tools","file_types":["rust"],"result_types":["definition"]}
```

## Related Docs

- [Tool Registry Guide](../guides/tool_registry.md)
- [Execution Policy](../development/EXECUTION_POLICY.md)
- [Grep Tool Guide](../development/grep-tool-guide.md)
- [AI Tool Surface Migration](../development/ai-tool-surface-migration.md)
