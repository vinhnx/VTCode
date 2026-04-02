use std::str::FromStr;

use vtcode_config::VTCodeConfig;
use vtcode_core::config::models::Provider;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::llm::{
    LightweightFeature, LightweightRouteSource, auto_lightweight_model, lightweight_model_choices,
    main_model_route, resolve_lightweight_route,
};
use vtcode_tui::app::{InlineListItem, InlineListSelection};

use super::DynamicModelRegistry;
use super::options::{MODEL_OPTIONS, option_indexes_for_provider};
use super::rendering::{
    dynamic_model_subtitle, model_search_value, static_model_search_terms, static_model_subtitle,
};

#[derive(Clone)]
pub(crate) struct LightweightModelPaletteView {
    pub(crate) lines: Vec<String>,
    pub(crate) items: Vec<InlineListItem>,
    pub(crate) selected: Option<InlineListSelection>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ConfiguredLightweightSetting {
    Disabled,
    Automatic,
    Main,
    Explicit(String),
}

pub(crate) async fn prepare_lightweight_model_palette_view(
    action_prefix: &str,
    config: &CoreAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
) -> LightweightModelPaletteView {
    let dynamic_models = DynamicModelRegistry::load(
        MODEL_OPTIONS.as_slice(),
        Some(config.workspace.as_path()),
        vt_cfg,
    )
    .await;
    build_lightweight_model_palette_view(action_prefix, config, vt_cfg, &dynamic_models)
}

pub(crate) fn build_lightweight_model_palette_view(
    action_prefix: &str,
    config: &CoreAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
    dynamic_models: &DynamicModelRegistry,
) -> LightweightModelPaletteView {
    let current_setting = configured_lightweight_setting(config, vt_cfg);
    let configured_label = current_setting.label();
    let resolution =
        resolve_lightweight_route(config, vt_cfg, LightweightFeature::PromptSuggestions, None);
    let effective_route = match resolution.source {
        LightweightRouteSource::MainModel => config.model.clone(),
        _ => match resolution.fallback_to_main_model() {
            Some(fallback) => format!(
                "{} -> fallback {}",
                resolution.primary.model, fallback.model
            ),
            None => resolution.primary.model.clone(),
        },
    };

    let main_route = main_model_route(config);
    let provider_name = main_route.provider_name;
    let main_model = main_route.model;
    let auto_model = auto_lightweight_model(&provider_name, &main_model);
    let provider = Provider::from_str(&provider_name).ok();

    let items = match provider {
        Some(provider) => provider_scoped_items(
            action_prefix,
            provider,
            &current_setting,
            &provider_name,
            &main_model,
            &auto_model,
            dynamic_models,
        ),
        None => custom_provider_items(
            action_prefix,
            &current_setting,
            &provider_name,
            &main_model,
            &auto_model,
        ),
    };

    let selected = Some(InlineListSelection::ConfigAction(match &current_setting {
        ConfiguredLightweightSetting::Main => format!("{action_prefix}main"),
        ConfiguredLightweightSetting::Explicit(model) => {
            format!("{action_prefix}{model}")
        }
        ConfiguredLightweightSetting::Disabled | ConfiguredLightweightSetting::Automatic => {
            format!("{action_prefix}auto")
        }
    }));

    let mut lines = vec![
        "Choose the shared lightweight model VT Code should prefer for memory triage, prompt suggestions, and smaller delegated tasks.".to_string(),
        format!("Current setting: {}", configured_label),
        format!("Effective route: {}", effective_route),
        "Selecting any option enables the shared lightweight route without changing the active main conversation model.".to_string(),
    ];

    if let Some(provider) = provider {
        if let Some(warning) = dynamic_models.warning_for(provider) {
            lines.push(format!("Live model notice: {}", warning));
        } else if let Some(error) = dynamic_models.error_for(provider) {
            lines.push(format!("Live model notice: {}", error));
        }
    }

    if let Some(warning) = resolution.warning {
        lines.push(format!("Route warning: {}", warning));
    }

    LightweightModelPaletteView {
        lines,
        items,
        selected,
    }
}

fn provider_scoped_items(
    action_prefix: &str,
    provider: Provider,
    current_setting: &ConfiguredLightweightSetting,
    current_provider: &str,
    main_model: &str,
    auto_model: &str,
    dynamic_models: &DynamicModelRegistry,
) -> Vec<InlineListItem> {
    let mut items = base_items(
        action_prefix,
        current_setting,
        current_provider,
        main_model,
        auto_model,
    );

    for option_index in option_indexes_for_provider(provider) {
        let Some(option) = MODEL_OPTIONS.get(*option_index) else {
            continue;
        };
        if option.id.eq_ignore_ascii_case(main_model) || option.id.eq_ignore_ascii_case(auto_model)
        {
            continue;
        }
        items.push(InlineListItem {
            title: option.display.to_string(),
            subtitle: Some(static_model_subtitle(option, current_provider, main_model)),
            badge: Some(model_badge(current_setting, option.id, provider.label())),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{action_prefix}{}",
                option.id
            ))),
            search_value: Some(model_search_value(
                provider,
                option.display,
                option.id,
                Some(option.description),
                &static_model_search_terms(option.model, option.supports_reasoning),
            )),
        });
    }

    for entry_index in dynamic_models.indexes_for(provider) {
        let Some(detail) = dynamic_models.detail(*entry_index) else {
            continue;
        };
        if detail.model_id.eq_ignore_ascii_case(main_model)
            || detail.model_id.eq_ignore_ascii_case(auto_model)
        {
            continue;
        }
        let mut extra_terms = Vec::new();
        if provider.is_local() {
            extra_terms.push("local".to_string());
        }
        if detail.reasoning_supported {
            extra_terms.push("reasoning".to_string());
        }
        items.push(InlineListItem {
            title: detail.model_display.clone(),
            subtitle: Some(dynamic_model_subtitle(
                provider,
                &detail.model_id,
                detail.reasoning_supported,
                current_provider,
                main_model,
            )),
            badge: Some(model_badge(
                current_setting,
                &detail.model_id,
                provider.label(),
            )),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{action_prefix}{}",
                detail.model_id
            ))),
            search_value: Some(model_search_value(
                provider,
                &detail.model_display,
                &detail.model_id,
                None,
                &extra_terms,
            )),
        });
    }

    items
}

