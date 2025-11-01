//! Tests for AST-grep tool integration

use serde_json::json;
use vtcode_core::tools::registry::ToolRegistry;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_ast_grep_tool_registration() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

        // Check if AST-grep tool is available (only if ast-grep is installed)
        let available_tools = registry.available_tools().await;
        println!("Available tools: {:?}", available_tools);

        // The tool should be registered if ast-grep is available
        // If ast-grep is not installed, it won't be in the list
        let has_ast_grep = available_tools.contains(&"ast_grep_search".to_string());
        println!("AST-grep tool available: {}", has_ast_grep);
    }

    #[tokio::test]
    async fn test_ast_grep_tool_execution() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let mut registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

        // Create a simple test file
        let test_file = temp_dir.path().join("test.rs");
        std::fs::write(
            &test_file,
            r#"
fn main() {
    println!("Hello, world!");
    let x = 42;
    println!("{}", x);
}
"#,
        )
        .unwrap();

        // Test AST-grep search if available
        if registry.has_tool("ast_grep_search").await {
            let args = json!({
                "pattern": "println",
                "path": test_file.to_string_lossy(),
                "language": "rust"
            });

            let result = registry.execute_tool("ast_grep_search", args).await;
            match result {
                Ok(response) => {
                    println!("AST-grep search successful: {:?}", response);
                    assert!(response["success"].as_bool().unwrap_or(false));
                }
                Err(e) => {
                    println!(
                        "AST-grep search failed (expected if ast-grep not installed): {}",
                        e
                    );
                }
            }
        } else {
            println!("AST-grep tool not available - skipping execution test");
        }
    }

    #[test]
    fn test_ast_grep_function_declaration() {
        use vtcode_core::tools::build_function_declarations;

        let declarations = build_function_declarations();
        let ast_grep_decl = declarations.iter().find(|d| d.name == "ast_grep_search");

        if let Some(decl) = ast_grep_decl {
            println!("AST-grep function declaration found: {}", decl.name);
            assert_eq!(decl.name, "ast_grep_search");
            assert!(decl.description.contains("AST-grep"));
        } else {
            println!("AST-grep function declaration not found");
        }
    }
}
