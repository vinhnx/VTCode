# vtcode-diff

`vtcode-diff` computes bounded, structured text diffs and lays them out as renderer-neutral semantic rows. It supports
unified and side-by-side previews, Unicode display-width wrapping, intraline emphasis, bounded head/tail excerpts, and
parsing existing unified-diff text.

```rust
use vtcode_diff::{DiffDocument, DiffLayout, DiffOptions, LayoutOptions};

let diff = DiffDocument::between("old\n", "new\n", DiffOptions::default());
let rows = diff.layout(LayoutOptions {
    layout: DiffLayout::Unified,
    width: 80,
    ..LayoutOptions::default()
});
assert!(rows.iter().any(|row| row.marker == '+'));
```

For paired old/new columns, select the side-by-side layout:

```rust
let rows = diff.layout(LayoutOptions {
    layout: DiffLayout::SideBySide,
    width: 120,
    ..LayoutOptions::default()
});
```

Layout automatically falls back to unified below `min_side_by_side_width`. `max_rows` retains a head/tail excerpt with
an omission row and accurate document statistics.

Features:

- `serde`: serialization for public model types.
- `ratatui`: conversion helpers for Ratatui text rows.
- `ansi`: ANSI rendering with a caller-supplied palette.

Practical Myers is the default algorithm. Computation and intraline refinement have separate timeouts, and costly
intraline work is skipped for large inputs.

## Acknowledgments

The presentation architecture and underlying ideas (unified and side-by-side terminal previews with intraline emphasis)
were informed by [OpenAI Codex](https://github.com/openai/codex), licensed under Apache-2.0. This crate is an
independent implementation for VT Code; no Codex source code was copied.

Licensed under MIT or Apache-2.0, at your option.