fn custom_provider_items(
    action_prefix: &str,
    current_setting: &ConfiguredLightweightSetting,
    current_provider: &str,
    main_model: &str,
    auto_model: &str,
) -> Vec<InlineListItem> {
    let mut items = base_items(
        action_prefix,
        current_setting,
        current_provider,
        main_model,
        auto_model,
    );

    for model in lightweight_model_choices(current_provider, main_model) {
        if model.eq_ignore_ascii_case(main_model) || model.eq_ignore_ascii_case(auto_model) {
            continue;
        }
        items.push(InlineListItem {
            title: model.clone(),
            subtitle: Some("Explicit same-provider lightweight model.".to_string()),
            badge: Some(model_badge(current_setting, &model, "Model")),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{action_prefix}{model}"
            ))),
            search_value: Some(format!("{} {} lightweight model", current_provider, model)),
        });
    }

    items
}

fn base_items(
    action_prefix: &str,
    current_setting: &ConfiguredLightweightSetting,
    current_provider: &str,
    main_model: &str,
    auto_model: &str,
) -> Vec<InlineListItem> {
    vec![
        InlineListItem {
            title: "Automatic (recommended)".to_string(),
            subtitle: Some(format!(
                "Use {} for lower-cost side tasks and fall back to {}.",
                auto_model, main_model
            )),
            badge: Some(
                if matches!(current_setting, ConfiguredLightweightSetting::Automatic) {
                    "Current".to_string()
                } else {
                    "Recommended".to_string()
                },
            ),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{action_prefix}auto"
            ))),
            search_value: Some(format!(
                "{} {} automatic lightweight model recommended",
                current_provider, auto_model
            )),
        },
        InlineListItem {
            title: "Use main model".to_string(),
            subtitle: Some(format!(
                "Keep lightweight work on {} for accuracy-first behavior.",
                main_model
            )),
            badge: Some(
                if matches!(current_setting, ConfiguredLightweightSetting::Main) {
                    "Current".to_string()
                } else {
                    "Accuracy".to_string()
                },
            ),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{action_prefix}main"
            ))),
            search_value: Some(format!(
                "{} {} main accuracy lightweight model",
                current_provider, main_model
            )),
        },
    ]
}

