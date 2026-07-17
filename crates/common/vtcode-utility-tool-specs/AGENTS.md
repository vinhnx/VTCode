# vtcode-utility-tool-specs

Passive JSON schemas for VT Code utility, file, scheduling, and collaboration tool surfaces. Defines the wire format for tool invocations and results.

## Conventions

- Schemas are defined using `serde` derive macros. Every schema type must derive `Serialize` and `Deserialize`.
- This crate is a leaf dependency -- do not add dependencies on other vtcode workspace crates (except `vtcode-commons` if needed).
- Schema types are passive data containers with no behavior or validation logic.
- Uses `rmcp` for MCP schema compatibility.

## Dependencies

- `rmcp` (MCP schema types)
- `serde` / `serde_json` (serialization)
