<!-- Compact maintainer rules retain the repository instruction line budget. -->
<!-- markdownlint-disable MD013 -->
# vtcode-macros

[Root AGENTS.md](../../../AGENTS.md) | `StringNewtype` + `DebugNoInline` derive macros shared across workspace crates.

## Conventions

- This is a proc-macro crate. It must not depend on any other vtcode workspace crate.
- Use `syn` for parsing, `quote` for code generation, `proc-macro2` for token streams.
- Keep macro implementations minimal -- generate code that delegates to runtime helpers in other crates.
- All macros must have doc comments with usage examples.
- `DebugNoInline` mirrors the built-in `Debug` derive's output exactly; any change to type shapes (empty variants, raw identifiers, generics, alternate formatting) needs a matching case in `tests/debug_no_inline.rs`.

## Dependencies

- `syn` (parsing)
- `quote` (code generation)
- `proc-macro2` (token streams)
