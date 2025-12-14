//! Container Skills Validation Demo
//!
//! This example demonstrates how the new container skills validation system
//! detects and handles skills that require Anthropic's container skills feature.

use vtcode_core::skills::{EnhancedSkillLoader, EnhancedSkill};
use std::path::PathBuf;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!(" Container Skills Validation Demo");
    println!("=====================================");
    
    // Initialize skill loader
    let workspace_root = PathBuf::from(".");
    let mut loader = EnhancedSkillLoader::new(workspace_root);
    
    // Test specific skills
    let test_skills = vec![
        "pdf-report-generator",
        "spreadsheet-generator", 
        "doc-generator",
    ];
    
    for skill_name in test_skills {
        println!("\n Testing skill: {}", skill_name);
        println!("------------------------");
        
        match loader.get_skill(skill_name).await {
            Ok(skill) => {
                match skill {
                    EnhancedSkill::Traditional(skill) => {
                        // Check container requirements
                        let analysis = loader.check_container_requirements(&skill);
                        
                        println!(" Skill loaded successfully");
                        println!("   Requirement: {:?}", analysis.requirement);
                        println!("   Analysis: {}", analysis.analysis);
                        
                        if !analysis.patterns_found.is_empty() {
                            println!("   Patterns found: {:?}", analysis.patterns_found);
                        }
                        
                        if !analysis.recommendations.is_empty() {
                            println!("   Recommendations:");
                            for rec in &analysis.recommendations {
                                println!("     • {}", rec);
                            }
                        }
                        
                        if analysis.should_filter {
                            println!("     This skill would be filtered out in production");
                        }
                    }
                    EnhancedSkill::CliTool(_) => {
                        println!(" CLI tool skill loaded");
                    }
                }
            }
            Err(e) => {
                println!(" Failed to load skill: {}", e);
                println!(   "This is expected for container-skills-dependent skills");
            }
        }
    }
    
    println!("\n Summary");
    println!("-----------");
    println!("• Skills requiring container skills without fallback are filtered");
    println!("• Skills with fallback alternatives show guidance");
    println!("• Clear error messages help users understand limitations");
    println!("• Transparent logging shows analysis results");
    
    Ok(())
}