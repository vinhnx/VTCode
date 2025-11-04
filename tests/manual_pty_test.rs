//! Manual test for PTY functionality

use anyhow::Result;
use assert_fs::TempDir;
use serde_json::json;
use vtcode_core::tools::ToolRegistry;

#[tokio::main]
async fn main() -> Result<()> {
    println!("Testing PTY functionality...");

    // Create a temporary directory for testing
    let temp_dir = TempDir::new()?;
    let workspace = temp_dir.path().to_path_buf();
    let mut registry = ToolRegistry::new(workspace.clone()).await;

    // Test 1: Basic PTY command
    println!("\n=== Test 1: Basic PTY command ===");
    let args = json!({
        "command": "echo",
        "args": ["Hello, PTY!"]
    });

    match registry.execute_tool("run_pty_cmd", args).await {
        Ok(result) => {
            println!("Success: {:?}", result);
            assert_eq!(result["success"], true);
            assert_eq!(result["code"], 0);
            assert!(result["output"].as_str().unwrap().contains("Hello, PTY!"));
            println!("✓ Test 1 passed");
        }
        Err(e) => {
            println!("Error: {}", e);
            return Err(e.into());
        }
    }

    // Test 2: PTY command with working directory
    println!("\n=== Test 2: PTY command with working directory ===");

    // Create a test file
    std::fs::write(workspace.join("test.txt"), "Hello, PTY from file!")?;

    let args = json!({
        "command": "cat",
        "args": ["test.txt"]
    });

    match registry.execute_tool("run_pty_cmd", args).await {
        Ok(result) => {
            println!("Success: {:?}", result);
            assert_eq!(result["success"], true);
            assert_eq!(result["code"], 0);
            assert!(
                result["output"]
                    .as_str()
                    .unwrap()
                    .contains("Hello, PTY from file!")
            );
            println!("✓ Test 2 passed");
        }
        Err(e) => {
            println!("Error: {}", e);
            return Err(e.into());
        }
    }

    // Test 3: PTY session management
    println!("\n=== Test 3: PTY session management ===");

    // Create a PTY session
    let args = json!({
        "session_id": "test_session",
        "command": "bash"
    });

    match registry.execute_tool("create_pty_session", args).await {
        Ok(result) => {
            println!("Create session result: {:?}", result);
            assert_eq!(result["success"], true);
            assert_eq!(result["session_id"], "test_session");
            println!("✓ Session created");
        }
        Err(e) => {
            println!("Error creating session: {}", e);
            return Err(e.into());
        }
    }

    // List PTY sessions
    let args = json!({});
    match registry.execute_tool("list_pty_sessions", args).await {
        Ok(result) => {
            println!("List sessions result: {:?}", result);
            assert!(
                result["sessions"]
                    .as_array()
                    .unwrap()
                    .contains(&"test_session".into())
            );
            println!("✓ Session listed");
        }
        Err(e) => {
            println!("Error listing sessions: {}", e);
            return Err(e.into());
        }
    }

    // Close PTY session
    let args = json!({
        "session_id": "test_session"
    });

    match registry.execute_tool("close_pty_session", args).await {
        Ok(result) => {
            println!("Close session result: {:?}", result);
            assert_eq!(result["success"], true);
            assert_eq!(result["session_id"], "test_session");
            println!("✓ Session closed");
        }
        Err(e) => {
            println!("Error closing session: {}", e);
            return Err(e.into());
        }
    }

    println!("\n=== All tests passed! ===");
    Ok(())
}
