# VT Code Tool Specifications

This document describes the public tool surface exposed to VT Code models after the Codex-style tool migration.

## Public Profiles

| Profile          | Tools                                                        | Use                                                                                           |
| ---------------- | ------------------------------------------------------------ | --------------------------------------------------------------------------------------------- |
| Default          | `exec_command`, `write_stdin`, `apply_patch`, `search_tools` | Normal repository work plus deferred capability discovery.                                    |
| Advanced VT Code | Default tools plus `code_search`                             | Bounded workspace search for definitions, syntactic usages, literal text, and matching paths. |

No `unified_*` schema, alias, hidden public tool, or compatibility profile is available after this migration. Those
names are legacy external schema names only and must not be used in new prompts, config examples, evals, or tool calls.

## Default Tools

### `exec_command`

Runs a shell command through the active shell profile. Permission-related fields express stable request intent; the
execution gateway resolves runtime enforcement without changing the model-visible schema.

File edits belong in `apply_patch` rather than shell redirection or in-place editors such as `sed -i`. Expanded
`sandbox_permissions` modes trigger an approval check before the command runs; `require_escalated` and `bypass_sandbox`
also need a non-empty `justification`.

Required:

- `cmd`: command text.

Common optional fields:

- `workdir`: working directory.
- `tty`: allocate a PTY when the command needs an interactive terminal.
- `yield_time_ms`: how long to wait before returning output.
- `max_output_tokens`: output budget for the response.

Every function tool accepts `max_output_tokens`. It defaults to 10,000 and must be between 1 and 50,000. VT Code spools
the complete output and limits only the model-visible preview. Completed output spools include `spool_line_count` so
targeted shell reads can stay within the available lines without probing beyond EOF. The count includes an unterminated
final line and is omitted while a live spool is pending.

The limit is validated during preflight before dispatch. Omit it to use the 10,000-token default or provide an integer
override in that range; strings, fractions, zero, and values above 50,000 are rejected. Execution-only approval inputs
such as `sandbox_permissions` and `justification` remain explicit `exec_command` arguments and are not injected into
tool descriptions or hidden approval metadata.

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

Applies a freeform patch through VT Code's workspace-boundary and edit-safety checks. Use it for file edits, additions,
moves, and deletions when the model has the patch tool. Successful responses include a bounded `diff` array with one
entry per planned file (`path`, `operation`, bounded unified `content`, and available `additions`/`deletions`); no-op
entries are marked `is_empty`. The legacy single-file `diff_preview` response is normalized to this same shape by the
terminal and session renderers.

Every `apply_patch` path, including move sources and destinations, must be workspace-relative. Absolute paths, `..`, and
traversal-like path forms are rejected; resolution also rejects an in-workspace symlink that leads outside the workspace
before any patch mutation begins.

After several effective mutations, anti-blind-editing temporarily allows reads, inspection, task tracking, and
verification while blocking further workspace mutations until a verification command completes.

The checkpoint is cleared only by a successful verification command. Inspection, link checks, and `git diff --check` are
useful diagnostics but do not prove that the requested build or tests pass. Failed mutations do not count as effective
mutations, and a blocked turn remains blocked even when its recovery response is published successfully. The recovery
response identifies the reason: pending verification includes `cargo check --locked` and the relevant
`cargo nextest run` guidance; context-capacity recovery says that completed outputs were retained and suggests resuming
after reducing context or switching models; other blocked turns receive a generic retry handoff.

Example:

```diff
*** Begin Patch
*** Update File: README.md
@@
-old text
+new text
*** End Patch
```

### `search_tools`

Searches the session tool catalog by name and description, so the model can find tools whose definitions are deferred
(for example `code_search`, `web_fetch`, `web_search`, `cron`, and MCP server tools). `query` is required; `limit` is
optional (1–25, default 5), and `detail_level` accepts `name`, `name_description` (default), or `full`. Deferred matches
are listed in `expanded_for_next_segment` and become callable on the next request segment. The `mcp` tool's
`action=search_tools` is narrower: it searches only tools exposed by configured MCP servers.

