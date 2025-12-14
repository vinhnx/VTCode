//! Test script for the new VTCode skills location system

use anyhow::Result;
use std::path::PathBuf;
use vtcode_core::skills::{SkillLocations, EnhancedSkillLoader};

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== VTCode Skills Location System Test ===\n");
    
    // Test 1: Default locations
    println!("1. Testing default skill locations...");
    let locations = SkillLocations::new();
    let location_types = locations.get_location_types();
    
    println!("Configured locations (in precedence order):");
    for (i, loc_type) in location_types.iter().enumerate() {
        println!("  {}. {:?}", i + 1, loc_type);
    }
    println!();
    
    // Test 2: Workspace with skills
    println!("2. Testing skill discovery in workspace...");
    let workspace_root = PathBuf::from(".");
    let mut loader = EnhancedSkillLoader::new(workspace_root);
    
    match loader.discover_all_skills().await {
        Ok(result) => {
            println!("Discovery successful!");
            println!("  Skills from locations: {}", result.traditional_skills.len());
            println!("  CLI tools from discovery: {}", result.cli_tools.len());
            println!("  Total skills found: {}", result.stats.total_skills_found);
            println!("  Discovery time: {}ms", result.stats.discovery_time_ms);
            println!("  Context token usage: {} tokens", result.stats.context_token_usage);
            
            // List discovered skills
            if !result.traditional_skills.is_empty() {
                println!("\n  Discovered skills:");
                for skill in &result.traditional_skills {
                    println!("    - {}: {}", skill.manifest().name, skill.manifest().description);
                }
            }
            
            if !result.cli_tools.is_empty() {
                println!("\n  Discovered CLI tools:");
                for tool in &result.cli_tools {
                    println!("    - {}: {}", tool.name, tool.description);
                }
            }
        }
        Err(e) => {
            println!("Discovery failed: {}", e);
        }
    }
    println!();
    
    // Test 3: Individual skill loading
    println!("3. Testing individual skill loading...");
    
    let skill_names = ["spreadsheet-generator", "doc-generator", "pdf-report-generator"];
    
    for skill_name in &skill_names {
        match loader.get_skill(skill_name).await {
            Ok(skill) => {
                println!("  ✓ Successfully loaded skill: {}", skill.name());
                println!("    Type: {}", if skill.is_cli_tool() { "CLI Tool" } else { "Traditional" });
                println!("    Description: {}", skill.description());
            }
            Err(e) => {
                println!("  ✗ Failed to load skill '{}': {}", skill_name, e);
            }
        }
    }
    println!();
    
    // Test 4: Location-specific discovery
    println!("4. Testing location-specific features...");
    
    // Test recursive vs one-level discovery
    let temp_dir = tempfile::TempDir::new()?;
    let test_workspace = temp_dir.path();
    
    // Create nested skill structure for testing recursive discovery
    let nested_skill = test_workspace.join(".vtcode/skills/web/tools/search-engine");
    std::fs::create_dir_all(&nested_skill)?;
    std::fs::write(
        nested_skill.join("SKILL.md"),
        "---\nname: web:tools:search-engine\ndescription: Web search engine skill\n---\n"
    )?;
    
    // Create one-level skill for testing Claude compatibility
    let one_level_skill = test_workspace.join(".claude/skills/file-analyzer");
    std::fs::create_dir_all(&one_level_skill)?;
    std::fs::write(
        one_level_skill.join("SKILL.md"),
        "---\nname: file-analyzer\ndescription: File analysis skill\n---\n"
    )?;
    
    let mut test_loader = EnhancedSkillLoader::new(test_workspace.to_path_buf());
    
    match test_loader.discover_all_skills().await {
        Ok(result) => {
            println!("  Test workspace discovery:");
            println!("    Skills found: {}", result.stats.total_skills_found);
            
            for skill in &result.traditional_skills {
                let manifest = skill.manifest();
                println!("    - {}: {} (from location system)", manifest.name, manifest.description);
            }
        }
        Err(e) => {
            println!("  Test workspace discovery failed: {}", e);
        }
    }
    
    println!("\n=== Test Complete ===");
    Ok(())
}