use super::*;
use crate::agent::runloop::unified::state::CtrlCState;
use anyhow::Result;
use std::fs;
use std::sync::Arc;
use tempfile::tempdir;
use tokio::sync::{Notify, mpsc};
use vtcode_config::OpenAIServiceTier;
use vtcode_config::core::CustomProviderConfig;
use vtcode_config::loader::VTCodeConfig;
use vtcode_core::config::models::ModelId;
use vtcode_core::utils::ansi::AnsiRenderer;
use vtcode_tui::app::{InlineHandle, InlineSession};

use self::options::{find_option_index, option_indexes_for_provider};

fn has_model(options: &[ModelOption], model: ModelId) -> bool {
    let id = model.as_str();
    let provider = model.provider();
    options
        .iter()
        .any(|option| option.id == id && option.provider == provider)
}

#[test]
fn model_picker_lists_new_anthropic_models() {
    let options = MODEL_OPTIONS.as_slice();
    assert!(has_model(options, ModelId::ClaudeOpus47));
    assert!(has_model(options, ModelId::ClaudeSonnet46));
    assert!(has_model(options, ModelId::ClaudeHaiku45));

    // OpenRouter variants
    assert!(has_model(
        options,
        ModelId::OpenRouterAnthropicClaudeSonnet46
    ));
    assert!(has_model(
        options,
        ModelId::OpenRouterAnthropicClaudeSonnet45
    ));
}

#[test]
fn model_picker_lists_new_nvidia_models() {
    let options = MODEL_OPTIONS.as_slice();
    assert!(has_model(
        options,
        ModelId::OpenRouterNvidiaNemotron3Super120bA12bFree
    ));
}

#[test]
fn model_picker_lists_new_zai_models() {
    let options = MODEL_OPTIONS.as_slice();
    assert!(has_model(options, ModelId::ZaiGlm5));
}

#[test]
fn model_picker_lists_new_ollama_cloud_models() {
    let options = MODEL_OPTIONS.as_slice();
    assert!(has_model(options, ModelId::OllamaGptOss20b));
    assert!(has_model(options, ModelId::OllamaGptOss120bCloud));
    assert!(has_model(options, ModelId::OllamaQwen3CoderNext));
    assert!(has_model(options, ModelId::OllamaDeepseekV32Cloud));
    assert!(has_model(options, ModelId::OllamaQwen3Next80bCloud));
    assert!(has_model(options, ModelId::OllamaGlm5Cloud));
    assert!(has_model(options, ModelId::OllamaMinimaxM25Cloud));
    assert!(has_model(options, ModelId::OllamaGemini3FlashPreviewCloud));
    assert!(has_model(options, ModelId::MinimaxM25));
}

#[test]
fn model_picker_lists_new_gemini_models() {
    let options = MODEL_OPTIONS.as_slice();
    assert!(has_model(options, ModelId::Gemini31ProPreview));
}

#[test]
fn model_picker_lists_new_openai_codex_models() {
    let options = MODEL_OPTIONS.as_slice();
    assert!(has_model(options, ModelId::GPT53Codex));
    assert!(has_model(options, ModelId::GPT52Codex));
    assert!(has_model(options, ModelId::GPT51Codex));
    assert!(has_model(options, ModelId::GPT51CodexMax));
    assert!(has_model(options, ModelId::GPT5Codex));
}

#[test]
fn subagent_model_shortcuts_include_expected_aliases() {
    let shortcuts = super::subagent_model_shortcuts()
        .iter()
        .map(|(shortcut, _)| *shortcut)
        .collect::<Vec<_>>();

    assert_eq!(
        shortcuts,
        vec!["inherit", "small", "haiku", "sonnet", "opus"]
    );
}

#[test]
fn subagent_dynamic_model_filter_keeps_only_parseable_model_ids() {
    let registry = DynamicModelRegistry {
        entries: vec![
            selection::selection_from_dynamic(Provider::OpenAI, "gpt-5.4", "gpt-5.4", None, None),
            selection::selection_from_dynamic(
                Provider::Ollama,
                "custom-local-model",
                "custom-local-model",
                None,
                None,
            ),
        ],
        ..Default::default()
    };

    let indexes = super::parseable_subagent_dynamic_indexes(&registry);
    assert_eq!(indexes, vec![0]);
}

