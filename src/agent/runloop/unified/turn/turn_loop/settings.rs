use anyhow::Result;
use vtcode_core::llm::provider::{self as uni};
use vtcode_core::utils::ansi::MessageStyle;

use super::TurnLoopContext;
use super::context::effective_vt_cfg;
use crate::agent::runloop::unified::model_selection::{ModelSwitchCompactionTargets, finalize_model_selection};
use crate::agent::runloop::unified::session_settings::SessionSettingsControl;
use crate::agent::runloop::unified::turn::turn_loop_helpers::{
    PrecomputedTurnConfig, apply_mode_switch_remaining_tool_call_floor, apply_mode_switch_remaining_tool_loop_floor,
    extract_turn_config, resolve_safety_tool_call_limits,
};

/// Consume completed selections just before the next model request is built.
pub(super) async fn apply_pending_session_settings(
    ctx: &mut TurnLoopContext<'_>,
    working_history: &mut Vec<uni::Message>,
) -> Result<bool> {
    let mut controls = Vec::new();
    if let Some(settings) = ctx.settings.as_mut() {
        while let Ok(control) = settings.receiver.try_recv() {
            controls.push(control);
        }
    }
    let applied = !controls.is_empty();

    for control in controls {
        match control {
            SessionSettingsControl::Model { picker, selection } => {
                let settings = ctx.settings.as_mut().expect("settings context exists while applying selection");
                let vt_cfg = ctx
                    .live_vt_cfg
                    .as_deref_mut()
                    .expect("live config exists while applying selection");
                let snapshot = ctx.tool_registry.harness_context_snapshot();
                if let Err(error) = finalize_model_selection(
                    ctx.renderer,
                    &picker,
                    selection,
                    ctx.config,
                    vt_cfg,
                    ctx.provider_client,
                    settings.session_bootstrap,
                    ctx.handle,
                    settings.header_context,
                    ctx.full_auto,
                    ModelSwitchCompactionTargets {
                        history: working_history,
                        session_stats: ctx.session_stats,
                        context_manager: ctx.context_manager,
                        session_id: &snapshot.session_id,
                        thread_id: settings.thread_id,
                        lifecycle_hooks: ctx.lifecycle_hooks,
                        harness_emitter: ctx.harness_emitter,
                    },
                )
                .await
                {
                    ctx.renderer
                        .line(MessageStyle::Error, &format!("Failed to apply model selection: {error:#}"))?;
                } else if let Some(mut metadata) = settings.thread_handle.metadata() {
                    metadata.provider.clone_from(&ctx.config.provider);
                    metadata.model.clone_from(&ctx.config.model);
                    metadata.reasoning_effort = ctx.config.reasoning_effort.as_str().to_string();
                    settings.thread_handle.replace_metadata(Some(metadata));
                }
            }
        }
    }
    Ok(applied)
}

/// Apply pending mid-turn model/effort switches, then re-derive the cached
/// turn config so the next request uses the effective settings.
///
/// Switches must land before fresh steering input is appended: model-switch
/// compaction summarizes history, so a prompt steered before compaction
/// would be summarized away and never reach the new model verbatim.
/// Returns whether any settings were applied (caller re-verifies prompt
/// delivery after steering when true).
pub(super) async fn apply_mid_turn_settings(
    ctx: &mut TurnLoopContext<'_>,
    working_history: &mut Vec<uni::Message>,
    turn_config: &mut PrecomputedTurnConfig,
    turn_history_start_len: &mut usize,
) -> Result<bool> {
    // Mid-turn model/effort switches must land BEFORE fresh steering input.
    // Model-switch compaction summarizes history, so a prompt steered
    // before compaction would be summarized away and the new model would
    // never see it verbatim (the TUI "Steered..." echo would then lie).
    // Applying settings first guarantees fresh input is appended after
    // compaction plus the resume note and reaches the next request verbatim.
    let history_len_before_settings = working_history.len();
    let settings_applied = apply_pending_session_settings(ctx, working_history).await?;
    if settings_applied {
        // A model or effort switch changes provider capabilities, context
        // budget, and prompt shaping for the request about to be built.
        // Re-derive the cached turn config so the next request uses the
        // effective settings; the in-flight request already finished with
        // its original settings.
        *turn_config = extract_turn_config(
            effective_vt_cfg(ctx.vt_cfg, &ctx.live_vt_cfg),
            ctx.is_planning_active(),
            ctx.renderer.supports_inline_ui(),
        );
        // Compaction may have shrunk history and injected a resume note;
        // rebase the turn baseline so later slicing (final response,
        // verification, TUI) cannot index into compacted-away messages.
        *turn_history_start_len = (*turn_history_start_len).min(working_history.len());
        tracing::info!(
            history_before = history_len_before_settings,
            history_after = working_history.len(),
            turn_history_start_len = *turn_history_start_len,
            "Rebased turn baseline after mid-turn settings/model switch"
        );
    }
    Ok(settings_applied)
}

