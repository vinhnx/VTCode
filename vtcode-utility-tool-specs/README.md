# vtcode-utility-tool-specs

Passive JSON schemas for VT Code utility and file tool surfaces.

This crate provides ready-made `serde_json::Value` parameter schemas for the
built-in tool surfaces (apply-patch, cron, file I/O, exec, search) so that
callers never have to hand-roll JSON Schema objects.

## Usage

```rust
use vtcode_utility_tool_specs::{
    apply_patch_parameters,
    read_file_parameters,
    unified_exec_parameters,
    with_semantic_anchor_guidance,
};

// Get the default apply-patch parameter schema
let schema = apply_patch_parameters();

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
- `cron_create_parameters()`, `cron_list_parameters()`, `cron_delete_parameters()`
- `list_files_parameters()`, `read_file_parameters()`
- `unified_exec_parameters()`, `unified_file_parameters()`, `unified_search_parameters()`

### Helpers

- `with_semantic_anchor_guidance(base: &str) -> String`

### Modules

| Module | Key types |
|---|---|
| `json_schema` | `JsonSchema`, `AdditionalProperties`, `parse_tool_input_schema` |
| `mcp_tool` | `ParsedMcpTool`, `parse_mcp_tool` |
| `responses_api` | `FreeformTool`, `FreeformToolFormat`, `ResponsesApiTool` |

## Dependencies

`rmcp`, `serde`, `serde_json`