#[test]
fn subagent_reasoning_levels_only_enable_xhigh_when_supported() {
    let supported = super::subagent_reasoning_levels("gpt-5.2", true);
    assert!(supported.contains(&ReasoningEffortLevel::XHigh));
    assert!(!supported.contains(&ReasoningEffortLevel::Max));

    let opus = super::subagent_reasoning_levels("claude-opus-4-7", true);
    assert!(opus.contains(&ReasoningEffortLevel::XHigh));
    assert!(opus.contains(&ReasoningEffortLevel::Max));

    let shortcut = super::subagent_reasoning_levels("haiku", true);
    assert!(!shortcut.contains(&ReasoningEffortLevel::XHigh));
    assert!(!shortcut.contains(&ReasoningEffortLevel::Max));

    let unsupported = super::subagent_reasoning_levels("gpt-4.1", true);
    assert!(!unsupported.contains(&ReasoningEffortLevel::XHigh));
    assert!(!unsupported.contains(&ReasoningEffortLevel::Max));
}

#[test]
fn subagent_reasoning_normalization_drops_invalid_or_unsupported_values() {
    let shortcut = super::SubagentModelTarget::Shortcut {
        model: "Haiku".to_string(),
    };
    assert_eq!(
        super::normalized_subagent_reasoning(&shortcut, Some("high")),
        Some("high".to_string())
    );
    assert_eq!(
        super::normalized_subagent_reasoning(&shortcut, Some("xhigh")),
        None
    );
    assert_eq!(
        super::normalized_subagent_reasoning(&shortcut, Some("max")),
        None
    );
    assert_eq!(
        super::normalized_subagent_reasoning(&shortcut, Some("bogus")),
        None
    );

    let concrete = super::SubagentModelTarget::Concrete(selection::selection_from_dynamic(
        Provider::OpenAI,
        "gpt-5.2",
        "gpt-5.2",
        None,
        None,
    ));
    assert_eq!(
        super::normalized_subagent_reasoning(&concrete, Some("xhigh")),
        Some("xhigh".to_string())
    );

    let opus = super::SubagentModelTarget::Concrete(selection::selection_from_dynamic(
        Provider::Anthropic,
        "claude-opus-4-7",
        "claude-opus-4-7",
        None,
        None,
    ));
    assert_eq!(
        super::normalized_subagent_reasoning(&opus, Some("max")),
        Some("max".to_string())
    );
}

#[test]
fn preferred_subagent_model_selection_canonicalizes_shortcuts() {
    let registry = DynamicModelRegistry::default();
    let selection = super::preferred_subagent_model_selection(&registry, "HaIkU");

    assert_eq!(
        selection,
        Some(InlineListSelection::ConfigAction(
            "subagent-model:shortcut:haiku".to_string()
        ))
    );
}

#[test]
fn model_search_value_includes_provider_model_aliases() {
    let extra_terms = vec![
        "reasoning".to_string(),
        "tools".to_string(),
        "image".to_string(),
    ];
    let value = super::rendering::model_search_value(
        Provider::OpenAI,
        "GPT-5.2",
        "gpt-5.2",
        Some("Latest frontier model"),
        &extra_terms,
    )
    .to_ascii_lowercase();

    assert!(value.contains("openai gpt-5.2"));
    assert!(value.contains("openai/gpt-5.2"));
    assert!(value.contains("reasoning"));
    assert!(value.contains("tools"));
    assert!(value.contains("image"));
}

#[test]
fn parse_model_selection_uses_custom_provider_display_and_env_key() {
    let mut cfg = VTCodeConfig::default();
    cfg.custom_providers.push(CustomProviderConfig {
        name: "mycorp".to_string(),
        display_name: "MyCorporateName".to_string(),
        base_url: "https://llm.corp.example/v1".to_string(),
        api_key_env: "MYCORP_API_KEY".to_string(),
        auth: None,
        model: "gpt-5-mini".to_string(),
    });

    let detail = selection::parse_model_selection(&MODEL_OPTIONS, "mycorp gpt-5-mini", Some(&cfg))
        .expect("custom provider should parse");

    assert_eq!(detail.provider_key, "mycorp");
    assert_eq!(detail.provider_label, "MyCorporateName");
    assert_eq!(detail.env_key, "MYCORP_API_KEY");
    assert_eq!(detail.provider_enum, None);
}

