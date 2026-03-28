use super::*;
use std::path::PathBuf;
use vtcode_core::{
    EditorContextSnapshot, EditorFileContext, EditorLineRange, EditorSelectionContext,
    EditorSelectionRange,
};

#[test]
fn normalize_history_for_request_drops_empty_noop_messages() {
    let manager = ContextManager::new(
        "sys".into(),
        (),
        Arc::new(RwLock::new(HashMap::new())),
        None,
    );
    let history = vec![
        uni::Message::user("hello".to_string()),
        uni::Message::assistant("   ".to_string()),
        uni::Message::assistant("world".to_string()),
    ];

    let normalized = manager.normalize_history_for_request(&history);
    assert_eq!(normalized.len(), 2);
    assert_eq!(normalized[0].content.as_text(), "hello");
    assert_eq!(normalized[1].content.as_text(), "world");
}

#[test]
fn normalize_history_for_request_merges_plain_assistant_text_messages() {
    let manager = ContextManager::new(
        "sys".into(),
        (),
        Arc::new(RwLock::new(HashMap::new())),
        None,
    );
    let history = vec![
        uni::Message::assistant("part one".to_string()),
        uni::Message::assistant("part two".to_string()),
        uni::Message::user("continue".to_string()),
    ];

    let normalized = manager.normalize_history_for_request(&history);
    assert_eq!(normalized.len(), 2);
    assert_eq!(normalized[0].content.as_text(), "part one\npart two");
    assert_eq!(normalized[1].content.as_text(), "continue");
}

#[test]
fn normalize_history_for_request_keeps_different_assistant_phases_separate() {
    let manager = ContextManager::new(
        "sys".into(),
        (),
        Arc::new(RwLock::new(HashMap::new())),
        None,
    );
    let history = vec![
        uni::Message::assistant("working".to_string())
            .with_phase(Some(uni::AssistantPhase::Commentary)),
        uni::Message::assistant("done".to_string())
            .with_phase(Some(uni::AssistantPhase::FinalAnswer)),
    ];

    let normalized = manager.normalize_history_for_request(&history);
    assert_eq!(normalized.len(), 2);
    assert_eq!(normalized[0].phase, Some(uni::AssistantPhase::Commentary));
    assert_eq!(normalized[1].phase, Some(uni::AssistantPhase::FinalAnswer));
}

#[test]
fn normalize_history_for_request_keeps_tool_sequences_intact() {
    let manager = ContextManager::new(
        "sys".into(),
        (),
        Arc::new(RwLock::new(HashMap::new())),
        None,
    );
    let history = vec![
        uni::Message::assistant_with_tools(
            String::new(),
            vec![uni::ToolCall::function(
                "call_1".to_string(),
                "read_file".to_string(),
                "{}".to_string(),
            )],
        ),
        uni::Message::tool_response("call_1".to_string(), "{\"ok\":true}".to_string()),
        uni::Message::assistant("done".to_string()),
    ];

    let normalized = manager.normalize_history_for_request(&history);
    assert_eq!(normalized.len(), 3);
    assert!(normalized[0].tool_calls.is_some());
    assert_eq!(normalized[1].role, uni::MessageRole::Tool);
}

#[test]
fn normalize_history_for_request_inserts_synthetic_outputs_for_missing_calls() {
    let manager = ContextManager::new(
        "sys".into(),
        (),
        Arc::new(RwLock::new(HashMap::new())),
        None,
    );
    let history = vec![uni::Message::assistant_with_tools(
        String::new(),
        vec![uni::ToolCall::function(
            "call_1".to_string(),
            "read_file".to_string(),
            "{}".to_string(),
        )],
    )];

    let normalized = manager.normalize_history_for_request(&history);
    assert_eq!(normalized.len(), 2);
    assert!(normalized[0].tool_calls.is_some());
    assert_eq!(normalized[1].tool_call_id.as_deref(), Some("call_1"));
    assert!(normalized[1].content.as_text().contains("canceled"));
}

#[test]
fn normalize_history_for_request_removes_orphan_outputs() {
    let manager = ContextManager::new(
        "sys".into(),
        (),
        Arc::new(RwLock::new(HashMap::new())),
        None,
    );
    let history = vec![uni::Message::tool_response(
        "orphan_call".to_string(),
        "{\"ok\":true}".to_string(),
    )];

    let normalized = manager.normalize_history_for_request(&history);
    assert!(normalized.is_empty());
}

