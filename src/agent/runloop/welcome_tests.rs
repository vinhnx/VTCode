use std::collections::BTreeMap;
use std::fs;

use tempfile::tempdir;
use vtcode_core::config::core::PromptCachingConfig;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::models::Provider;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::config::types::{ModelSelectionSource, ReasoningEffortLevel, UiSurfacePreference};
use vtcode_core::core::agent::snapshots::{DEFAULT_CHECKPOINTS_ENABLED, DEFAULT_MAX_AGE_DAYS, DEFAULT_MAX_SNAPSHOTS};

use super::welcome::prepare_session_bootstrap;

#[tokio::test]
async fn test_prepare_session_bootstrap_builds_sections() {
    let tmp = tempdir().expect("Failed to create temp directory");
    fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\ndescription = \"Demo project\"\n",
    )
    .expect("Failed to write Cargo.toml");
    fs::create_dir_all(tmp.path().join("src")).expect("Failed to create src directory");
    fs::write(tmp.path().join("src/main.rs"), "fn main() {}\n").expect("Failed to write main.rs");
    fs::write(
        tmp.path().join("AGENTS.md"),
        "- Follow workspace guidelines\n- Prefer 4-space indentation\n- Run cargo fmt before commits\n",
    )
    .expect("Failed to write AGENTS.md");
    fs::write(tmp.path().join("README.md"), "Demo workspace\n").expect("Failed to write README.md");

    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.agent.onboarding.include_language_summary = false;
    vt_cfg.agent.onboarding.guideline_highlight_limit = 2;
    vt_cfg.agent.onboarding.include_usage_tips_in_welcome = true;
    vt_cfg.agent.onboarding.include_recommended_actions_in_welcome = true;
    vt_cfg.agent.onboarding.usage_tips = vec!["Tip one".into()];
    vt_cfg.agent.onboarding.recommended_actions = vec!["Do something".into()];
    vt_cfg.agent.onboarding.chat_placeholder = Some("Type your plan".into());

    let runtime_cfg = CoreAgentConfig {
        model: vtcode_core::config::constants::models::google::GEMINI_3_FLASH_PREVIEW.to_string(),
        api_key: "test".to_string(),
        provider: "gemini".to_string(),
        api_key_env: Provider::Gemini.default_api_key_env().to_string(),
        workspace: tmp.path().to_path_buf(),
        verbose: false,
        theme: vtcode_tui_runtime::ui::theme::DEFAULT_THEME_ID.to_string(),
        reasoning_effort: ReasoningEffortLevel::default(),
        ui_surface: UiSurfacePreference::default(),
        prompt_cache: PromptCachingConfig::default(),
        model_source: ModelSelectionSource::WorkspaceConfig,
        custom_api_keys: BTreeMap::new(),
        checkpointing_enabled: DEFAULT_CHECKPOINTS_ENABLED,
        checkpointing_storage_dir: None,
        checkpointing_max_snapshots: DEFAULT_MAX_SNAPSHOTS,
        checkpointing_max_age_days: Some(DEFAULT_MAX_AGE_DAYS),
        quiet: false,
        max_conversation_turns: 1000,
        model_behavior: None,
        openai_chatgpt_auth: None,
    };

    let bootstrap = prepare_session_bootstrap(&runtime_cfg, Some(&vt_cfg), None).await;

    assert_eq!(bootstrap.header_highlights.len(), 4);

    let slash_commands = &bootstrap.header_highlights[0];
    assert!(slash_commands.title.is_empty());
    assert!(slash_commands.lines.iter().any(|line| line.contains("/{command}")));
    assert!(slash_commands.lines.iter().any(|line| line.contains("/help")));
    assert!(slash_commands.lines.iter().any(|line| line.contains("Enter")));
    assert!(slash_commands.lines.iter().any(|line| line.contains("Escape")));

    let usage_tips = &bootstrap.header_highlights[1];
    assert_eq!(usage_tips.title, "Usage Tips");
    assert!(usage_tips.lines.iter().any(|line| line.contains("Tip one")));

    let recommended_actions = &bootstrap.header_highlights[2];
    assert_eq!(recommended_actions.title, "Suggested Next Actions");
    assert!(recommended_actions.lines.iter().any(|line| line.contains("Do something")));

    let prompt = bootstrap.prompt_addendum.expect("prompt addendum");
    assert!(prompt.contains("## SESSION CONTEXT"));
    assert!(prompt.contains("Workflow Hint"));
    assert!(prompt.contains("@file"));
    assert!(prompt.contains("Planning workflow"));
    assert!(prompt.contains("task_tracker"));
    assert!(prompt.contains("Suggested Next Actions"));

    assert_eq!(bootstrap.placeholder.as_deref(), Some("Type your plan"));
}