fn model_badge(
    current_setting: &ConfiguredLightweightSetting,
    model_id: &str,
    default_badge: &str,
) -> String {
    match current_setting {
        ConfiguredLightweightSetting::Explicit(current)
            if current.eq_ignore_ascii_case(model_id) =>
        {
            "Current".to_string()
        }
        _ => default_badge.to_string(),
    }
}

fn configured_lightweight_setting(
    config: &CoreAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
) -> ConfiguredLightweightSetting {
    let Some(vt_cfg) = vt_cfg else {
        return ConfiguredLightweightSetting::Automatic;
    };
    if !vt_cfg.agent.small_model.enabled {
        return ConfiguredLightweightSetting::Disabled;
    }

    let configured_model = vt_cfg.agent.small_model.model.trim();
    if configured_model.is_empty() {
        return ConfiguredLightweightSetting::Automatic;
    }
    if configured_model.eq_ignore_ascii_case(config.model.as_str()) {
        return ConfiguredLightweightSetting::Main;
    }

    ConfiguredLightweightSetting::Explicit(configured_model.to_string())
}

impl ConfiguredLightweightSetting {
    fn label(&self) -> String {
        match self {
            ConfiguredLightweightSetting::Disabled => "Disabled".to_string(),
            ConfiguredLightweightSetting::Automatic => "Automatic".to_string(),
            ConfiguredLightweightSetting::Main => "Use main model".to_string(),
            ConfiguredLightweightSetting::Explicit(model) => model.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::runloop::model_picker::selection;
    use vtcode_core::config::constants::models;
    use vtcode_core::config::core::PromptCachingConfig;
    use vtcode_core::config::types::{
        ModelSelectionSource, ReasoningEffortLevel, UiSurfacePreference,
    };
    use vtcode_core::core::agent::snapshots::{
        DEFAULT_CHECKPOINTS_ENABLED, DEFAULT_MAX_AGE_DAYS, DEFAULT_MAX_SNAPSHOTS,
    };

    fn runtime_config(provider: &str, model: &str) -> CoreAgentConfig {
        CoreAgentConfig {
            model: model.to_string(),
            api_key: "test-key".to_string(),
            provider: provider.to_string(),
            api_key_env: Provider::OpenAI.default_api_key_env().to_string(),
            workspace: std::env::current_dir().expect("current_dir"),
            verbose: false,
            quiet: false,
            theme: vtcode_core::ui::theme::DEFAULT_THEME_ID.to_string(),
            reasoning_effort: ReasoningEffortLevel::default(),
            ui_surface: UiSurfacePreference::default(),
            prompt_cache: PromptCachingConfig::default(),
            model_source: ModelSelectionSource::WorkspaceConfig,
            custom_api_keys: std::collections::BTreeMap::new(),
            checkpointing_enabled: DEFAULT_CHECKPOINTS_ENABLED,
            checkpointing_storage_dir: None,
            checkpointing_max_snapshots: DEFAULT_MAX_SNAPSHOTS,
            checkpointing_max_age_days: Some(DEFAULT_MAX_AGE_DAYS),
            max_conversation_turns: 1000,
            model_behavior: None,
            openai_chatgpt_auth: None,
        }
    }

    #[test]
    fn lightweight_palette_lists_curated_same_provider_models() {
        let config = runtime_config("openai", models::openai::GPT_5_4);
        let vt_cfg = VTCodeConfig::default();

        let view = build_lightweight_model_palette_view(
            "lightweight_model:",
            &config,
            Some(&vt_cfg),
            &DynamicModelRegistry::default(),
        );

        assert!(
            view.items
                .iter()
                .any(|item| item.title == "Automatic (recommended)")
        );
        assert!(view.items.iter().any(|item| item.title == "Use main model"));
        assert!(view.items.iter().any(|item| {
            item.selection
                == Some(InlineListSelection::ConfigAction(
                    "lightweight_model:gpt-5-mini".to_string(),
                ))
        }));
    }

    #[test]
    fn lightweight_palette_keeps_shortcuts_first() {
        let config = runtime_config("openai", models::openai::GPT_5_4);
        let vt_cfg = VTCodeConfig::default();

        let view = build_lightweight_model_palette_view(
            "lightweight_model:",
            &config,
            Some(&vt_cfg),
            &DynamicModelRegistry::default(),
        );

        assert_eq!(
            view.items.first().map(|item| item.title.as_str()),
            Some("Automatic (recommended)")
        );
        assert_eq!(
            view.items.get(1).map(|item| item.title.as_str()),
            Some("Use main model")
        );
    }

    #[test]
    fn lightweight_palette_search_value_includes_provider_and_model_terms() {
        let config = runtime_config("openai", models::openai::GPT_5_4);
        let vt_cfg = VTCodeConfig::default();

        let view = build_lightweight_model_palette_view(
            "lightweight_model:",
            &config,
            Some(&vt_cfg),
            &DynamicModelRegistry::default(),
        );

        let model_item = view
            .items
            .iter()
            .find(|item| {
                item.selection
                    == Some(InlineListSelection::ConfigAction(
                        "lightweight_model:gpt-5-mini".to_string(),
                    ))
            })
            .expect("gpt-5-mini entry");

        let search = model_item
            .search_value
            .as_deref()
            .expect("search value")
            .to_ascii_lowercase();
        assert!(search.contains("openai"));
        assert!(search.contains("gpt-5-mini"));
        assert!(search.contains("tools"));
    }

    #[test]
    fn lightweight_palette_marks_explicit_current_selection() {
        let config = runtime_config("openai", models::openai::GPT_5_4);
        let mut vt_cfg = VTCodeConfig::default();
        vt_cfg.agent.small_model.model = "gpt-5-mini".to_string();

        let view = build_lightweight_model_palette_view(
            "lightweight_model:",
            &config,
            Some(&vt_cfg),
            &DynamicModelRegistry::default(),
        );

        assert_eq!(
            view.selected,
            Some(InlineListSelection::ConfigAction(
                "lightweight_model:gpt-5-mini".to_string(),
            ))
        );
    }

    #[test]
    fn lightweight_palette_appends_same_provider_dynamic_models() {
        let config = runtime_config("openai", models::openai::GPT_5_4);
        let vt_cfg = VTCodeConfig::default();
        let registry = DynamicModelRegistry {
            entries: vec![
                selection::selection_from_dynamic(
                    Provider::OpenAI,
                    "gpt-5.4-experimental",
                    "GPT-5.4 Experimental",
                    None,
                    None,
                ),
                selection::selection_from_dynamic(
                    Provider::Ollama,
                    "llama-local",
                    "Llama Local",
                    None,
                    None,
                ),
            ],
            provider_models: hashbrown::HashMap::from([
                (Provider::OpenAI, vec![0]),
                (Provider::Ollama, vec![1]),
            ]),
            provider_errors: hashbrown::HashMap::new(),
            provider_warnings: hashbrown::HashMap::new(),
        };

        let view = build_lightweight_model_palette_view(
            "lightweight_model:",
            &config,
            Some(&vt_cfg),
            &registry,
        );

        assert!(view.items.iter().any(|item| {
            item.selection
                == Some(InlineListSelection::ConfigAction(
                    "lightweight_model:gpt-5.4-experimental".to_string(),
                ))
        }));
        assert!(!view.items.iter().any(|item| {
            item.selection
                == Some(InlineListSelection::ConfigAction(
                    "lightweight_model:llama-local".to_string(),
                ))
        }));
    }

    #[test]
    fn lightweight_palette_surfaces_live_model_errors_for_openai() {
        let config = runtime_config("openai", models::openai::GPT_5_4);
        let vt_cfg = VTCodeConfig::default();
        let registry = DynamicModelRegistry {
            provider_errors: hashbrown::HashMap::from([(
                Provider::OpenAI,
                "Failed to query OpenAI".to_string(),
            )]),
            ..DynamicModelRegistry::default()
        };

        let view = build_lightweight_model_palette_view(
            "lightweight_model:",
            &config,
            Some(&vt_cfg),
            &registry,
        );

        assert!(
            view.lines
                .iter()
                .any(|line| line.contains("Live model notice: Failed to query OpenAI"))
        );
    }
}
