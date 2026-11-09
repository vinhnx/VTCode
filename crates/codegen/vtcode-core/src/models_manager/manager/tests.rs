use super::*;
use tempfile::tempdir;

#[tokio::test]
async fn test_new_manager() {
    let manager = ModelsManager::new();
    assert!(!manager.local_models.is_empty());
}

#[tokio::test]
async fn test_list_models() {
    let manager = ModelsManager::new();
    let models = manager.list_models().await;
    assert!(!models.is_empty());
}

#[tokio::test]
async fn test_list_models_for_provider() {
    let manager = ModelsManager::new();
    let gemini_models = manager.list_models_for_provider(Provider::Gemini).await;
    assert!(!gemini_models.is_empty());
    assert!(gemini_models.iter().all(|m| m.provider == Provider::Gemini));
}

#[tokio::test]
async fn test_get_model_with_default() {
    let manager = ModelsManager::with_provider(Provider::Gemini);
    let model = manager.get_model(None).await;
    assert!(!model.is_empty());
}

#[tokio::test]
async fn test_get_model_with_explicit() {
    let manager = ModelsManager::new();
    let model = manager.get_model(Some("custom-model")).await;
    assert_eq!(model, "custom-model");
}

#[tokio::test]
async fn test_construct_model_family() {
    let manager = ModelsManager::new();
    let family = manager.construct_model_family("gemini-3-flash-preview").await;
    assert_eq!(family.family, "gemini-3");
    assert_eq!(family.provider, Provider::Gemini);
}

#[tokio::test]
async fn test_find_model() {
    let manager = ModelsManager::new();
    let model = manager.find_model("gemini-3-flash-preview").await;
    assert!(model.is_some());
}

#[tokio::test]
async fn test_model_exists() {
    let manager = ModelsManager::new();
    assert!(manager.model_exists("gemini-3-flash-preview").await);
    assert!(!manager.model_exists("nonexistent-model").await);
}

#[test]
fn model_exists_sync_covers_local_and_gateway_providers() {
    // `create_provider_for_model` warns for anything this misses; a local
    // route and a gateway route pin both sides of the builtin preset set.
    let manager = ModelsManager::new();
    assert!(manager.model_exists_sync("gpt-oss:20b"));
    assert!(manager.model_exists_sync("meta/muse-glimmer-30b"));
    assert!(!manager.model_exists_sync("nonexistent-model"));
}

#[tokio::test]
async fn test_set_provider() {
    let manager = ModelsManager::new();
    manager.set_provider(Provider::Anthropic).await;
    assert_eq!(manager.get_provider().await, Provider::Anthropic);
}

#[test]
fn merge_catalog_models_map_to_picker_metadata_and_hide_deprecated_routes() {
    let model = MergeCatalogModel {
        model: "anthropic/claude-sonnet-5".to_string(),
        provider: "anthropic".to_string(),
        display_name: Some("Claude Sonnet 5".to_string()),
        availability: MergeCatalogAvailability::Available,
        context_window: Some(200_000),
        max_output_tokens: Some(16_384),
        supports_tool_use: true,
        supports_streaming: true,
        supports_vision: true,
        supports_structured_output: true,
        service_tiers: Vec::new(),
        supports_reasoning: true,
        reasoning_disable_supported: true,
        reasoning_controls: vec!["thinking.budget_tokens".to_string()],
    };

    let info = ModelsManager::merge_catalog_model_info(model).expect("available model should map");
    assert_eq!(info.slug, "anthropic/claude-sonnet-5");
    assert_eq!(info.display_name, "Claude Sonnet 5");
    assert_eq!(info.provider, Provider::MergeGateway);
    assert_eq!(info.context_window, Some(200_000));
    assert_eq!(info.max_output_tokens, Some(16_384));
    assert!(info.supports_vision);
    assert!(info.supports_structured_output);
    assert!(info.supports_reasoning);
    assert_eq!(info.supported_reasoning_levels.len(), ModelsManager::merge_supported_reasoning_presets(true).len());

    let deprecated = MergeCatalogModel {
        availability: MergeCatalogAvailability::Deprecated,
        ..MergeCatalogModel {
            model: "openai/old".to_string(),
            provider: "openai".to_string(),
            display_name: None,
            availability: MergeCatalogAvailability::Available,
            context_window: None,
            max_output_tokens: None,
            supports_tool_use: false,
            supports_streaming: false,
            supports_vision: false,
            supports_structured_output: false,
            service_tiers: Vec::new(),
            supports_reasoning: false,
            reasoning_disable_supported: false,
            reasoning_controls: Vec::new(),
        }
    };
    assert!(ModelsManager::merge_catalog_model_info(deprecated).is_none());
}