#[test]
fn parse_model_selection_marks_command_auth_custom_provider_as_keyless() {
    let mut cfg = VTCodeConfig::default();
    cfg.custom_providers.push(CustomProviderConfig {
        name: "mycorp".to_string(),
        display_name: "MyCorporateName".to_string(),
        base_url: "https://llm.corp.example/v1".to_string(),
        api_key_env: String::new(),
        auth: Some(vtcode_config::core::CustomProviderCommandAuthConfig {
            command: "print-token".to_string(),
            args: Vec::new(),
            cwd: None,
            timeout_ms: 1_000,
            refresh_interval_ms: 60_000,
        }),
        model: "gpt-5-mini".to_string(),
    });

    let detail = selection::parse_model_selection(&MODEL_OPTIONS, "mycorp gpt-5-mini", Some(&cfg))
        .expect("custom provider should parse");

    assert!(!detail.requires_api_key);
    assert!(detail.env_key.is_empty());
}

#[test]
fn static_model_subtitle_formats_current_capabilities() {
    let option = MODEL_OPTIONS
        .iter()
        .find(|option| option.model == ModelId::GPT54)
        .expect("gpt-5.4 option should exist");

    let subtitle = super::rendering::static_model_subtitle(option, "openai", "gpt-5.4");

    assert_eq!(
        subtitle,
        "gpt-5.4 • Current • Context: 1M • Reasoning • Tools • Input: text, image"
    );
}

#[test]
fn static_model_search_terms_include_modalities_and_tool_state() {
    let terms =
        super::rendering::static_model_search_terms(ModelId::OpenRouterOpenAIGpt5Chat, false);

    assert!(terms.iter().any(|term| term == "no tools"));
    assert!(terms.iter().any(|term| term == "no-tools"));
    assert!(terms.iter().any(|term| term == "tool_call disabled"));
    assert!(terms.iter().any(|term| term == "modalities"));
    assert!(terms.iter().any(|term| term == "file"));
    assert!(terms.iter().any(|term| term == "image"));
    assert!(terms.iter().any(|term| term == "text"));
}

#[test]
fn dynamic_model_subtitle_stays_conservative_for_unknown_local_models() {
    let subtitle = super::rendering::dynamic_model_subtitle(
        Provider::Ollama,
        "custom-local-model",
        false,
        "ollama",
        "custom-local-model",
    );

    assert_eq!(subtitle, "custom-local-model • Current • Local");
}

#[test]
fn current_model_line_shows_effective_anthropic_context_window() {
    let line = super::rendering::current_model_line("anthropic", "claude-sonnet-4-6");
    assert_eq!(line, "Current: anthropic / claude-sonnet-4-6 • Context: 1M");
}

#[test]
fn step_one_header_lines_explain_codex_runtime_configuration() {
    let lines = super::rendering::step_one_header_lines("codex", "gpt-5.3-codex");

    assert!(
        lines.iter().any(|line| line.contains("/config codex")),
        "expected Codex runtime note in picker header"
    );
    assert!(
        lines.iter().any(|line| line.contains("/model")),
        "expected note to clarify /model scope"
    );
}

fn base_picker_state(current_provider: &str, current_model: &str) -> ModelPickerState {
    ModelPickerState {
        options: MODEL_OPTIONS.as_slice(),
        step: PickerStep::AwaitModel,
        inline_enabled: true,
        vt_cfg: None,
        current_reasoning: ReasoningEffortLevel::Medium,
        current_service_tier: None,
        current_provider: current_provider.to_string(),
        current_model: current_model.to_string(),
        selection: None,
        custom_providers: Vec::new(),
        selected_reasoning: None,
        selected_service_tier: None,
        pending_api_key: None,
        workspace: None,
        ctrl_c_state: None,
        ctrl_c_notify: None,
        dynamic_models: DynamicModelRegistry::default(),
        plain_mode_active: false,
    }
}

fn session_with_channels() -> (InlineHandle, InlineSession) {
    let (command_tx, _command_rx) = mpsc::unbounded_channel();
    let (_event_tx, event_rx) = mpsc::unbounded_channel();
    let handle = InlineHandle::new_for_tests(command_tx);
    let session = InlineSession {
        handle: handle.clone(),
        events: event_rx,
    };
    (handle, session)
}

#[test]
fn preferred_model_selection_matches_current_static_model() {
    let model_id = ModelId::ClaudeOpus47.as_str();
    let picker = base_picker_state("anthropic", model_id);

    let selection = picker.preferred_model_selection();
    let Some(InlineListSelection::Model(index)) = selection else {
        panic!("expected static model selection, got {selection:?}");
    };

    let option = picker
        .options
        .get(index)
        .expect("selected index should be valid");
    assert_eq!(option.provider, Provider::Anthropic);
    assert_eq!(option.id, model_id);
}

