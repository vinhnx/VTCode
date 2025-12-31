# File Search Implementation Guide

This guide provides step-by-step instructions for implementing the `vtcode-file-search` crate based on the OpenAI Codex pattern.

## Step 1: Create the Crate

```bash
cd vtcode-file-search
cargo init --lib
```

Update `Cargo.toml`:

```toml
[package]
name = "vtcode-file-search"
version.workspace = true
edition.workspace = true
license.workspace = true

[lib]
name = "vtcode_file_search"
path = "src/lib.rs"

[[bin]]
name = "vtcode-file-search"
path = "src/main.rs"

[dependencies]
anyhow = { workspace = true }
clap = { workspace = true, features = ["derive"] }
ignore = "0.4"
nucleo-matcher = "0.3"
serde = { workspace = true, features = ["derive"] }
serde_json = { workspace = true }
tokio = { workspace = true, features = ["full"] }

[dev-dependencies]
pretty_assertions = { workspace = true }
```

Update root `Cargo.toml` workspace members:

```toml
members = [
    # ... existing members
    "vtcode-file-search",
]
```

## Step 2: Core Types

Create `src/lib.rs`:

```rust
//! Fast fuzzy file search library for VT Code.
//!
//! Uses the `ignore` crate (same as ripgrep) for parallel directory traversal
//! and `nucleo-matcher` for fuzzy matching.

use serde::Serialize;
use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::num::NonZero;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

/// A single file match result.
#[derive(Debug, Clone, Serialize)]
pub struct FileMatch {
    pub score: u32,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub indices: Option<Vec<u32>>,
}

/// Search results with total count.
#[derive(Debug)]
pub struct FileSearchResults {
    pub matches: Vec<FileMatch>,
    pub total_match_count: usize,
}

/// Extract filename from path, with fallback to full path.
pub fn file_name_from_path(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}

/// Run fuzzy file search with parallel traversal.
pub fn run(
    pattern_text: &str,
    limit: NonZero<usize>,
    search_directory: &Path,
    exclude: Vec<String>,
    threads: NonZero<usize>,
    cancel_flag: Arc<AtomicBool>,
    compute_indices: bool,
    respect_gitignore: bool,
) -> anyhow::Result<FileSearchResults> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_name_from_path() {
        assert_eq!(file_name_from_path("src/main.rs"), "main.rs");
        assert_eq!(file_name_from_path("Cargo.toml"), "Cargo.toml");
        assert_eq!(file_name_from_path("/path/to/file.txt"), "file.txt");
        assert_eq!(file_name_from_path(""), "");
    }
}
```

## Step 3: Directory Traversal

```rust
use ignore::WalkBuilder;
use ignore::overrides::OverrideBuilder;
use std::sync::atomic::Ordering;

pub fn run(
    pattern_text: &str,
    limit: NonZero<usize>,
    search_directory: &Path,
    exclude: Vec<String>,
    threads: NonZero<usize>,
    cancel_flag: Arc<AtomicBool>,
    compute_indices: bool,
    respect_gitignore: bool,
) -> anyhow::Result<FileSearchResults> {
    // 1. Build pattern
    let pattern = nucleo_matcher::pattern::Pattern::parse(
        pattern_text,
        Default::default(),
    );

    // 2. Configure walker
    let mut walk_builder = WalkBuilder::new(search_directory);
    walk_builder
        .threads(threads.get())
        .hidden(false)
        .follow_links(true)
        .require_git(false);

    if !respect_gitignore {
        walk_builder
            .git_ignore(false)
            .git_global(false)
            .git_exclude(false)
            .ignore(false)
            .parents(false);
    }

    // 3. Add exclusion patterns
    if !exclude.is_empty() {
        let mut override_builder = OverrideBuilder::new(search_directory);
        for exclude in exclude {
            let exclude_pattern = format!("!{exclude}");
            override_builder.add(&exclude_pattern)?;
        }
        let override_matcher = override_builder.build()?;
        walk_builder.overrides(override_matcher);
    }

    let walker = walk_builder.build_parallel();

    // 4. Collect results (details in next step)
    let results = collect_matches(
        walker,
        &pattern,
        limit,
        &cancel_flag,
        compute_indices,
    )?;

    Ok(results)
}

fn collect_matches(
    walker: ignore::WalkParallel,
    pattern: &nucleo_matcher::pattern::Pattern,
    limit: NonZero<usize>,
    cancel_flag: &Arc<AtomicBool>,
    compute_indices: bool,
) -> anyhow::Result<FileSearchResults> {
    todo!()
}
```

## Step 4: Parallel Result Collection

