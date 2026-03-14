use super::*;
use serde_json::json;

#[tokio::test]
async fn test_pre_tool_use_hook_allows_by_default() {
    let temp_dir = create_test_workspace();
    let workspace = temp_dir.path();

    let hooks_config = LifecycleHooksConfig {
        pre_tool_use: vec![HookGroupConfig {
            matcher: Some(".*".into()),
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

    let hooks_config = LifecycleHooksConfig {
        pre_tool_use: vec![HookGroupConfig {
            matcher: Some("TestTool".into()),
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

    assert!(matches!(outcome.decision, PreToolHookDecision::Deny));
    assert!(
        outcome
            .messages
            .iter()
            .any(|m| m.text.contains("Tool blocked"))
    );
}

#[tokio::test]
async fn test_pre_tool_use_hook_exit_code_2_requires_feedback() {
    let temp_dir = create_test_workspace();
    let workspace = temp_dir.path();

    let hooks_config = LifecycleHooksConfig {
        pre_tool_use: vec![HookGroupConfig {
            matcher: Some("TestTool".into()),
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
        .run_pre_tool_use("TestTool", Some(&json!({"param": "value"})))
        .await
        .expect("Failed to run pre-tool use hook");

    assert!(matches!(outcome.decision, PreToolHookDecision::Continue));
    assert!(outcome.messages.iter().any(|m| {
        m.text
            .contains("exited with code 2 without stderr feedback")
    }));
}

#[tokio::test]
async fn test_pre_tool_use_hook_allows_with_json_response() {
    let temp_dir = create_test_workspace();
    let workspace = temp_dir.path();

    let hooks_config = LifecycleHooksConfig {
        pre_tool_use: vec![HookGroupConfig {
            matcher: Some("TestTool".into()),
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

    assert!(matches!(outcome.decision, PreToolHookDecision::Allow));
}

#[tokio::test]
async fn test_post_tool_use_hook_execution() {
    let temp_dir = create_test_workspace();
    let workspace = temp_dir.path();

    let hooks_config = LifecycleHooksConfig {
        post_tool_use: vec![HookGroupConfig {
            matcher: Some("TestTool".into()),
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
async fn test_post_tool_use_json_like_stdout_failure_is_reported() {
    let temp_dir = create_test_workspace();
    let workspace = temp_dir.path();

    let hooks_config = LifecycleHooksConfig {
        post_tool_use: vec![HookGroupConfig {
            matcher: Some("TestTool".into()),
            hooks: vec![HookCommandConfig {
                kind: Default::default(),
                command: r#"printf '{"decision":"block"'"#.into(),
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

    assert!(outcome.block_reason.is_none());
    assert!(
        outcome
            .messages
            .iter()
            .any(|m| { m.text.contains("returned invalid JSON output") })
    );
}

#[tokio::test]
async fn test_post_tool_use_block_requires_reason() {
    let temp_dir = create_test_workspace();
    let workspace = temp_dir.path();

    let hooks_config = LifecycleHooksConfig {
        post_tool_use: vec![HookGroupConfig {
            matcher: Some("TestTool".into()),
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
        .run_post_tool_use(
            "TestTool",
            Some(&json!({"param": "value"})),
            &json!({"result": "success"}),
        )
        .await
        .expect("Failed to run post-tool use hook");

    assert!(outcome.block_reason.is_none());
    assert!(
        outcome
            .messages
            .iter()
            .any(|m| { m.text.contains("decision=block without a non-empty reason") })
    );
}

#[tokio::test]
async fn test_quiet_success_output_suppresses_plain_stdout() {
    let temp_dir = create_test_workspace();
    let workspace = temp_dir.path();

    let hooks_config = LifecycleHooksConfig {
        quiet_success_output: true,
        post_tool_use: vec![HookGroupConfig {
            matcher: Some("TestTool".into()),
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

    assert!(outcome.messages.is_empty());
}

#[tokio::test]
async fn test_quiet_success_output_keeps_structured_context() {
    let temp_dir = create_test_workspace();
    let workspace = temp_dir.path();

    let hooks_config = LifecycleHooksConfig {
        quiet_success_output: true,
        user_prompt_submit: vec![HookGroupConfig {
            matcher: Some("Test prompt".into()),
            hooks: vec![HookCommandConfig {
                kind: Default::default(),
                command: r#"printf '{"systemMessage":"Structured note","hookSpecificOutput":{"hookEventName":"UserPromptSubmit","additionalContext":"Added context"}}\n'"#.into(),
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

    assert!(
        outcome
            .messages
            .iter()
            .any(|msg| msg.text == "Structured note")
    );
    assert_eq!(outcome.additional_context, vec!["Added context"]);
}

#[tokio::test]
async fn test_hook_with_timeout() {
    let temp_dir = create_test_workspace();
    let workspace = temp_dir.path();

    let hooks_config = LifecycleHooksConfig {
        session_start: vec![HookGroupConfig {
            matcher: None,
            hooks: vec![HookCommandConfig {
                kind: Default::default(),
                command: "sleep 2".into(),
                timeout_seconds: Some(1),
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

    let hooks_config = LifecycleHooksConfig {
        pre_tool_use: vec![
            HookGroupConfig {
                matcher: Some("Write".into()),
                hooks: vec![HookCommandConfig {
                    kind: Default::default(),
                    command: "echo 'Write tool matched'".into(),
                    timeout_seconds: None,
                }],
            },
            HookGroupConfig {
                matcher: Some("Bash".into()),
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

    let outcome = engine
        .run_pre_tool_use("Write", Some(&json!({"path": "/test"})))
        .await
        .expect("Failed to run pre-tool use hook for Write");

    assert!(
        outcome
            .messages
            .iter()
            .any(|m| m.text.contains("Write tool matched"))
    );

    let outcome = engine
        .run_pre_tool_use("Bash", Some(&json!({"command": "ls"})))
        .await
        .expect("Failed to run pre-tool use hook for Bash");

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

    let hooks_config = LifecycleHooksConfig {
        user_prompt_submit: vec![HookGroupConfig {
            matcher: Some(".*security.*".into()),
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

    let outcome = engine
        .run_user_prompt_submit("Add a new feature")
        .await
        .expect("Failed to run user prompt submit hook");

    assert!(outcome.additional_context.is_empty());
}
