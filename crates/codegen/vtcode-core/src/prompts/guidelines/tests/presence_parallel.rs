use super::*;

#[test]
fn tool_presence_flags_record_decision_in_the_single_pass() {
    let with = ToolPresence::of(&["matrix".to_string(), "record_decision".to_string()]);
    assert!(with.matrix);
    assert!(with.record_decision);
    let without = ToolPresence::of(&["matrix".to_string()]);
    assert!(without.matrix);
    assert!(!without.record_decision);
}
#[test]
fn public_decision_guidance_is_optional_and_bounded() {
    for profile in [ToolGuidanceProfile::Minimal, ToolGuidanceProfile::Default] {
        let enabled = generate_tool_guidelines_with_capabilities(
            &[TOOL_READ_FILE.into(), "record_decision".into()],
            None,
            ResolvedShellPromptProfile::UnixLike,
            profile,
            false,
        );
        let disabled = generate_tool_guidelines_with_capabilities(
            &[TOOL_READ_FILE.into()],
            None,
            ResolvedShellPromptProfile::UnixLike,
            profile,
            false,
        );
        assert!(enabled.contains(PUBLIC_DECISION_GUIDANCE));
        assert!(!disabled.contains("record_decision"));
        assert!(enabled.len().saturating_sub(disabled.len()) < 300);
        assert!(!enabled.contains("chain of thought"));
    }
}
#[test]
fn tool_guidance_uses_actual_parallel_capabilities_for_three_families() {
    use crate::config::constants::models;
    use crate::llm::provider::LLMProvider;
    use crate::llm::providers::{AnthropicProvider, GeminiProvider, OpenAIProvider};
    let providers: [(Box<dyn LLMProvider>, &str); 3] = [
        (Box::new(OpenAIProvider::new("offline-fixture".into())), models::openai::DEFAULT_MODEL),
        (Box::new(AnthropicProvider::new("offline-fixture".into())), models::anthropic::DEFAULT_MODEL),
        (Box::new(GeminiProvider::new("offline-fixture".into())), models::google::DEFAULT_MODEL),
    ];
    for (provider, model) in providers {
        for profile in [ToolGuidanceProfile::Minimal, ToolGuidanceProfile::Default] {
            let parallel = provider.supports_parallel_tool_config(model);
            let text = generate_tool_guidelines_with_capabilities(
                &[TOOL_EXEC_COMMAND.to_owned()],
                None,
                ResolvedShellPromptProfile::UnixLike,
                profile,
                parallel,
            );
            assert_eq!(text.contains("tools in parallel"), parallel);
            let serial = generate_tool_guidelines_with_capabilities(
                &[TOOL_EXEC_COMMAND.to_owned()],
                None,
                ResolvedShellPromptProfile::UnixLike,
                profile,
                false,
            );
            assert!(!serial.contains("tools in parallel"));
        }
    }
}
#[test]
fn test_read_only_capability_detection() {
    let tools = vec![TOOL_CODE_SEARCH.to_string()];
    let guidelines = guidelines_for(&tools, None);
    assert!(guidelines.contains("Capabilities: read-only"));
    assert!(guidelines.contains("do not modify files"));
}
#[test]
fn test_tool_preference_guidance() {
    let tools = vec![TOOL_EXEC_COMMAND.to_string(), TOOL_CODE_SEARCH.to_string()];
    let guidelines = generate_tool_guidelines_for_profile(&tools, None, ResolvedShellPromptProfile::UnixLike, true);
    assert!(guidelines.contains("literal and smart-case"));
    assert!(guidelines.contains("Do not JSON-encode arrays or integers as strings"));
    // The query/filter shape is documented by the code_search schema.
    assert!(!guidelines.contains("Advanced `code_search` takes `query`"));
    assert!(!guidelines.contains("omit unused filters"));
    assert!(!guidelines.contains("path: \"\""));
    // Search steering: with both tools present the browse line must
    // prefer `code_search` over `rg` for code (session evidence showed
    // `rg`-via-exec crowding out `code_search` 654:15).
    assert!(guidelines.contains("Prefer `code_search` over `rg`/`grep` for code"));
    // Latency steering: fast checks before full builds (tool-agnostic).
    assert!(guidelines.contains("Run fast checks before full builds"));
    // Completion-as-checkpoint guidance lives in the operating profiles;
    // the guidelines section no longer repeats it.
    assert!(!guidelines.contains("Completion is a checkpoint"));
}
#[test]
fn edit_guidance_keeps_patch_permissions_without_repeating_universal_inspection() {
    for profile in [ToolGuidanceProfile::Minimal, ToolGuidanceProfile::Default] {
        let guidance = generate_tool_guidelines_with_capabilities(
            &[TOOL_APPLY_PATCH.to_owned()],
            None,
            ResolvedShellPromptProfile::UnixLike,
            profile,
            false,
        );
        assert!(!guidance.contains("Inspect relevant source, edit a bounded region"));
        // The apply_patch call shape and "keep patches small" rule are
        // documented by the tool description.
        assert!(!guidance.contains("`apply_patch`"));
        assert!(!guidance.contains("patches small"));
        assert!(guidance.contains("diff stays bounded"));
        assert!(guidance.contains("terminal permission stays authoritative"));
        assert!(guidance.contains("untrusted"));
        assert!(vtcode_commons::estimate_tokens(&guidance) < 180);
    }
}
