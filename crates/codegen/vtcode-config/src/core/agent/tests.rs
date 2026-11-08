use super::*;

#[test]
fn test_continuation_policy_defaults_and_parses() {
    assert_eq!(ContinuationPolicy::default(), ContinuationPolicy::All);
    assert_eq!(ContinuationPolicy::parse("off"), Some(ContinuationPolicy::Off));
    assert_eq!(ContinuationPolicy::parse("exec-only"), Some(ContinuationPolicy::ExecOnly));
    assert_eq!(ContinuationPolicy::parse("all"), Some(ContinuationPolicy::All));
    assert_eq!(ContinuationPolicy::parse("invalid"), None);
}

#[test]
fn test_tracker_continuation_defaults_and_deserializes() {
    assert!(TrackerContinuationConfig::default().auto_continue_tracker);
    assert_eq!(TrackerContinuationConfig::default().cross_turn_turns, 32);
    let parsed: AgentHarnessConfig =
        toml::from_str("[continuation]\nauto_continue_tracker = false\ncross_turn_turns = 3")
            .expect("valid harness config");
    assert!(!parsed.continuation.auto_continue_tracker);
    assert_eq!(parsed.continuation.cross_turn_turns, 3);
    let fallback: AgentHarnessConfig = toml::from_str("").expect("empty harness config");
    assert!(fallback.continuation.auto_continue_tracker);
    assert_eq!(fallback.continuation.cross_turn_turns, 32);
}

#[test]
fn test_harness_config_continuation_policy_deserializes_with_fallback() {
    let parsed: AgentHarnessConfig = toml::from_str("continuation_policy = \"all\"").expect("valid harness config");
    assert_eq!(parsed.continuation_policy, ContinuationPolicy::All);

    let fallback: AgentHarnessConfig = toml::from_str("continuation_policy = \"unexpected\"").expect("fallback config");
    assert_eq!(fallback.continuation_policy, ContinuationPolicy::All);
}

#[test]
fn test_harness_config_tool_call_budget_defaults_to_120() {
    assert_eq!(AgentHarnessConfig::default().max_tool_calls_per_turn, tool_limits::DEFAULT_MAX_TOOL_CALLS_PER_TURN);

    let parsed: AgentHarnessConfig = toml::from_str("").expect("default harness config");
    assert_eq!(parsed.max_tool_calls_per_turn, tool_limits::DEFAULT_MAX_TOOL_CALLS_PER_TURN);
}

#[test]
fn test_harness_orchestration_mode_defaults_and_parses() {
    assert_eq!(HarnessOrchestrationMode::default(), HarnessOrchestrationMode::PlanBuildEvaluate);
    assert_eq!(HarnessOrchestrationMode::parse("single"), Some(HarnessOrchestrationMode::Single));
    assert_eq!(
        HarnessOrchestrationMode::parse("plan_build_evaluate"),
        Some(HarnessOrchestrationMode::PlanBuildEvaluate)
    );
    assert_eq!(
        HarnessOrchestrationMode::parse("planner-generator-evaluator"),
        Some(HarnessOrchestrationMode::PlanBuildEvaluate)
    );
    assert_eq!(HarnessOrchestrationMode::parse("unexpected"), None);
}

#[test]
fn test_harness_config_orchestration_deserializes_with_fallback() {
    let parsed: AgentHarnessConfig =
        toml::from_str("orchestration_mode = \"plan_build_evaluate\"").expect("valid harness config");
    assert_eq!(parsed.orchestration_mode, HarnessOrchestrationMode::PlanBuildEvaluate);
    assert_eq!(parsed.max_revision_rounds, 2);

    let fallback: AgentHarnessConfig = toml::from_str("orchestration_mode = \"unexpected\"").expect("fallback config");
    assert_eq!(fallback.orchestration_mode, HarnessOrchestrationMode::PlanBuildEvaluate);
}

/// Drift guard for hand-written enum string mappings. Each of these enums
/// exposes the same value through serde, `as_str()`, and `parse()`; see
/// [`crate::test_support::assert_string_enum_lockstep`].
#[test]
fn string_config_enums_keep_as_str_and_serde_in_lockstep() {
    use crate::test_support::assert_string_enum_lockstep;

    assert_string_enum_lockstep!(
        ContinuationPolicy,
        [
            ContinuationPolicy::Off,
            ContinuationPolicy::ExecOnly,
            ContinuationPolicy::All
        ]
    );
    assert_string_enum_lockstep!(
        ContextResetMode,
        [
            ContextResetMode::Off,
            ContextResetMode::OnStall,
            ContextResetMode::OnCompaction
        ]
    );
    assert_string_enum_lockstep!(
        HarnessOrchestrationMode,
        [
            HarnessOrchestrationMode::Single,
            HarnessOrchestrationMode::PlanBuildEvaluate
        ]
    );
}

