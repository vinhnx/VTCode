//! VTCode Enhanced Skills System Demo
//!
//! This example demonstrates the new enhanced skills system with:
//! - CLI tool integration
//! - Progressive context management
//! - Dynamic discovery
//! - Streaming execution
//! - Skill validation

use anyhow::Result;
use std::path::PathBuf;
use vtcode_core::skills::{
    EnhancedSkillLoader, EnhancedSkill, SearchPathType,
    StreamingExecution, StreamEvent,
    SkillValidator, ValidationConfig,
    TemplateEngine, TemplateType,
    ContextConfig, DiscoveryConfig,
};
use futures::StreamExt;

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== VTCode Enhanced Skills System Demo ===\n");
    
    // Setup workspace
    let workspace_root = PathBuf::from("./examples/skills");
    println!("Workspace: {}", workspace_root.display());
    
    // Create enhanced skill loader with custom configuration
    let context_config = ContextConfig {
        max_context_tokens: 100_000,
        max_cached_skills: 50,
        enable_monitoring: true,
        ..Default::default()
    };
    
    let mut loader = EnhancedSkillLoader::with_context_config(workspace_root.clone(), context_config);
    
    // Add custom search paths
    loader.add_search_path(PathBuf::from("./examples/skills"), SearchPathType::Both);
    
    println!("1. Discovering all available skills...\n");
    
    // Discover skills
    let discovery_result = loader.discover_all_skills().await?;
    
    println!("Discovery Results:");
    println!("  Traditional skills: {}", discovery_result.traditional_skills.len());
    println!("  CLI tools: {}", discovery_result.cli_tools.len());
    println!("  Total skills: {}", discovery_result.stats.total_skills_found);
    println!("  Discovery time: {}ms", discovery_result.stats.discovery_time_ms);
    println!("  Context token usage: {} tokens\n", discovery_result.stats.context_token_usage);
    
    // List available skills
    let available_skills = loader.get_available_skills();
    println!("Available Skills:");
    for skill_name in &available_skills {
        println!("  - {}", skill_name);
    }
    println!();
    
    // Demonstrate CLI tool skill execution
    if available_skills.contains(&"web-search".to_string()) {
        println!("2. Executing web-search skill with streaming...\n");
        
        match loader.get_skill("web-search").await? {
            EnhancedSkill::CliTool(bridge) => {
                // Execute with streaming
                let args = serde_json::json!({
                    "query": "rust programming language",
                    "format": "summary",
                    "max_results": 5
                });
                
                println!("Searching for: rust programming language");
                
                let mut stream = bridge.execute_streaming(args);
                
                while let Some(event) = stream.next().await {
                    match event {
                        Ok(StreamEvent::Started { command, args, .. }) => {
                            println!("  Started: {} {:?}", command, args);
                        }
                        Ok(StreamEvent::Progress { percentage, message, .. }) => {
                            println!("  Progress: {:.1}% - {}", percentage, message);
                        }
                        Ok(StreamEvent::Output { data, output_type, .. }) => {
                            let output_type_str = match output_type {
                                vtcode_core::skills::streaming::OutputType::Stdout => "STDOUT",
                                vtcode_core::skills::streaming::OutputType::Stderr => "STDERR",
                            };
                            println!("  {}: {}", output_type_str, data.trim());
                        }
                        Ok(StreamEvent::Completed { exit_code, total_time_ms, .. }) => {
                            println!("  Completed with exit code: {} ({}ms)", exit_code, total_time_ms);
                        }
                        Ok(StreamEvent::Error { message, .. }) => {
                            println!("  Error: {}", message);
                        }
                        Err(e) => {
                            println!("  Stream error: {}", e);
                            break;
                        }
                    }
                }
            }
            EnhancedSkill::Traditional(_) => {
                println!("  web-search is not a CLI tool skill");
            }
        }
        println!();
    }
    
    // Demonstrate file analyzer skill
    if available_skills.contains(&"file-analyzer".to_string()) {
        println!("3. Executing file-analyzer skill...\n");
        
        match loader.get_skill("file-analyzer").await? {
            EnhancedSkill::CliTool(bridge) => {
                // Create a sample file to analyze
                let sample_file = workspace_root.join("sample.rs");
                std::fs::write(&sample_file, SAMPLE_RUST_CODE)?;
                
                let args = serde_json::json!({
                    "file_path": sample_file.to_str().unwrap(),
                    "language": "rust",
                    "analysis_type": "detailed",
                    "metrics": ["complexity", "structure", "quality"]
                });
                
                println!("Analyzing file: {}", sample_file.display());
                
                let result = bridge.execute(args).await?;
                if let Ok(json_result) = serde_json::from_str::<serde_json::Value>(&result["stdout"].as_str().unwrap_or("{}")) {
                    println!("Analysis Results:");
                    println!("  Language: {}", json_result["language"]);
                    println!("  Total lines: {}", json_result["basic_metrics"]["total_lines"]);
                    println!("  Code lines: {}", json_result["basic_metrics"]["code_lines"]);
                    
                    if let Some(complexity) = json_result.get("complexity_metrics") {
                        println!("  Cyclomatic complexity: {}", complexity["cyclomatic_complexity"]);
                        println!("  Function count: {}", complexity["function_count"]);
                    }
                    
                    if let Some(quality) = json_result.get("quality_metrics") {
                        println!("  Quality score: {}/100", quality["quality_score"]);
                        if !quality["quality_issues"].as_array().unwrap().is_empty() {
                            println!("  Quality issues: {:?}", quality["quality_issues"]);
                        }
                    }
                }
                
                // Clean up
                std::fs::remove_file(&sample_file).ok();
            }
            EnhancedSkill::Traditional(_) => {
                println!("  file-analyzer is not a CLI tool skill");
            }
        }
        println!();
    }
    
    // Demonstrate skill validation
    println!("4. Validating skills...\n");
    
    let mut validator = SkillValidator::with_config(ValidationConfig {
        enable_security_checks: true,
        enable_performance_checks: true,
        strict_mode: false,
        ..Default::default()
    });
    
    for skill_name in &available_skills {
        if let Ok(skill) = loader.get_skill(skill_name).await {
            match skill {
                EnhancedSkill::CliTool(bridge) => {
                    println!("Validating CLI tool: {}", skill_name);
                    match validator.validate_cli_tool(&bridge.config).await {
                        Ok(report) => {
                            println!("  Status: {:?}", report.status);
                            println!("  Validation time: {}ms", report.performance.total_time_ms);
                            
                            if !report.recommendations.is_empty() {
                                println!("  Recommendations:");
                                for rec in &report.recommendations {
                                    println!("    - {}", rec);
                                }
                            }
                        }
                        Err(e) => {
                            println!("  Validation failed: {}", e);
                        }
                    }
                }
                EnhancedSkill::Traditional(_) => {
                    println!("Validating traditional skill: {} (skipped - CLI validation only)", skill_name);
                }
            }
        }
    }
    println!();
    
    // Demonstrate template system
    println!("5. Generating skills from templates...\n");
    
    let template_engine = TemplateEngine::new();
    let templates = template_engine.get_template_names();
    
    println!("Available templates:");
    for template_name in &templates {
        if let Some(template) = template_engine.get_template(template_name) {
            println!("  - {}: {}", template_name, template.description);
        }
    }
    println!();
    
    // Generate a new skill from template
    let output_dir = workspace_root.join("generated");
    std::fs::create_dir_all(&output_dir)?;
    
    let mut variables = std::collections::HashMap::new();
    variables.insert("tool_name".to_string(), "example-tool".to_string());
    variables.insert("tool_description".to_string(), "An example CLI tool generated from template".to_string());
    variables.insert("tool_command".to_string(), "echo".to_string());
    variables.insert("supports_json".to_string(), "false".to_string());
    
    match template_engine.generate_skill("cli-tool", variables, &output_dir) {
        Ok(skill_path) => {
            println!("Generated CLI tool skill: {}", skill_path.display());
            println!("  - tool.json: Configuration file");
            println!("  - tool.sh: Executable wrapper script");
            println!("  - README.md: Documentation");
        }
        Err(e) => {
            println!("Failed to generate skill: {}", e);
        }
    }
    println!();
    
    // Show statistics
    println!("6. System Statistics\n");
    
    let discovery_stats = loader.get_discovery_stats();
    println!("Discovery Statistics:");
    println!("  Directories scanned: {}", discovery_stats.directories_scanned);
    println!("  Files checked: {}", discovery_stats.files_checked);
    println!("  Skills found: {}", discovery_stats.skills_found);
    println!("  Tools found: {}", discovery_stats.tools_found);
    println!("  Errors encountered: {}", discovery_stats.errors_encountered);
    
    let context_stats = loader.get_context_stats();
    println!("\nContext Statistics:");
    println!("  Total skills loaded: {}", context_stats.total_skills_loaded);
    println!("  Total skills evicted: {}", context_stats.total_skills_evicted);
    println!("  Cache hits: {}", context_stats.cache_hits);
    println!("  Cache misses: {}", context_stats.cache_misses);
    println!("  Peak token usage: {}", context_stats.peak_token_usage);
    println!("  Current token usage: {}", context_stats.current_token_usage);
    
    let template_stats = template_engine.get_template_stats();
    println!("\nTemplate Statistics:");
    println!("  Total templates: {}", template_stats.total_templates);
    println!("  Traditional templates: {}", template_stats.traditional_templates);
    println!("  CLI tool templates: {}", template_stats.cli_tool_templates);
    println!("  Code generator templates: {}", template_stats.code_generator_templates);
    
    println!("\n=== Demo Complete ===");
    
    Ok(())
}