#[tokio::test]
async fn test_cache_operations() {
    let dir = tempdir().expect("create temp dir");
    let manager = ModelsManager::with_home(dir.path().to_path_buf());

    // Initially no cache
    let cached = manager.try_load_cache().await;
    assert!(!cached);

    // Persist some models
    let models = vec![ModelInfo {
        slug: "test-model".to_string(),
        display_name: "Test Model".to_string(),
        description: "A test".to_string(),
        provider: Provider::Gemini,
        default_reasoning_level: crate::config::types::ReasoningEffortLevel::Medium,
        supported_reasoning_levels: vec![],
        context_window: Some(128_000),
        supports_tool_use: true,
        supports_streaming: true,
        supports_vision: false,
        supports_structured_output: false,
        supports_reasoning: false,
        max_output_tokens: None,
        priority: 0,
        visibility: "list".to_string(),
        supported_in_api: true,
        upgrade: None,
    }];
    manager.persist_cache(&models, None).await;

    // Now cache should load
    let cached = manager.try_load_cache().await;
    assert!(cached);
}

#[tokio::test]
async fn loads_provider_scoped_legacy_cache_and_republishes_it() {
    let temp_dir = tempdir().expect("create temp dir");
    let canonical_path = temp_dir.path().join("current/models_cache_gemini.json");
    let legacy_path = temp_dir.path().join("legacy/models_cache_gemini.json");
    let legacy_cache = ModelsCache::new(Provider::Gemini.to_string(), Vec::new());
    cache::save_cache(&legacy_path, &legacy_cache).await.expect("save legacy cache");

    let loaded = load_cache_from_paths(&canonical_path, std::slice::from_ref(&legacy_path), Provider::Gemini)
        .await
        .expect("load legacy cache");

    assert_eq!(loaded.provider, Provider::Gemini.to_string());
    assert!(
        cache::load_cache(&canonical_path)
            .await
            .expect("load republished cache")
            .is_some()
    );
}

#[tokio::test]
async fn malformed_canonical_cache_recovers_legacy_without_replacing_it() {
    let temp_dir = tempdir().expect("create temp dir");
    let canonical_path = temp_dir.path().join("current/models_cache_gemini.json");
    let legacy_path = temp_dir.path().join("legacy/models_cache_gemini.json");
    std::fs::create_dir_all(canonical_path.parent().expect("canonical parent")).expect("canonical directory");
    std::fs::write(&canonical_path, b"not json").expect("malformed canonical cache");
    let legacy_cache = ModelsCache::new(Provider::Gemini.to_string(), Vec::new());
    cache::save_cache(&legacy_path, &legacy_cache).await.expect("save legacy cache");

    let loaded = load_cache_from_paths(&canonical_path, std::slice::from_ref(&legacy_path), Provider::Gemini)
        .await
        .expect("recover legacy cache");

    assert_eq!(loaded.provider, Provider::Gemini.to_string());
    assert_eq!(std::fs::read(&canonical_path).expect("read canonical cache"), b"not json");
}

#[tokio::test]
async fn mismatched_canonical_cache_falls_back_to_matching_legacy_cache() {
    let temp_dir = tempdir().expect("create temp dir");
    let canonical_path = temp_dir.path().join("current/models_cache_gemini.json");
    let legacy_path = temp_dir.path().join("legacy/models_cache_gemini.json");
    let canonical_cache = ModelsCache::new(Provider::OpenAI.to_string(), Vec::new());
    cache::save_cache(&canonical_path, &canonical_cache)
        .await
        .expect("save mismatched cache");
    let legacy_cache = ModelsCache::new(Provider::Gemini.to_string(), Vec::new());
    cache::save_cache(&legacy_path, &legacy_cache).await.expect("save legacy cache");

    let loaded = load_cache_from_paths(&canonical_path, std::slice::from_ref(&legacy_path), Provider::Gemini)
        .await
        .expect("recover matching legacy cache");

    assert_eq!(loaded.provider, Provider::Gemini.to_string());
    assert_eq!(
        cache::load_cache(&canonical_path)
            .await
            .expect("read canonical cache")
            .unwrap()
            .provider,
        Provider::OpenAI.to_string()
    );
}