#[test]
fn static_picker_indexes_resolve_provider_models() {
    let openai_indexes = option_indexes_for_provider(Provider::OpenAI);
    assert!(!openai_indexes.is_empty());

    let gpt54_index = find_option_index(Provider::OpenAI, "GPT-5.4")
        .expect("gpt-5.4 should be indexed case-insensitively");
    let option = MODEL_OPTIONS
        .get(gpt54_index)
        .expect("indexed option should exist");
    assert_eq!(option.id, "gpt-5.4");
    assert_eq!(option.provider, Provider::OpenAI);
}

#[test]
fn preferred_model_selection_returns_none_for_unknown_model() {
    let picker = base_picker_state("anthropic", "does-not-exist");
    assert_eq!(picker.preferred_model_selection(), None);
}

#[test]
fn preferred_model_selection_matches_current_custom_provider() {
    let mut picker = base_picker_state("mycorp", "gpt-5-mini");
    let config = CustomProviderConfig {
        name: "mycorp".to_string(),
        display_name: "MyCorporateName".to_string(),
        base_url: "https://llm.corp.example/v1".to_string(),
        api_key_env: "MYCORP_API_KEY".to_string(),
        auth: None,
        model: "gpt-5-mini".to_string(),
    };
    picker.custom_providers = vec![selection::selection_from_custom_provider(&config)];

    let selection = picker.preferred_model_selection();
    let Some(InlineListSelection::CustomProvider(index)) = selection else {
        panic!("expected custom provider selection, got {selection:?}");
    };

    let detail = picker
        .custom_providers
        .get(index)
        .expect("selected custom provider should be valid");
    assert_eq!(detail.provider_key, "mycorp");
    assert_eq!(detail.provider_label, "MyCorporateName");
    assert_eq!(detail.model_id, "gpt-5-mini");
}

#[test]
fn read_workspace_env_returns_value_when_present() -> Result<()> {
    let dir = tempdir()?;
    let env_path = dir.path().join(".env");
    fs::write(&env_path, "OPENAI_API_KEY=sk-test\n")?;
    let value = super::read_workspace_env(dir.path(), "OPENAI_API_KEY")?;
    assert_eq!(value, Some("sk-test".to_string()));
    Ok(())
}

#[test]
fn read_workspace_env_returns_none_when_missing_file() -> Result<()> {
    let dir = tempdir()?;
    let value = super::read_workspace_env(dir.path(), "OPENAI_API_KEY")?;
    assert_eq!(value, None);
    Ok(())
}

#[test]
fn read_workspace_env_returns_none_when_key_absent() -> Result<()> {
    let dir = tempdir()?;
    let env_path = dir.path().join(".env");
    fs::write(&env_path, "OTHER_KEY=value\n")?;
    let value = super::read_workspace_env(dir.path(), "OPENAI_API_KEY")?;
    assert_eq!(value, None);
    Ok(())
}

#[test]
fn selection_marks_openai_service_tier_support_for_supported_models() {
    let detail = selection::selection_from_option(
        MODEL_OPTIONS
            .iter()
            .find(|option| option.id == "gpt-5.2")
            .expect("gpt-5.2 option should exist"),
    );

    assert!(detail.service_tier_supported);
}

#[test]
fn selection_omits_openai_service_tier_support_for_gpt_oss() {
    let detail = selection::selection_from_option(
        MODEL_OPTIONS
            .iter()
            .find(|option| option.id == "gpt-oss-20b")
            .expect("gpt-oss option should exist"),
    );

    assert!(!detail.service_tier_supported);
}

