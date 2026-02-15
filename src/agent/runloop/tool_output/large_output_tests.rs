use std::fs;
use std::path::PathBuf;

use tempfile::TempDir;

use super::large_output::{
    LargeOutputConfig, SpoolResult, format_spool_notification, generate_session_hash,
    spool_large_output,
};

#[test]
fn test_below_threshold_returns_none() {
    let temp_dir = TempDir::new().unwrap();
    let config = LargeOutputConfig {
        base_dir: temp_dir.path().to_path_buf(),
        threshold_bytes: 1000,
        session_id: None,
    };

    let content = "Small content";
    let result = spool_large_output(content, "test_tool", &config).unwrap();
    assert!(result.is_none());
}

#[test]
fn test_above_threshold_spools() {
    let temp_dir = TempDir::new().unwrap();
    let config = LargeOutputConfig {
        base_dir: temp_dir.path().to_path_buf(),
        threshold_bytes: 10,
        session_id: Some("test-session".to_string()),
    };

    let content = "This is content that exceeds the threshold";
    let result = spool_large_output(content, "test_tool", &config)
        .unwrap()
        .unwrap();

    assert!(result.was_spooled);
    assert!(result.file_path.exists());
    assert_eq!(result.size_bytes, content.len());

    let saved = fs::read_to_string(&result.file_path).unwrap();
    assert!(saved.contains(content));
    assert!(saved.contains("# Tool: test_tool"));

    let full_content = result.read_full_content().unwrap();
    assert!(full_content.contains(content));

    let preview = result.get_preview().unwrap();
    assert!(!preview.is_empty());
}

#[test]
fn test_format_notification() {
    let result = SpoolResult {
        file_path: PathBuf::from("/home/user/.vtcode/tmp/abc123/call_def456.output"),
        size_bytes: 100_000,
        line_count: 1000,
        tool_name: "test_tool".to_string(),
        was_spooled: true,
    };

    let notification = format_spool_notification(&result);
    assert!(notification.contains("100000 bytes"));
    assert!(notification.contains(".vtcode/tmp"));
}

#[test]
fn test_agent_response_format() {
    let temp_dir = TempDir::new().unwrap();
    let config = LargeOutputConfig {
        base_dir: temp_dir.path().to_path_buf(),
        threshold_bytes: 10,
        session_id: Some("test-session".to_string()),
    };

    let content = (1..=50)
        .map(|i| format!("Line {}: Some test content here", i))
        .collect::<Vec<_>>()
        .join("\n");

    let result = spool_large_output(&content, "run_pty_cmd", &config)
        .unwrap()
        .unwrap();

    let agent_response = result.to_agent_response().unwrap();

    assert!(agent_response.contains("source of truth"));
    assert!(agent_response.contains("run_pty_cmd"));
    assert!(agent_response.contains("Preview"));
    assert!(agent_response.contains("read_file"));
}

#[test]
fn test_read_lines() {
    let temp_dir = TempDir::new().unwrap();
    let config = LargeOutputConfig {
        base_dir: temp_dir.path().to_path_buf(),
        threshold_bytes: 10,
        session_id: None,
    };

    let content = "Line 1\nLine 2\nLine 3\nLine 4\nLine 5";
    let result = spool_large_output(content, "test", &config)
        .unwrap()
        .unwrap();

    let lines = result.read_lines(2, 4).unwrap();
    assert!(lines.contains("Line 2"));
    assert!(lines.contains("Line 3"));
    assert!(lines.contains("Line 4"));
    assert!(!lines.contains("Line 1"));
    assert!(!lines.contains("Line 5"));
}

#[test]
fn test_session_hash_uniqueness() {
    let hash1 = generate_session_hash(Some("session1"));
    let hash2 = generate_session_hash(Some("session2"));
    assert_ne!(hash1, hash2);
}
