use super::*;

use serde_json::json;
use tempfile::TempDir;
use tokio;

use vtcode_core::config::{HookCommandConfig, HookGroupConfig, HooksConfig, LifecycleHooksConfig};

/// Create a temporary directory for testing with sample hooks
fn create_test_workspace() -> TempDir {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let workspace = temp_dir.path();

    // Create a simple setup script for testing
    let hook_script = workspace.join("test_hook.sh");
    std::fs::write(
        &hook_script,
        r#"#!/bin/bash
# Test script that reads JSON from stdin and echoes back
cat > /dev/null  # consume stdin
echo "Setup complete"
"#,
    )
    .expect("Failed to write test hook script");

    // Make the script executable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&hook_script).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&hook_script, perms).unwrap();
    }

    temp_dir
}

#[tokio::test]
async fn test_lifecycle_hook_engine_creation() {
    let temp_dir = create_test_workspace();
    let workspace = temp_dir.path();

    // Test with empty config - should return None
    let empty_config = HooksConfig::default();
    let result = LifecycleHookEngine::new(
        workspace.to_path_buf(),
        &empty_config,
        SessionStartTrigger::Startup,
    )
    .expect("Failed to create hook engine");

    assert!(result.is_none());

    // Test with non-empty config - should return Some
    let hooks_config = LifecycleHooksConfig {
        session_start: vec![HookGroupConfig {
            matcher: None,
            hooks: vec![HookCommandConfig {
                kind: Default::default(),
                command: "echo 'test'".into(),
                timeout_seconds: None,
            }],
        }],
        ..Default::default()
    };

    let config = HooksConfig {
        lifecycle: hooks_config,
    };

    let result = LifecycleHookEngine::new(
        workspace.to_path_buf(),
        &config,
        SessionStartTrigger::Startup,
    )
    .expect("Failed to create hook engine");

    assert!(result.is_some());
}

#[tokio::test]
#[cfg_attr(
    not(target_os = "macos"),
    ignore = "Lifecycle hooks are for local development only"
)]
async fn test_session_start_hook_execution() {
    let temp_dir = create_test_workspace();
    let workspace = temp_dir.path();

    // Create a simple hook that outputs JSON
    let hooks_config = LifecycleHooksConfig {
        session_start: vec![HookGroupConfig {
            matcher: None, // Match all
            hooks: vec![HookCommandConfig {
                kind: Default::default(),
                command: "printf '{\"additional_context\": \"Session started successfully\"}'"
                    .into(),
                timeout_seconds: None,
            }],
        }],
        ..Default::default()
    };

    let config = HooksConfig {
        lifecycle: hooks_config,
    };

    let engine = LifecycleHookEngine::new(
        workspace.to_path_buf(),
        &config,
        SessionStartTrigger::Startup,
    )
    .expect("Failed to create hook engine")
    .unwrap();

    let outcome = engine
        .run_session_start()
        .await
        .expect("Failed to run session start hook");

    assert_eq!(outcome.additional_context.len(), 1);
    assert_eq!(
        outcome.additional_context[0],
        "Session started successfully"
    );
    assert!(outcome.messages.is_empty());
}

#[tokio::test]
#[cfg_attr(
    not(target_os = "macos"),
    ignore = "Lifecycle hooks are for local development only"
)]
async fn test_session_start_hook_with_plain_text_output() {
    let temp_dir = create_test_workspace();
    let workspace = temp_dir.path();

    // Create a hook that outputs plain text
    let hooks_config = LifecycleHooksConfig {
        session_start: vec![HookGroupConfig {
            matcher: None, // Match all
            hooks: vec![HookCommandConfig {
                kind: Default::default(),
                command: "printf 'Plain text context'".into(),
                timeout_seconds: None,
            }],
        }],
        ..Default::default()
    };

    let config = HooksConfig {
        lifecycle: hooks_config,
    };

    let engine = LifecycleHookEngine::new(
        workspace.to_path_buf(),
        &config,
        SessionStartTrigger::Startup,
    )
    .expect("Failed to create hook engine")
    .unwrap();

    let outcome = engine
        .run_session_start()
        .await
        .expect("Failed to run session start hook");

    assert_eq!(outcome.additional_context.len(), 1);
    assert_eq!(outcome.additional_context[0], "Plain text context");
    assert!(outcome.messages.is_empty());
}