#[test]
fn test_token_budget_status_thresholds() {
    let manager = ContextManager::new(
        "sys".into(),
        (),
        Arc::new(RwLock::new(HashMap::new())),
        None,
    );

    // Context window of 200K tokens
    let context_size = 200_000;

    // Zero usage should be Normal
    assert_eq!(
        manager.get_token_budget_status(context_size),
        TokenBudgetStatus::Normal
    );
}

#[test]
fn test_token_budget_status_with_zero_context() {
    let manager = ContextManager::new(
        "sys".into(),
        (),
        Arc::new(RwLock::new(HashMap::new())),
        None,
    );

    // Zero context window should return Normal (avoid division by zero)
    assert_eq!(
        manager.get_token_budget_status(0),
        TokenBudgetStatus::Normal
    );
}

#[tokio::test]
async fn build_system_prompt_with_empty_base_prompt_fails() {
    let mut manager = ContextManager::new(
        "".to_string(),
        (),
        Arc::new(RwLock::new(HashMap::new())),
        None,
    );

    let params = SystemPromptParams {
        full_auto: false,
        auto_mode: false,
        plan_mode: false,
        supports_context_awareness: false,
        context_window_size: None,
        prompt_cache_shaping_mode: PromptCacheShapingMode::Disabled,
    };

    let result = manager.build_system_prompt(&[], 0, params).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("empty"));
}

#[tokio::test]
async fn build_system_prompt_includes_active_editor_context_block() {
    let workspace = assert_fs::TempDir::new().expect("workspace");
    let mut manager = ContextManager::new(
        "System prompt".to_string(),
        (),
        Arc::new(RwLock::new(HashMap::new())),
        None,
    );
    manager.set_workspace_root(workspace.path());
    let snapshot = EditorContextSnapshot {
        workspace_root: Some(PathBuf::from(workspace.path())),
        active_file: Some(EditorFileContext {
            path: workspace.path().join("src/main.rs").display().to_string(),
            language_id: Some("rust".to_string()),
            line_range: Some(EditorLineRange { start: 40, end: 64 }),
            dirty: false,
            truncated: false,
            selection: Some(EditorSelectionContext {
                range: EditorSelectionRange {
                    start_line: 48,
                    start_column: 1,
                    end_line: 52,
                    end_column: 8,
                },
                text: Some("fn main() {}\n".to_string()),
            }),
        }),
        visible_editors: vec![EditorFileContext {
            path: workspace.path().join("src/lib.rs").display().to_string(),
            language_id: Some("rust".to_string()),
            line_range: Some(EditorLineRange { start: 1, end: 12 }),
            dirty: false,
            truncated: false,
            selection: None,
        }],
        ..EditorContextSnapshot::default()
    };

    manager.set_editor_context_snapshot(
        Some(snapshot),
        Some(&vtcode_config::IdeContextConfig::default()),
    );
    let prompt = manager
        .build_system_prompt(
            &[],
            0,
            SystemPromptParams {
                full_auto: false,
                auto_mode: false,
                plan_mode: false,
                supports_context_awareness: false,
                context_window_size: None,
                prompt_cache_shaping_mode: PromptCacheShapingMode::Disabled,
            },
        )
        .await
        .expect("system prompt");

    assert!(prompt.contains("## Active Editor Context"));
    assert!(prompt.contains("- Active file: src/main.rs"));
    assert!(prompt.contains("- Selection: 48:1-52:8"));
    assert!(prompt.contains("- Open files:"));
    assert!(prompt.contains("  - src/lib.rs"));
}

