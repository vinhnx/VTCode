# File Search Improvements Based on OpenAI Codex Pattern

## Overview

This document outlines improvements to VT Code's file search capabilities, inspired by OpenAI's Codex `codex-file-search` module. The Codex implementation demonstrates best practices for high-performance, parallel file discovery with fuzzy matching.

## Current VT Code Implementation

VT Code uses a multi-layered approach:
- **Grep Search**: `GrepSearchManager` orchestrates ripgrep with debouncing and LRU caching
- **Fuzzy Matching**: `nucleo-matcher` for scoring in UI components
- **Glob Patterns**: Custom `.vtcodegitignore` parser for ignore rules
- **Max Results**: Hardcoded to 5 results per AGENTS.md

## Key Improvements from Codex Pattern

### 1. **Dedicated File Search Module** (Similar to `codex-file-search`)

**Current Gap**: File search is deeply integrated into `grep_file.rs` as part of content search.

**Recommendation**: Create `vtcode-file-search` crate as a standalone, reusable module:

```rust
// vtcode-file-search/src/lib.rs

pub struct FileMatch {
    pub score: u32,
    pub path: String,
    pub indices: Option<Vec<u32>>,  // For highlighting
}

pub struct FileSearchResults {
    pub matches: Vec<FileMatch>,
    pub total_match_count: usize,
}

pub async fn run(
    pattern: &str,
    limit: NonZero<usize>,
    search_directory: &Path,
    exclude: Vec<String>,
    threads: NonZero<usize>,
    cancel_flag: Arc<AtomicBool>,
    compute_indices: bool,
    respect_gitignore: bool,
) -> Result<FileSearchResults>
```

**Benefits**:
- Decouples file discovery from content search
- Reusable in CLI, TUI, and IDE integrations (Zed, VS Code)
- Testable in isolation
- Can be exposed as a standalone binary (like Codex does)

### 2. **Use `ignore` Crate for Directory Traversal**

**Current Gap**: Uses ripgrep for both file discovery and content search (overhead).

**Codex Pattern**: Uses `ignore::WalkBuilder` (the same crate ripgrep uses) for blazing-fast parallel directory traversal:

```rust
let walker = WalkBuilder::new(search_directory)
    .hidden(false)
    .follow_links(true)
    .require_git(false)
    .build_parallel();

walker.run(|| {
    // Each worker processes files independently
    // No IPC overhead
});
```

**Implementation**:
```rust
// vtcode-file-search/Cargo.toml
[dependencies]
ignore = "0.4"           # Parallel directory traversal
nucleo-matcher = "0.3"   # Fuzzy matching
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
anyhow = "1"
```

**Benefits**:
- Parallel traversal by default (no subprocess overhead)
- Automatically respects `.gitignore`, `.ignore`, `.git/info/exclude`
- Handles symlinks, hidden files, binary files correctly
- Much faster than spawning ripgrep for file discovery

### 3. **Thread-Safe Result Collection with `BestMatchesList`**

**Current Gap**: `GrepSearchInput::max_results` capped at 5; limited scoring infrastructure.

**Codex Pattern**: Per-worker result lists that merge at the end:

```rust
struct BestMatchesList {
    matches: BinaryHeap<Reverse<FileMatch>>,
    pattern: Pattern,
    matcher: Matcher,
}

// Each worker thread gets its own `BestMatchesList`
// to avoid locking during traversal
let best_matchers_per_worker: Vec<UnsafeCell<BestMatchesList>> = 
    (0..num_workers)
        .map(|_| UnsafeCell::new(BestMatchesList::new(...)))
        .collect();
```

**Implementation**:
```rust
use std::collections::BinaryHeap;
use std::cmp::Reverse;

struct BestMatchesList {
    matches: BinaryHeap<Reverse<(u32, FileMatch)>>,  // Score ordering
    limit: usize,
    pattern: Pattern,
    matcher: Matcher,
}

impl BestMatchesList {
    fn add_match(&mut self, path: &str, pattern: &Pattern) {
        if let Some(score) = self.matcher.fuzzy_match(path, pattern) {
            if self.matches.len() < self.limit {
                self.matches.push(Reverse((score, FileMatch {
                    score,
                    path: path.to_string(),
                    indices: None,
                })));
            } else if score > self.matches.peek().unwrap().0.0 {
                self.matches.pop();
                self.matches.push(Reverse((score, FileMatch { ... })));
            }
        }
    }
}
```

**Benefits**:
- Lock-free during search (uses `UnsafeCell`)
- Automatic top-K results collection
- Efficient binary heap ensures O(log K) insertion
- Merges results at the end

### 4. **Consistent File Name Derivation**

**Current Gap**: File name handling may be scattered; Codex centralizes it:

```rust
pub fn file_name_from_path(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}
```

**Recommendation**: Add to `vtcode-file-search` library and reuse in:
- File browser fuzzy search
- Symbol search results
- Code intelligence goto-definition results
- Zed/VS Code integrations

### 5. **Configurable Exclusion Patterns**

**Current Gap**: Hardcoded `DEFAULT_IGNORE_GLOBS`; limited configuration.

