use anyhow::Result;
use serde_json::json;
use std::collections::HashMap;

use vtcode_core::tools::autonomous_executor::AutonomousExecutor;

#[tokio::test]
async fn test_adaptive_loop_detection_integration() -> Result<()> {
    // 1. Setup AutonomousExecutor
    let executor = AutonomousExecutor::new();

    // 2. Configure Limits
    let mut limits = HashMap::new();
    limits.insert("read_file".to_string(), 3); // Strict limit for read_file
    limits.insert("list_files".to_string(), 5); // Relaxed limit for list_files
    executor.configure_loop_limits(&limits).await;

    // 3. Test "read_file" limit (Should trigger on 3rd attempt)
    let tool_name = "read_file";
    let tool_args_1 = json!({ "path": "/tmp/test-a.txt" });
    let tool_args_2 = json!({ "path": "/tmp/test-b.txt" });
    let tool_args_3 = json!({ "path": "/tmp/test-c.txt" });

    // Call 1
    let warning1 = executor.record_tool_call(tool_name, &tool_args_1);
    assert!(warning1.is_none(), "Call 1 should not warn");

    // Call 2
    let warning2 = executor.record_tool_call(tool_name, &tool_args_2);
    assert!(warning2.is_none(), "Call 2 should not warn (limit is 3)");

    let warning3 = executor.record_tool_call(tool_name, &tool_args_3);
    assert!(warning3.is_some(), "Call 3 should warn");
    let msg = warning3.unwrap();
    println!("Warning message: {}", msg);
    assert!(
        msg.contains("HARD STOP") || msg.to_lowercase().contains("loop"),
        "Message should mention Loop or Hard Stop"
    );

    // Verify hard limit check
    let detector_arc = executor.loop_detector();
    let detector = detector_arc.read().unwrap();
    assert!(
        !detector.is_hard_limit_exceeded(tool_name),
        "Hard limit should not be exceeded at soft-limit warning"
    );
    drop(detector);

    // 4. Test "list_files" limit (Should NOT trigger on 3rd attempt)
    let list_tool = "list_files";
    // Call 1-3 with different paths to avoid identical-call hard-stop behavior.
    executor.record_tool_call(list_tool, &json!({ "path": "/tmp/a" }));
    executor.record_tool_call(list_tool, &json!({ "path": "/tmp/b" }));
    let warning_list = executor.record_tool_call(list_tool, &json!({ "path": "/tmp/c" }));
    assert!(
        warning_list.is_none(),
        "Call 3 for list_files should NOT warn (limit is 5)"
    );

    Ok(())
}
