//! Utility functions for the VT Code agent
//!
//! This module contains common utility functions that are used across different parts
//! of the VT Code agent, helping to reduce code duplication and improve maintainability.

use crate::utils::colors::style;
use anyhow::Result;
use std::fmt::Write as FmtWrite;
use std::io::Write;
use std::path::{Path, PathBuf};
use tokio::fs;

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

/// Lightweight project overview extracted from workspace files
pub struct ProjectOverview {
    pub name: Option<String>,
    pub version: Option<String>,
    pub description: Option<String>,
    pub readme_excerpt: Option<String>,
    pub root: PathBuf,
}

impl ProjectOverview {
    pub fn short_for_display(&self) -> String {
        let mut out = String::new();
        if let Some(name) = &self.name {
            let _ = write!(out, "Project: {}", name);
        }
        if let Some(ver) = &self.version {
            if !out.is_empty() {
                out.push(' ');
            }
            let _ = write!(out, "v{}", ver);
        }
        if !out.is_empty() {
            out.push('\n');
        }
        if let Some(desc) = &self.description {
            out.push_str(desc);
            out.push('\n');
        }
        let _ = write!(out, "Root: {}", self.root.display());
        out
    }

    pub fn as_prompt_block(&self) -> String {
        let mut s = String::new();
        if let Some(name) = &self.name {
            let _ = writeln!(s, "- Name: {}", name);
        }
        if let Some(ver) = &self.version {
            let _ = writeln!(s, "- Version: {}", ver);
        }
        if let Some(desc) = &self.description {
            let _ = writeln!(s, "- Description: {}", desc);
        }
        let _ = writeln!(s, "- Workspace Root: {}", self.root.display());
        if let Some(excerpt) = &self.readme_excerpt {
            s.push_str("- README Excerpt: \n");
            s.push_str(excerpt);
            if !excerpt.ends_with('\n') {
                s.push('\n');
            }
        }
        s
    }
}

/// Build a minimal project overview from Cargo.toml and README.md
pub async fn build_project_overview(root: &Path) -> Option<ProjectOverview> {
    let mut overview = ProjectOverview {
        name: None,
        version: None,
        description: None,
        readme_excerpt: None,
        root: root.to_path_buf(),
    };

    // Parse Cargo.toml (best-effort, no extra deps)
    let cargo_toml_path = root.join("Cargo.toml");
    if let Ok(cargo_toml) = fs::read_to_string(&cargo_toml_path).await {
        overview.name = extract_toml_str(&cargo_toml, "name");
        overview.version = extract_toml_str(&cargo_toml, "version");
        overview.description = extract_toml_str(&cargo_toml, "description");
    }

    // Read README.md excerpt
    let readme_path = root.join("README.md");
    if let Ok(readme) = fs::read_to_string(&readme_path).await {
        overview.readme_excerpt = Some(extract_readme_excerpt(&readme, 1200));
    } else {
        // Fallback to QUICKSTART.md or user-context.md if present
        for alt in [
            "QUICKSTART.md",
            "user-context.md",
            "docs/project/ROADMAP.md",
        ] {
            let path = root.join(alt);
            if let Ok(txt) = fs::read_to_string(&path).await {
                overview.readme_excerpt = Some(extract_readme_excerpt(&txt, 800));
                break;
            }
        }
    }

    // If nothing found, return None
    if overview.name.is_none() && overview.readme_excerpt.is_none() {
        return None;
    }
    Some(overview)
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
