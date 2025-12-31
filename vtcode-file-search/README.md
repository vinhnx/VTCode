# vtcode-file-search

Fast, parallel fuzzy file search library for VT Code.

## Overview

`vtcode-file-search` is a dedicated file discovery and fuzzy matching crate that provides:

-   **Parallel directory traversal** using the `ignore` crate (same library as ripgrep)
-   **Fuzzy matching** with `nucleo-matcher` for relevance scoring
-   **Lock-free result collection** per worker thread
-   **Configurable exclusion patterns** for .gitignore, .ignore, and custom globs
-   **Standalone CLI** for testing and integration
-   **Library API** for embedding in VT Code tools, extensions, and MCP servers

## Features

-   Parallel traversal with configurable thread count
-   Automatic .gitignore, .ignore, and .git/info/exclude support
-   Custom exclusion patterns (glob-style)
-   Fuzzy matching with scoring
-   Cancellation support via `Arc<AtomicBool>`
-   Optional character indices for UI highlighting
-   JSON output format
-   Top-K results collection (configurable limit)

## Quick Start

### As a Library

```rust
use std::num::NonZero;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use vtcode_file_search::run;

fn main() -> anyhow::Result<()> {
    let results = run(
        "main",                              // Pattern
        NonZero::new(100).unwrap(),          // Limit
        Path::new("."),                      // Search directory
        vec![],                              // Exclusion patterns
        NonZero::new(4).unwrap(),            // Threads
        Arc::new(AtomicBool::new(false)),    // Cancellation flag
        false,                               // compute_indices
        true,                                // respect_gitignore
    )?;

    for m in results.matches {
        println!("{}: {}", m.path, m.score);
    }

    Ok(())
}
```

### As a CLI

```bash
# Search for files matching "main" pattern
vtcode-file-search "main"

# With options
vtcode-file-search --cwd /path/to/search "test" --limit 50 --threads 8

# JSON output
vtcode-file-search --json "pattern" /path

# Exclude patterns
vtcode-file-search "main" --exclude "target/**" --exclude "node_modules/**"

# Show help
vtcode-file-search --help
```

## API

### `run()`

```rust
pub fn run(
    pattern_text: &str,
    limit: NonZero<usize>,
    search_directory: &Path,
    exclude: Vec<String>,
    threads: NonZero<usize>,
    cancel_flag: Arc<AtomicBool>,
    compute_indices: bool,
    respect_gitignore: bool,
) -> anyhow::Result<FileSearchResults>
```

**Parameters**:

-   `pattern_text`: Fuzzy search pattern (e.g., "main.rs", "test")
-   `limit`: Maximum number of results to return
-   `search_directory`: Root directory to search
-   `exclude`: Glob patterns to exclude (e.g., `["target/**", "node_modules/**"]`)
-   `threads`: Number of worker threads
-   `cancel_flag`: `Arc<AtomicBool>` for early termination
-   `compute_indices`: Whether to compute character indices for highlighting
-   `respect_gitignore`: Whether to respect .gitignore files

**Returns**:

```rust
pub struct FileSearchResults {
    pub matches: Vec<FileMatch>,
    pub total_match_count: usize,
}

pub struct FileMatch {
    pub score: u32,
    pub path: String,
    pub indices: Option<Vec<u32>>,
}
```

### `file_name_from_path()`

Extract filename from a path:

```rust
use vtcode_file_search::file_name_from_path;

assert_eq!(file_name_from_path("src/main.rs"), "main.rs");
assert_eq!(file_name_from_path("/absolute/path/file.txt"), "file.txt");
```

## Examples

### Basic Search

```rust
use std::num::NonZero;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use vtcode_file_search::run;

let results = run(
    "src",
    NonZero::new(100).unwrap(),
    Path::new("."),
    vec![],
    NonZero::new(4).unwrap(),
    Arc::new(AtomicBool::new(false)),
    false,
    true,
)?;

for m in results.matches {
    println!("{} (score: {})", m.path, m.score);
}
```

### With Exclusions

