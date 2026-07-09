# AI Tool Surface Migration

VT Code now exposes a small Codex-style default tool surface to models:
`exec_command`, `write_stdin`, and `apply_patch`.

## What Changed

The legacy external schemas `unified_exec`, `unified_file`, and
`unified_search` have been removed from the model-facing surface. No
`unified_*` schema, alias, compatibility profile, hidden public tool, or
advanced profile remains available after migration.

## Replacement Map

| Removed legacy schema | Use now |
|---|---|
| `unified_exec` | `exec_command` to start commands, `write_stdin` for live sessions. |
| `unified_file` patch or edit | `apply_patch`. |
| `unified_file` read or write | Shell commands through `exec_command.cmd` by default. Separately named non-default file tools may be added later only with a concrete justification. |
| `unified_search` text search | `rg` or `grep` through `exec_command.cmd`. |
| `unified_search` semantic search | `code_search` in the advanced VT Code profile. |
| `unified_search` web, skills, errors, discovery | Separate tools only where retained. |

## Short Examples

Text search:

```json
{"cmd":"rg -n \"ToolProfile\" vtcode-core","workdir":"/repo"}
```

Interactive continuation:

```json
{"session_id":7,"chars":"\u0003"}
```

Patch edit:

```diff
*** Begin Patch
*** Update File: docs/example.md
@@
-old
+new
*** End Patch
```

Semantic search with the advanced profile:

```json
{"action":"outline","path":"vtcode-core/src/tools","lang":"rust","view":"names"}
```

## Advanced Profile

Enable the advanced VT Code profile when a task needs syntax-aware search.
The advanced profile keeps the default tools and adds `code_search` for
Tree-sitter outlines and ast-grep structural queries.

Use shell `rg` first for text, filenames, and prose. Use `code_search` when the
shape of the code matters.

## File Tool Finding

Codex core does not expose default model-visible `read_file` or `write_file`
tools. VT Code follows that finding: inspect files with shell commands, internal
filesystem affordances, or MCP tools when present, and edit files with
`apply_patch`.

## Platform Handling

`agent.shell_prompt_profile` controls command examples:

- `auto`: Linux, macOS, and WSL use Unix-like examples; native Windows uses
  PowerShell examples.
- `unix_like`: force Unix-like examples.
- `powershell`: force PowerShell examples.

VT Code does not rewrite GNU flags for macOS BSD tools. Use WSL when Windows
workflows need Unix-like command syntax.
