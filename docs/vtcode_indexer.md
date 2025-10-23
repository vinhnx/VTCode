`vtcode-indexer` provides a lightweight, pluggable workspace indexer suitable for
command-line tooling and autonomous agents that need fast filesystem scans
without external services.

## Core concepts

- **`SimpleIndexer`** walks a workspace, caches metadata, and offers helpers for
  search, file lookup, and content retrieval.
- **`SimpleIndexerConfig`** lets callers toggle hidden directory handling,
  specify custom index directories, and refine include/exclude lists without
  hardcoding VTCode's `.vtcode` layout.
- **`IndexStorage`** is a trait abstraction for persisting `FileIndex` entries.
  The crate ships with a Markdown implementation and accepts custom backends
  through `SimpleIndexer::with_storage`.
- **`TraversalFilter`** centralizes directory descent and file-level inclusion
  decisions, giving downstream users a single place to implement glob rules or
  binary detection before indexing.

## Customizing persistence

The default `MarkdownIndexStorage` writes summaries as Markdown files under the
configured index directory. Downstream projects can provide their own
implementation to target alternative formats or external services:

```rust
use std::path::Path;
use std::sync::Arc;
use anyhow::Result;
use vtcode_indexer::{FileIndex, IndexStorage, SimpleIndexer};

#[derive(Clone, Default)]
struct MemoryStorage;

impl IndexStorage for MemoryStorage {
    fn init(&self, _index_dir: &Path) -> Result<()> {
        Ok(())
    }

    fn persist(&self, _index_dir: &Path, entry: &FileIndex) -> Result<()> {
        println!("indexed {} ({} bytes)", entry.path, entry.size);
        Ok(())
    }
}

let mut indexer = SimpleIndexer::new(workspace_root.clone())
    .with_storage(Arc::new(MemoryStorage::default()));
indexer.init()?;
indexer.index_directory(workspace_root.as_path())?;
```

Any `IndexStorage` implementation is free to establish connections, emit
telemetry, or fan out writes during `persist` as long as it surfaces errors
through `anyhow::Result`.

## Tailoring traversal

`TraversalFilter` implementors can short-circuit directory descent or file-level
indexing. The default `ConfigTraversalFilter` follows `SimpleIndexerConfig`
settings. Custom filters can extend those decisions with domain-specific logic:

```rust
use std::path::Path;
use std::sync::Arc;
use vtcode_indexer::{ConfigTraversalFilter, SimpleIndexer, SimpleIndexerConfig, TraversalFilter};

#[derive(Default)]
struct SkipGeneratedFilter {
    inner: ConfigTraversalFilter,
}

impl TraversalFilter for SkipGeneratedFilter {
    fn should_descend(&self, path: &Path, config: &SimpleIndexerConfig) -> bool {
        if path.ends_with("generated") {
            return false;
        }
        self.inner.should_descend(path, config)
    }

    fn should_index_file(&self, path: &Path, config: &SimpleIndexerConfig) -> bool {
        if path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("lock"))
        {
            return false;
        }
        self.inner.should_index_file(path, config)
    }
}

let config = SimpleIndexerConfig::new(workspace_root.clone());
let mut indexer = SimpleIndexer::with_config(config)
    .with_filter(Arc::new(SkipGeneratedFilter::default()));
indexer.init()?;
indexer.index_directory(workspace_root.as_path())?;
```

Filters can coordinate with configuration to honor allowlists while still
blocking noisy directories or file types.

## End-to-end example

The `examples/custom_storage.rs` program demonstrates indexing a temporary
workspace using in-memory storage and a filter that skips Rust sources. Run it
with `cargo run -p vtcode-indexer --example custom_storage` to see the indexed
paths printed to stdout.

## Next steps

- Publish release notes and crate-level documentation updates before the first
  crates.io release.
- Gather feedback from early adopters to validate trait ergonomics and identify
  additional feature flags worth exposing.
