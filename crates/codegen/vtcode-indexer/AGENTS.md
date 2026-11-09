# vtcode-indexer

[Root AGENTS.md](../../../AGENTS.md) | Lightweight workspace file indexer with Markdown-backed persistence.

## Key Types

`SimpleIndexer` main API | `SimpleIndexerConfig` | `FileIndex` | `SearchResult` | `IndexStorage` and
`TraversalFilter` traits | `MarkdownIndexStorage`, `ConfigTraversalFilter` defaults

## Rules

- `IndexStorage` is the persistence extension point; `TraversalFilter` decides what is indexed.
- `.env`, `.git`, `.DS_Store` are always skipped (`should_index_file()`); config excludes `target/`,
  `node_modules/`, `.vtcode/index/`.
- `storage.rs` writes the single-file `index.md` snapshot; `snapshot.rs` only parses it, strictly.
- All writes go through `atomic_write`; key-derived names use `contained_record_path`.
- Directory walks stat once per file: new logic uses the `*_with_metadata` variants.

## Testing

`cargo nextest run -p vtcode-indexer`, with `tempfile::tempdir()` for isolation.

## Gotchas

- `FileIndexCache` builds run in `spawn_blocking` behind a single-build gate; keep that gate.
- Publish the cache and snapshot together after a successful rebuild or incremental update.
- `index_directory()` respects `.gitignore`; binary or unreadable files are skipped silently.

Details: [maintainer notes](../../../docs/development/maintainer-notes/vtcode-indexer.md) — read before changing a
subsystem.
