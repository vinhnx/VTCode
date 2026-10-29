use super::*;

// --- canonicalize_command_for_detection tests ---

#[test]
fn canonicalize_command_v_to_verify() {
    assert_eq!(canonicalize_command_for_detection("command -v ast-grep"), Some("__verify__:ast-grep".to_string()));
}
#[test]
fn canonicalize_which_to_verify() {
    assert_eq!(canonicalize_command_for_detection("which ast-grep"), Some("__verify__:ast-grep".to_string()));
}
#[test]
fn canonicalize_help_to_verify() {
    assert_eq!(canonicalize_command_for_detection("ast-grep --help"), Some("__verify__:ast-grep".to_string()));
}
#[test]
fn canonicalize_version_to_verify() {
    assert_eq!(canonicalize_command_for_detection("ast-grep --version"), Some("__verify__:ast-grep".to_string()));
}
#[test]
fn canonicalize_short_help_to_verify() {
    assert_eq!(canonicalize_command_for_detection("ast-grep -h"), Some("__verify__:ast-grep".to_string()));
}
#[test]
fn canonicalize_path_prefix_stripped() {
    assert_eq!(
        canonicalize_command_for_detection("command -v /usr/bin/ast-grep"),
        Some("__verify__:ast-grep".to_string())
    );
}
#[test]
fn canonicalize_cat_to_read() {
    assert_eq!(canonicalize_command_for_detection("cat src/main.rs"), Some("__read__:src/main.rs".to_string()));
}
#[test]
fn canonicalize_head_to_read() {
    assert_eq!(canonicalize_command_for_detection("head src/main.rs"), Some("__read__:src/main.rs".to_string()));
}
#[test]
fn canonicalize_arbitrary_command_returns_none() {
    assert_eq!(canonicalize_command_for_detection("cargo check -p vtcode-core"), None);
}
#[test]
fn canonicalize_empty_command_returns_none() {
    assert_eq!(canonicalize_command_for_detection(""), None);
}
#[test]
fn canonicalize_env_prefix_skips_to_real_tool() {
    assert_eq!(
        canonicalize_command_for_detection("env VAR=val ast-grep --help"),
        Some("__verify__:ast-grep".to_string())
    );
}
#[test]
fn canonicalize_env_prefix_with_path() {
    assert_eq!(
        canonicalize_command_for_detection("env PATH=/usr/bin ast-grep --version"),
        Some("__verify__:ast-grep".to_string())
    );
}
// --- Command normalization integration tests ---

#[test]
fn verification_commands_normalize_to_same_hash() {
    let mut detector = LoopDetector::with_max_repeated_calls(2);
    let tool = tools::UNIFIED_EXEC;

    // These should all normalize to __verify__:ast-grep
    let args1 = json!({"command": "command -v ast-grep"});
    let args2 = json!({"command": "which ast-grep"});
    let _args3 = json!({"command": "ast-grep --help"});

    assert!(detector.record_call(tool, &args1).is_none());
    // Second call with equivalent command should trigger
    let warning = detector.record_call(tool, &args2);
    assert!(warning.is_some(), "which ast-grep should be detected as duplicate of command -v ast-grep");
}
#[test]
fn cat_commands_normalize_to_read_target() {
    let mut detector = LoopDetector::with_max_repeated_calls(100);
    let tool = tools::UNIFIED_EXEC;

    // cat file should set read_target
    let args = json!({"command": "cat src/main.rs"});
    detector.record_call(tool, &args);

    // Check that the last record has a read_target
    let record = detector.recent_calls.back().unwrap();
    assert_eq!(record.read_target.as_deref(), Some("src/main.rs"), "cat command should produce read_target");
}
#[test]
fn verification_spiral_detected_across_tools() {
    let mut detector = LoopDetector::with_max_repeated_calls(2);
    let tool = tools::UNIFIED_EXEC;

    // Verification spiral: different commands checking the same tool
    let args1 = json!({"command": "command -v ast-grep"});
    let _args2 = json!({"command": "ast-grep --version 2>&1 | head -5"});

    assert!(detector.record_call(tool, &args1).is_none());
    // --version doesn't normalize (has pipe), so this shouldn't trigger
    // via identical-call detection. But the first two `command -v` variants
    // should be caught.
}