#[tokio::test]
async fn test_welcome_hides_optional_sections_by_default() {
    let tmp = tempdir().expect("Failed to create temp directory");
    fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\ndescription = \"Demo project\"\n",
    )
    .expect("Failed to write Cargo.toml");
    fs::write(tmp.path().join("README.md"), "Demo workspace\n").expect("Failed to write README.md");

    let runtime_cfg = CoreAgentConfig {
        model: vtcode_core::config::constants::models::google::GEMINI_3_FLASH_PREVIEW.to_string(),
        api_key: "test".to_string(),
        provider: "gemini".to_string(),
        api_key_env: Provider::Gemini.default_api_key_env().to_string(),
        workspace: tmp.path().to_path_buf(),
        verbose: false,
        theme: vtcode_tui_runtime::ui::theme::DEFAULT_THEME_ID.to_string(),
        reasoning_effort: ReasoningEffortLevel::default(),
        ui_surface: UiSurfacePreference::default(),
        prompt_cache: PromptCachingConfig::default(),
        model_source: ModelSelectionSource::WorkspaceConfig,
        custom_api_keys: BTreeMap::new(),
        checkpointing_enabled: DEFAULT_CHECKPOINTS_ENABLED,
        checkpointing_storage_dir: None,
        checkpointing_max_snapshots: DEFAULT_MAX_SNAPSHOTS,
        checkpointing_max_age_days: Some(DEFAULT_MAX_AGE_DAYS),
        quiet: false,
        max_conversation_turns: 1000,
        model_behavior: None,
        openai_chatgpt_auth: None,
    };

    let vt_cfg = VTCodeConfig::default();
    let bootstrap = prepare_session_bootstrap(&runtime_cfg, Some(&vt_cfg), None).await;

    assert_eq!(bootstrap.header_highlights.len(), 2);
    let slash_commands = &bootstrap.header_highlights[0];
    assert!(slash_commands.title.is_empty());
    assert!(slash_commands.lines.iter().any(|line| line.contains("/{command}")));
    assert!(slash_commands.lines.iter().any(|line| line.contains("Enter")));
    assert!(slash_commands.lines.iter().any(|line| line.contains("Escape")));
}

#[tokio::test]
async fn test_prepare_session_bootstrap_hides_placeholder_when_planning_disabled() {
    let tmp = tempdir().expect("Failed to create temp directory");
    fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\ndescription = \"Demo\"\n",
    )
    .expect("Failed to write Cargo.toml");
    fs::create_dir_all(tmp.path().join("src")).expect("Failed to create src directory");
    fs::write(tmp.path().join("src/lib.rs"), "pub fn demo() {}\n").expect("Failed to write lib.rs");

    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.agent.todo_planning_mode = false;
    vt_cfg.agent.onboarding.chat_placeholder = Some("Type your plan".into());

    let runtime_cfg = CoreAgentConfig {
        model: vtcode_core::config::constants::models::google::GEMINI_3_FLASH_PREVIEW.to_string(),
        api_key: "test".to_string(),
        provider: "gemini".to_string(),
        api_key_env: Provider::Gemini.default_api_key_env().to_string(),
        workspace: tmp.path().to_path_buf(),
        verbose: false,
        theme: vtcode_tui_runtime::ui::theme::DEFAULT_THEME_ID.to_string(),
        reasoning_effort: ReasoningEffortLevel::default(),
        ui_surface: UiSurfacePreference::default(),
        prompt_cache: PromptCachingConfig::default(),
        model_source: ModelSelectionSource::WorkspaceConfig,
        custom_api_keys: BTreeMap::new(),
        checkpointing_enabled: DEFAULT_CHECKPOINTS_ENABLED,
        checkpointing_storage_dir: None,
        checkpointing_max_snapshots: DEFAULT_MAX_SNAPSHOTS,
        checkpointing_max_age_days: Some(DEFAULT_MAX_AGE_DAYS),
        quiet: false,
        max_conversation_turns: 1000,
        model_behavior: None,
        openai_chatgpt_auth: None,
    };

    let bootstrap = prepare_session_bootstrap(&runtime_cfg, Some(&vt_cfg), None).await;
    assert!(bootstrap.placeholder.is_none());

    if let Some(prompt) = bootstrap.prompt_addendum.as_deref() {
        assert!(!prompt.contains("Workflow Hint"));
        assert!(!prompt.contains("task_tracker"));
    }
}

