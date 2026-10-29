use crate::providers::anthropic::capabilities::*;
use vtcode_config::constants::models;

#[test]
fn test_supports_structured_output() {
    assert!(supports_structured_output(models::CLAUDE_SONNET_5, models::anthropic::DEFAULT_MODEL));
    assert!(supports_structured_output("claude-opus-4-5-20251101", models::anthropic::DEFAULT_MODEL));
    assert!(supports_structured_output("claude-sonnet-4-5-20250929", models::anthropic::DEFAULT_MODEL));
    assert!(supports_structured_output(models::CLAUDE_SONNET_5, models::anthropic::DEFAULT_MODEL));
    assert!(!supports_structured_output("claude-3-7-sonnet-test", models::anthropic::DEFAULT_MODEL));
}

#[test]
fn structured_output_models_lists_exactly_the_accepted_models() {
    let listed = structured_output_models();
    for model in models::anthropic::SUPPORTED_MODELS {
        assert!(listed.contains(model), "{model} accepts structured output but is not listed");
    }
    for model in &listed {
        assert!(supports_structured_output(model, ""), "{model} is listed but rejected");
    }
    for rejected in ["claude-sonnet-4-6", "claude-opus-4-8", "claude-haiku-4-5"] {
        assert!(!supports_structured_output(rejected, ""), "{rejected}");
        assert!(!listed.contains(&rejected), "{rejected}");
    }
}

#[test]
fn test_supports_vision() {
    assert!(supports_vision(models::CLAUDE_SONNET_5, models::anthropic::DEFAULT_MODEL));
    assert!(supports_vision("claude-3-opus", models::anthropic::DEFAULT_MODEL));
    assert!(supports_vision("claude-4-sonnet", models::anthropic::DEFAULT_MODEL));
    assert!(supports_vision(models::minimax::MINIMAX_M3_1_FLASH, models::anthropic::DEFAULT_MODEL));
}

#[test]
fn test_supports_effort() {
    assert!(supports_effort(models::CLAUDE_SONNET_5, models::anthropic::DEFAULT_MODEL));
}

#[test]
fn test_effective_context_size() {
    assert_eq!(effective_context_size(models::CLAUDE_SONNET_5), 1_000_000);
    assert_eq!(effective_context_size("claude-sonnet-4-5-latest"), 200_000);
    assert_eq!(effective_context_size("claude-haiku-4-5-latest"), 200_000);
    assert_eq!(effective_context_size("claude-3-opus"), 200_000);
    assert_eq!(effective_context_size(models::minimax::MINIMAX_M3_1_FLASH), 1_000_000);
}

#[test]
fn test_supported_models() {
    let models = supported_models();
    assert!(!models.is_empty());
    assert!(models.iter().any(|m| m.contains("claude")));
}
