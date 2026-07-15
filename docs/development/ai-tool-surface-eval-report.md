# AI Tool Surface Eval Report

This report records the executable evaluation artefacts after deletion of the
old tool-surface implementation was completed.

## Scope

The checked-in task set is [ai-tool-surface-eval-cases.json](./ai-tool-surface-eval-cases.json). It contains seven executable cases and covers:

- Ordinary file discovery through `exec_command.cmd`.
- Text search through shell commands such as `rg`.
- Definitions, syntactic usages, text, and matching paths through the advanced
  `code_search` profile.
- Literal smart-case and bounded-result refinement through another call.
- Patch editing through `apply_patch`.
- Interactive command continuation through `write_stdin`.

The suite runner grades the final response. It does not extract or enforce tool
telemetry, so the case schema makes no telemetry claim.

## Baseline Data

No checked-in archived baseline report for the old tool surface was found in
`evals/`, `docs/`, or benchmark report paths during this slice. No live provider
run was performed for this report. The baseline column is therefore marked
unavailable. This context is report prose, not an executable eval case.

## Comparison Table

| Task id | Capability | Default profile | Advanced profile | Archived baseline | Suite validation |
|---|---|---|---|---|---|
| `tool_surface_discovery_default` | Ordinary file discovery | Uses `exec_command.cmd` with shell file discovery | Same target, no advanced tool needed | Unavailable | JSON loads; profile and `llm_grader` metric are recognised |
| `tool_surface_text_search_default` | Text search | Uses `exec_command.cmd` with `rg` | Same target, no advanced tool needed | Unavailable | JSON loads; profile and `llm_grader` metric are recognised |
| `tool_surface_code_search_types_advanced` | Definition, usage, text, and path classification | Unavailable | Requests all four result classifications | Unavailable | JSON loads; profile and `llm_grader` metric are recognised |
| `tool_surface_code_search_smart_case_advanced` | Literal smart-case | Unavailable | Compares lower-case and mixed-case queries | Unavailable | JSON loads; profile and `llm_grader` metric are recognised |
| `tool_surface_code_search_bounds_advanced` | Bounded refinement | Unavailable | Narrows filters in another call after truncation | Unavailable | JSON loads; profile and `llm_grader` metric are recognised |
| `tool_surface_patch_default` | Patch editing | Uses `apply_patch` | Same target, no advanced tool needed | Unavailable | JSON loads; profile and `contains_match` metric are recognised |
| `tool_surface_interactive_default` | Interactive continuation | Uses `exec_command` then `write_stdin` | Same target, no advanced tool needed | Unavailable | JSON loads; profile and `contains_match` metric are recognised |

## Result Format

Each executed row stores the final agent response, grading result, latency, and
raw JSON event stream. The runner passes the case profile as an explicit
`tools.profile` CLI override, so results do not depend on host configuration.
If a later archival import becomes available, report it as archival comparison
data. Do not add an unavailable placeholder to the executable suite.

`code_search` is deliberately bounded. Eval assertions must inspect returned
results, classification, truncation, and refinement behaviour. They must not
assert an exact repository-wide match total.

The checks recorded here validate JSON loading, executable profiles, and metric
selection only. They are suite validation, not a live run or dry run. This
report contains no provider result, measured baseline, or telemetry enforcement.