/// Re-derive turn config and grant a full *remaining* mode floor after a
/// Build<->Plan auto-switch mid-turn, so the new phase is not starved by
/// the prior phase's consumption.
pub(super) fn apply_mid_turn_mode_switch(
    ctx: &mut TurnLoopContext<'_>,
    last_planning_active: &mut bool,
    turn_config: &mut PrecomputedTurnConfig,
    current_max_tool_loops: &mut usize,
    step_count: usize,
) {
    // Build↔Plan auto-switch mid-turn: re-derive turn config and grant a
    // full *remaining* mode floor so the new phase is not starved by the
    // prior phase's consumption (checkpoint turn_804 + user Build→Plan
    // tool-call-limit report).
    let planning_now = ctx.is_planning_active();
    if planning_now != *last_planning_active {
        *last_planning_active = planning_now;
        if planning_now {
            ctx.plan_session.start_turn();
            // Planning just became active inside a running turn: promote to
            // the stage and show the researching row once, since work is live.
            crate::agent::runloop::unified::planning_workflow_state::mark_planning_turn_started(
                ctx.renderer,
                ctx.handle,
            );
        }
        *turn_config = extract_turn_config(
            effective_vt_cfg(ctx.vt_cfg, &ctx.live_vt_cfg),
            planning_now,
            ctx.renderer.supports_inline_ui(),
        );
        if ctx.plan_session.is_interview_denied() {
            turn_config.request_user_input_enabled = false;
        }
        apply_mode_switch_remaining_tool_call_floor(
            &mut ctx.harness_state.max_tool_calls,
            ctx.harness_state.tool_calls,
            planning_now,
        );
        apply_mode_switch_remaining_tool_loop_floor(current_max_tool_loops, step_count, planning_now);
        let (max_per_turn, max_per_session) = resolve_safety_tool_call_limits(
            ctx.harness_state.max_tool_calls,
            turn_config.max_session_turns,
            planning_now,
        );
        ctx.safety_validator.set_limits(max_per_turn, max_per_session);
    }
}

#[cfg(test)]
mod tests {
    use crate::agent::runloop::unified::session_settings::SessionSettingsControl;
    use vtcode_core::config::types::ReasoningEffortLevel;
    use vtcode_core::llm::provider::LLMProvider;

    fn supported_efforts(provider: &dyn LLMProvider, model: &str) -> Vec<ReasoningEffortLevel> {
        provider
            .supported_reasoning_efforts(model)
            .iter()
            .filter_map(|value| ReasoningEffortLevel::parse(value))
            .collect()
    }

