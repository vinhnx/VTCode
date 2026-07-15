# Per-Call Output Limits

## Overview

Public command tools can cap large responses per call. Prefer
`max_output_tokens` in model-facing calls; internal compatibility paths may still
normalise older output-limit field names.

Output caps reduce prompt pressure while preserving command status, exit codes,
and spool metadata when the full output is too large for the response.

## Supported Public Tools

- `exec_command`: caps stdout and stderr returned from a command.
- `write_stdin`: caps output returned after sending input to a live session.
- `code_search`: returns bounded code-search results when available in the
  advanced profile.

File inspection in the default profile uses shell commands through
`exec_command.cmd`. Use commands such as `sed`, `head`, `tail`, `rg`, and
`find` to request the slice of data you need.

## Examples

Limit command output:

```json
{
  "cmd": "rg -n \"ToolProfile\" vtcode-core",
  "workdir": "/repo",
  "max_output_tokens": 5000
}
```

Read a bounded file slice:

```json
{
  "cmd": "sed -n '1,160p' docs/tools/TOOL_SPECS.md",
  "max_output_tokens": 4000
}
```

Continue an interactive session with a cap:

```json
{
  "session_id": 12,
  "chars": "status\n",
  "max_output_tokens": 3000
}
```

## Result Shape

Command responses preserve execution metadata even when output is capped:

```json
{
  "success": true,
  "exit_code": 0,
  "stdout": "... truncated output ...",
  "stderr": "",
  "output_truncated": true,
  "spool_path": "/path/to/full-output.log"
}
```

If a response includes `spool_path`, inspect it once with a targeted shell
command rather than repeatedly dumping the whole file.

## Recommended Budgets

| Operation | Recommended `max_output_tokens` |
|---|---:|
| Small file slice | 2,000 |
| Medium file slice | 5,000 |
| Large grep result | 8,000 |
| Command output | 10,000 |
| Session continuation | 10,000 |

## Guidance

- Start with targeted shell commands that naturally return small output.
- Use `rg --files`, `rg -n`, `sed -n`, `head`, and `tail` before reading broad
  file content.
- Use `code_search` for bounded definitions, syntactic usages, text, or path
  results when the advanced profile is enabled.
- If complete output matters, write it to a file and inspect precise slices with
  follow-up shell commands.