```rust
use std::cell::UnsafeCell;

struct BestMatchesList {
    matches: BinaryHeap<Reverse<(u32, String)>>,
    limit: usize,
    matcher: nucleo_matcher::Matcher,
}

impl BestMatchesList {
    fn new(limit: usize) -> Self {
        Self {
            matches: BinaryHeap::new(),
            limit,
            matcher: nucleo_matcher::Matcher::new(Default::default()),
        }
    }

    fn try_add(
        &mut self,
        path: &str,
        pattern: &nucleo_matcher::pattern::Pattern,
    ) -> Option<u32> {
        // Fuzzy match the path against pattern
        let score = self.matcher.fuzzy_match(path, pattern)?;

        if self.matches.len() < self.limit {
            self.matches.push(Reverse((score, path.to_string())));
            Some(score)
        } else if score > self.matches.peek().unwrap().0.0 {
            self.matches.pop();
            self.matches.push(Reverse((score, path.to_string())));
            Some(score)
        } else {
            None
        }
    }

    fn into_matches(self) -> Vec<(u32, String)> {
        self.matches.into_sorted_vec()
            .into_iter()
            .map(|Reverse((score, path))| (score, path))
            .collect()
    }
}

fn collect_matches(
    walker: ignore::WalkParallel,
    pattern: &nucleo_matcher::pattern::Pattern,
    limit: NonZero<usize>,
    cancel_flag: &Arc<AtomicBool>,
    compute_indices: bool,
) -> anyhow::Result<FileSearchResults> {
    let num_workers = num_cpus::get();
    let best_matchers_per_worker: Vec<UnsafeCell<BestMatchesList>> =
        (0..num_workers)
            .map(|_| UnsafeCell::new(BestMatchesList::new(limit.get())))
            .collect();

    let index_counter = std::sync::atomic::AtomicUsize::new(0);
    let total_match_count = std::sync::atomic::AtomicUsize::new(0);

    walker.run(|| {
        let worker_id = index_counter.fetch_add(1, Ordering::Relaxed)
            % best_matchers_per_worker.len();
        let best_list = &best_matchers_per_worker[worker_id];

        Box::new(move |result| {
            if cancel_flag.load(Ordering::Relaxed) {
                return ignore::WalkState::Quit;
            }

            let entry = match result {
                Ok(e) => e,
                Err(_) => return ignore::WalkState::Continue,
            };

            let path = match entry.path().to_str() {
                Some(p) => p,
                None => return ignore::WalkState::Continue,
            };

            if entry.metadata().map_or(true, |m| m.is_dir()) {
                return ignore::WalkState::Continue;
            }

            // Safe because each worker has its own index
            unsafe {
                if let Some(_score) = (*best_list.get()).try_add(path, pattern) {
                    total_match_count.fetch_add(1, Ordering::Relaxed);
                }
            }

            ignore::WalkState::Continue
        })
    });

    // Merge results from all workers
    let mut all_matches = Vec::new();
    for cell in best_matchers_per_worker {
        let matches = cell.into_inner().into_matches();
        all_matches.extend(matches);
    }

    // Sort and limit final results
    all_matches.sort_by(|a, b| b.0.cmp(&a.0)); // Descending by score
    all_matches.truncate(limit.get());

    let matches = all_matches
        .into_iter()
        .map(|(score, path)| FileMatch {
            score,
            path,
            indices: None, // TODO: compute indices if requested
        })
        .collect();

    Ok(FileSearchResults {
        matches,
        total_match_count: total_match_count.load(Ordering::Relaxed),
    })
}
```

## Step 5: CLI Interface

Create `src/main.rs`:

```rust
use clap::Parser;
use std::num::NonZero;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use tokio::signal;

#[derive(Parser)]
#[command(name = "vtcode-file-search")]
#[command(about = "Fast fuzzy file search for VT Code")]
struct Cli {
    /// Search pattern
    pattern: Option<String>,

    /// Search directory
    #[arg(short, long, default_value = ".")]
    cwd: PathBuf,

    /// Maximum number of results
    #[arg(short, long, default_value = "100")]
    limit: NonZero<usize>,

    /// Exclude patterns (can be repeated)
    #[arg(short, long)]
    exclude: Vec<String>,

    /// Number of threads
    #[arg(short, long)]
    threads: Option<NonZero<usize>>,

    /// Output results as JSON
    #[arg(long)]
    json: bool,

    /// Compute character indices for highlighting
    #[arg(long)]
    compute_indices: bool,

    /// Respect .gitignore files
    #[arg(long, default_value = "true")]
    respect_gitignore: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    if cli.pattern.is_none() {
        eprintln!("No pattern provided");
        return Ok(());
    }

    let pattern = cli.pattern.unwrap();
    let threads = cli.threads.unwrap_or(
        NonZero::new(num_cpus::get()).unwrap()
    );

    let cancel_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let cancel_flag_clone = cancel_flag.clone();

    // Handle Ctrl+C
    tokio::spawn(async move {
        signal::ctrl_c().await.ok();
        cancel_flag_clone.store(true, std::sync::atomic::Ordering::Relaxed);
    });

    let results = vtcode_file_search::run(
        &pattern,
        cli.limit,
        &cli.cwd,
        cli.exclude,
        threads,
        cancel_flag,
        cli.compute_indices,
        cli.respect_gitignore,
    )?;

    if cli.json {
        println!("{}", serde_json::to_string_pretty(&results.matches)?);
    } else {
        for m in results.matches {
            println!("{} (score: {})", m.path, m.score);
        }
        if results.total_match_count > cli.limit.get() {
            eprintln!(
                "Truncated: {} matches found, showing {}",
                results.total_match_count,
                cli.limit
            );
        }
    }

    Ok(())
}
```

