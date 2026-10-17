# AI Tool Surface Migration

VT Code now exposes a small Codex-style default tool surface to models: `exec_command`, `write_stdin`, and
`apply_patch`.

## What Changed

The legacy external schemas `unified_exec`, `unified_file`, and `unified_search` have been removed from the model-facing
surface. No `unified_*` schema or alias remains available after migration. The advanced profile adds the query-led
`code_search` tool.

## Replacement Map

| Removed legacy schema                           | Use now                                                                                                                                              |
| ----------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- |
| `unified_exec`                                  | `exec_command` to start commands, `write_stdin` for live sessions.                                                                                   |
| `unified_file` patch or edit                    | `apply_patch`.                                                                                                                                       |
| `unified_file` read or write                    | Shell commands through `exec_command.cmd` by default. Separately named non-default file tools may be added later only with a concrete justification. |
| `unified_search` text search                    | `rg` or `grep` through `exec_command.cmd`.                                                                                                           |
| `unified_search` search                         | `code_search` in the advanced VT Code profile.                                                                                                       |
| `unified_search` web, skills, errors, discovery | Separate tools only where retained.                                                                                                                  |

## Short Examples

Text search:

```json
{"cmd":"rg -n \"ToolProfile\" vtcode-core","workdir":"/repo"}
```

Interactive continuation:

```json
{"session_id":"7","chars":"\u0003"}
```

Pipe commands now receive EOF on stdin by default. Set `"stdin": true` on `exec_command` when later input is
required; PTY commands keep interactive input. Existing callers that write to a pipe must opt in at launch.
Use `write_stdin` with `action: "inspect"`, `"terminate"`, or `"close"` to inspect, stop, or release an owned
session without providing `chars`. Termination kills the process group and captures its final output; close
cancels if necessary and removes the record. Missing handles return a typed error rather than rerunning commands.

Patch edit (call `apply_patch` directly; it is never a shell executable):

```json
{"input":"*** Begin Patch\n*** Update File: docs/example.md\n@@\n-old\n+new\n*** End Patch\n"}
```

`input` is canonical; the `patch` alias and supported raw/native payloads remain compatible. JSON-only models receive
the patch function eagerly; native models retain their native representation. Planning and permission restrictions still
block unauthorized editing.

Copy complete current context and deletion lines, preserving internal whitespace. The matcher supports boundary
whitespace and Unicode punctuation normalization; partial lines and different internal spacing are rejected.
Recover from the reported path rather than testing matching behavior through scratch edits or implementation searches.
Missing or invalid arguments return a concrete JSON example and one
correction retry. A typed context mismatch reports the affected path and bounded evidence, and permits one fresh
targeted file read with a limit of 1–200 lines per affected path per turn. Use an available file read tool or a single
`sed -n` range through `exec_command`, for example `{"cmd":"sed -n '1,100p' docs/example.md"}`. It can pass the per-path
cap and bypasses stale cache reuse. Repeated failures cannot replenish it; identical-read loop guards, permissions,
budgets, and other safeguards still apply. Malformed patches, denied permissions, and path violations grant no
allowance. Quoted file content does not trigger configuration repair advice; genuine configuration errors retain that
advice.

The equivalent native patch payload is:

```diff
*** Begin Patch
*** Update File: docs/example.md
@@
-old
+new
*** End Patch
```

Code search with the advanced profile:

```json
{"query":"ToolRegistration","path":"crates/codegen/vtcode-core/src/tools","file_types":["rust"],"result_types":["definition","usage"]}
```

## Advanced Profile

Enable the advanced VT Code profile when a task needs bounded code search. The advanced profile keeps the default tools
and adds `code_search`, with required `query` and optional `path`, `file_types`, `result_types`, and `max_results`. It
returns recognised definitions, exact syntactic usages, literal text, and matching paths. Usage results are
same-spelling syntax occurrences, not resolved references. Literal smart-case applies. A truncated result is refined by
narrowing filters in another call and never claims an exact repository-wide total. Use `exec_command` or the specialised
ast-grep skill for arbitrary structural patterns.

## File Tool Finding

Codex core does not expose default model-visible `read_file` or `write_file` tools. VT Code follows that finding:
inspect files with shell commands, internal filesystem affordances, or MCP tools when present, and edit files with
`apply_patch`.

## Platform Handling

`agent.shell_prompt_profile` controls command examples:

- `auto`: Linux, macOS, and WSL use Unix-like examples; native Windows uses PowerShell examples.
- `unix_like`: force Unix-like examples.
- `powershell`: force PowerShell examples.

VT Code does not rewrite GNU flags for macOS BSD tools. Use WSL when Windows workflows need Unix-like command syntax.
