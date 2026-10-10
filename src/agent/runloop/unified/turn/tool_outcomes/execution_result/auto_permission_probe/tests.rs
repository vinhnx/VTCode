use super::*;
use crate::agent::runloop::unified::turn::turn_processing::test_support::TestTurnProcessingBacking;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use vtcode_core::llm::provider as uni;

struct CountingProvider {
    calls: Arc<AtomicUsize>,
    eligible: bool,
}

#[async_trait::async_trait]
impl uni::LLMProvider for CountingProvider {
    fn name(&self) -> &str {
        "openai"
    }
    fn supports_decisions(&self) -> bool {
        self.eligible
    }
    async fn decide_choice(&self, _: uni::ChoiceDecisionRequest) -> Result<uni::ChoiceDecisionResponse, uni::LLMError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(uni::ChoiceDecisionResponse {
            answer: Some(uni::ChoiceDecisionAnswer {
                name: "tool_output_injection".into(),
                choice: "SUSPECT".into(),
                confidence: 0.5,
                probabilities: vec![],
            }),
            usage: None,
        })
    }
    async fn generate(&self, _: uni::LLMRequest) -> Result<uni::LLMResponse, uni::LLMError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(uni::LLMResponse {
            content: Some("SAFE".into()),
            model: "fixture-model".into(),
            tool_calls: None,
            usage: None,
            finish_reason: uni::FinishReason::Stop,
            reasoning: None,
            reasoning_details: None,
            organization_id: None,
            request_id: None,
            tool_references: vec![],
            compaction: None,
        })
    }
    fn supported_models(&self) -> Vec<String> {
        vec![]
    }
    fn validate_request(&self, _: &uni::LLMRequest) -> Result<(), uni::LLMError> {
        Ok(())
    }
}

