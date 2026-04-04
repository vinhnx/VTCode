//! Compatibility re-export for provider capability metadata.

pub use super::provider::ProviderCapabilities;

#[cfg(test)]
mod tests {
    use super::ProviderCapabilities;

    #[test]
    fn capability_summary_reports_enabled_features() {
        let caps = ProviderCapabilities {
            provider_name: "gemini".to_owned(),
            model: "gemini-2.0-pro".to_owned(),
            streaming: true,
            reasoning: false,
            reasoning_effort: false,
            tools: true,
            parallel_tool_config: false,
            structured_output: true,
            context_caching: true,
            responses_compaction: false,
            context_edits: false,
            context_awareness: false,
            vision: true,
            context_size: 2_000_000,
        };

        let summary = caps.summary();
        assert!(summary.contains("gemini-2.0-pro"));
        assert!(summary.contains("2000000"));
        assert!(summary.contains("structured-output"));
        assert!(summary.contains("context-caching"));
    }

    #[test]
    fn advanced_feature_detection_matches_fields() {
        let basic = ProviderCapabilities {
            provider_name: "basic".to_owned(),
            model: "basic-model".to_owned(),
            streaming: false,
            reasoning: false,
            reasoning_effort: false,
            tools: true,
            parallel_tool_config: false,
            structured_output: false,
            context_caching: false,
            responses_compaction: false,
            context_edits: false,
            context_awareness: false,
            vision: false,
            context_size: 128_000,
        };

        assert!(!basic.has_advanced_features());

        let advanced = ProviderCapabilities {
            structured_output: true,
            ..basic
        };

        assert!(advanced.has_advanced_features());
    }
}
