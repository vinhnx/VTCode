#[cfg(test)]
mod minimax_integration_tests {
    use vtcode_core::config::constants::{models, urls};

    #[test]
    fn test_minimax_m2_constant_exists() {
        // Test that the MiniMax-M2 constant is defined
        assert_eq!(models::minimax::MINIMAX_M2, "MiniMax-M2");
        assert_eq!(models::MINIMAX_M2, "MiniMax-M2");
    }

    #[test]
    fn test_minimax_m2_in_supported_models() {
        // Test that MiniMax-M2 is in the MiniMax supported models list
        let supported = models::minimax::SUPPORTED_MODELS;
        assert!(
            supported.contains(&"MiniMax-M2"),
            "MiniMax-M2 should be in the MiniMax supported models list"
        );
    }

    #[test]
    fn test_minimax_api_base_url_constant() {
        // Test that the MiniMax API base URL constant is defined
        assert_eq!(
            urls::MINIMAX_ANTHROPIC_API_BASE,
            "https://api.minimax.io/anthropic/v1"
        );
    }

    #[test]
    fn test_minimax_models_count() {
        // Verify MiniMax namespace has at least 1 model
        let supported = models::minimax::SUPPORTED_MODELS;
        assert!(supported.len() >= 1, "MiniMax should have at least 1 model");
    }
}
