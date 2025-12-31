//! Example demonstrating the file_search_bridge module
//!
//! This example shows how to use the file search bridge for various file discovery tasks.
//!
//! Run with: cargo run -p vtcode-core --example file_search_bridge_demo -- [OPTIONS]

use std::path::PathBuf;
use vtcode_core::tools::file_search_bridge::{
    FileSearchConfig, filter_by_extension, match_filename, search_files,
};

fn main() -> anyhow::Result<()> {
    // Example 1: Basic file search
    println!("=== Example 1: Basic File Search ===");
    basic_search()?;

    println!("\n=== Example 2: Filter by Extension ===");
    filter_by_ext()?;

    println!("\n=== Example 3: With Exclusions ===");
    with_exclusions()?;

    println!("\n=== Example 4: Limited Results ===");
    limited_results()?;

    Ok(())
}

fn basic_search() -> anyhow::Result<()> {
    let config = FileSearchConfig::new("main".to_string(), PathBuf::from("vtcode-core"));

    let results = search_files(config, None)?;

    println!(
        "Found {} total matches, showing {}:",
        results.total_match_count,
        results.matches.len()
    );
    for m in results.matches.iter().take(5) {
        println!("  {} (score: {})", m.path, m.score);
    }

    Ok(())
}

fn filter_by_ext() -> anyhow::Result<()> {
    let config =
        FileSearchConfig::new("src".to_string(), PathBuf::from("vtcode-core")).with_limit(50);

    let results = search_files(config, None)?;

    // Filter to only Rust files
    let rust_files = filter_by_extension(results.matches, &["rs"]);

    println!("Found {} Rust files:", rust_files.len());
    for m in rust_files.iter().take(5) {
        let filename = match_filename(&m);
        println!("  {} (path: {})", filename, m.path);
    }

    Ok(())
}

fn with_exclusions() -> anyhow::Result<()> {
    let config = FileSearchConfig::new("test".to_string(), PathBuf::from("vtcode-core"))
        .exclude("target/**")
        .exclude(".git/**")
        .exclude("node_modules/**")
        .with_threads(4);

    let results = search_files(config, None)?;

    println!(
        "Found {} matches (excluding target, .git, node_modules):",
        results.matches.len()
    );
    for m in results.matches.iter().take(5) {
        println!("  {} (score: {})", m.path, m.score);
    }

    Ok(())
}

fn limited_results() -> anyhow::Result<()> {
    let config = FileSearchConfig::new("lib".to_string(), PathBuf::from("vtcode-core"))
        .with_limit(5)
        .with_threads(2)
        .respect_gitignore(true);

    let results = search_files(config, None)?;

    println!("Top 5 matches for 'lib':");
    for (i, m) in results.matches.iter().enumerate() {
        let filename = match_filename(&m);
        println!(
            "  {}. {} (score: {}, path: {})",
            i + 1,
            filename,
            m.score,
            m.path
        );
    }

    Ok(())
}