/// The shell paint path and the full bootstrap must derive the same
/// placeholder so the ready re-drive never visibly swaps it.
#[test]
fn configured_chat_placeholder_matches_bootstrap_derivation() {
    // Custom placeholder surfaces on both paths.
    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.agent.onboarding.chat_placeholder = Some("  Type your plan  ".into());
    let configured = super::welcome::configured_chat_placeholder(Some(&vt_cfg));
    assert_eq!(configured.as_deref(), Some("Type your plan"));

    // Default config: no custom placeholder -> None on both paths (the shell
    // falls back to CHAT_INPUT_PLACEHOLDER_BOOTSTRAP, which the bootstrap
    // also leaves unset).
    assert!(super::welcome::configured_chat_placeholder(Some(&VTCodeConfig::default())).is_none());
    assert!(super::welcome::configured_chat_placeholder(None).is_none());
}

#[test]
fn configured_chat_placeholder_respects_gates() {
    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.agent.onboarding.chat_placeholder = Some("Type your plan".into());

    // Planning disabled hides the custom placeholder.
    vt_cfg.agent.todo_planning_mode = false;
    assert!(super::welcome::configured_chat_placeholder(Some(&vt_cfg)).is_none());
    vt_cfg.agent.todo_planning_mode = true;

    // Onboarding disabled hides it too.
    vt_cfg.agent.onboarding.enabled = false;
    assert!(super::welcome::configured_chat_placeholder(Some(&vt_cfg)).is_none());
    vt_cfg.agent.onboarding.enabled = true;

    // Whitespace-only values are treated as unset.
    vt_cfg.agent.onboarding.chat_placeholder = Some("   ".into());
    assert!(super::welcome::configured_chat_placeholder(Some(&vt_cfg)).is_none());
}

#[tokio::test]
async fn test_prepare_session_bootstrap_does_not_surface_acp_trust_in_terminal_session() {
    let tmp = tempdir().expect("Failed to create temp directory");
    fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\ndescription = \"Demo\"\n",
    )
    .expect("Failed to write Cargo.toml");

    let runtime_cfg = CoreAgentConfig {
        model: vtcode_core::config::constants::models::google::GEMINI_3_FLASH_PREVIEW.to_string(),
        api_key: "test".to_string(),
        provider: "gemini".to_string(),
        api_key_env: Provider::Gemini.default_api_key_env().to_string(),
        workspace: tmp.path().to_path_buf(),
        verbose: false,
        theme: vtcode_tui_runtime::ui::theme::DEFAULT_THEME_ID.to_string(),
        reasoning_effort: ReasoningEffortLevel::default(),
        ui_surface: UiSurfacePreference::default(),
        prompt_cache: PromptCachingConfig::default(),
        model_source: ModelSelectionSource::WorkspaceConfig,
        custom_api_keys: BTreeMap::new(),
        checkpointing_enabled: DEFAULT_CHECKPOINTS_ENABLED,
        checkpointing_storage_dir: None,
        checkpointing_max_snapshots: DEFAULT_MAX_SNAPSHOTS,
        checkpointing_max_age_days: Some(DEFAULT_MAX_AGE_DAYS),
        quiet: false,
        max_conversation_turns: 1000,
        model_behavior: None,
        openai_chatgpt_auth: None,
    };

    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.acp.enabled = true;
    vt_cfg.acp.zed.enabled = true;
    vt_cfg.acp.zed.workspace_trust = vtcode_core::config::AgentClientProtocolZedWorkspaceTrustMode::FullAuto;

    let bootstrap = prepare_session_bootstrap(&runtime_cfg, Some(&vt_cfg), None).await;

    assert!(bootstrap.acp_workspace_trust.is_none());
}
