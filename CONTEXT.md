# VT Code Agent Context

This file captures runtime invariants for automation-oriented usage of VT Code.

## Output Contracts

- Use `vtcode ask --output-format json` when downstream tooling needs structured replies.
- Use `vtcode exec --json` for JSONL event streams.
- Use `vtcode exec --events <path>` to persist machine-readable transcripts.

## Runtime Introspection

- Discover built-in tool contracts from the binary at runtime:
  - `vtcode schema tools`
  - `vtcode schema tools --mode minimal`
  - `vtcode schema tools --name unified_search --name unified_file`

## Safety Defaults

- For autonomous mutation-oriented work, run a read-only rehearsal first:
  - `vtcode exec --dry-run "<task>"`
- Treat prompts and MCP CLI values as untrusted input.
- Prefer small, explicit tool arguments over broad/unbounded payloads.

## Context Discipline

- Limit request scope in prompts.
- Prefer focused tool calls and incremental execution.
- Avoid requesting full-file/full-workspace payloads unless required.
