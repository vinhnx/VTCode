# `vtcode-markdown-store`

`vtcode-markdown-store` provides lightweight persistence primitives for Rust
applications that prefer human-readable state over database dependencies. The
crate powers VT Code's workspace storage layer and is being extracted so other
projects can reuse the same markdown-backed patterns.

## Storage building blocks

The crate exposes three opt-in capabilities layered on top of the core
`MarkdownStorage` type:

-   **Key-value storage (`kv` feature)** – `SimpleKVStorage` persists arbitrary
    serde data structures into individual markdown files, automatically creating
    the backing directory when needed.
-   **Project metadata (`projects` feature)** – `ProjectStorage` and
    `SimpleProjectManager` manage a folder of project descriptors with helpers to
    create, list, update, and locate per-project assets.
-   **Filesystem cache (`cache` feature)** – `SimpleCache` wraps a configurable
    cache root with typed read/write helpers that emit contextual errors instead
    of silent overwrites.

All three layers share the same markdown serialization format: a title, JSON
code block, YAML code block, and lightly formatted summary. Because the output
is plain text, it plays nicely with git history review, manual editing, and
inspection during debugging sessions.

## Feature flags

The crate enables all capabilities by default:

```toml
[dependencies]
vtcode-markdown-store = { version = "0.30", features = ["projects", "kv", "cache"] }
```

Downstream consumers can slim their dependency footprint by disabling optional
modules they do not need:

```toml
[dependencies]
vtcode-markdown-store = { version = "0.30", default-features = false, features = ["kv"] }
```

-   `projects` – project metadata management (`SimpleProjectManager`,
    `ProjectStorage`).
-   `kv` – generic key/value helpers (`SimpleKVStorage`).
-   `cache` – filesystem caching (`SimpleCache`).

`MarkdownStorage` itself is always available because it is part of the crate's
core API.

## Usage examples

Initialize storage rooted at a workspace-specific directory and persist a custom
record:

```rust
use serde::{Deserialize, Serialize};
use vtcode_markdown_store::MarkdownStorage;

#[derive(Serialize, Deserialize)]
struct RunSummary {
    duration_ms: u64,
    status: String,
}

let storage = MarkdownStorage::new(workspace_root.join("runs"));
storage.init()?;

let summary = RunSummary {
    duration_ms: 1_245,
    status: "succeeded".into(),
};

storage.store("latest", &summary, "Run Summary")?;
let loaded: RunSummary = storage.load("latest")?;
assert_eq!(loaded.status, "succeeded");
```

Opt into the `projects` feature when higher-level coordination is required:

```rust
use vtcode_markdown_store::{ProjectData, ProjectStorage, SimpleProjectManager};

let project_root = workspace_root.join("projects");
let storage = ProjectStorage::new(project_root.clone());
let manager = SimpleProjectManager::with_project_root(workspace_root.clone(), project_root);

manager.init()?;

let mut project = ProjectData::new("sample");
project.description = Some("Markdown-backed project metadata".into());
manager.update_project(&project)?;

let saved = storage.load_project("sample")?;
assert_eq!(saved.name, "sample");
```

See the crate docs and tests for more examples, including cache helpers and
custom storage roots for downstream tooling.

## Concurrency guarantees

All write operations take an exclusive filesystem lock and truncate the target
file only after the lock is secured. Reads take a shared lock and release it as
soon as the contents are buffered, keeping the critical section small. This
locking strategy is powered by the [`fs2`](https://docs.rs/fs2) crate and works
across Unix and Windows platforms. Because writes are flushed and synced before
the lock is released, concurrent agents can safely coordinate on the same
markdown-backed state without corrupting files or observing partially written
data.
