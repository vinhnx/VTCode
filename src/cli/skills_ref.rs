//! skills-ref compatible CLI commands for Agent Skills
//!
//! Implements the reference CLI interface from agentskills.io:
//! - `validate <path>` - Validate a skill directory
//! - `to-prompt <path>...` - Generate <available_skills> XML for agent prompts
//! - `list` - List discovered skills

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use vtcode_core::skills::discovery::SkillDiscovery;
use vtcode_core::skills::manifest::parse_skill_file;
use vtcode_core::skills::model::SkillScope;
use vtcode_core::skills::prompt_integration::generate_skills_prompt_xml;

/// Validate a skill directory (skills-ref compatible)
///
/// Usage: vtcode skills-ref validate <path>
pub async fn handle_skills_ref_validate(path: &Path) -> Result<()> {
    println!("Validating skill at: {}\n", path.display());

    // Parse and validate the skill
    let (manifest, _instructions) =
        parse_skill_file(path).context(format!("Failed to parse skill at {}", path.display()))?;

    // Run validation
    manifest
        .validate()
        .context("Skill manifest validation failed")?;

    // Output success in skills-ref format
    println!("✓ Skill validation passed");
    println!("  Name: {}", manifest.name);
    println!("  Description: {}", manifest.description);
    if let Some(version) = &manifest.version {
        println!("  Version: {}", version);
    }
    if let Some(author) = &manifest.author {
        println!("  Author: {}", author);
    }
    if let Some(license) = &manifest.license {
        println!("  License: {}", license);
    }
    if let Some(tools) = &manifest.tools {
        println!("  Tools: {}", tools.join(", "));
    }

    Ok(())
}

/// Generate <available_skills> XML for agent prompts (skills-ref compatible)
///
/// Usage: vtcode skills-ref to-prompt <path>...
pub async fn handle_skills_ref_to_prompt(paths: &[PathBuf]) -> Result<()> {
    use vtcode_core::skills::model::SkillMetadata;

    let mut skills = Vec::new();

    for path in paths {
        if path.is_dir() {
            // Try to parse as skill directory
            match parse_skill_file(path) {
                Ok((manifest, _)) => {
                    skills.push(SkillMetadata {
                        name: manifest.name.clone(),
                        description: manifest.description.clone(),
                        short_description: None,
                        path: path.clone(),
                        scope: SkillScope::User,
                        manifest: Some(manifest),
                    });
                }
                Err(e) => {
                    eprintln!(
                        "Warning: Failed to parse skill at {}: {}",
                        path.display(),
                        e
                    );
                }
            }
        }
    }

    if skills.is_empty() {
        println!("<!-- No valid skills found -->");
        return Ok(());
    }

    // Generate XML output
    let xml = generate_skills_prompt_xml(&skills);
    println!("{}", xml);

    Ok(())
}

/// List all discovered skills (skills-ref compatible)
///
/// Usage: vtcode skills-ref list [path]
pub async fn handle_skills_ref_list(path: Option<&Path>) -> Result<()> {
    let workspace_root = path
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    let mut discovery = SkillDiscovery::new();
    let result = discovery
        .discover_all(&workspace_root)
        .await
        .context("Failed to discover skills")?;

    println!(
        "Discovered {} skills:\n",
        result.skills.len() + result.tools.len()
    );

    // Traditional skills
    if !result.skills.is_empty() {
        println!("Traditional Skills:");
        for skill_ctx in &result.skills {
            let manifest = skill_ctx.manifest();
            println!("  {} - {}", manifest.name, manifest.description);
            if let Some(version) = &manifest.version {
                println!("    version: {}", version);
            }
            if let Some(tools) = &manifest.tools {
                println!("    tools: {}", tools.join(", "));
            }
        }
        println!();
    }

    // CLI tools
    if !result.tools.is_empty() {
        println!("CLI Tool Skills:");
        for tool in &result.tools {
            println!("  {} - {}", tool.name, tool.description);
        }
    }

    // Stats
    println!("\nDiscovery Stats:");
    println!(
        "  Directories scanned: {}",
        result.stats.directories_scanned
    );
    println!("  Files checked: {}", result.stats.files_checked);
    println!("  Discovery time: {}ms", result.stats.discovery_time_ms);

    Ok(())
}