## Step 6: Tests

Add to `src/lib.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

    #[test]
    fn test_file_name_from_path() {
        assert_eq!(file_name_from_path("src/main.rs"), "main.rs");
        assert_eq!(file_name_from_path("/path/to/file.txt"), "file.txt");
    }

    #[test]
    fn test_run_search() -> anyhow::Result<()> {
        // Create temp directory with test files
        let temp = TempDir::new()?;
        fs::write(temp.path().join("hello.rs"), "fn main() {}")?;
        fs::write(temp.path().join("world.txt"), "world")?;

        let results = run(
            "hello",
            NonZero::new(10).unwrap(),
            temp.path(),
            vec![],
            NonZero::new(1).unwrap(),
            Arc::new(AtomicBool::new(false)),
            false,
            false,
        )?;

        assert_eq!(results.matches.len(), 1);
        assert!(results.matches[0].path.contains("hello"));

        Ok(())
    }
}
```

## Step 7: Integration Tests

Create `tests/integration_test.rs`:

```rust
use std::fs;
use std::num::NonZero;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use tempfile::TempDir;
use vtcode_file_search::run;

#[test]
fn test_multiple_matches() -> anyhow::Result<()> {
    let temp = TempDir::new()?;
    fs::write(temp.path().join("src.rs"), "")?;
    fs::write(temp.path().join("test.rs"), "")?;
    fs::write(temp.path().join("main.rs"), "")?;

    let results = run(
        "rs",
        NonZero::new(10).unwrap(),
        temp.path(),
        vec![],
        NonZero::new(2).unwrap(),
        Arc::new(AtomicBool::new(false)),
        false,
        false,
    )?;

    assert_eq!(results.matches.len(), 3);
    Ok(())
}

#[test]
fn test_exclusion() -> anyhow::Result<()> {
    let temp = TempDir::new()?;
    fs::write(temp.path().join("keep.rs"), "")?;
    fs::create_dir(temp.path().join("target"))?;
    fs::write(temp.path().join("target/ignore.rs"), "")?;

    let results = run(
        "rs",
        NonZero::new(10).unwrap(),
        temp.path(),
        vec!["target/**".to_string()],
        NonZero::new(2).unwrap(),
        Arc::new(AtomicBool::new(false)),
        false,
        false,
    )?;

    assert_eq!(results.matches.len(), 1);
    assert!(results.matches[0].path.contains("keep.rs"));
    Ok(())
}
```

## Step 8: Benchmarks

Create `benches/file_search.rs`:

```rust
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::fs;
use std::num::NonZero;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use tempfile::TempDir;
use vtcode_file_search::run;

fn create_test_files(count: usize) -> anyhow::Result<TempDir> {
    let temp = TempDir::new()?;
    for i in 0..count {
        let path = temp.path().join(format!("file_{}.rs", i));
        fs::write(path, format!("// File {}", i))?;
    }
    Ok(temp)
}

fn bench_file_search(c: &mut Criterion) {
    c.bench_function("search_1000_files", |b| {
        b.to_async(tokio::runtime::Runtime::new().unwrap())
            .iter(|| async {
                let temp = create_test_files(1000).unwrap();
                run(
                    black_box("file_"),
                    NonZero::new(100).unwrap(),
                    temp.path(),
                    vec![],
                    NonZero::new(4).unwrap(),
                    Arc::new(AtomicBool::new(false)),
                    false,
                    false,
                )
            })
    });
}

criterion_group!(benches, bench_file_search);
criterion_main!(benches);
```

## Verification Checklist

```bash
# Test compilation
cargo check -p vtcode-file-search

# Run all tests
cargo test -p vtcode-file-search

# Check with strict linting
cargo clippy -p vtcode-file-search

# Format code
cargo fmt -p vtcode-file-search --check

# Run benchmarks
cargo bench -p vtcode-file-search

# Manual CLI test
cargo run -p vtcode-file-search -- --help
cargo run -p vtcode-file-search -- "\.rs$" /path/to/search
cargo run -p vtcode-file-search -- --json "main" /path/to/search
```

## Integration with `GrepSearchManager`

After crate is stable:

1. Update `grep_file.rs` to use `vtcode_file_search::run()` for file discovery
2. Remove ripgrep file enumeration overhead
3. Update `vtcode-core` to depend on `vtcode-file-search`
4. Update file browser to use centralized `file_name_from_path()`

