# vtcode-macros

Procedural macros for VT Code. Contains derive macros and attribute macros shared across workspace crates.

## Conventions

- This is a proc-macro crate. It must not depend on any other vtcode workspace crate.
- Use `syn` for parsing, `quote` for code generation, `proc-macro2` for token streams.
- Keep macro implementations minimal -- generate code that delegates to runtime helpers in other crates.
- All macros must have doc comments with usage examples.

## Dependencies

- `syn` (parsing)
- `quote` (code generation)
- `proc-macro2` (token streams)
