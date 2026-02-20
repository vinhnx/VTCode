#[cfg(test)]
mod minimax_integration_tests {
    use vtcode_core::config::constants::{model_helpers, models, urls};
    use vtcode_core::llm::provider::LLMProvider;
    use vtcode_core::llm::providers::AnthropicProvider;

    #[test]
    fn test_minimax_m2_constants_exist() {
        // Test that the MiniMax constants are defined
        assert_eq!(models::minimax::MINIMAX_M2, "MiniMax-M2");
        assert_eq!(models::MINIMAX_M2, "MiniMax-M2");
        assert_eq!(models::minimax::MINIMAX_M2_5, "MiniMax-M2.5");
    }

    #[test]
    fn test_minimax_models_in_supported_models() {
        // Test that MiniMax models are in the MiniMax supported models list
        let supported = models::minimax::SUPPORTED_MODELS;
        assert!(
            supported.contains(&"MiniMax-M2"),
            "MiniMax-M2 should be in the MiniMax supported models list"
        );
        assert!(
            supported.contains(&"MiniMax-M2.5"),
            "MiniMax-M2.5 should be in the MiniMax supported models list"
        );
    }

    #[test]
    fn test_minimax_api_base_url_constant() {
        // Test that the MiniMax API base URL constant is defined
        assert_eq!(urls::MINIMAX_API_BASE, "https://api.minimax.io/anthropic");
    }

    #[test]
    fn test_minimax_models_count() {
        // Verify MiniMax namespace has at least 1 model
        let supported = models::minimax::SUPPORTED_MODELS;
        assert!(
            !supported.is_empty(),
            "MiniMax should have at least 1 model"
        );
    }

    #[test]
    fn test_minimax_model_helpers_mapping() {
        let supported = model_helpers::supported_for("minimax")
            .expect("minimax provider should have supported models");
        assert!(
            supported.contains(&models::minimax::MINIMAX_M2),
            "MiniMax-M2 should be listed for minimax provider"
        );
        assert!(
            supported.contains(&models::minimax::MINIMAX_M2_5),
            "MiniMax-M2.5 should be listed for minimax provider"
        );

        let default = model_helpers::default_for("minimax")
            .expect("minimax provider should have a default model");
        assert_eq!(
            default,
            models::minimax::DEFAULT_MODEL,
            "MiniMax provider default model should be MiniMax-M2.5"
        );
    }

    #[test]
    fn test_anthropic_provider_supports_minimax_models() {
        let provider = AnthropicProvider::from_config(
            Some(String::new()),
            Some(models::minimax::MINIMAX_M2_5.to_string()),
            None,
            None,
            None,
            None,
            None,
        );

        let supported = provider.supported_models();
        assert!(
            supported.contains(&models::minimax::MINIMAX_M2.to_string()),
            "Anthropic provider should surface MiniMax-M2 support"
        );
        assert!(
            supported.contains(&models::minimax::MINIMAX_M2_5.to_string()),
            "Anthropic provider should surface MiniMax-M2.5 support"
        );
    }
}