#[test]
fn test_verification_auto_recovery_defaults_to_bounded_auto_execute() {
    let config = VerificationAutoRecoveryConfig::default();
    assert!(config.auto_execute);
    assert_eq!(config.in_turn_attempts, 2);
    assert_eq!(config.cross_turn_turns, 2);
    assert_eq!(config.default_verifier_override, None);
    assert_eq!(config.max_consecutive_failures, 3);
}

#[test]
fn test_verification_auto_recovery_survives_missing_field_for_backward_compatibility() {
    // Older vtcode.toml files predate [agent.harness.verification]: the
    // whole table and each field must fall back to defaults.
    let without_table: AgentHarnessConfig = toml::from_str("").expect("default harness config");
    assert!(without_table.verification.auto_execute);
    assert_eq!(without_table.verification.in_turn_attempts, 2);

    let partial: VerificationAutoRecoveryConfig =
        toml::from_str("auto_execute = false").expect("minimal verification config parses");
    assert!(!partial.auto_execute);
    assert_eq!(partial.in_turn_attempts, 2);
    assert_eq!(partial.max_consecutive_failures, 3);

    let full: VerificationAutoRecoveryConfig = toml::from_str(
        "auto_execute = true\nin_turn_attempts = 1\ncross_turn_turns = 0\ndefault_verifier_override = \"cargo nextest run -p mycrate\"\nmax_consecutive_failures = 5",
    )
    .expect("full verification config parses");
    assert_eq!(full.in_turn_attempts, 1);
    assert_eq!(full.cross_turn_turns, 0);
    assert_eq!(full.default_verifier_override.as_deref(), Some("cargo nextest run -p mycrate"));
    assert_eq!(full.max_consecutive_failures, 5);
}

#[test]
fn test_plan_confirmation_config_default() {
    let config = AgentConfig::default();
    assert!(config.require_plan_confirmation);
}

#[test]
fn test_system_prompt_budget_defaults() {
    let config = AgentConfig::default();
    assert_eq!(config.max_system_prompt_tokens, prompt_budget::DEFAULT_MAX_SYSTEM_PROMPT_TOKENS);
    assert!(config.system_prompt_budget_warning);
    assert!(config.trim_system_prompt);

    let parsed: AgentConfig = toml::from_str(
        r#"
max_system_prompt_tokens = 4000
system_prompt_budget_warning = false
trim_system_prompt = true
"#,
    )
    .expect("agent config should parse");
    assert_eq!(parsed.max_system_prompt_tokens, 4000);
    assert!(!parsed.system_prompt_budget_warning);
    assert!(parsed.trim_system_prompt);
}

#[test]
fn test_budget_warning_threshold_default_and_validation() {
    let config = AgentConfig::default();
    assert!((config.harness.budget_warning_threshold - 0.75).abs() < f64::EPSILON);
    assert!(config.validate_llm_params().is_ok());

    let parsed: AgentHarnessConfig = toml::from_str(
        r#"
max_budget_usd = 5.0
budget_warning_threshold = 0.5
"#,
    )
    .expect("harness config should parse");
    assert_eq!(parsed.max_budget_usd, Some(5.0));
    assert!((parsed.budget_warning_threshold - 0.5).abs() < f64::EPSILON);

    let mut invalid = AgentConfig::default();
    invalid.harness.budget_warning_threshold = 1.5;
    assert!(invalid.validate_llm_params().is_err());
}

#[test]
fn test_nested_temperature_validation_matches_top_level_range() {
    let config = AgentConfig::default();
    assert!(config.validate_llm_params().is_ok());

    for value in [0.0, 0.3, 1.0] {
        let mut valid = AgentConfig::default();
        valid.small_model.temperature = value;
        valid.prompt_suggestions.temperature = value;
        assert!(valid.validate_llm_params().is_ok(), "temperature {value} should be accepted");
    }

    for value in [-0.1, 1.5, f32::NAN] {
        let mut invalid = AgentConfig::default();
        invalid.small_model.temperature = value;
        let err = invalid
            .validate_llm_params()
            .expect_err("small_model.temperature should be rejected");
        assert!(err.contains("small_model.temperature"), "unexpected error: {err}");

        let mut invalid = AgentConfig::default();
        invalid.prompt_suggestions.temperature = value;
        let err = invalid
            .validate_llm_params()
            .expect_err("prompt_suggestions.temperature should be rejected");
        assert!(err.contains("prompt_suggestions.temperature"), "unexpected error: {err}");
    }
}

#[test]
fn test_persistent_memory_is_disabled_by_default() {
    let config = AgentConfig::default();
    assert!(!config.persistent_memory.enabled);
    assert!(config.persistent_memory.auto_write);
}

#[test]
fn test_tool_result_clearing_defaults() {
    let config = AgentConfig::default();
    let clearing = config.harness.tool_result_clearing;

    assert!(clearing.enabled);
    assert_eq!(clearing.trigger_tokens, 40_000);
    assert_eq!(clearing.keep_tool_uses, 2);
    assert_eq!(clearing.keep_recent_tool_batches, 2);
    assert_eq!(clearing.clear_at_least_tokens, 30_000);
    assert!(clearing.clear_tool_inputs);
}

