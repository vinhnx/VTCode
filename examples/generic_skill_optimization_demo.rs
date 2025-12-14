//! Generic Skill Optimization Demo
//! 
//! Demonstrates how file tracking works for ALL skills generically,
//! not just execute_code.

use vtcode_core::tools::ToolRegistry;
use vtcode_core::skills::auto_verification::AutoSkillVerifier;
use serde_json::json;
use tempfile::TempDir;

#[tokio::main]
async fn main() {
    println!("=== Generic Skill File Tracking Demo ===\n");
    
    let temp_dir = TempDir::new().unwrap();
    let workspace_root = temp_dir.path().to_path_buf();
    
    println!("📁 Workspace: {}", workspace_root.display());
    
    // Demo 1: PDF Generator Skill (Generic)
    println!("\n=== Demo 1: PDF Generator Skill ===");
    demo_pdf_generator(&workspace_root).await;
    
    // Demo 2: Spreadsheet Generator Skill (Generic)
    println!("\n=== Demo 2: Spreadsheet Generator Skill ===");
    demo_spreadsheet_generator(&workspace_root).await;
    
    // Demo 3: Doc Generator Skill (Generic)
    println!("\n=== Demo 3: Doc Generator Skill ===");
    demo_doc_generator(&workspace_root).await;
    
    // Demo 4: Generic skill that generates multiple files
    println!("\n=== Demo 4: Multi-File Generator ===");
    demo_multi_file_generator(&workspace_root).await;
    
    // Summary
    println!("\n=== Optimization Summary ===");
    println!("✅ Benefits for ALL skills:");
    println!("   • No skill-specific code needed");
    println!("   • Automatic file detection from output text");
    println!("   • Works with existing skills immediately");
    println!("   • Generic pattern matching for file paths");
    println!("   • Automatic alternative location search");
    println!("   • Prevents 'where is it?' for all skills");
}

async fn demo_pdf_generator(workspace_root: &std::path::Path) {
    // Simulate what a PDF generator skill produces
    let skill_output = r#"
=== PDF Generator Output ===

Generated PDF with the following specifications:
- Format: A4
- Pages: 3
- Content: Report with charts

Output saved to: quarterly_report.pdf

You can open this file with any PDF viewer.
    "#;
    
    println!("📄 Skill output before auto-verification:");
    println!("{}", skill_output);
    
    // Apply auto-verification (this happens automatically for all skills)
    let verifier = AutoSkillVerifier::new(workspace_root.to_path_buf());
    let enhanced = verifier.process_skill_output("pdf-generator-vtcode", skill_output.to_string()).await.unwrap();
    
    println!("\n🔍 After auto-verification:");
    println!("{}", enhanced);
}

async fn demo_spreadsheet_generator(workspace_root: &std::path::Path) {
    let skill_output = r#"
=== Spreadsheet Generator Output ===

Created Excel spreadsheet with:
- 3 worksheets
- Charts and formulas
- Formatted tables

Generated: financial_dashboard.xlsx
    "#;
    
    println!("📄 Original output:");
    println!("{}", skill_output);
    
    let verifier = AutoSkillVerifier::new(workspace_root.to_path_buf());
    let enhanced = verifier.process_skill_output("spreadsheet-generator", skill_output.to_string()).await.unwrap();
    
    println!("\n🔍 Enhanced with file tracking:");
    println!("{}", enhanced);
}

async fn demo_doc_generator(workspace_root: &std::path::Path) {
    let skill_output = r#"
=== Doc Generator Output ===

Generated Word document:
- Professional formatting applied
- Table of contents included
- Headers and footers added

Document created at: project_proposal.docx
    "#;
    
    println!("📄 Original output:");
    println!("{}", skill_output);
    
    let verifier = AutoSkillVerifier::new(workspace_root.to_path_buf());
    let enhanced = verifier.process_skill_output("doc-generator", skill_output.to_string()).await.unwrap();
    
    println!("\n🔍 Enhanced with verification:");
    println!("{}", enhanced);
}

async fn demo_multi_file_generator(workspace_root: &std::path::Path) {
    let skill_output = r#"
=== Multi-File Generator Output ===

Generated complete report package:
- Main report: annual_report.pdf (25 pages)
- Data summary: data_summary.xlsx (3 sheets)
- Charts: performance_charts.png (4 charts)
- Raw data: metrics.csv

All files saved to output directory.
    "#;
    
    println!("📄 Original output (mentions 4 files):");
    println!("{}", skill_output);
    
    let verifier = AutoSkillVerifier::new(workspace_root.to_path_buf());
    let enhanced = verifier.process_skill_output("multi-report-generator", skill_output.to_string()).await.unwrap();
    
    println!("\n🔍 Enhanced with multi-file verification:");
    println!("{}", enhanced);
}

fn show_usage_example() {
    println!("\n=== Usage Example ===");
    println!("No code changes needed! Just use skills normally:");
    println!();
    println!("User: /skills use pdf-generator-vtcode");
    println!("Agent: ✓ Skill loaded with auto-file-tracking enabled");
    println!("User: Generate a report");
    println!("Agent: [executes skill]");
    println!("Agent: ✓ Skill executed");
    println!("       Generated report.pdf");
    println!("       ✅ Generated: /workspace/report.pdf (2048 bytes)");
    println!();
    println!("No 'where is it?' needed!");
}

// Run this with: cargo run --example generic_skill_optimization_demo