#[test]
fn openai_codex_reasoning_helpers_match_supported_variants() {
    assert!(!selection::supports_gpt5_none_reasoning("gpt"));
    assert!(selection::supports_gpt5_none_reasoning("gpt-5.2-codex"));
    assert!(selection::supports_gpt5_none_reasoning("gpt-5.3-codex"));
    assert!(!selection::supports_gpt5_none_reasoning("gpt-5.1-codex"));
    assert!(!selection::supports_gpt5_none_reasoning("gpt-5-codex"));

    assert!(!selection::supports_xhigh_reasoning("gpt"));
    assert!(selection::supports_xhigh_reasoning("gpt-5.2"));
    assert!(selection::supports_xhigh_reasoning("gpt-5.2-codex"));
    assert!(selection::supports_xhigh_reasoning("gpt-5.3-codex"));
    assert!(selection::supports_xhigh_reasoning("claude-opus-4-7"));
    assert!(!selection::supports_xhigh_reasoning("gpt-5.1-codex"));
    assert!(!selection::supports_xhigh_reasoning("gpt-5.1-codex-max"));

    assert!(selection::supports_max_reasoning("claude-opus-4-7"));
    assert!(!selection::supports_max_reasoning("claude-sonnet-4-6"));
    assert!(!selection::supports_max_reasoning("gpt-5.4"));
}

#[test]
fn build_result_uses_selected_service_tier() {
    let mut picker = base_picker_state("openai", "gpt-5.2");
    picker.selection = Some(selection::SelectionDetail {
        provider_key: "openai".to_string(),
        provider_label: "OpenAI".to_string(),
        provider_enum: Some(Provider::OpenAI),
        model_id: "gpt-5.2".to_string(),
        model_display: "GPT-5.2".to_string(),
        known_model: true,
        reasoning_supported: true,
        reasoning_optional: false,
        reasoning_off_model: None,
        service_tier_supported: true,
        requires_api_key: false,
        uses_chatgpt_auth: false,
        env_key: "OPENAI_API_KEY".to_string(),
    });
    picker.selected_reasoning = Some(ReasoningEffortLevel::Low);
    picker.selected_service_tier = Some(Some(OpenAIServiceTier::Priority));

    let result = picker.build_result().expect("result should build");

    assert_eq!(result.service_tier, Some(OpenAIServiceTier::Priority));
    assert!(result.service_tier_changed);
}

#[test]
fn build_result_uses_selected_flex_service_tier() {
    let mut picker = base_picker_state("openai", "gpt-5.2");
    picker.selection = Some(selection::SelectionDetail {
        provider_key: "openai".to_string(),
        provider_label: "OpenAI".to_string(),
        provider_enum: Some(Provider::OpenAI),
        model_id: "gpt-5.2".to_string(),
        model_display: "GPT-5.2".to_string(),
        known_model: true,
        reasoning_supported: true,
        reasoning_optional: false,
        reasoning_off_model: None,
        service_tier_supported: true,
        requires_api_key: false,
        uses_chatgpt_auth: false,
        env_key: "OPENAI_API_KEY".to_string(),
    });
    picker.selected_reasoning = Some(ReasoningEffortLevel::Low);
    picker.selected_service_tier = Some(Some(OpenAIServiceTier::Flex));

    let result = picker.build_result().expect("result should build");

    assert_eq!(result.service_tier, Some(OpenAIServiceTier::Flex));
    assert!(result.service_tier_changed);
}

#[tokio::test]
async fn openai_login_stays_in_picker_when_ctrl_c_cancels_auth() {
    let mut picker = base_picker_state("openai", "gpt-5.2");
    picker.selection = Some(selection::SelectionDetail {
        provider_key: "openai".to_string(),
        provider_label: "OpenAI".to_string(),
        provider_enum: Some(Provider::OpenAI),
        model_id: "gpt-5.2".to_string(),
        model_display: "GPT-5.2".to_string(),
        known_model: true,
        reasoning_supported: true,
        reasoning_optional: false,
        reasoning_off_model: None,
        service_tier_supported: true,
        requires_api_key: true,
        uses_chatgpt_auth: false,
        env_key: "OPENAI_API_KEY".to_string(),
    });

    let (handle, mut session) = session_with_channels();
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    let ctrl_c_state = Arc::new(CtrlCState::new());
    assert!(matches!(
        ctrl_c_state.register_signal(),
        crate::agent::runloop::unified::state::CtrlCSignal::Cancel
    ));
    let ctrl_c_notify = Arc::new(Notify::new());
    let url_guard =
        crate::agent::runloop::unified::external_url_guard::ExternalUrlGuardContext::new(
            &handle,
            &mut session,
            &ctrl_c_state,
            &ctrl_c_notify,
        );

    let progress = picker
        .handle_api_key(&mut renderer, "login", url_guard)
        .await
        .expect("openai login should cancel cleanly");

    assert!(matches!(progress, ModelPickerProgress::InProgress));
    assert!(picker.pending_api_key.is_none());
}