#[tokio::test]
async fn test_session_end_hook_execution() {
    let temp_dir = create_test_workspace();
    let workspace = temp_dir.path();

    // Create a hook for session end that outputs a message
    let hooks_config = LifecycleHooksConfig {
        session_end: vec![HookGroupConfig {
            matcher: None, // Match all
            hooks: vec![HookCommandConfig {
                kind: Default::default(),
                command: "echo 'Cleanup complete'".into(),
                timeout_seconds: None,
            }],
        }],
        ..Default::default()
    };

    let config = HooksConfig {
        lifecycle: hooks_config,
    };

    let engine = LifecycleHookEngine::new(
        workspace.to_path_buf(),
        &config,
        SessionStartTrigger::Startup,
    )
    .expect("Failed to create hook engine")
    .unwrap();

    let messages = engine
        .run_session_end(SessionEndReason::Completed)
        .await
        .expect("Failed to run session end hook");

    // Should have one info message with the output
    assert_eq!(messages.len(), 1);
    assert!(messages[0].text.contains("Cleanup complete"));
}

#[tokio::test]
#[cfg_attr(
    not(target_os = "macos"),
    ignore = "Lifecycle hooks are for local development only"
)]
async fn test_user_prompt_submit_hook_allows_prompt_by_default() {
    let temp_dir = create_test_workspace();
    let workspace = temp_dir.path();

    // Create a config with user prompt hooks
    let hooks_config = LifecycleHooksConfig {
        user_prompt_submit: vec![HookGroupConfig {
            matcher: None, // Match all prompts
            hooks: vec![HookCommandConfig {
                kind: Default::default(),
                command: "printf 'Processing prompt...'".into(),
                timeout_seconds: None,
            }],
        }],
        ..Default::default()
    };

    let config = HooksConfig {
        lifecycle: hooks_config,
    };

    let engine = LifecycleHookEngine::new(
        workspace.to_path_buf(),
        &config,
        SessionStartTrigger::Startup,
    )
    .expect("Failed to create hook engine")
    .unwrap();

    let outcome = engine
        .run_user_prompt_submit("Test prompt")
        .await
        .expect("Failed to run user prompt submit hook");

    // Should allow the prompt by default
    assert!(outcome.allow_prompt);
    assert!(outcome.block_reason.is_none());
    assert_eq!(outcome.additional_context.len(), 1);
    assert_eq!(outcome.additional_context[0], "Processing prompt...");
}

#[tokio::test]
async fn test_user_prompt_submit_hook_blocks_prompt_with_exit_code_2() {
    let temp_dir = create_test_workspace();
    let workspace = temp_dir.path();

    // Create a hook that returns exit code 2 to block the prompt
    let hooks_config = LifecycleHooksConfig {
        user_prompt_submit: vec![HookGroupConfig {
            matcher: None, // Match all prompts
            hooks: vec![HookCommandConfig {
                kind: Default::default(),
                command: "echo 'Prompt blocked' >&2; exit 2".into(),
                timeout_seconds: None,
            }],
        }],
        ..Default::default()
    };

    let config = HooksConfig {
        lifecycle: hooks_config,
    };

    let engine = LifecycleHookEngine::new(
        workspace.to_path_buf(),
        &config,
        SessionStartTrigger::Startup,
    )
    .expect("Failed to create hook engine")
    .unwrap();

    let outcome = engine
        .run_user_prompt_submit("Test prompt")
        .await
        .expect("Failed to run user prompt submit hook");

    // Should block the prompt
    assert!(!outcome.allow_prompt);
    assert!(outcome.block_reason.is_some());
    assert!(outcome.block_reason.unwrap().contains("Prompt blocked"));
}

#[tokio::test]
async fn test_pre_tool_use_hook_allows_by_default() {
    let temp_dir = create_test_workspace();
    let workspace = temp_dir.path();

    // Create a pre-tool hook that doesn't block
    let hooks_config = LifecycleHooksConfig {
        pre_tool_use: vec![HookGroupConfig {
            matcher: Some(".*".into()), // Match all tools using regex
            hooks: vec![HookCommandConfig {
                kind: Default::default(),
                command: "echo 'Pre-tool processing'".into(),
                timeout_seconds: None,
            }],
        }],
        ..Default::default()
    };

    let config = HooksConfig {
        lifecycle: hooks_config,
    };

    let engine = LifecycleHookEngine::new(
        workspace.to_path_buf(),
        &config,
        SessionStartTrigger::Startup,
    )
    .expect("Failed to create hook engine")
    .unwrap();

    let outcome = engine
        .run_pre_tool_use("TestTool", Some(&json!({"param": "value"})))
        .await
        .expect("Failed to run pre-tool use hook");

    // Should continue by default (not deny)
    assert!(matches!(
        outcome.decision,
        PreToolHookDecision::Continue | PreToolHookDecision::Allow
    ));
    assert!(
        outcome.messages.is_empty()
            || outcome
                .messages
                .iter()
                .all(|m| m.text.contains("Pre-tool processing"))
    );
}