```json
{"query":"GitHub pull request review","limit":5,"detail_level":"name_description"}
```

## Advanced Code Search

`code_search` is available only when the advanced VT Code profile is enabled. It accepts exactly five inputs:

- `query` is required and searched literally. A wholly lower-case query is case-insensitive; a query containing an
  upper-case character is case-sensitive.
- `path` optionally limits the search to one workspace file or directory.
- `file_types` optionally limits results by language name or common extension.
- `result_types` optionally selects `definition`, `usage`, `text`, or `path`.
- `max_results` optionally sets the returned limit from 1 to 100. It defaults to 20.

Definitions are recognised declarations with an exact matching name. Usages are exact syntactic identifiers outside
recognised declaration names. They are not resolved references, so an unrelated identifier with the same spelling may
appear. Text results cover comments, strings, prose, configuration, and other unclassified content. Path results match
existing filenames or paths.

Omit `result_types` to search all four categories. Results are ordered by category, then source location.
`truncated: true` means the bounded search may have more candidates; it does not report an exact repository-wide total.
Narrow `path`, `file_types`, or `result_types` in another call.

Example:

```json
{"query":"ToolProfile","path":"vtcode-core","file_types":["rust"],"result_types":["definition","usage"],"max_results":20}
```

Use `exec_command` or the specialised ast-grep skill for arbitrary structural patterns.

## File Inspection

The default profile has no public `read_file` or `write_file` tools. This matches the Codex core finding: file
inspection uses shell commands, internal filesystem affordances, or MCP tools when present, and edits use `apply_patch`.

Examples:

```json
{"cmd":"sed -n '1,120p' docs/tools/TOOL_SPECS.md"}
```

```json
{"cmd":"rg --files docs | sort"}
```

Separately named non-default file tools may be added later only if a concrete use case justifies them. They must not
reuse a legacy `unified_*` name.

## Platform Profiles

VT Code selects the model-facing shell guidance from `agent.shell_prompt_profile`.

| Platform       | Default profile | Guidance                                              |
| -------------- | --------------- | ----------------------------------------------------- |
| Linux          | `unix_like`     | Use Unix-like shell commands in `exec_command.cmd`.   |
| macOS          | `unix_like`     | Use BSD-compatible flags where BSD tools differ.      |
| WSL            | `unix_like`     | Recommended route for Unix-like workflows on Windows. |
| Native Windows | `powershell`    | Use native PowerShell syntax.                         |

The setting accepts `auto`, `unix_like`, or `powershell`. It controls prompt examples and expected command syntax only.
VT Code does not translate GNU flags for macOS BSD tools, and it does not translate Unix commands to PowerShell.

## Migration From Removed Schemas

External users of the removed legacy schemas must update their calls directly:

| Removed legacy schema                           | Replacement                                                                                                 |
| ----------------------------------------------- | ----------------------------------------------------------------------------------------------------------- |
| `unified_exec` run                              | `exec_command`                                                                                              |
| `unified_exec` session input                    | `write_stdin`                                                                                               |
| `unified_file` patch or edit                    | `apply_patch`                                                                                               |
| `unified_file` read or write                    | Shell commands through `exec_command.cmd` by default, or separately named non-default tools if added later. |
| `unified_search` text search                    | `rg` or `grep` through `exec_command.cmd`                                                                   |
| `unified_search` search                         | `code_search` in the advanced profile, using its five query-led inputs                                      |
| `unified_search` web, skills, errors, discovery | Separate tools only where those affordances are retained.                                                   |

Short replacements:

```json
{"cmd":"rg -n \"struct ToolRegistration\" vtcode-core"}
```

```json
{"query":"ToolRegistration","path":"crates/codegen/vtcode-core/src/tools","file_types":["rust"],"result_types":["definition"]}
```

## Related Docs

- [Tool Registry Guide](../guides/tool_registry.md)
- [Execution Policy](../development/EXECUTION_POLICY.md)
- [Grep Tool Guide](../development/grep-tool-guide.md)
- [AI Tool Surface Migration](../development/ai-tool-surface-migration.md)
