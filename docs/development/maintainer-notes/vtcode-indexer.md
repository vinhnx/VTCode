<!-- markdownlint-disable MD013 -->

# vtcode-indexer maintainer notes

Detailed contracts and gotchas for `crates/codegen/vtcode-indexer/`, moved verbatim from its [AGENTS.md](../../../crates/codegen/vtcode-indexer/AGENTS.md). Read the section for the subsystem you are changing.

[Root AGENTS.md](../../../AGENTS.md) | Lightweight workspace file indexer with Markdown-backed persistence.

## Key Types

`SimpleIndexer` main API | `SimpleIndexerConfig` configuration | `FileIndex` entry | `SearchResult` match | `IndexStorage` trait | `TraversalFilter` trait | `MarkdownIndexStorage` default backend | `ConfigTraversalFilter` default filter

## Rules

- `IndexStorage` trait is the persistence extension point. Implement for custom backends. `TraversalFilter` trait controls which files/dirs are indexed. `ConfigTraversalFilter` is default. Always skips `.env`, `.git`, `.DS_Store` — hardcoded in `should_index_file()`. `SimpleIndexerConfig` excludes `target/`, `node_modules/`, `.vtcode/index/` by default. Snapshot storage (`prefers_snapshot_persistence()`) writes a single `index.md` — legacy per-file `.md` entries are auto-cleaned; `snapshot.rs` parses it strictly (any deviation → empty cache) and `hydrate_from_snapshot()` feeds policy-filtered entries back so `index_directory` skips unchanged (size + second-granularity mtime) files and no-op snapshot rewrites. All snapshot/record writes go through `atomic_write` (canonical dir + exclusive tmp + rename); key-derived names must use `contained_record_path`, which rejects separators, `..`, and absolute keys.
- `storage.rs` owns `IndexStorage`/`MarkdownIndexStorage` and snapshot writing (re-exported from the crate root); `snapshot.rs` only parses. Directory walks stat once per file: new filter/build logic must use the `*_with_metadata` variants, and custom `TraversalFilter`s inherit metadata delegation by default.

## Testing

`cargo nextest run -p vtcode-indexer`. Uses `tempfile::tempdir()` for isolation.

## Gotchas

- `FileIndexCache` builds traverse the workspace and use synchronous/Rayon work; keep them in `spawn_blocking` and preserve the single-build gate so Tokio workers remain responsive and concurrent misses do not stampede the filesystem. Indexed searches read immutable `StringId` path text through the shared snapshot; keep mutable interner access on incremental-update paths only. Parallel uncached traversal and full index builds aggregate worker-local results once; preserve total counts and lexical tie ordering. `refresh_background` returns the lock-free latest snapshot and schedules work only inside Tokio; publish cache and snapshot together after successful rebuilds or incremental updates.
- `index_directory()` respects `.gitignore` via `ignore` crate's `WalkBuilder`.
- `path_cache` is not used here — that's `vtcode-bash-runner`. Indexer uses `index_cache: HashMap`.
- Binary/unreadable files are silently skipped (`ErrorKind::InvalidData`).
