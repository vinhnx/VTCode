<!-- Compact maintainer rules retain the repository instruction line budget. -->
<!-- markdownlint-disable MD013 -->
# vtcode-indexer

[Root AGENTS.md](../../../AGENTS.md) | Lightweight workspace file indexer with Markdown-backed persistence.

## Key Types

`SimpleIndexer` main API | `SimpleIndexerConfig` configuration | `FileIndex` entry | `SearchResult` match | `IndexStorage` trait | `TraversalFilter` trait | `MarkdownIndexStorage` default backend | `ConfigTraversalFilter` default filter

## Rules

- `IndexStorage` trait is the persistence extension point. Implement for custom backends. `TraversalFilter` trait controls which files/dirs are indexed. `ConfigTraversalFilter` is default. Always skips `.env`, `.git`, `.DS_Store` — hardcoded in `should_index_file()`. `SimpleIndexerConfig` excludes `target/`, `node_modules/`, `.vtcode/index/` by default. Snapshot storage (`prefers_snapshot_persistence()`) writes a single `index.md` — legacy per-file `.md` entries are auto-cleaned.

## Testing

`cargo nextest run -p vtcode-indexer`. Uses `tempfile::tempdir()` for isolation.

## Gotchas

- `FileIndexCache` builds traverse the workspace and use synchronous/Rayon work; keep them in `spawn_blocking` and preserve the single-build gate so Tokio workers remain responsive and concurrent misses do not stampede the filesystem. Indexed searches read immutable `StringId` path text through the shared snapshot; keep mutable interner access on incremental-update paths only. Parallel uncached traversal and full index builds aggregate worker-local results once; preserve total counts and lexical tie ordering. `refresh_background` returns the lock-free latest snapshot and schedules work only inside Tokio; publish cache and snapshot together after successful rebuilds or incremental updates.
- `index_directory()` respects `.gitignore` via `ignore` crate's `WalkBuilder`.
- `path_cache` is not used here — that's `vtcode-bash-runner`. Indexer uses `index_cache: HashMap`.
- Binary/unreadable files are silently skipped (`ErrorKind::InvalidData`).