#[tokio::test]
async fn build_system_prompt_skips_disallowed_provider_family() {
    let workspace = assert_fs::TempDir::new().expect("workspace");
    let mut manager = ContextManager::new(
        "System prompt".to_string(),
        (),
        Arc::new(RwLock::new(HashMap::new())),
        None,
    );
    manager.set_workspace_root(workspace.path());
    let snapshot = EditorContextSnapshot {
        provider_family: vtcode_config::IdeContextProviderFamily::Zed,
        workspace_root: Some(PathBuf::from(workspace.path())),
        active_file: Some(EditorFileContext {
            path: workspace.path().join("src/main.rs").display().to_string(),
            language_id: Some("rust".to_string()),
            line_range: None,
            dirty: false,
            truncated: false,
            selection: None,
        }),
        ..EditorContextSnapshot::default()
    };
    let config = vtcode_config::IdeContextConfig {
        provider_mode: vtcode_config::IdeContextProviderMode::VscodeCompatible,
        ..vtcode_config::IdeContextConfig::default()
    };

    manager.set_editor_context_snapshot(Some(snapshot), Some(&config));
    let prompt = manager
        .build_system_prompt(
            &[],
            0,
            SystemPromptParams {
                full_auto: false,
                auto_mode: false,
                plan_mode: false,
                supports_context_awareness: false,
                context_window_size: None,
                prompt_cache_shaping_mode: PromptCacheShapingMode::Disabled,
            },
        )
        .await
        .expect("system prompt");

    assert!(!prompt.contains("## Active Editor Context"));
}

#[tokio::test]
async fn build_system_prompt_respects_session_local_ide_toggle() {
    let workspace = assert_fs::TempDir::new().expect("workspace");
    let mut manager = ContextManager::new(
        "System prompt".to_string(),
        (),
        Arc::new(RwLock::new(HashMap::new())),
        None,
    );
    manager.set_workspace_root(workspace.path());
    let snapshot = EditorContextSnapshot {
        workspace_root: Some(PathBuf::from(workspace.path())),
        active_file: Some(EditorFileContext {
            path: workspace.path().join("src/main.rs").display().to_string(),
            language_id: Some("rust".to_string()),
            line_range: Some(EditorLineRange { start: 8, end: 16 }),
            dirty: false,
            truncated: false,
            selection: None,
        }),
        ..EditorContextSnapshot::default()
    };

    manager.set_editor_context_snapshot(
        Some(snapshot),
        Some(&vtcode_config::IdeContextConfig::default()),
    );

    let enabled_prompt = manager
        .build_system_prompt(
            &[],
            0,
            SystemPromptParams {
                full_auto: false,
                auto_mode: false,
                plan_mode: false,
                supports_context_awareness: false,
                context_window_size: None,
                prompt_cache_shaping_mode: PromptCacheShapingMode::Disabled,
            },
        )
        .await
        .expect("enabled prompt");
    assert!(enabled_prompt.contains("## Active Editor Context"));

    assert!(!manager.toggle_session_ide_context());
    let disabled_prompt = manager
        .build_system_prompt(
            &[],
            0,
            SystemPromptParams {
                full_auto: false,
                auto_mode: false,
                plan_mode: false,
                supports_context_awareness: false,
                context_window_size: None,
                prompt_cache_shaping_mode: PromptCacheShapingMode::Disabled,
            },
        )
        .await
        .expect("disabled prompt");
    assert!(!disabled_prompt.contains("## Active Editor Context"));

    assert!(manager.toggle_session_ide_context());
    let reenabled_prompt = manager
        .build_system_prompt(
            &[],
            0,
            SystemPromptParams {
                full_auto: false,
                auto_mode: false,
                plan_mode: false,
                supports_context_awareness: false,
                context_window_size: None,
                prompt_cache_shaping_mode: PromptCacheShapingMode::Disabled,
            },
        )
        .await
        .expect("reenabled prompt");
    assert!(reenabled_prompt.contains("## Active Editor Context"));
}

#[test]
fn test_token_budget_status_warning_threshold() {
    let mut manager = ContextManager::new(
        "sys".into(),
        (),
        Arc::new(RwLock::new(HashMap::new())),
        None,
    );

    // Set token usage to 70% (140000/200000)
    manager.cached_stats.total_token_usage = 140_000;

    assert_eq!(
        manager.get_token_budget_status(200_000),
        TokenBudgetStatus::Warning
    );
    assert_eq!(
        manager.get_token_budget_guidance(200_000),
        "WARNING: Update progress docs to preserve context."
    );
}

#[test]
fn test_token_budget_status_high_threshold() {
    let mut manager = ContextManager::new(
        "sys".into(),
        (),
        Arc::new(RwLock::new(HashMap::new())),
        None,
    );

    // Set token usage to 90% (180000/200000)
    manager.cached_stats.total_token_usage = 180_000;

    assert_eq!(
        manager.get_token_budget_status(200_000),
        TokenBudgetStatus::High
    );
    assert_eq!(
        manager.get_token_budget_guidance(200_000),
        "HIGH: Summarize key findings and prepare a handoff."
    );
}

