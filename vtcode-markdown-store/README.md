# vtcode-markdown-store

Markdown-backed storage utilities extracted from VT Code.

This crate provides lightweight persistence helpers that serialize
structured data into Markdown files with embedded JSON and YAML blocks.
Human-readable state files replace the need for a database, and
file-level locking via `fs2` ensures safe concurrent access.

## Features

- Serialize any `serde::Serialize` type into Markdown with embedded JSON + YAML sections
- Deserialize back from either embedded format
- Exclusive/shared file locking (`fs2`) for concurrent safety
- Optional project management, key-value store, and cache modules

## Public entrypoints

- `MarkdownStorage` — core storage manager (`new()`, `init()`, `store()`, `load()`, `list()`, `delete()`, `exists()`)
- `SimpleKVStorage` — simple key-value store backed by Markdown (feature `kv`)
- `SimpleProjectManager` / `ProjectStorage` / `ProjectData` — project metadata persistence (feature `projects`)
- `SimpleCache` — file-system cache with Markdown-backed locking (feature `cache`)

## Usage

```rust,ignore
use vtcode_markdown_store::MarkdownStorage;
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize)]
struct Note { title: String, body: String }

let storage = MarkdownStorage::new("/tmp/notes".into());
storage.init()?;

let note = Note { title: "Hello".into(), body: "World".into() };
storage.store("greeting", &note, "Greeting Note")?;

let loaded: Note = storage.load("greeting")?;
```

## Feature flags

| Flag | Description |
|---|---|
| `projects` (default) | Project metadata management |
| `kv` (default) | Simple key-value storage |
| `cache` (default) | File-system cache utilities |

## API reference

See [docs.rs/vtcode-markdown-store](https://docs.rs/vtcode-markdown-store).
