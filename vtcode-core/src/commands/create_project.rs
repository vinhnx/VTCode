//! Create project command implementation

use crate::config::constants::tools;
use crate::config::types::AgentConfig;
use crate::tools::ToolRegistry;
use crate::utils::colors::style;
use anyhow::{Result, anyhow};
use serde_json::json;

/// Helper to print tool execution result with consistent formatting
fn print_result(action: &str, result: &Result<serde_json::Value>) {
    match result {
        Ok(_) => println!("   {} {}", style("[+]").green(), action),
        Err(e) => println!("   {} {}: {}", style("[X]").red(), action, e),
    }
}

/// Handle the create-project command - create a complete project structure
pub async fn handle_create_project_command(
    config: AgentConfig,
    name: String,
    features: String,
) -> Result<()> {
    println!(
        "{}",
        style(format!(
            "Creating Rust project '{}' with features: {}",
            name, features
        ))
        .cyan()
        .bold()
    );

    let features: Vec<String> = if features.is_empty() {
        vec![]
    } else {
        features.split(',').map(|s| s.trim().to_owned()).collect()
    };

    let registry = ToolRegistry::new(config.workspace.clone()).await;

    // Step 1: Create project directory structure
    println!(
        "{}",
        style("Step 1: Creating project directory structure...").cyan()
    );
    let create_dir_result = registry
        .execute_tool(
            tools::WRITE_FILE,
            json!({
                "path": format!("{}/.gitkeep", name),
                "content": "",
                "overwrite": true,
                "create_dirs": true
            }),
        )
        .await;

    if let Err(e) = &create_dir_result {
        println!("   {} Failed to create directory: {}", style("✗").red(), e);
        return Err(anyhow!("Failed to create project directory: {}", e));
    }
    print_result("Created project directory", &create_dir_result);

    // Step 2: Create Cargo.toml
    println!("{}", style("Step 2: Generating Cargo.toml...").cyan());
    let cargo_toml_content = format!(
        r#"[package]
name = "{}"
version = "0.1.0"
edition = "2021"

[dependencies]
{}"#,
        name,
        if features.iter().any(|s| s == "serde") {
            "serde = { version = \"1.0\", features = [\"derive\"] }"
        } else {
            ""
        }
    );

    let cargo_result = registry
        .execute_tool(
            tools::WRITE_FILE,
            json!({
                "path": format!("{}/Cargo.toml", name),
                "content": cargo_toml_content,
                "overwrite": true,
                "create_dirs": true
            }),
        )
        .await;

    print_result("Created Cargo.toml", &cargo_result);

    // Step 3: Create src directory and main.rs
    println!(
        "{}",
        style("Step 3: Creating source code structure...").cyan()
    );

    let main_rs_content = if features.iter().any(|s| s == "serde") {
        r#"use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
struct Person {
    name: String,
    age: u32,
}

fn main() {
    println!("Hello, {}!", env!("CARGO_PKG_NAME"));

    let person = Person {
        name: "Alice".to_owned(),
        age: 30,
    };

    println!("Created person: {:?}", person);
}"#
    } else {
        &format!(
            r#"fn main() {{
    println!("Hello, {}!", env!("CARGO_PKG_NAME"));
}}"#,
            name
        )
    };

    let main_rs_result = registry
        .execute_tool(
            tools::WRITE_FILE,
            json!({
                "path": format!("{}/src/main.rs", name),
                "content": main_rs_content,
                "overwrite": true,
                "create_dirs": true
            }),
        )
        .await;

    print_result("Created src/main.rs", &main_rs_result);

    // Step 4: Create README.md
    println!("{}", style("Step 4: Generating documentation...").cyan());
    let readme_content = format!(
        r#"# {}

A Rust project with the following features: {}

## Building

```bash
cargo build
```

## Running

```bash
cargo run
```

## Testing

```bash
cargo test
```
"#,
        name,
        features.join(", ")
    );

    let readme_result = registry
        .execute_tool(
            tools::WRITE_FILE,
            json!({
                "path": format!("{}/README.md", name),
                "content": readme_content,
                "overwrite": true,
                "create_dirs": true
            }),
        )
        .await;

    print_result("Created README.md", &readme_result);

    // Step 5: Create .gitignore
    println!("{}", style("Step 5: Adding .gitignore...").cyan());
    let gitignore_content = r#"/target/
Cargo.lock
.DS_Store
*.log
.env
"#;

    let gitignore_result = registry
        .execute_tool(
            tools::WRITE_FILE,
            json!({
                "path": format!("{}/.gitignore", name),
                "content": gitignore_content,
                "overwrite": true,
                "create_dirs": true
            }),
        )
        .await;

    print_result("Created .gitignore", &gitignore_result);

    // Step 6: Test the build
    println!("{}", style("Step 6: Testing project build...").cyan());
    let test_build_result = registry
        .execute_tool(
            tools::LIST_FILES,
            json!({
                "path": format!("{}/src", name),
                "include_hidden": false
            }),
        )
        .await;

    match test_build_result {
        Ok(result) => {
            if let Some(files) = result.get("files")
                && let Some(files_array) = files.as_array()
                && !files_array.is_empty()
            {
                println!("   {} Project structure verified", style("[+]").green());
            }
        }
        Err(e) => println!(
            "   {} Failed to verify project structure: {}",
            style("✗").red(),
            e
        ),
    }

    println!("{}", style("Project creation complete!").green().bold());
    println!(
        "{}",
        style(format!(
            " Project '{}' created with {} features",
            name,
            features.len()
        ))
        .cyan()
    );
    println!(
        "{}",
        style(format!(
            " Run 'cd {} && cargo run' to test your new project",
            name
        ))
        .dim()
    );

    Ok(())
}
