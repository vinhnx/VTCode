use super::*;

use tempfile::TempDir;
use tokio;

use crate::config::{HookCommandConfig, HookGroupConfig, HooksConfig, LifecycleHooksConfig};

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
async fn test_notification_hook_execution_uses_notification_type_matcher() {
    let temp_dir = create_test_workspace();
    let workspace = temp_dir.path();

    let hooks_config = LifecycleHooksConfig {
        notification: vec![HookGroupConfig {
            matcher: Some("permission_prompt".into()),
            hooks: vec![HookCommandConfig {
                kind: Default::default(),
                command: "echo 'Notification hook fired'".into(),
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
        .run_notification(
            NotificationHookType::PermissionPrompt,
            "VT Code approval required",
            "Review the permission prompt.",
        )
        .await
        .expect("Failed to run notification hook");

    assert_eq!(messages.len(), 1);
    assert!(messages[0].text.contains("Notification hook fired"));
}

#[tokio::test]
async fn test_pre_compact_hook_execution_uses_trigger_matcher() {
    let temp_dir = create_test_workspace();
    let workspace = temp_dir.path();

    let hooks_config = LifecycleHooksConfig {
        pre_compact: vec![HookGroupConfig {
            matcher: Some("auto".into()),
            hooks: vec![HookCommandConfig {
                kind: Default::default(),
                command: "echo 'Compaction hook fired'".into(),
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
        .run_pre_compact(
            crate::exec::events::CompactionTrigger::Auto,
            crate::exec::events::CompactionMode::Local,
            12,
            5,
            None,
        )
        .await
        .expect("Failed to run pre-compact hook");

    assert_eq!(outcome.messages.len(), 1);
    assert!(outcome.messages[0].text.contains("Compaction hook fired"));
}

#[tokio::test]
async fn test_subagent_start_payload_includes_thread_context() {
    let temp_dir = create_test_workspace();
    let workspace = temp_dir.path();

    let config = HooksConfig {
        lifecycle: LifecycleHooksConfig {
            subagent_start: vec![HookGroupConfig {
                matcher: None,
                hooks: vec![HookCommandConfig {
                    kind: Default::default(),
                    command: "echo payload".into(),
                    timeout_seconds: None,
                }],
            }],
            ..Default::default()
        },
    };
    let engine = LifecycleHookEngine::new(
        workspace.to_path_buf(),
        &config,
        SessionStartTrigger::Startup,
    )
    .expect("Failed to create hook engine")
    .expect("engine");

    let transcript_path = workspace.join("subagents/agent-1.jsonl");
    let payload = engine
        .build_subagent_start_payload(
            "parent-session",
            "child-thread",
            "worker",
            "@worker",
            true,
            "running",
            Some(transcript_path.as_path()),
        )
        .await
        .expect("payload");

    assert_eq!(payload["parent_session_id"], "parent-session");
    assert_eq!(payload["child_thread_id"], "child-thread");
    assert_eq!(payload["agent_name"], "worker");
    assert_eq!(payload["display_label"], "@worker");
    assert_eq!(payload["background"], true);
    assert_eq!(payload["status"], "running");
    assert_eq!(payload["hook_event_name"], "SubagentStart");
    assert_eq!(
        payload["transcript_path"],
        transcript_path.to_string_lossy().into_owned()
    );
}

#[tokio::test]
async fn test_subagent_start_hook_execution_uses_agent_name_matcher() {
    let temp_dir = create_test_workspace();
    let workspace = temp_dir.path();

    let hooks_config = LifecycleHooksConfig {
        subagent_start: vec![HookGroupConfig {
            matcher: Some("worker".into()),
            hooks: vec![HookCommandConfig {
                kind: Default::default(),
                command: "echo 'Subagent start matched'".into(),
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
    .expect("engine");

    let messages = engine
        .run_subagent_start(
            "parent-session",
            "child-thread",
            "worker",
            "@worker",
            false,
            "running",
            None,
        )
        .await
        .expect("run subagent start hook");

    assert_eq!(messages.len(), 1);
    assert!(messages[0].text.contains("Subagent start matched"));
}

#[tokio::test]
async fn test_subagent_stop_hook_execution_uses_agent_name_matcher() {
    let temp_dir = create_test_workspace();
    let workspace = temp_dir.path();

    let hooks_config = LifecycleHooksConfig {
        subagent_stop: vec![HookGroupConfig {
            matcher: Some("worker".into()),
            hooks: vec![HookCommandConfig {
                kind: Default::default(),
                command: "echo 'Subagent stop matched'".into(),
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
    .expect("engine");

    let messages = engine
        .run_subagent_stop(
            "parent-session",
            "child-thread",
            "worker",
            "@worker",
            true,
            "completed",
            None,
        )
        .await
        .expect("run subagent stop hook");

    assert_eq!(messages.len(), 1);
    assert!(messages[0].text.contains("Subagent stop matched"));
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
async fn test_session_start_json_like_stdout_failure_does_not_become_context() {
    let temp_dir = create_test_workspace();
    let workspace = temp_dir.path();

    let hooks_config = LifecycleHooksConfig {
        session_start: vec![HookGroupConfig {
            matcher: None,
            hooks: vec![HookCommandConfig {
                kind: Default::default(),
                command: r#"printf '{"additional_context":"missing brace"'"#.into(),
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
        .expect("run session start hook");

    assert!(outcome.additional_context.is_empty());
    assert!(
        outcome
            .messages
            .iter()
            .any(|message| { message.text.contains("returned invalid JSON output") })
    );
}

#[tokio::test]
async fn test_user_prompt_submit_block_requires_stderr_feedback() {
    let temp_dir = create_test_workspace();
    let workspace = temp_dir.path();

    let hooks_config = LifecycleHooksConfig {
        user_prompt_submit: vec![HookGroupConfig {
            matcher: None,
            hooks: vec![HookCommandConfig {
                kind: Default::default(),
                command: "exit 2".into(),
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

    assert!(outcome.allow_prompt);
    assert!(outcome.block_reason.is_none());
    assert!(outcome.messages.iter().any(|message| {
        message
            .text
            .contains("exited with code 2 without stderr feedback")
    }));
}

#[tokio::test]
async fn test_user_prompt_submit_json_block_requires_reason() {
    let temp_dir = create_test_workspace();
    let workspace = temp_dir.path();

    let hooks_config = LifecycleHooksConfig {
        user_prompt_submit: vec![HookGroupConfig {
            matcher: None,
            hooks: vec![HookCommandConfig {
                kind: Default::default(),
                command: r#"printf '{"decision":"block"}'"#.into(),
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

    assert!(outcome.allow_prompt);
    assert!(outcome.block_reason.is_none());
    assert!(outcome.messages.iter().any(|message| {
        message
            .text
            .contains("decision=block without a non-empty reason")
    }));
}

mod hook_tooling;
