<!-- Compact maintainer rules retain the repository instruction line budget. -->
<!-- markdownlint-disable MD013 -->
# vtcode-utility-tool-specs

[Root AGENTS.md](../../../AGENTS.md) | Passive JSON schemas for VT Code utility, file, scheduling, and collaboration tool surfaces. Defines the wire format for tool invocations and results.

## Conventions

- Schemas are defined using `serde` derive macros. Every schema type must derive `Serialize` and `Deserialize`. This crate is a leaf dependency -- do not add dependencies on other vtcode workspace crates (except `vtcode-commons` if needed). Schema types are passive data containers with no behavior or validation logic.
- Uses `rmcp` for MCP schema compatibility.
- Use `with_max_output_tokens_parameter` when exposing object-shaped function schemas to models; execution validation belongs in `vtcode-core`.
- `record_decision_parameters` bounds public rationale and canonical item IDs; current-task identity and ordered evidence validation belong in the core registry, not this passive schema crate.
- `matrix_parameters` defines control/spec/report arguments; worker task/attempt identity is runtime-owned. Canonical lifecycle types live in `vtcode-exec-events`, never this leaf schema crate.

## Module Groups

| Area | Modules |
| --- | --- |
| Schemas | `collaboration/`, `matrix.rs`, `json_schema/`, `responses_api/`, `mcp_tool/` |
| Taxonomy | `tool_kind/` (`ToolKind`, `ToolNamespace`, `CanonicalToolMeta`, `TokenBucket`) |

## Dependencies

- `rmcp` (MCP schema types)
- `serde` / `serde_json` (serialization)

`write_stdin` keeps legacy `session_id`/`chars` calls valid; wait/inspect/terminate/close need no chars. `exec_command.background` and pipe `stdin` are opt-in and default to false; PTYs retain input.