#[tokio::test]
#[cfg_attr(
    not(target_os = "macos"),
    ignore = "Lifecycle hooks are for local development only"
)]
async fn test_pre_tool_use_hook_blocks_with_exit_code_2() {
    let temp_dir = create_test_workspace();
    let workspace = temp_dir.path();

    // Create a pre-tool hook that blocks with exit code 2
    let hooks_config = LifecycleHooksConfig {
        pre_tool_use: vec![HookGroupConfig {
            matcher: Some("TestTool".into()), // Match specific tool
            hooks: vec![HookCommandConfig {
                kind: Default::default(),
                command: "printf 'Tool blocked' >&2; exit 2".into(),
                timeout_seconds: None,
            }],
        }],
        ..Default::default()
    };

    let config = HooksConfig {
        lifecycle: hooks_config,
    };

    let engine = LifecycleHookEngine::new(
        workspace.to_path_buf(),
        &config,
        SessionStartTrigger::Startup,
    )
    .expect("Failed to create hook engine")
    .unwrap();

    let outcome = engine
        .run_pre_tool_use("TestTool", Some(&json!({"param": "value"})))
        .await
        .expect("Failed to run pre-tool use hook");

    // Should deny the tool execution
    assert!(matches!(outcome.decision, PreToolHookDecision::Deny));
    assert!(
        outcome
            .messages
            .iter()
            .any(|m| m.text.contains("Tool blocked"))
    );
}

#[tokio::test]
async fn test_pre_tool_use_hook_allows_with_json_response() {
    let temp_dir = create_test_workspace();
    let workspace = temp_dir.path();

    // Create a pre-tool hook that allows with JSON response
    let hooks_config = LifecycleHooksConfig {
        pre_tool_use: vec![HookGroupConfig {
            matcher: Some("TestTool".into()), // Match specific tool
            hooks: vec![HookCommandConfig {
                kind: Default::default(),
                command: r#"printf '{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow"}}\n'"#.into(),
                timeout_seconds: None,
            }],
        }],
        ..Default::default()
    };

    let config = HooksConfig {
        lifecycle: hooks_config,
    };

    let engine = LifecycleHookEngine::new(
        workspace.to_path_buf(),
        &config,
        SessionStartTrigger::Startup,
    )
    .expect("Failed to create hook engine")
    .unwrap();

    let outcome = engine
        .run_pre_tool_use("TestTool", Some(&json!({"param": "value"})))
        .await
        .expect("Failed to run pre-tool use hook");

    // Should allow the tool execution
    assert!(matches!(outcome.decision, PreToolHookDecision::Allow));
}

#[tokio::test]
async fn test_post_tool_use_hook_execution() {
    let temp_dir = create_test_workspace();
    let workspace = temp_dir.path();

    // Create a post-tool hook
    let hooks_config = LifecycleHooksConfig {
        post_tool_use: vec![HookGroupConfig {
            matcher: Some("TestTool".into()), // Match specific tool
            hooks: vec![HookCommandConfig {
                kind: Default::default(),
                command: "echo 'Post-tool processing complete'".into(),
                timeout_seconds: None,
            }],
        }],
        ..Default::default()
    };

    let config = HooksConfig {
        lifecycle: hooks_config,
    };

    let engine = LifecycleHookEngine::new(
        workspace.to_path_buf(),
        &config,
        SessionStartTrigger::Startup,
    )
    .expect("Failed to create hook engine")
    .unwrap();

    let outcome = engine
        .run_post_tool_use(
            "TestTool",
            Some(&json!({"param": "value"})),
            &json!({"result": "success"}),
        )
        .await
        .expect("Failed to run post-tool use hook");

    // Should have an info message with the output
    assert!(
        outcome
            .messages
            .iter()
            .any(|m| m.text.contains("Post-tool processing complete"))
    );
    assert!(outcome.additional_context.is_empty());
    assert!(outcome.block_reason.is_none());
}