const SAMPLE_RUST_CODE: &str = r#"
use std::collections::HashMap;

/// A simple calculator that performs basic arithmetic operations
pub struct Calculator {
    history: HashMap<String, f64>,
}

impl Calculator {
    /// Create a new calculator instance
    pub fn new() -> Self {
        Self {
            history: HashMap::new(),
        }
    }
    
    /// Add two numbers
    pub fn add(&mut self, a: f64, b: f64) -> f64 {
        let result = a + b;
        self.history.insert("add".to_string(), result);
        result
    }
    
    /// Subtract two numbers
    pub fn subtract(&mut self, a: f64, b: f64) -> f64 {
        let result = a - b;
        self.history.insert("subtract".to_string(), result);
        result
    }
    
    /// Multiply two numbers
    pub fn multiply(&mut self, a: f64, b: f64) -> f64 {
        let result = a * b;
        self.history.insert("multiply".to_string(), result);
        result
    }
    
    /// Divide two numbers with error handling
    pub fn divide(&mut self, a: f64, b: f64) -> Result<f64, String> {
        if b == 0.0 {
            return Err("Division by zero".to_string());
        }
        let result = a / b;
        self.history.insert("divide".to_string(), result);
        Ok(result)
    }
    
    /// Get calculation history
    pub fn get_history(&self) -> &HashMap<String, f64> {
        &self.history
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_addition() {
        let mut calc = Calculator::new();
        assert_eq!(calc.add(2.0, 3.0), 5.0);
    }
    
    #[test]
    fn test_division_by_zero() {
        let mut calc = Calculator::new();
        assert!(calc.divide(10.0, 0.0).is_err());
    }
}
"#;