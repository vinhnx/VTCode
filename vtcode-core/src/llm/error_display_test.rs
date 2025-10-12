#[cfg(test)]
mod tests {
    use crate::llm::error_display::*;

    #[test]
    fn test_llm_error_display() {
        // Test basic error styling
        let error_msg = "Connection failed";
        let styled_error = style_llm_error(error_msg);
        assert!(!styled_error.is_empty());
        assert!(styled_error.contains(error_msg));

        // Test warning styling
        let warning_msg = "Rate limit approaching";
        let styled_warning = style_llm_warning(warning_msg);
        assert!(!styled_warning.is_empty());
        assert!(styled_warning.contains(warning_msg));

        // Test success styling
        let success_msg = "Request completed";
        let styled_success = style_llm_success(success_msg);
        assert!(!styled_success.is_empty());
        assert!(styled_success.contains(success_msg));

        // Test provider name styling
        let providers = vec!["gemini", "openai", "anthropic", "ollama", "unknown"];
        for provider in providers {
            let styled_name = style_provider_name(provider);
            assert!(!styled_name.is_empty());
            // For known providers, we could check specific colors, but for now just ensure it's not empty
        }

        // Test formatted error messages
        let formatted_error = format_llm_error("gemini", "API error occurred");
        assert!(formatted_error.contains("gemini"));
        assert!(formatted_error.contains("API error occurred"));

        let formatted_warning = format_llm_warning("openai", "Rate limit warning");
        assert!(formatted_warning.contains("openai"));
        assert!(formatted_warning.contains("Rate limit warning"));

        let formatted_success = format_llm_success("anthropic", "Operation successful");
        assert!(formatted_success.contains("anthropic"));
        assert!(formatted_success.contains("Operation successful"));
    }
}
