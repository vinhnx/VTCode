//! A custom route must not inherit another provider's catalog capabilities
//! just because its model slug happens to exist in that provider's catalog.

use crate::models::provider::ProviderModelSupport;
use crate::models::{ModelId, Provider};

#[test]
fn custom_provider_key_preserves_declared_name() {
    let custom = ModelId::Custom("my-gateway".to_string(), "gpt-5.6-sol".to_string());
    assert_eq!(custom.provider_key(), "my-gateway");
    // `provider()` deliberately still returns a built-in enum value for callers
    // that need one; capability lookups use the raw key instead.
    assert_eq!(custom.provider(), Provider::OpenAI);
}

#[test]
fn catalog_provider_keys_match_canonical_provider_names() {
    // The raw accessor must be a drop-in for the old catalog key derivation on
    // every built-in model, so this change cannot shift catalog metadata.
    for model in ModelId::all_models() {
        assert_eq!(model.provider_key(), model.provider().as_ref(), "{model}");
    }
}

#[test]
fn custom_route_does_not_inherit_foreign_catalog_capabilities() {
    let gateway = ModelId::Custom("my-gateway".to_string(), "gpt-5.6-sol".to_string());
    assert!(gateway.input_modalities().is_empty());
    assert!(gateway.preferred_lightweight_variant().is_none());
    assert!(!gateway.supports_reasoning());
    assert!(!gateway.supports_reasoning_effort());

    // Control: the same slug under its real provider keeps catalog metadata.
    let real = ModelId::Custom("openai".to_string(), "gpt-5.6-sol".to_string());
    assert!(!real.input_modalities().is_empty(), "control slug must have catalog modalities");
    assert_eq!(real.input_modalities(), ModelId::GPT56Sol.input_modalities());
    assert_eq!(real.supports_reasoning(), ModelId::GPT56Sol.supports_reasoning());
    assert_eq!(real.supports_reasoning_effort(), ModelId::GPT56Sol.supports_reasoning_effort());
}

#[test]
fn known_custom_provider_key_keeps_its_provider_identity() {
    let custom = ModelId::Custom("lmstudio".to_string(), "ornith-1.0".to_string());
    assert_eq!(custom.provider_key(), "lmstudio");
    assert_eq!(custom.provider(), Provider::LmStudio);
    // A built-in key is allowed to use that provider's capability heuristics.
    assert_eq!(custom.supports_reasoning_effort(), Provider::LmStudio.supports_reasoning_effort("ornith-1.0"));
}
