# vtcode-collaboration-tool-specs

Passive JSON schemas for VT Code collaboration and human-in-the-loop (HITL) tool surfaces.

## Conventions

- Schemas are defined using `serde` derive macros. Every schema type must derive `Serialize` and `Deserialize`.
- This crate is a leaf dependency -- do not add dependencies on other vtcode workspace crates.
- Schema types are passive data containers with no behavior or validation logic.

## Dependencies

- `serde_json` (serialization)
