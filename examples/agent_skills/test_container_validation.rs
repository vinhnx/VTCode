use vtcode_core::skills::{EnhancedSkillLoader, EnhancedSkill};
use std::path::PathBuf;

fn main() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        run_test().await
    }).unwrap();
}

async fn run_test() -> anyhow::Result<()> {
    println!(" Testing Container Skills Validation");
    println!("======================================");
    
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
        
        // Try to discover the skill first
        match loader.discover_all_skills().await {
            Ok(result) => {
                println!(" Discovery successful: {} traditional skills, {} CLI tools", 
                    result.traditional_skills.len(), result.cli_tools.len());
                
                // Check if skill was discovered
                let skill_found = result.traditional_skills.iter()
                    .any(|s| s.manifest().name == skill_name);
                
                if skill_found {
                    println!(" Skill '{}' was discovered", skill_name);
                } else {
                    println!(" Skill '{}' was not discovered", skill_name);
                    continue;
                }
            }
            Err(e) => {
                println!(" Discovery failed: {}", e);
                continue;
            }
        }
        
        // Now try to load the skill
        match loader.get_skill(skill_name).await {
            Ok(skill) => {
                match skill {
                    EnhancedSkill::Traditional(skill) => {
                        // Check container requirements
                        let analysis = loader.container_validator().analyze_skill(&skill);
                        
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
                
                // Try to get more detailed error info
                if e.to_string().contains("container skills") {
                    println!("    This is the expected container skills validation error");
                }
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