#[test]
fn test_token_budget_status_critical_threshold() {
    let mut manager = ContextManager::new(
        "sys".into(),
        (),
        Arc::new(RwLock::new(HashMap::new())),
        None,
    );

    // Set token usage to 95% (190000/200000)
    manager.cached_stats.total_token_usage = 190_000;

    assert_eq!(
        manager.get_token_budget_status(200_000),
        TokenBudgetStatus::Critical
    );
    assert_eq!(
        manager.get_token_budget_guidance(200_000),
        "CRITICAL: Update artifacts and consider a new session."
    );
}

#[test]
fn test_token_budget_status_normal() {
    let mut manager = ContextManager::new(
        "sys".into(),
        (),
        Arc::new(RwLock::new(HashMap::new())),
        None,
    );

    // Set token usage to 50% (100000/200000)
    manager.cached_stats.total_token_usage = 100_000;

    assert_eq!(
        manager.get_token_budget_status(200_000),
        TokenBudgetStatus::Normal
    );
    assert_eq!(manager.get_token_budget_guidance(200_000), "");
}

#[test]
fn test_token_budget_status_and_guidance_together() {
    let mut manager = ContextManager::new(
        "sys".into(),
        (),
        Arc::new(RwLock::new(HashMap::new())),
        None,
    );

    // Test critical threshold
    manager.cached_stats.total_token_usage = 195_000;
    let (status, guidance) = manager.get_token_budget_status_and_guidance(200_000);
    assert_eq!(status, TokenBudgetStatus::Critical);
    assert!(guidance.contains("CRITICAL"));

    // Test high threshold
    manager.cached_stats.total_token_usage = 185_000;
    let (status, guidance) = manager.get_token_budget_status_and_guidance(200_000);
    assert_eq!(status, TokenBudgetStatus::High);
    assert!(guidance.contains("HIGH"));

    // Test warning threshold
    manager.cached_stats.total_token_usage = 145_000;
    let (status, guidance) = manager.get_token_budget_status_and_guidance(200_000);
    assert_eq!(status, TokenBudgetStatus::Warning);
    assert!(guidance.contains("WARNING"));

    // Test normal
    manager.cached_stats.total_token_usage = 50_000;
    let (status, guidance) = manager.get_token_budget_status_and_guidance(200_000);
    assert_eq!(status, TokenBudgetStatus::Normal);
    assert!(guidance.is_empty());
}

#[test]
fn test_update_token_usage_prefers_prompt_pressure() {
    let mut manager = ContextManager::new(
        "sys".into(),
        (),
        Arc::new(RwLock::new(HashMap::new())),
        None,
    );

    // Initial state
    assert_eq!(manager.current_token_usage(), 0);

    // Update with first response: prompt-side pressure becomes authoritative.
    manager.update_token_usage(&Some(uni::Usage {
        prompt_tokens: 1000,
        completion_tokens: 500,
        total_tokens: 1500,
        cached_prompt_tokens: None,
        cache_creation_tokens: None,
        cache_read_tokens: None,
    }));
    assert_eq!(manager.current_token_usage(), 1000);

    // Update with second response: usage tracks latest prompt pressure, not cumulative output.
    manager.update_token_usage(&Some(uni::Usage {
        prompt_tokens: 2500,
        completion_tokens: 800,
        total_tokens: 3300,
        cached_prompt_tokens: None,
        cache_creation_tokens: None,
        cache_read_tokens: None,
    }));
    assert_eq!(manager.current_token_usage(), 2500);
}

#[test]
fn test_update_token_usage_falls_back_when_prompt_missing() {
    let mut manager = ContextManager::new(
        "sys".into(),
        (),
        Arc::new(RwLock::new(HashMap::new())),
        None,
    );

    manager.update_token_usage(&Some(uni::Usage {
        prompt_tokens: 0,
        completion_tokens: 800,
        total_tokens: 3300,
        cached_prompt_tokens: None,
        cache_creation_tokens: None,
        cache_read_tokens: None,
    }));

    // Fallback estimate = total - completion.
    assert_eq!(manager.current_token_usage(), 2500);
}
