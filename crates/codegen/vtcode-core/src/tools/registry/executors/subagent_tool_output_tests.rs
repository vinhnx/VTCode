use serde_json::json;
use tempfile::TempDir;

use super::sanitize_subagent_tool_output_paths;

#[test]
fn strips_transcript_paths_outside_workspace() {
    let temp = TempDir::new().expect("tempdir");
    let mut value = json!({
        "completed": true,
        "entry": {
            "id": "agent-1",
            "transcript_path": "/Users/example/.vtcode/sessions/agent-1.json",
        }
    });

    sanitize_subagent_tool_output_paths(temp.path(), &mut value);

    assert!(value["entry"].get("transcript_path").is_none());
}

#[test]
fn keeps_transcript_paths_inside_workspace() {
    let temp = TempDir::new().expect("tempdir");
    let transcript_path = temp.path().join(".vtcode/context/subagents/agent-1.json");
    let mut value = json!({
        "id": "agent-1",
        "transcript_path": transcript_path,
    });

    sanitize_subagent_tool_output_paths(temp.path(), &mut value);

    assert_eq!(value["transcript_path"].as_str(), transcript_path.to_str());
}