#[tokio::test]
async fn test_hook_with_timeout() {
    let temp_dir = create_test_workspace();
    let workspace = temp_dir.path();

    // Create a hook with a short timeout that will exceed it
    let hooks_config = LifecycleHooksConfig {
        session_start: vec![HookGroupConfig {
            matcher: None,
            hooks: vec![HookCommandConfig {
                kind: Default::default(),
                command: "sleep 2".into(), // Sleep longer than timeout
                timeout_seconds: Some(1),  // 1 second timeout
            }],
        }],
        ..Default::default()
    };

    let config = HooksConfig {
        lifecycle: hooks_config,
    };

    let engine = LifecycleHookEngine::new(
        workspace.to_path_buf(),
        &config,
        SessionStartTrigger::Startup,
    )
    .expect("Failed to create hook engine")
    .unwrap();

    let outcome = engine
        .run_session_start()
        .await
        .expect("Failed to run session start hook");

    // Should have a timeout error message
    assert!(
        outcome
            .messages
            .iter()
            .any(|m| m.text.contains("timed out"))
    );
}

#[tokio::test]
async fn test_hook_matcher_functionality() {
    let temp_dir = create_test_workspace();
    let workspace = temp_dir.path();

    // Create hooks with different matchers
    let hooks_config = LifecycleHooksConfig {
        pre_tool_use: vec![
            HookGroupConfig {
                matcher: Some("Write".into()), // Match Write tool
                hooks: vec![HookCommandConfig {
                    kind: Default::default(),
                    command: "echo 'Write tool matched'".into(),
                    timeout_seconds: None,
                }],
            },
            HookGroupConfig {
                matcher: Some("Bash".into()), // Match Bash tool
                hooks: vec![HookCommandConfig {
                    kind: Default::default(),
                    command: "echo 'Bash tool matched'".into(),
                    timeout_seconds: None,
                }],
            },
        ],
        ..Default::default()
    };

    let config = HooksConfig {
        lifecycle: hooks_config,
    };

    let engine = LifecycleHookEngine::new(
        workspace.to_path_buf(),
        &config,
        SessionStartTrigger::Startup,
    )
    .expect("Failed to create hook engine")
    .unwrap();

    // Test Write tool - should match first hook
    let outcome = engine
        .run_pre_tool_use("Write", Some(&json!({"path": "/test"})))
        .await
        .expect("Failed to run pre-tool use hook for Write");

    // Should have a message from the Write hook
    assert!(
        outcome
            .messages
            .iter()
            .any(|m| m.text.contains("Write tool matched"))
    );

    // Test Bash tool - should match second hook
    let outcome = engine
        .run_pre_tool_use("Bash", Some(&json!({"command": "ls"})))
        .await
        .expect("Failed to run pre-tool use hook for Bash");

    // Should have a message from the Bash hook
    assert!(
        outcome
            .messages
            .iter()
            .any(|m| m.text.contains("Bash tool matched"))
    );
}

#[tokio::test]
async fn test_regex_matcher_functionality() {
    let temp_dir = create_test_workspace();
    let workspace = temp_dir.path();

    // Create hooks with regex matchers
    let hooks_config = LifecycleHooksConfig {
        user_prompt_submit: vec![HookGroupConfig {
            matcher: Some(".*security.*".into()), // Match prompts containing "security"
            hooks: vec![HookCommandConfig {
                kind: Default::default(),
                command: "echo 'Security prompt detected'".into(),
                timeout_seconds: None,
            }],
        }],
        ..Default::default()
    };

    let config = HooksConfig {
        lifecycle: hooks_config,
    };

    let engine = LifecycleHookEngine::new(
        workspace.to_path_buf(),
        &config,
        SessionStartTrigger::Startup,
    )
    .expect("Failed to create hook engine")
    .unwrap();

    // Test prompt with "security" - should match
    let outcome = engine
        .run_user_prompt_submit("Please add security validation")
        .await
        .expect("Failed to run user prompt submit hook");

    assert!(
        outcome
            .additional_context
            .iter()
            .any(|ctx| ctx.contains("Security prompt detected"))
    );

    // Test prompt without "security" - should not match
    let outcome = engine
        .run_user_prompt_submit("Add a new feature")
        .await
        .expect("Failed to run user prompt submit hook");

    assert!(outcome.additional_context.is_empty());
}
