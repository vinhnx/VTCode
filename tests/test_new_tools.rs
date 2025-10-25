use serde_json::json;
use std::path::PathBuf;
use vtcode_core::tools::ToolRegistry;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut registry = ToolRegistry::new(PathBuf::from(".")).await;

    println!("Testing new Codex-inspired tools...\n");

    // Test extract_json_markers
    println!("1. Testing extract_json_markers:");
    let test_input = r#"
Some text before
=== BEGIN_JSON ===
{"name": "test", "value": 42}
=== END_JSON ===
Some text after
"#;

    let result = registry
        .execute_tool(
            "extract_json_markers",
            json!({
                "input_text": test_input
            }),
        )
        .await?;

    println!("Result: {}", serde_json::to_string_pretty(&result)?);

    // Test security_scan
    println!("\n2. Testing security_scan:");
    let scan_result = registry
        .execute_tool(
            "security_scan",
            json!({
                "scan_type": "sast",
                "output_format": "gitlab"
            }),
        )
        .await?;

    println!("Result: {}", serde_json::to_string_pretty(&scan_result)?);

    // Test generate_security_patch
    println!("\n3. Testing generate_security_patch:");
    let patch_result = registry
        .execute_tool(
            "generate_security_patch",
            json!({
                "vulnerability_report": "{\"vulnerabilities\": []}"
            }),
        )
        .await?;

    println!("Result: {}", serde_json::to_string_pretty(&patch_result)?);

    println!("\nAll tests completed successfully!");

    Ok(())
}
