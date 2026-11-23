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

fn with_consent_harness<R>(
    workspace_toml: Option<&str>,
    with_legacy_cache: bool,
    action: impl FnOnce(&std::path::Path) -> R,
) -> R {
    let root = tempdir().expect("hermetic root");
    let data = root.path().join("data");
    let codex = root.path().join("codex");
    let workspace = root.path().join("workspace");
    for dir in [&data, &codex, &workspace] {
        fs::create_dir_all(dir).expect("hermetic dir");
    }
    if let Some(content) = workspace_toml {
        fs::write(workspace.join("vtcode.toml"), content).expect("workspace config");
    }
    if with_legacy_cache {
        vtcode_core::skills::system::install_system_skills(&codex).expect("legacy cache");
    }
    let guard = vtcode_commons::env_lock::lock();
    let previous_data = std::env::var_os("VTCODE_DATA");
    let previous_codex = std::env::var_os("CODEX_HOME");
    guard.set_var("VTCODE_DATA", &data);
    guard.set_var("CODEX_HOME", &codex);

    let static_paths = vtcode_commons::reference::StaticWorkspacePaths::new(&workspace, workspace.join(".vtcode"));
    let provider = vtcode_config::defaults::WorkspacePathsDefaults::new(std::sync::Arc::new(static_paths))
        .with_home_paths(Vec::new());
    let previous_provider = vtcode_config::defaults::install_config_defaults_provider(std::sync::Arc::new(provider));
    let outcome = action(workspace.as_path());
    let _restored = vtcode_config::defaults::install_config_defaults_provider(previous_provider);
    guard.restore_var("VTCODE_DATA", previous_data.as_deref());
    guard.restore_var("CODEX_HOME", previous_codex.as_deref());
    outcome
}

#[test]
#[serial_test::serial]
fn consent_hint_shows_only_while_unasked_without_cache() {
    use super::welcome::bundled_skills_consent_highlight;

    let shown = with_consent_harness(None, false, bundled_skills_consent_highlight);
    let highlight = shown.expect("unasked setup must show the hint");
    assert_eq!(highlight.title, "Bundled Skills");
    assert!(
        highlight.lines.iter().any(|line| line.contains("/config")),
        "hint must name the recovery path: {lines:?}",
        lines = highlight.lines
    );

    for answered in [
        Some("[skills.bundled]\nenabled = true\n"),
        Some("[skills.bundled]\nenabled = false\n"),
    ] {
        let hidden = with_consent_harness(answered, false, bundled_skills_consent_highlight);
        assert!(hidden.is_none(), "answered setup must stay silent");
    }

    let grandfathered = with_consent_harness(None, true, bundled_skills_consent_highlight);
    assert!(grandfathered.is_none(), "grandfathered setup must stay silent");
}