#[tokio::test]
async fn decisions_full_auto_keeps_generation_when_disabled_or_unsupported() {
    for (enabled, eligible) in [(false, true), (true, false)] {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut backing = TestTurnProcessingBacking::new(120).await;
        backing.set_provider(Box::new(CountingProvider { calls: calls.clone(), eligible }));
        let mut config = vtcode_core::config::loader::VTCodeConfig::default();
        config.permissions.auto_permission.use_decisions_probe = enabled;
        backing.set_vt_cfg_for_test(config);
        let mut ctx = backing.turn_processing_context();
        ctx.full_auto = true;
        assert!(
            auto_permission_probe_warning(&mut ctx, "read_file", "output", None)
                .await
                .is_none()
        );
        // Decisions would return SUSPECT. SAFE comes from the generation path.
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn decisions_tui_probe_enabled_preserves_explicit_tool_denial_before_execution() {
    use crate::agent::runloop::unified::turn::tool_outcomes::handlers::{ToolOutcomeContext, handle_single_tool_call};
    use crate::agent::runloop::unified::turn::tool_outcomes::helpers::LoopTracker;
    use vtcode_core::tool_policy::ToolPolicy;
    let calls = Arc::new(AtomicUsize::new(0));
    let mut backing = TestTurnProcessingBacking::new(120).await;
    backing.set_provider(Box::new(CountingProvider { calls: calls.clone(), eligible: true }));
    let mut config = vtcode_core::config::loader::VTCodeConfig::default();
    config.permissions.auto_permission.use_decisions_probe = true;
    backing.set_vt_cfg_for_test(config);
    let mut ctx = backing.turn_processing_context();
    ctx.skip_confirmations = false;
    ctx.tool_registry.set_tool_policy("read_file", ToolPolicy::Deny).await.unwrap();
    let mut attempts = LoopTracker::new();
    let mut modified = std::collections::BTreeSet::new();
    let mut outcome_ctx = ToolOutcomeContext {
        ctx: &mut ctx,
        repeated_tool_attempts: &mut attempts,
        turn_modified_files: &mut modified,
    };
    assert!(
        handle_single_tool_call(&mut outcome_ctx, "denied", "read_file", serde_json::json!({"path":"README.md"}))
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(outcome_ctx.ctx.harness_state.tool_calls, 0);
    assert!(
        outcome_ctx
            .ctx
            .working_history
            .iter()
            .any(|message| message.content.as_text().contains("execution denied by policy"))
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn decisions_probe_admission_preserves_empty_planning_and_three_dispatch_budget() {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut backing = TestTurnProcessingBacking::new(120).await;
    backing.set_provider(Box::new(CountingProvider { calls: calls.clone(), eligible: true }));
    let mut config = vtcode_core::config::loader::VTCodeConfig::default();
    config.permissions.auto_permission.use_decisions_probe = true;
    backing.set_vt_cfg_for_test(config);
    {
        let mut ctx = backing.turn_processing_context();
        assert!(!ctx.full_auto);
        assert!(ctx.renderer.supports_inline_ui());
        assert!(
            auto_permission_probe_warning(&mut ctx, "read_file", " \n\t", None)
                .await
                .is_none()
        );
        assert!(ctx.harness_state.can_spend_auto_permission_probe_model_call());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        for _ in 0..3 {
            let warning = auto_permission_probe_warning(&mut ctx, "read_file", "instructions", None)
                .await
                .unwrap();
            append_probe_warning(&mut ctx, "read_file", warning).unwrap();
        }
        assert!(
            auto_permission_probe_warning(&mut ctx, "read_file", "fourth output", None)
                .await
                .is_none()
        );
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        // Batch publication queues one model-facing warning until it is flushed.
        flush_auto_permission_probe_warning(&mut ctx);
        let count = ctx
            .working_history
            .iter()
            .filter(|message| message.content.as_text().contains("potentially malicious prompt injection"))
            .count();
        assert_eq!(count, 1);
        flush_auto_permission_probe_warning(&mut ctx);
        assert_eq!(ctx.working_history.len(), 1);
    }
    backing.enable_planning();
    let mut ctx = backing.turn_processing_context();
    ctx.full_auto = true;
    assert!(
        auto_permission_probe_warning(&mut ctx, "read_file", "output", None)
            .await
            .is_none()
    );
    assert_eq!(calls.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn decisions_probe_cancelled_admission_makes_zero_requests() {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut backing = TestTurnProcessingBacking::new(120).await;
    backing.set_provider(Box::new(CountingProvider { calls: calls.clone(), eligible: true }));
    let mut config = vtcode_core::config::loader::VTCodeConfig::default();
    config.permissions.auto_permission.use_decisions_probe = true;
    backing.set_vt_cfg_for_test(config);
    let mut ctx = backing.turn_processing_context();
    ctx.ctrl_c_state.request_local_cancel();
    assert!(
        auto_permission_probe_warning(&mut ctx, "read_file", "output", None)
            .await
            .is_none()
    );
    assert!(ctx.harness_state.can_spend_auto_permission_probe_model_call());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn decisions_tui_probe_reevaluates_provider_and_settings_without_spending_rejected_budget() {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut backing = TestTurnProcessingBacking::new(120).await;
    let mut config = vtcode_core::config::loader::VTCodeConfig::default();
    for (enabled, eligible, admitted) in [
        (false, true, false),
        (true, false, false),
        (true, true, true),
        (true, false, false),
        (false, true, false),
        (true, true, true),
        (true, true, true),
    ] {
        config.permissions.auto_permission.use_decisions_probe = enabled;
        backing.set_vt_cfg_for_test(config.clone());
        backing.set_provider(Box::new(CountingProvider { calls: calls.clone(), eligible }));
        let mut ctx = backing.turn_processing_context();
        assert!(!ctx.full_auto);
        let before = calls.load(Ordering::SeqCst);
        assert_eq!(
            auto_permission_probe_warning(&mut ctx, "read_file", "output", None)
                .await
                .is_some(),
            admitted
        );
        assert_eq!(calls.load(Ordering::SeqCst), before + usize::from(admitted));
    }
    let mut ctx = backing.turn_processing_context();
    assert!(!ctx.harness_state.can_spend_auto_permission_probe_model_call());
    assert!(
        auto_permission_probe_warning(&mut ctx, "read_file", "fourth", None)
            .await
            .is_none()
    );
    assert_eq!(calls.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn decisions_tui_probe_headless_planning_and_stopped_paths_do_not_consume_budget() {
    for boundary in ["headless", "planning", "cancelled", "handled", "exit", "missing_config"] {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut backing = TestTurnProcessingBacking::new(120).await;
        backing.set_provider(Box::new(CountingProvider { calls: calls.clone(), eligible: true }));
        if boundary != "missing_config" {
            let mut config = vtcode_core::config::loader::VTCodeConfig::default();
            config.permissions.auto_permission.use_decisions_probe = true;
            backing.set_vt_cfg_for_test(config);
        }
        if boundary == "planning" {
            backing.enable_planning();
        }
        let mut ctx = backing.turn_processing_context();
        match boundary {
            "headless" => *ctx.renderer = vtcode_core::utils::ansi::AnsiRenderer::stdout(),
            "cancelled" => ctx.ctrl_c_state.request_local_cancel(),
            "handled" => {
                ctx.ctrl_c_state.request_local_cancel();
                ctx.ctrl_c_state.mark_cancel_handled();
            }
            "exit" => ctx.ctrl_c_state.request_exit(),
            _ => {}
        }
        assert!(
            auto_permission_probe_warning(&mut ctx, "read_file", "output", None)
                .await
                .is_none(),
            "{boundary}"
        );
        // All three dispatch slots remain available after rejection.
        for _ in 0..3 {
            assert!(ctx.harness_state.can_spend_auto_permission_probe_model_call(), "{boundary}");
            ctx.harness_state.record_auto_permission_probe_model_call();
        }
        assert_eq!(calls.load(Ordering::SeqCst), 0, "{boundary}");
    }
}