#[test]
fn test_tool_result_clearing_missing_key_defaults_clear_tool_inputs_true() {
    let parsed: AgentHarnessConfig = toml::from_str(
        r#"
            [tool_result_clearing]
            enabled = true
            trigger_tokens = 40000
            keep_tool_uses = 2
            clear_at_least_tokens = 30000
        "#,
    )
    .expect("valid harness config");

    assert!(parsed.tool_result_clearing.clear_tool_inputs);
    assert_eq!(parsed.tool_result_clearing.keep_recent_tool_batches, 2);
}

#[test]
fn tool_result_clearing_recent_batches_round_trip_including_compatibility() {
    for batches in [0, 1, 2, 9] {
        let parsed: AgentHarnessConfig =
            toml::from_str(&format!("[tool_result_clearing]\nkeep_recent_tool_batches = {batches}")).unwrap();
        assert!(parsed.tool_result_clearing.validate().is_ok());
        let restored: AgentHarnessConfig = toml::from_str(&toml::to_string(&parsed).unwrap()).unwrap();
        assert_eq!(restored.tool_result_clearing.keep_recent_tool_batches, batches);
    }
}

#[test]
fn test_tool_result_clearing_explicit_false_opts_out_of_input_clearing() {
    let parsed: AgentHarnessConfig = toml::from_str(
        r#"
            [tool_result_clearing]
            clear_tool_inputs = false
        "#,
    )
    .expect("valid harness config");

    assert!(!parsed.tool_result_clearing.clear_tool_inputs);
}

#[test]
fn test_codex_app_server_experimental_features_default_to_disabled() {
    let config = AgentConfig::default();

    assert!(!config.codex_app_server.experimental_features);
}

#[test]
fn test_codex_app_server_experimental_features_parse_from_toml() {
    let parsed: AgentCodexAppServerConfig = toml::from_str(
        r#"
            command = "codex"
            args = ["app-server"]
            startup_timeout_secs = 15
            experimental_features = true
        "#,
    )
    .expect("valid codex app-server config");

    assert!(parsed.experimental_features);
    assert_eq!(parsed.startup_timeout_secs, 15);
}

#[test]
fn test_tool_result_clearing_parses_and_validates() {
    let parsed: AgentHarnessConfig = toml::from_str(
        r#"
            [tool_result_clearing]
            enabled = true
            trigger_tokens = 123456
            keep_tool_uses = 6
            clear_at_least_tokens = 4096
            clear_tool_inputs = true
        "#,
    )
    .expect("valid harness config");

    assert!(parsed.tool_result_clearing.enabled);
    assert_eq!(parsed.tool_result_clearing.trigger_tokens, 123_456);
    assert_eq!(parsed.tool_result_clearing.keep_tool_uses, 6);
    assert_eq!(parsed.tool_result_clearing.clear_at_least_tokens, 4_096);
    assert!(parsed.tool_result_clearing.clear_tool_inputs);
    assert!(parsed.tool_result_clearing.validate().is_ok());
}

#[test]
fn test_tool_result_clearing_rejects_zero_values() {
    let clearing = ToolResultClearingConfig {
        trigger_tokens: 0,
        ..ToolResultClearingConfig::default()
    };
    assert!(clearing.validate().is_err());

    let clearing = ToolResultClearingConfig {
        keep_tool_uses: 0,
        ..ToolResultClearingConfig::default()
    };
    assert!(clearing.validate().is_err());

    let clearing = ToolResultClearingConfig {
        clear_at_least_tokens: 0,
        ..ToolResultClearingConfig::default()
    };
    assert!(clearing.validate().is_err());
}

#[test]
fn test_structured_reasoning_is_opt_in_for_every_prompt_mode() {
    let default_prompt_config = AgentConfig {
        system_prompt_mode: SystemPromptMode::Default,
        ..Default::default()
    };
    assert!(!default_prompt_config.should_include_structured_reasoning_tags());

    let specialized_mode = AgentConfig {
        system_prompt_mode: SystemPromptMode::Specialized,
        ..Default::default()
    };
    assert!(!specialized_mode.should_include_structured_reasoning_tags());

    let minimal_mode = AgentConfig {
        system_prompt_mode: SystemPromptMode::Minimal,
        ..Default::default()
    };
    assert!(!minimal_mode.should_include_structured_reasoning_tags());

    let lightweight_mode = AgentConfig {
        system_prompt_mode: SystemPromptMode::Lightweight,
        ..Default::default()
    };
    assert!(!lightweight_mode.should_include_structured_reasoning_tags());
}

#[test]
fn test_structured_reasoning_explicit_override() {
    let mut config = AgentConfig {
        system_prompt_mode: SystemPromptMode::Minimal,
        include_structured_reasoning_tags: Some(true),
        ..AgentConfig::default()
    };
    assert!(config.should_include_structured_reasoning_tags());

    config.include_structured_reasoning_tags = Some(false);
    assert!(!config.should_include_structured_reasoning_tags());
}