    struct StubProvider {
        efforts: &'static [&'static str],
    }

    #[async_trait::async_trait]
    impl LLMProvider for StubProvider {
        fn name(&self) -> &str {
            "stub"
        }

        fn supports_streaming(&self) -> bool {
            false
        }

        async fn generate(
            &self,
            _request: super::uni::LLMRequest,
        ) -> Result<super::uni::LLMResponse, super::uni::LLMError> {
            panic!("stub provider never generates in settings tests")
        }

        fn supported_models(&self) -> Vec<String> {
            vec!["stub-model".to_string()]
        }

        fn supported_reasoning_efforts(&self, _model: &str) -> &'static [&'static str] {
            self.efforts
        }

        fn validate_request(&self, _request: &super::uni::LLMRequest) -> Result<(), super::uni::LLMError> {
            Ok(())
        }
    }

    #[test]
    fn supported_efforts_parses_provider_strings_and_drops_unknown() {
        let provider = StubProvider { efforts: &["low", "medium", "bogus"] };
        let efforts = supported_efforts(&provider, "stub-model");
        assert_eq!(efforts, vec![ReasoningEffortLevel::Low, ReasoningEffortLevel::Medium]);
    }

    #[test]
    fn supported_efforts_empty_when_provider_exposes_none() {
        let provider = StubProvider { efforts: &[] };
        assert!(supported_efforts(&provider, "stub-model").is_empty());
    }

    #[test]
    fn effective_config_prefers_live_over_fallback() {
        use super::super::effective_vt_cfg;
        use vtcode_core::config::loader::VTCodeConfig;
        let mut fallback = VTCodeConfig::default();
        fallback.agent.reasoning_effort = ReasoningEffortLevel::Low;
        let mut live_value = Some(VTCodeConfig::default());
        if let Some(cfg) = live_value.as_mut() {
            cfg.agent.reasoning_effort = ReasoningEffortLevel::High;
        }
        let mut live: Option<&mut Option<VTCodeConfig>> = Some(&mut live_value);
        let effective = effective_vt_cfg(Some(&fallback), &live).expect("live config wins");
        assert_eq!(effective.agent.reasoning_effort, ReasoningEffortLevel::High);

        let mut live_none: Option<VTCodeConfig> = None;
        let mut live_empty: Option<&mut Option<VTCodeConfig>> = Some(&mut live_none);
        let effective_fallback =
            effective_vt_cfg(Some(&fallback), &live_empty).expect("fallback used when live is empty");
        assert_eq!(effective_fallback.agent.reasoning_effort, ReasoningEffortLevel::Low);
        let _ = &mut live;
        let _ = &mut live_empty;
    }

    /// Drain controls through the real request-boundary entry point on a
    /// headless turn context. Returns (applied, live config, header, rendered).
    async fn drain_controls(
        controls: Vec<SessionSettingsControl>,
    ) -> (
        bool,
        Option<vtcode_core::config::loader::VTCodeConfig>,
        vtcode_ui::tui::app::InlineHeaderContext,
        String,
    ) {
        use crate::agent::runloop::unified::turn::turn_processing::test_support::TestTurnProcessingBacking;
        use crate::agent::runloop::welcome::SessionBootstrap;

        let mut backing = TestTurnProcessingBacking::new(4).await;
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        for control in controls {
            sender.send(control).expect("queue boundary control");
        }
        drop(sender);
        let mut header = vtcode_ui::tui::app::InlineHeaderContext::default();
        let bootstrap = SessionBootstrap::default();
        let manager = vtcode_core::core::threads::ThreadManager::new();
        let thread = manager.start_thread_with_identifier(
            "test-thread".to_string(),
            vtcode_core::core::threads::ThreadBootstrap::new(None),
        );
        let thread_id = thread.thread_id().to_string();
        let mut live_cfg: Option<vtcode_core::config::loader::VTCodeConfig> = None;
        let mut working_history = Vec::new();
        let applied = {
            let mut ctx = backing.turn_loop_context();
            ctx.live_vt_cfg = Some(&mut live_cfg);
            ctx.settings = Some(super::super::ActiveSettingsContext {
                receiver: &mut receiver,
                header_context: &mut header,
                session_bootstrap: &bootstrap,
                thread_id: thread_id.as_str(),
                thread_handle: &thread,
            });
            super::apply_pending_session_settings(&mut ctx, &mut working_history)
                .await
                .expect("boundary drain succeeds")
        };
        let rendered = backing.rendered_inline_output();
        (applied, live_cfg, header, rendered)
    }

    #[tokio::test]
    async fn no_pending_controls_leaves_turn_untouched() {
        let (applied, live_cfg, header, rendered) = drain_controls(vec![]).await;

        assert!(!applied, "empty queue must report nothing applied");
        assert!(live_cfg.is_none());
        assert_eq!(header.reasoning, "unavailable");
        assert!(rendered.is_empty(), "nothing may render without a selection: {rendered}");
    }
}
