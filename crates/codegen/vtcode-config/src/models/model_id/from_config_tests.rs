//! Identity tests for `ModelId::from_config` when a declared custom route
//! serves a slug that also exists in the built-in catalog.

use std::collections::BTreeMap;

use crate::core::{CustomProviderConfig, ProviderOverrideConfig};
use crate::models::ModelId;

#[test]
fn prefers_active_custom_provider_for_catalog_slug() {
    let custom = CustomProviderConfig {
        name: "my-gateway".to_string(),
        display_name: "My Gateway".to_string(),
        base_url: "https://llm.example/v1".to_string(),
        api_key_env: "GATEWAY_KEY".to_string(),
        model: "".to_string(),
        models: vec!["gpt-5.6-sol".to_string(), "gpt-5.4".to_string()],
        ..CustomProviderConfig::default()
    };

    // A catalog slug (and its alias) served by a declared custom gateway keeps
    // the gateway identity instead of silently becoming an OpenAI model.
    for slug in ["gpt-5.6-sol", "gpt-5.4"] {
        let parsed = ModelId::from_config(slug, "my-gateway", &BTreeMap::new(), std::slice::from_ref(&custom)).unwrap();
        assert_eq!(parsed, ModelId::Custom("my-gateway".to_string(), slug.to_string()));
        assert_eq!(parsed.as_str(), slug);
    }

    // Control: the same declaration belongs to a different provider, so catalog
    // identity still wins for the unrelated active provider.
    let catalog =
        ModelId::from_config("gpt-5.6-sol", "unrelated-gateway", &BTreeMap::new(), std::slice::from_ref(&custom))
            .unwrap();
    assert_eq!(catalog, ModelId::GPT56Sol);
}

#[test]
fn prefers_active_provider_override_for_catalog_slug() {
    let mut overrides = BTreeMap::new();
    overrides.insert(
        "merge-gateway".to_string(),
        ProviderOverrideConfig {
            models: vec!["gpt-5.6-sol".to_string()],
            base_url: None,
            api_key_env: None,
        },
    );

    let parsed = ModelId::from_config("gpt-5.6-sol", "merge-gateway", &overrides, &[]).unwrap();
    assert_eq!(parsed, ModelId::Custom("merge-gateway".to_string(), "gpt-5.6-sol".to_string()));

    // Control: a catalog slug declared under the model's own provider keeps
    // its catalog variant, so display name, context window and pricing survive.
    let mut same_provider = BTreeMap::new();
    same_provider.insert(
        "openai".to_string(),
        ProviderOverrideConfig {
            models: vec!["gpt-5.6-sol".to_string()],
            base_url: None,
            api_key_env: None,
        },
    );
    let catalog = ModelId::from_config("gpt-5.6-sol", "openai", &same_provider, &[]).unwrap();
    assert_eq!(catalog, ModelId::GPT56Sol);
}