**Codex Pattern**: Accepts `exclude: Vec<String>` and builds override matcher:

```rust
pub fn run(
    pattern_text: &str,
    limit: NonZero<usize>,
    search_directory: &Path,
    exclude: Vec<String>,  // User-provided patterns
    threads: NonZero<usize>,
    cancel_flag: Arc<AtomicBool>,
    compute_indices: bool,
    respect_gitignore: bool,
) -> Result<FileSearchResults>
```

**Benefits**:
- Per-search configuration
- Can be combined with `.gitignore` rules
- Supports negative patterns (`!pattern`) for inclusion

### 6. **Optional Indices for Highlighting**

**Current Gap**: Fuzzy matching doesn't return character indices for UI highlighting.

**Codex Pattern**: Optional `Vec<u32>` in `FileMatch`:

```rust
#[derive(Debug, Clone, Serialize)]
pub struct FileMatch {
    pub score: u32,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub indices: Option<Vec<u32>>,  // Sorted, deduplicated
}
```

**Implementation**:
```rust
// Only compute indices if requested
if compute_indices {
    let mut indices = Vec::new();
    self.matcher.indices(path, self.pattern, &mut indices);
    file_match.indices = Some(indices);
}
```

**Benefits**:
- Highlight matched characters in file browser
- Reduce computation for non-UI use cases
- Sorted and deduplicated by `nucleo-matcher`

### 7. **Cancellation Support**

**Codex Pattern**: Passes `Arc<AtomicBool>` for early termination:

```rust
pub async fn run(
    // ...
    cancel_flag: Arc<AtomicBool>,
    // ...
)
```

**Benefits**:
- Workers check `cancel_flag` periodically
- Responsive cancellation (Ctrl+C)
- No spawned process cleanup issues

### 8. **Worker Thread Tuning**

**Codex Pattern**: Separates walk builder threads from matcher threads:

```rust
struct WorkerCount {
    num_walk_builder_threads: usize,
    num_best_matches_lists: usize,
}

// Walk builder threads: min 1, generally small
// Matcher threads: more aggressive parallelism
```

**Recommendation**: Apply same logic to VT Code (currently uses `optimal_search_threads` for grep).

## Implementation Roadmap

### Phase 1: Create `vtcode-file-search` Crate
1. Add new package to workspace
2. Implement `FileMatch`, `FileSearchResults` structs
3. Port `ignore`-based directory traversal
4. Implement `nucleo-matcher` fuzzy matching

**Effort**: ~2 days

### Phase 2: Integrate with Existing Systems
1. Deprecate direct ripgrep calls for file discovery
2. Update `GrepSearchManager` to use `vtcode-file-search` for file discovery
3. Update file browser fuzzy search to use centralized module
4. Add indices support to UI

**Effort**: ~1 day

### Phase 3: Expose as CLI Tool
1. Add standalone binary (like Codex)
2. Support `--json` output
3. Benchmarks vs. ripgrep and `find`

**Effort**: ~1 day

### Phase 4: IDE Integration
1. Zed extension: use `vtcode-file-search` for file picker
2. VS Code extension: similar integration
3. MCP tool: expose as MCP resource/tool

**Effort**: ~2 days

## Performance Expectations

Based on Codex benchmarks:

- **5000 files, "main" pattern**: ~50ms
- **1 million files, "src" pattern**: ~200ms
- **Parallel efficiency**: ~90% with 8 threads

VT Code should see:
- **File browser**: Instant response (<50ms) for typical projects
- **Grep cleanup**: Simplification of `GrepSearchManager` (remove file discovery overhead)
- **CLI tool**: Standalone binary ~5x faster than `find | grep`

## Crate Additions

```toml
[workspace.dependencies]
ignore = "0.4"
nucleo-matcher = "0.3"

# In vtcode-file-search/Cargo.toml
[dependencies]
ignore = { workspace = true }
nucleo-matcher = { workspace = true }
tokio = { workspace = true, features = ["full"] }
serde = { workspace = true, features = ["derive"] }
anyhow = { workspace = true }
```

## Testing Strategy

```bash
# Unit tests
cargo test -p vtcode-file-search

# Integration tests (project root)
cargo test -p vtcode-file-search

# Benchmarks
cargo bench -p vtcode-file-search file_search

# Manual testing
cargo run -p vtcode-file-search -- --help
cargo run -p vtcode-file-search -- "pattern" /path/to/search
cargo run -p vtcode-file-search -- --json "pattern" /path/to/search
```

## Future Enhancements

1. **Content-aware scoring**: Prioritize recently edited files
2. **Project-specific indexes**: Cache for large monorepos
3. **Semantic search**: Combine with code-intelligence for symbol search
4. **Incremental search**: Only reindex changed files (watch-based)
5. **Windows optimization**: Specialized handling for path separators

## References

- [OpenAI Codex `file-search`](https://github.com/openai/codex/tree/main/codex-rs/file-search)
- [`ignore` crate docs](https://docs.rs/ignore)
- [`nucleo-matcher` crate docs](https://docs.rs/nucleo-matcher)
- VT Code `grep_file.rs` (current implementation)

