//! Utility functions for the VT Code agent
//!
//! This module contains common utility functions that are used across different parts
//! of the VT Code agent, helping to reduce code duplication and improve maintainability.

use crate::utils::colors::style;
use anyhow::Result;
use std::io::Write;

pub use vtcode_commons::project::{ProjectOverview, build_project_overview};
pub use vtcode_commons::utils::{
    current_timestamp, extract_readme_excerpt, extract_toml_str, safe_replace_text,
};

/// Render PTY output in a terminal-like interface
pub fn render_pty_output_fn(output: &str, title: &str, command: Option<&str>) -> Result<()> {
    // Print top border
    println!("{}", style("=".repeat(80)).dim());

    // Print title
    println!("{} {}", style("==").bold(), style(title).bold());

    // Print command if available
    if let Some(cmd) = command {
        println!("{}", style(format!("> {}", cmd)).dim());
    }

    // Print separator
    println!("{}", style("-".repeat(80)).dim());

    // Print the output
    print!("{}", output);
    std::io::stdout().flush()?;

    // Print bottom border
    println!("{}", style("-".repeat(80)).dim());
    println!("{}", style("==").bold());
    println!("{}", style("=".repeat(80)).dim());

    Ok(())
}

/// Summarize workspace languages
pub fn summarize_workspace_languages(root: &std::path::Path) -> Option<String> {
    use indexmap::IndexMap;
    let analyzer = match crate::tools::tree_sitter::analyzer::TreeSitterAnalyzer::new() {
        Ok(a) => a,
        Err(_) => return None,
    };
    let mut counts: IndexMap<String, usize> = IndexMap::new();
    let mut total = 0usize;
    for entry in walkdir::WalkDir::new(root)
        .max_depth(4)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.is_file()
            && let Ok(lang) = analyzer.detect_language_from_path(path)
        {
            *counts.entry(format!("{:?}", lang)).or_insert(0) += 1;
            total += 1;
        }
        if total > 5000 {
            break;
        }
    }
    if counts.is_empty() {
        None
    } else {
        let mut parts: Vec<String> = counts
            .into_iter()
            .map(|(k, v)| format!("{}:{}", k, v))
            .collect();
        parts.sort();
        Some(parts.join(", "))
    }
}
