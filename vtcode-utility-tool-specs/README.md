# vtcode-utility-tool-specs

Passive JSON schemas for VT Code utility, file, scheduling, and collaboration tool surfaces.

This crate provides ready-made `serde_json::Value` parameter schemas for the
built-in tool surfaces (apply-patch, cron, exec, search, and collaboration/HITL tools)
so that callers never have to hand-roll JSON Schema objects.

<!-- cargo-rdme start -->

Passive JSON schemas for utility, file, scheduling, and collaboration tool surfaces.

<!-- cargo-rdme end -->

## Usage

```rust
use vtcode_utility_tool_specs::{
    apply_patch_parameters,
    exec_command_parameters,
    with_semantic_anchor_guidance,
};

// Get the default apply-patch parameter schema
let schema = apply_patch_parameters();
let exec_schema = exec_command_parameters();

// Attach semantic-anchor guidance to a custom description
let desc = with_semantic_anchor_guidance("Apply a unified diff");
```

## API Reference

### Constants

- `SEMANTIC_ANCHOR_GUIDANCE` – default guidance string for semantic anchors
- `APPLY_PATCH_ALIAS_DESCRIPTION` – alias description for the apply-patch tool
- `DEFAULT_APPLY_PATCH_INPUT_DESCRIPTION` – default input description for apply-patch

### Schema functions

Each returns a `serde_json::Value` representing a JSON Schema object:

- `apply_patch_parameters()` / `apply_patch_parameter_schema(input_description)`
- `exec_command_parameters()`, `write_stdin_parameters()`
- `code_search_parameters()`
- `cron_create_parameters()`, `cron_list_parameters()`, `cron_delete_parameters()`
- `list_files_parameters()`

### Collaboration / HITL functions

| Function | Description |
|---|---|
| `spawn_agent_parameters()` | Schema for spawning a delegated child thread. |
| `spawn_background_subprocess_parameters()` | Schema for launching a managed background subprocess. |
| `send_input_parameters()` | Schema for sending follow-up input to a child agent. |
| `wait_agent_parameters()` | Schema for blocking until child agents complete. |
| `resume_agent_parameters()` | Schema for resuming a paused child agent. |
| `close_agent_parameters()` | Schema for closing a child agent. |
| `request_user_input_parameters()` | Schema for the HITL tool that prompts the user with 1-3 questions. |
| `request_user_input_description()` | Static description string for the HITL tool. |

### Helpers

- `with_semantic_anchor_guidance(base: &str) -> String`

### Modules

| Module | Key types |
|---|---|
| `json_schema` | `JsonSchema`, `AdditionalProperties`, `parse_tool_input_schema` |
| `mcp_tool` | `ParsedMcpTool`, `parse_mcp_tool` |
| `responses_api` | `FreeformTool`, `FreeformToolFormat`, `ResponsesApiTool` |

## API reference

See [docs.rs/vtcode-utility-tool-specs](https://docs.rs/vtcode-utility-tool-specs).

## Dependencies

`rmcp`, `serde`, `serde_json`
