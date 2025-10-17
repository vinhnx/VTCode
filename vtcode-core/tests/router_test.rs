use std::collections::BTreeMap;

use vtcode_core::config::core::PromptCachingConfig;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::router::{HeuristicSettings, RouterConfig};
use vtcode_core::config::types::{
    AgentConfig as CoreAgentConfig, ModelSelectionSource, ReasoningEffortLevel, UiSurfacePreference,
};
use vtcode_core::core::agent::snapshots::{
    DEFAULT_CHECKPOINTS_ENABLED, DEFAULT_MAX_AGE_DAYS, DEFAULT_MAX_SNAPSHOTS,
};
use vtcode_core::core::router::{ModelSelector, Router, TaskClass, TaskClassifier};

fn core_cfg(model: &str) -> CoreAgentConfig {
    CoreAgentConfig {
        model: model.to_string(),
        api_key: "test".to_string(),
        provider: "gemini".to_string(),
        api_key_env: "GEMINI_API_KEY".to_string(),
        workspace: std::env::current_dir().unwrap(),
        verbose: false,
        theme: vtcode_core::ui::theme::DEFAULT_THEME_ID.to_string(),
        reasoning_effort: ReasoningEffortLevel::default(),
        ui_surface: UiSurfacePreference::default(),
        prompt_cache: PromptCachingConfig::default(),
        model_source: ModelSelectionSource::WorkspaceConfig,
        custom_api_keys: BTreeMap::new(),
        checkpointing_enabled: DEFAULT_CHECKPOINTS_ENABLED,
        checkpointing_storage_dir: None,
        checkpointing_max_snapshots: DEFAULT_MAX_SNAPSHOTS,
        checkpointing_max_age_days: Some(DEFAULT_MAX_AGE_DAYS),
    }
}

#[test]
fn classify_simple_and_codegen() {
    let classifier = TaskClassifier::new(&HeuristicSettings::default());
    assert_eq!(classifier.classify("list files"), TaskClass::Simple);
    assert_eq!(
        classifier.classify(
            "```
fn main() {}
```",
        ),
        TaskClass::CodegenHeavy
    );
}

#[test]
fn classifier_uses_custom_markers() {
    let mut settings = HeuristicSettings::default();
    settings.retrieval_markers = vec!["bing it".into()];
    let classifier = TaskClassifier::new(&settings);
    assert_eq!(
        classifier.classify("Could you bing it and summarize the docs?"),
        TaskClass::RetrievalHeavy
    );
}

#[test]
fn route_uses_model_mapping() {
    let mut cfg = VTCodeConfig::default();
    cfg.router.enabled = true;
    cfg.router.models.standard = "gemini-2.5-flash-preview-05-20".to_string();
    cfg.router.models.codegen_heavy = "gemini-2.5-pro".to_string();

    let core = core_cfg("gemini-2.5-flash-preview-05-20");
    let r1 = Router::route(&cfg, &core, "summarize this text");
    assert_eq!(r1.selected_model, "gemini-2.5-flash-preview-05-20");

    let r2 = Router::route(&cfg, &core, "Provide a patch:\n```diff\n- a\n+ b\n```\n");
    assert_eq!(r2.selected_model, "gemini-2.5-pro");
}

#[test]
fn model_selector_falls_back_to_agent_model() {
    let cfg = RouterConfig::default();
    let selector = ModelSelector::new(&cfg, "gemini-2.5-flash-preview");
    assert_eq!(
        selector.select(TaskClass::RetrievalHeavy),
        cfg.models.retrieval_heavy
    );

    let mut cfg = RouterConfig::default();
    let mut models = cfg.models.clone();
    models.retrieval_heavy.clear();
    cfg.models = models;
    let selector = ModelSelector::new(&cfg, "gemini-2.5-flash-preview");
    assert_eq!(
        selector.select(TaskClass::RetrievalHeavy),
        "gemini-2.5-flash-preview"
    );
}
