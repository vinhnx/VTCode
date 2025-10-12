//! Utility functions for the VTCode agent
//!
//! This module contains common utility functions that are used across different parts
//! of the VTCode agent, helping to reduce code duplication and improve maintainability.

use anyhow::Result;
use console::style;
use regex::Regex;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Render PTY output in a terminal-like interface
pub fn render_pty_output_fn(output: &str, title: &str, command: Option<&str>) -> Result<()> {
    // Print top border
    println!("{}", style("=".repeat(80)).dim());

    // Print title
    println!(
        "{} {}",
        style("==").blue().bold(),
        style(title).blue().bold()
    );

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
    println!("{}", style("==").blue().bold());
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
            out.push_str(&format!("Project: {}", name));
        }
        if let Some(ver) = &self.version {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(&format!("v{}", ver));
        }
        if !out.is_empty() {
            out.push('\n');
        }
        if let Some(desc) = &self.description {
            out.push_str(desc);
            out.push('\n');
        }
        out.push_str(&format!("Root: {}", self.root.display()));
        out
    }

    pub fn as_prompt_block(&self) -> String {
        let mut s = String::new();
        if let Some(name) = &self.name {
            s.push_str(&format!("- Name: {}\n", name));
        }
        if let Some(ver) = &self.version {
            s.push_str(&format!("- Version: {}\n", ver));
        }
        if let Some(desc) = &self.description {
            s.push_str(&format!("- Description: {}\n", desc));
        }
        s.push_str(&format!("- Workspace Root: {}\n", self.root.display()));
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
pub fn build_project_overview(root: &Path) -> Option<ProjectOverview> {
    let mut overview = ProjectOverview {
        name: None,
        version: None,
        description: None,
        readme_excerpt: None,
        root: root.to_path_buf(),
    };

    // Parse Cargo.toml (best-effort, no extra deps)
    let cargo_toml_path = root.join("Cargo.toml");
    if let Ok(cargo_toml) = fs::read_to_string(&cargo_toml_path) {
        overview.name = extract_toml_str(&cargo_toml, "name");
        overview.version = extract_toml_str(&cargo_toml, "version");
        overview.description = extract_toml_str(&cargo_toml, "description");
    }

    // Read README.md excerpt
    let readme_path = root.join("README.md");
    if let Ok(readme) = fs::read_to_string(&readme_path) {
        overview.readme_excerpt = Some(extract_readme_excerpt(&readme, 1200));
    } else {
        // Fallback to QUICKSTART.md or user-context.md if present
        for alt in [
            "QUICKSTART.md",
            "user-context.md",
            "docs/project/ROADMAP.md",
        ] {
            let path = root.join(alt);
            if let Ok(txt) = fs::read_to_string(&path) {
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

/// Extract a string value from a simple TOML key assignment within \[package]
pub fn extract_toml_str(content: &str, key: &str) -> Option<String> {
    // Only consider the [package] section to avoid matching other tables
    let pkg_section = if let Some(start) = content.find("[package]") {
        let rest = &content[start + "[package]".len()..];
        // Stop at next section header or end
        if let Some(_next) = rest.find('\n') {
            &content[start..]
        } else {
            &content[start..]
        }
    } else {
        content
    };

    // Example target: name = "vtcode"
    let pattern = format!(r#"(?m)^\s*{}\s*=\s*"([^"]+)"\s*$"#, regex::escape(key));
    let re = Regex::new(&pattern).ok()?;
    re.captures(pkg_section)
        .and_then(|caps| caps.get(1).map(|m| m.as_str().to_string()))
}

/// Get the first meaningful section of the README/markdown as an excerpt
pub fn extract_readme_excerpt(md: &str, max_len: usize) -> String {
    // Take from start until we pass the first major sections or hit max_len
    let mut excerpt = String::new();
    for line in md.lines() {
        // Stop if we reach a deep section far into the doc
        if excerpt.len() > max_len {
            break;
        }
        excerpt.push_str(line);
        excerpt.push('\n');
        // Prefer stopping after an initial overview section
        if line.trim().starts_with("## ") && excerpt.len() > (max_len / 2) {
            break;
        }
    }
    if excerpt.len() > max_len {
        excerpt.truncate(max_len);
        excerpt.push_str("...\n");
    }
    excerpt
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

/// Safe text replacement with validation
pub fn safe_replace_text(
    content: &str,
    old_str: &str,
    new_str: &str,
) -> Result<String, anyhow::Error> {
    if old_str.is_empty() {
        return Err(anyhow::anyhow!("old_string cannot be empty"));
    }

    if !content.contains(old_str) {
        return Err(anyhow::anyhow!("Text '{}' not found in file", old_str));
    }

    Ok(content.replace(old_str, new_str))
}
