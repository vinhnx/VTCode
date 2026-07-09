# AI Tool Surface Eval Report

This report captures the comparison format for the Codex-like default tool surface before the old internal implementation code is deleted.

## Scope

The checked-in task set is [ai-tool-surface-eval-cases.json](./ai-tool-surface-eval-cases.json). It covers:

- Ordinary file discovery through `exec_command.cmd`.
- Text search through shell commands such as `rg`.
- Semantic search through the advanced `code_search` profile.
- Patch editing through `apply_patch`.
- Interactive command continuation through `write_stdin`.

The per-task telemetry snapshot is derived from `ToolExecutionHistory` and exposes these counters:

- `total_tool_calls`
- `repeated_equivalent_calls`
- `failed_tool_calls`
- `spooled_outputs`
- `fallback_calls`
- `read_after_spool_calls`
- `command_approval_prompts`
- `task_completed_successfully`
- `calls_by_tool`

Public telemetry labels use `exec_command`, `write_stdin`, `apply_patch`, `code_search`, and `file_operation` for archival internal file-operation records. Historical internal labels that contain `unified_exec`, `unified_file`, or `unified_search` are normalised before export. Mentions of those old labels in changelog or source history are archival, not public model-visible telemetry labels.

## Baseline Data

No checked-in archived baseline report for the old tool surface was found in `evals/`, `docs/`, or benchmark report paths during this slice. The baseline column below is therefore marked unavailable. These rows are placeholders for future comparison import, not fabricated measurements.

## Comparison Table

| Task id | Capability | Default profile | Advanced profile | Archived baseline | Slice 8 dry-run result |
|---|---|---|---|---|---|
| `tool_surface_discovery_default` | Ordinary file discovery | Uses `exec_command.cmd` with shell listing or `rg --files` | Same target, no advanced tool needed | Unavailable | Case loads from JSON |
| `tool_surface_text_search_default` | Text search | Uses `exec_command.cmd` with `rg` | Same target, no advanced tool needed | Unavailable | Case loads from JSON |
| `tool_surface_semantic_search_advanced` | Semantic search | Expected to avoid default-only semantic search | Uses `code_search` structural or outline search | Unavailable | Case loads from JSON |
| `tool_surface_patch_default` | Patch editing | Uses `apply_patch` | Same target, no advanced tool needed | Unavailable | Case loads from JSON |
| `tool_surface_interactive_default` | Interactive continuation | Uses `exec_command` then `write_stdin` | Same target, no advanced tool needed | Unavailable | Case loads from JSON |

## Result Format

Each executed row should attach a telemetry snapshot shaped like:

```json
{
  "task_id": "tool_surface_text_search_default",
  "total_tool_calls": 2,
  "repeated_equivalent_calls": 0,
  "failed_tool_calls": 0,
  "spooled_outputs": 0,
  "fallback_calls": 0,
  "read_after_spool_calls": 0,
  "command_approval_prompts": 0,
  "task_completed_successfully": true,
  "calls_by_tool": {
    "exec_command": 2
  }
}
```

For a paired run, store the task output and snapshot for each profile under the same logical task id. If a later archival import becomes available, add it as a separate `archived_baseline` profile and keep the row labelled archival.
