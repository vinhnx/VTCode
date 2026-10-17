use super::config_trace_filter;
use tracing_subscriber::prelude::*;

#[test]
fn config_filter_applies_info_to_each_default_target_including_llm() {
    let filter = tracing_subscriber::EnvFilter::new(config_trace_filter(&[], "info"));
    tracing::subscriber::with_default(tracing_subscriber::registry().with(filter), || {
        assert!(!tracing::info_span!(target: "vtcode::probe", "binary_info").is_disabled());
        assert!(!tracing::info_span!(target: "vtcode_core::tools", "core_info").is_disabled());
        assert!(!tracing::info_span!(target: "vtcode_llm::providers::openai", "provider_info").is_disabled());
        assert!(tracing::debug_span!(target: "vtcode_core::tools", "core_debug").is_disabled());
        assert!(tracing::debug_span!(target: "vtcode::probe", "binary_debug").is_disabled());
    });
}

#[test]
fn config_filter_respects_custom_targets_and_debug_level() {
    let filter = tracing_subscriber::EnvFilter::new(config_trace_filter(&["vtcode_llm".to_string()], "debug"));
    tracing::subscriber::with_default(tracing_subscriber::registry().with(filter), || {
        assert!(!tracing::debug_span!(target: "vtcode_llm::providers::openai", "provider_debug").is_disabled());
        assert!(tracing::trace_span!(target: "vtcode_llm::providers::openai", "provider_trace").is_disabled());
        assert!(tracing::info_span!(target: "vtcode::probe", "excluded_binary").is_disabled());
    });
}
