# vtcode-markdown-store

Markdown document storage, parsing, and rendering utilities for VT Code.

## Conventions

- Uses `pulldown-cmark` for markdown parsing. Do not add alternative parsers.
- Storage backends are abstracted behind a trait -- implement the trait for new backends.
- Document metadata is stored separately from content for fast listing/search.

## Dependencies

- `pulldown-cmark` (markdown parsing)
- `serde` / `serde_json` (serialization)