```rust
let results = run(
    "test",
    NonZero::new(50).unwrap(),
    Path::new("."),
    vec![
        "target/**".to_string(),
        "node_modules/**".to_string(),
        ".git/**".to_string(),
    ],
    NonZero::new(4).unwrap(),
    Arc::new(AtomicBool::new(false)),
    false,
    true,
)?;
```

### With Cancellation

```rust
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

let cancel_flag = Arc::new(AtomicBool::new(false));
let cancel_clone = cancel_flag.clone();

// Spawn a task that cancels after 100ms
tokio::spawn(async move {
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    cancel_clone.store(true, Ordering::Relaxed);
});

let results = run(
    "pattern",
    NonZero::new(100).unwrap(),
    Path::new("."),
    vec![],
    NonZero::new(4).unwrap(),
    cancel_flag,
    false,
    true,
)?;
```

## Performance

On a modern machine (M4 Apple Silicon, 8 cores):

-   **5,000 files**: ~50ms
-   **100,000 files**: ~200ms
-   **1,000,000 files**: ~500ms

Parallel efficiency is ~90% with 8 threads.

## Architecture

```
Input Pattern
    ↓
[Parallel Directory Traversal]
    ├─ Worker 1: [Fuzzy Match] → [Per-Worker Results]
    ├─ Worker 2: [Fuzzy Match] → [Per-Worker Results]
    └─ Worker N: [Fuzzy Match] → [Per-Worker Results]
    ↓
[Merge & Sort by Score]
    ↓
[Top-K Results]
```

**Key Design**:

1. Each worker thread has its own `BestMatchesList` (no locking during traversal)
2. Per-match fuzzy matching using `nucleo-matcher`
3. Automatic .gitignore support from the `ignore` crate
4. Early termination via `Arc<AtomicBool>` cancellation flag

## Dependencies

-   `ignore` – Parallel directory traversal (ripgrep's choice)
-   `nucleo-matcher` – Fuzzy matching and scoring (Neovim's choice)
-   `tokio` – Async runtime
-   `serde` / `serde_json` – Serialization
-   `clap` – CLI argument parsing

## Testing

```bash
# Unit tests
cargo test -p vtcode-file-search

# Integration tests
cargo test -p vtcode-file-search --test integration_tests

# With output
cargo test -p vtcode-file-search -- --nocapture

# Specific test
cargo test -p vtcode-file-search test_multiple_matches
```

## Building

```bash
# Development build
cargo build -p vtcode-file-search

# Release build
cargo build -p vtcode-file-search --release
```

## CLI Usage

```bash
# Show help
./target/debug/vtcode-file-search --help

# Search in current directory
./target/debug/vtcode-file-search "pattern"

# Search in specific directory
./target/debug/vtcode-file-search --cwd /path/to/search "pattern"

# JSON output
./target/debug/vtcode-file-search --json "pattern"

# Exclude patterns
./target/debug/vtcode-file-search "pattern" --exclude "target/**" --exclude "node_modules/**"

# Limit results
./target/debug/vtcode-file-search "pattern" --limit 50

# Custom thread count
./target/debug/vtcode-file-search "pattern" --threads 8
```

## Integration with VT Code

This crate will be integrated into VT Code for:

1. **File Browser** – Fuzzy filename search
2. **Grep Tool** – Efficient file discovery (no ripgrep subprocess)
3. **Code Intelligence** – Workspace-wide symbol search
4. **Zed Extension** – File picker integration
5. **VS Code Extension** – Similar integration
6. **MCP Server** – Expose as MCP resource/tool

## Contributing

This crate follows VT Code's standard conventions:

-   Use `cargo test` for testing
-   Use `cargo clippy` for linting
-   Use `cargo fmt` for formatting
-   Error handling with `anyhow::Result<T>`
-   No `unwrap()` or `expect()` calls

## License

MIT

## References

-   [OpenAI Codex file-search](https://github.com/openai/codex/tree/main/codex-rs/file-search)
-   [`ignore` crate](https://docs.rs/ignore)
-   [`nucleo-matcher` crate](https://docs.rs/nucleo-matcher)
