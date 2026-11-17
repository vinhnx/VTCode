use std::str::FromStr;

use crate::constants::models;
use crate::models::{ModelId, Provider, ProviderModelSupport, model_catalog_entry};

#[test]
fn gpt_62_openai_and_merge_gateway() {
    for (id, variant, display) in [
        (models::openai::GPT_6_2_ASTRA, ModelId::GPT62Astra, "GPT-6.2 Astra"),
        (models::openai::GPT_6_2_SOL, ModelId::GPT62Sol, "GPT-6.2 Sol"),
    ] {
        assert_eq!(ModelId::from_str(id).unwrap(), variant);
        assert_eq!(variant.as_str(), id);
        assert_eq!(variant.display_name(), display);
        assert!(models::openai::SUPPORTED_MODELS.contains(&id));
        assert!(models::openai::REASONING_MODELS.contains(&id));
        assert!(models::openai::RESPONSES_API_MODELS.contains(&id));
        assert!(!models::openai::STREAMING_REQUIRED_MODELS.contains(&id));
        assert!(Provider::OpenAI.supports_service_tier(id));
        assert!(variant.is_top_tier());
        assert_eq!(variant.generation(), "6.2");
        assert!(crate::core::prompt_cache::supports_openai_explicit_prompt_cache(id));

        let catalog = model_catalog_entry("openai", id).expect("OpenAI 6.2 catalog entry");
        assert!(catalog.reasoning);
        assert!(catalog.vision);
        assert_eq!(catalog.context_window, 1_050_000);
        assert!(!catalog.reasoning_efforts.is_empty());
        assert!(!catalog.reasoning_efforts.contains(&"none"));
    }
    assert_eq!(ModelId::GPT62Astra.preferred_lightweight_variant(), Some(ModelId::GPT62Sol));

    for (route, variant) in [
        (models::merge_gateway::OPENAI_GPT_6_2_ASTRA, ModelId::MergeGatewayOpenAIGpt62Astra),
        (models::merge_gateway::OPENAI_GPT_6_2_SOL, ModelId::MergeGatewayOpenAIGpt62Sol),
    ] {
        assert_eq!(ModelId::from_str(route).unwrap(), variant);
        assert_eq!(variant.as_str(), route);
        assert!(models::merge_gateway::SUPPORTED_MODELS.contains(&route));
        assert!(models::merge_gateway::REASONING_MODELS.contains(&route));
        assert!(models::merge_gateway::REASONING_EFFORT_ROUTES.contains(&route));
        assert_eq!(variant.generation(), "6.2");
        assert!(variant.is_top_tier());

        let catalog = model_catalog_entry("merge-gateway", route).expect("Merge Gateway 6.2 catalog entry");
        assert!(catalog.reasoning);
        assert!(catalog.vision);
        assert_eq!(catalog.context_window, 1_050_000);
        assert!(!catalog.reasoning_efforts.is_empty());
        assert!(!catalog.reasoning_efforts.contains(&"none"));
    }
}