#[test]
fn legacy_cache_paths_preserve_provider_scoped_and_unscoped_names() {
    let temp_dir = tempdir().expect("create temp dir");
    let manager = ModelsManager::with_home(temp_dir.path().to_path_buf());

    assert_eq!(
        manager.legacy_cache_paths(Provider::Gemini),
        vec![
            temp_dir.path().join("models_cache_gemini.json"),
            temp_dir.path().join(LEGACY_MODEL_CACHE_FILE),
        ]
    );
}

#[tokio::test]
async fn ignores_fresh_cache_for_another_provider() {
    let dir = tempdir().expect("create temp dir");
    let manager = ModelsManager::with_home_and_provider(dir.path().to_path_buf(), Provider::Gemini);
    let cache = ModelsCache::new("openai", Vec::new());
    cache::save_cache(&manager.cache_path_for(Provider::Gemini), &cache)
        .await
        .expect("save mismatched cache");

    assert!(!manager.try_load_cache().await);
    assert!(manager.remote_models.read().await.is_empty());
}

#[tokio::test]
async fn ignores_stale_inflight_models_after_provider_switch() {
    let dir = tempdir().expect("create temp dir");
    let manager = ModelsManager::with_home_and_provider(dir.path().to_path_buf(), Provider::Gemini);
    manager.set_provider(Provider::Anthropic).await;

    let models = vec![ModelInfo {
        slug: "gemini-stale".to_string(),
        display_name: "Gemini Stale".to_string(),
        description: "Stale fetch result".to_string(),
        provider: Provider::Gemini,
        default_reasoning_level: crate::config::types::ReasoningEffortLevel::Medium,
        supported_reasoning_levels: vec![],
        context_window: Some(128_000),
        supports_tool_use: true,
        supports_streaming: true,
        supports_vision: false,
        supports_structured_output: false,
        supports_reasoning: false,
        max_output_tokens: None,
        priority: 0,
        visibility: "list".to_string(),
        supported_in_api: true,
        upgrade: None,
    }];

    assert!(
        !manager
            .apply_remote_state_for_provider(Provider::Gemini, models, Some("etag-1".to_string()))
            .await
    );
    assert!(manager.remote_models.read().await.is_empty());
    assert!(manager.etag.read().await.is_none());
}

#[tokio::test]
async fn persists_cache_under_requested_provider_scope() {
    let dir = tempdir().expect("create temp dir");
    let manager = ModelsManager::with_home_and_provider(dir.path().to_path_buf(), Provider::Gemini);
    manager.set_provider(Provider::Anthropic).await;

    let models = vec![ModelInfo {
        slug: "gemini-cached".to_string(),
        display_name: "Gemini Cached".to_string(),
        description: "Cached fetch result".to_string(),
        provider: Provider::Gemini,
        default_reasoning_level: crate::config::types::ReasoningEffortLevel::Medium,
        supported_reasoning_levels: vec![],
        context_window: Some(128_000),
        supports_tool_use: true,
        supports_streaming: true,
        supports_vision: false,
        supports_structured_output: false,
        supports_reasoning: false,
        max_output_tokens: None,
        priority: 0,
        visibility: "list".to_string(),
        supported_in_api: true,
        upgrade: None,
    }];

    manager.persist_cache_for(Provider::Gemini, &models, None).await;

    let gemini_cache = cache::load_cache(&manager.cache_path_for(Provider::Gemini))
        .await
        .expect("load gemini cache");
    let anthropic_cache = cache::load_cache(&manager.cache_path_for(Provider::Anthropic))
        .await
        .expect("load anthropic cache");

    assert!(gemini_cache.is_some());
    assert!(anthropic_cache.is_none());
}

#[test]
fn test_client_version() {
    let version = ModelsManager::client_version();
    assert!(!version.is_empty());
    assert!(version.contains('.'));
}

#[test]
fn test_supported_providers() {
    let providers = ModelsManager::supported_providers();
    assert!(!providers.is_empty());
    assert!(providers.contains(&Provider::Gemini));
    assert!(providers.contains(&Provider::OpenAI));
}

#[test]
fn moonshot_default_model_uses_curated_default() {
    let manager = ModelsManager::new();
    assert_eq!(
        manager.get_default_model_for_provider(Provider::Moonshot),
        crate::config::constants::models::moonshot::DEFAULT_MODEL
    );
}
