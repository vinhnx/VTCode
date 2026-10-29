use super::*;

#[test]
fn duckduckgo_guidance_is_present_only_with_search_and_stays_bounded() {
    let enabled = guidelines_for(&["web_search".to_owned()], None);
    let disabled = guidelines_for(&[], None);
    assert!(enabled.contains("keyless DuckDuckGo"));
    assert!(enabled.contains("separate approval"));
    assert!(enabled.contains("antiban_blocked"));
    assert!(enabled.contains("retryable: false"));
    assert!(enabled.contains("retry_after_ms"));
    assert!(!disabled.contains("DuckDuckGo results"));
    assert!(enabled.len().saturating_sub(disabled.len()) < 250);
}
#[test]
fn model_tool_prompt_reuse_matches_rebuild_across_density_and_capabilities() {
    use crate::config::constants::models;
    use crate::llm::provider::LLMProvider;
    use crate::llm::providers::{AnthropicProvider, HuggingFaceProvider};

    struct ProviderCase {
        provider: Box<dyn LLMProvider>,
        model: &'static str,
        parallel: bool,
    }
    let providers = [
        ProviderCase {
            provider: Box::new(AnthropicProvider::new("offline-fixture".into())),
            model: models::anthropic::DEFAULT_MODEL,
            parallel: true,
        },
        ProviderCase {
            provider: Box::new(HuggingFaceProvider::new("offline-fixture".into())),
            model: "offline-fixture",
            parallel: false,
        },
    ];
    let initial = "Base prompt\n\n## Active Tools\nold guidance\n\n# PLANNING WORKFLOW (READ-ONLY)\nKeep the canonical contract.\n\n[Runtime Tool Catalog]\nold metadata\n\n## Tail\nKeep this section.\n\n";
    for case in providers {
        assert_eq!(case.provider.supports_parallel_tool_config(case.model), case.parallel);
        for names in [
            None,
            Some(&[][..]),
            Some(&[TOOL_TASK_TRACKER, TOOL_REQUEST_USER_INPUT][..]),
            Some(&[TOOL_READ_FILE][..]),
            Some(&[TOOL_WRITE_STDIN][..]),
            Some(&[TOOL_EXEC_COMMAND, TOOL_APPLY_PATCH, TOOL_CODE_SEARCH, "record_decision"][..]),
        ] {
            for planning in [false, true] {
                let snapshot = SessionToolCatalogSnapshot::new(
                    7,
                    9,
                    planning,
                    true,
                    names.map(|names| {
                        std::sync::Arc::new(
                            names
                                .iter()
                                .map(|name| {
                                    ToolDefinition::function(
                                        (*name).to_string(),
                                        "fixture".to_string(),
                                        serde_json::json!({"type": "object"}),
                                    )
                                })
                                .collect(),
                        )
                    }),
                    false,
                );
                for shell in [
                    ResolvedShellPromptProfile::UnixLike,
                    ResolvedShellPromptProfile::PowerShell,
                ] {
                    for metadata in [false, true] {
                        let mut measured = initial.to_string();
                        append_runtime_tool_prompt_sections_for_profile(&mut measured, &snapshot, metadata, shell);
                        let measured_tokens = vtcode_commons::estimate_tokens(&measured) as u64;
                        for budget in [1, measured_tokens - 1, measured_tokens, 100_000] {
                            let mut config = crate::config::VTCodeConfig::default();
                            config.context.max_context_tokens = 128_000;
                            config.agent.max_system_prompt_tokens = budget;
                            let expected = rebuilt_model_tool_prompt(
                                initial.to_string(),
                                &snapshot,
                                metadata,
                                shell,
                                case.provider.as_ref(),
                                case.model,
                                &config,
                            );
                            let mut actual = initial.to_string();
                            append_runtime_tool_prompt_sections_for_model(
                                &mut actual,
                                &snapshot,
                                metadata,
                                shell,
                                case.provider.as_ref(),
                                case.model,
                                Some(&config),
                            );

                            assert_eq!(
                                actual,
                                expected,
                                "provider={}, names={names:?}, planning={planning}, shell={shell:?}, metadata={metadata}, budget={budget}",
                                case.provider.name()
                            );
                            assert_eq!(
                                actual.matches("## Active Tools").count(),
                                usize::from(budget >= measured_tokens || names.is_some_and(|names| !names.is_empty()))
                            );
                            assert!(actual.contains("# PLANNING WORKFLOW (READ-ONLY)\nKeep the canonical contract."));
                            assert!(actual.contains("## Tail\nKeep this section."));
                        }
                    }
                }
            }
        }
    }
}
#[test]
fn matrix_guidance_has_presence_and_budget_in_both_densities() {
    for profile in [ToolGuidanceProfile::Default, ToolGuidanceProfile::Minimal] {
        let enabled = generate_tool_guidelines_with_capabilities(
            &["matrix".to_string(), "request_user_input".to_string()],
            None,
            ResolvedShellPromptProfile::UnixLike,
            profile,
            false,
        );
        let disabled = generate_tool_guidelines_with_capabilities(
            &["request_user_input".to_string()],
            None,
            ResolvedShellPromptProfile::UnixLike,
            profile,
            false,
        );
        assert_eq!(enabled.matches(MATRIX_GUIDANCE).count(), 1);
        assert!(enabled.contains("declared input changes"));
        assert!(enabled.contains("effective permissions and sandbox gates"));
        assert!(enabled.contains("Replay restores admission before discovery"));
        assert!(enabled.contains("user cancel is terminal"));
        assert!(!disabled.contains("scheduler-owned"));
        assert!(vtcode_commons::estimate_tokens(&enabled) <= 220, "{profile:?}: {enabled}");
    }
}
