use super::AgentRunner;
#[cfg(test)]
use super::execute_helpers::record_terminal_turn_event;
use super::execute_helpers::{
    discard_refused_assistant_message, prepare_responses_request_messages, stop_reason_from_finish_reason,
};
use super::headless_planning;
use crate::config::constants::tools;
use crate::config::models::Provider as ModelProvider;
use crate::config::tool_loop_limit_reached;
use crate::config::types::ReasoningEffortLevel;
use crate::config::{build_session_affinity_prompt_cache_key, session_affinity_key_enabled};
use crate::core::agent::harness_kernel::{HarnessRequestPlanInput, build_harness_request_plan};
use crate::core::agent::refusal;
use crate::core::agent::runtime::RuntimeControl;
use crate::core::agent::task::{ContextItem, Task, TaskOutcome, TaskResults};
use crate::exec::events::planning::{PlanningOutcome, PlanningRecoverySnapshot, PlanningResponseClass};
use crate::llm::provider::{Message, ToolChoice, supports_responses_chaining};
use crate::tools::handlers::planning_workflow::persist_plan_draft;
use crate::utils::colors::style;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::Arc;
use tracing::warn;
use vtcode_commons::diagnostics::{Diagnostic, DiagnosticSeverity};

pub(super) use super::runtime_prompt::RuntimePromptBundle;

/// An aborted headless parent cannot leave scheduler-owned commands running.
struct MatrixExecutionCleanup(Option<Arc<crate::subagents::SubagentController>>);
impl Drop for MatrixExecutionCleanup {
    fn drop(&mut self) {
        if let Some(controller) = self.0.take().filter(|controller| controller.matrix_is_driving()) {
            controller.request_matrix_stop();
            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                handle.spawn(async move {
                    if let Err(error) = controller.cancel_matrix().await {
                        tracing::warn!(%error, "failed to persist aborted matrix cancellation");
                    }
                });
            }
        }
    }
}

impl AgentRunner {
    /// Execute a task with this agent
    pub async fn execute_task(&mut self, task: &Task, contexts: &[ContextItem]) -> Result<TaskResults> {
        let _matrix_cleanup = MatrixExecutionCleanup(self.tool_registry.subagent_controller());
        self.tool_registry.begin_tracker_request(false);
        self.tool_registry.begin_patch_recovery_turn();
        // Phase 1: Setup — harness alignment, conversation building, session init,
        // orchestration planning. Extracted to `prepare_task_execution` for testability.
        let setup = self.prepare_task_execution(task, contexts).await?;

        let agent_prefix = setup.agent_prefix;
        let session_store_handle = setup.session_store_handle;
        let mut event_recorder = setup.event_recorder;
        let run_started_at = setup.run_started_at;
        let is_simple_task = setup.is_simple_task;
        let mut prompt_bundle = setup.prompt_bundle;
        let preserve_recent_turns = setup.preserve_recent_turns;
        let max_tool_loops = setup.max_tool_loops;
        let max_context_tokens = setup.max_context_tokens;
        let mut runtime = setup.runtime;
        self.tool_registry
            .restore_completion_verification_from_history(&runtime.state.messages);
        // Normalize usage from the provider actually serving this session;
        // configuration may contain an alias (or be inferred from the model).
        runtime.state.stats.provider_name = self.provider_client.name().to_string();
        let mut continuation_controller = setup.continuation_controller;
        let effective_task = setup.effective_task;
        let orchestration_enabled = setup.orchestration_enabled;
        let mut budget_warning_emitted = false;
        let mut session_costs = crate::llm::usage_cost::SessionCostAccumulator::default();
        let max_budget_usd = setup.max_budget_usd;
        let max_revision_rounds = setup.max_revision_rounds;
        let mut revision_rounds_used = 0usize;
        let mut verification_recovery_used = false;
        let mut should_write_blocked_handoff = false;
        let mut planning_synthesis_required = false;
        let mut planning_recovery = PlanningRecoverySnapshot::default();
        let mut planning_failure_detail = None;
        let planning_handoff_text = None;

        // Keep fallible execution inside its own return boundary so every error
        // reaches terminal event recording and canonical persistence cleanup.
        let result: Result<_> = async {
            self.restore_planning_recovery(
                &mut runtime,
                &mut planning_recovery,
                &mut planning_synthesis_required,
                &session_store_handle,
                task,
            )
            .await?;
            for turn in 0..self.max_turns {
                if matches!(runtime.poll_turn_control().await, RuntimeControl::StopRequested) {
                    self.runner_println(format_args!(
                        "{} {}",
                        agent_prefix,
                        style("Stopped by steering signal.").red().bold()
                    ));
                    runtime.state.outcome = TaskOutcome::Cancelled;
                    break;
                }

                if let Some(input) = runtime.run_until_idle() {
                    self.runner_println(format_args!(
                        "{} {}: {}",
                        agent_prefix,
                        style("Follow-up Input").cyan().bold(),
                        input
                    ));
                }

                if runtime.state.is_completed {
                    break;
                }
                if self.tool_registry.is_planning_active() {
                    if planning_recovery.outcome == PlanningOutcome::AwaitingInput {
                        planning_failure_detail = Some("Planning remains idle until an explicit recovery action is supplied.".to_owned());
                        runtime.state.outcome = TaskOutcome::failed(
                            "Planning needs your input.".to_owned(), vec![],
                            Some("Retry from retained evidence, add clarification, or choose another model.".to_owned()), None,
                        );
                        break;
                    }
                    if planning_recovery.outcome == PlanningOutcome::AwaitingApproval {
                        if let Some(draft) = planning_recovery.best_draft.as_deref() {
                            match persist_plan_draft(&self.tool_registry.planning_workflow_state(), draft).await {
                                Ok(_) => {
                                    runtime.state.messages_mut().push(Message::assistant(draft.to_owned()));
                                    runtime.state.is_completed = true;
                                    runtime.state.outcome = TaskOutcome::Success;
                                }
                                Err(error) => {
                                    planning_failure_detail = Some(format!("The retained plan could not be persisted: {error:#}"));
                                    planning_recovery.outcome = PlanningOutcome::AwaitingInput;
                                    runtime.state.outcome = TaskOutcome::failed("Planning needs your input; the plan could not be saved.".to_owned(), vec![], None, None);
                                }
                            }
                        } else {
                            planning_failure_detail = Some("The saved approval checkpoint has no retained plan.".to_owned());
                            planning_recovery.outcome = PlanningOutcome::AwaitingInput;
                            runtime.state.outcome = TaskOutcome::failed("Planning needs your input; no saved plan is available.".to_owned(), vec![], None, None);
                        }
                        break;
                    }
                }

                self.runner_println(format_args!(
                    "{} {} is processing turn {}...",
                    agent_prefix,
                    style("(PROC)").cyan().bold(),
                    turn + 1
                ));

                let turn_model = self.get_selected_model();
                self.refresh_runtime_prompt_bundle_if_catalog_changed(&mut prompt_bundle, is_simple_task)
                    .await?;
                let provider_name = self.provider_client.name().to_string();
                if let Err(error) =
                    crate::llm::usage_cost::require_budget_pricing(&provider_name, &turn_model, max_budget_usd)
                {
                    let message = error.to_string();
                    event_recorder.turn_blocked(vtcode_exec_events::TurnBlockedEvent {
                        completed_at: None,
                        message: message.clone(),
                        last_tool: None,
                        blocked_streak: 0,
                        blocked_total: 0,
                        consecutive_cap: 0,
                        total_cap: 0,
                        recovery_active: false,
                        usage: Some(runtime.state.stats.total_usage.clone()),
                    });
                    should_write_blocked_handoff = true;
                    runtime.state.outcome = TaskOutcome::failed(
                        message,
                        Vec::new(),
                        Some(
                            "Choose a model route with complete pricing or remove the USD budget explicitly"
                                .to_string(),
                        ),
                        None,
                    );
                    break;
                }

                let turn_inputs =
                    self.derive_turn_request_inputs(&mut runtime, &turn_model, turn, is_simple_task, planning_synthesis_required);
                let sampling_overrides = turn_inputs.sampling_overrides;
                let turn_reasoning = turn_inputs.turn_reasoning;
                let turn_verbosity = turn_inputs.turn_verbosity;
                let max_tokens = turn_inputs.max_tokens;
                let provider_kind = turn_inputs.provider_kind;
                let planning_synthesis_turn = turn_inputs.planning_synthesis_turn;
                if planning_synthesis_turn {
                    if !planning_recovery.admit_request() {
                        planning_failure_detail = Some("The saved planning recovery allowance is exhausted.".to_owned());
                        runtime.state.outcome = TaskOutcome::failed(
                            "Saved planning recovery allowance exhausted.".to_owned(), vec![],
                            Some("Choose an explicit recovery action.".to_owned()), None,
                        );
                        break;
                    }
                    headless_planning::retain_progress(&mut planning_recovery, &runtime.state.messages,
                        &task.description, &provider_name, &turn_model);
                    headless_planning::persist_checkpoint(session_store_handle.as_ref(), &planning_recovery,
                        &self.session_id, turn + 1).await?;
                    runtime.state.add_synthetic_user_message(
                        "This is a tool-free planning synthesis within the configured budget. Return a concise, complete plan now using only evidence already collected. Fit the entire plan within the response token limit; shorten detail rather than repeating a truncated draft. Make no tool calls. State missing evidence explicitly; do not claim implementation or verification.".to_string(),
                    );
                }
                let context_budget = crate::compaction::effective_context_budget(
                    Some(self.config()),
                    self.provider_client.as_ref(),
                    &turn_model,
                );
                runtime.state.constraints.max_context_tokens = context_budget;
                let reserved_output_tokens = max_tokens
                    .map_or(crate::compaction::memory_envelope::DEFAULT_OUTPUT_RESERVE_TOKENS, |value| value as usize);
                let prompt_overhead_tokens = (prompt_bundle.system_prompt_report.token_estimate as usize)
                    .saturating_add(prompt_bundle.tool_def_tokens as usize);
                tracing::info!(
                    turn, model = %turn_model, context_budget,
                    reserved_output_tokens, prompt_overhead_tokens,
                    "Resolved per-turn context budget denominator"
                );
                let mut normalized_messages = self.project_request_history(
                    &runtime.state,
                    &turn_model,
                    prompt_overhead_tokens,
                    reserved_output_tokens,
                );
                let projected_token_usage = normalized_messages
                    .iter()
                    .map(Message::estimate_tokens)
                    .fold(0usize, usize::saturating_add)
                    .saturating_add(prompt_overhead_tokens);
                let compacted = self
                    .maybe_auto_compact(
                        &mut runtime.state,
                        &mut event_recorder,
                        &turn_model,
                        preserve_recent_turns,
                        projected_token_usage,
                        reserved_output_tokens,
                        &mut prompt_bundle.request_envelope,
                    )
                    .await;
                let estimated = if compacted {
                    normalized_messages = self.project_request_history(
                        &runtime.state,
                        &turn_model,
                        prompt_overhead_tokens,
                        reserved_output_tokens,
                    );
                    normalized_messages
                        .iter()
                        .map(Message::estimate_tokens)
                        .fold(0usize, usize::saturating_add)
                        .saturating_add(prompt_overhead_tokens)
                } else {
                    projected_token_usage
                };
                let budget = context_budget.saturating_sub(reserved_output_tokens);
                let fits = context_budget == 0 || estimated <= budget;
                if !fits {
                    // Advisory only: the estimate can overshoot (un-droppable
                    // messages, suppressed compaction), and the provider owns
                    // the authoritative context limit. Aborting here turned a
                    // recoverable situation into a dead `exec`/auto run.
                    tracing::warn!(
                        estimated,
                        budget,
                        "Pre-flight token check failed: prompt exceeds context budget after compaction"
                    );
                    #[allow(
                        clippy::cast_sign_loss,
                        reason = "Intentional compatibility, platform, or test-only suppression."
                    )]
                    let pct = (estimated as f64 / budget.max(1) as f64 * 100.0) as u32;
                    runtime
                        .state
                        .warnings
                        .push(format!("Pre-flight check: {pct}% of context budget used before LLM call"));
                }

                let parallel_tool_config = if self.provider_client.supports_parallel_tool_config(&turn_model) {
                    Some(Box::new(crate::llm::provider::ParallelToolConfig::anthropic_optimized()))
                } else {
                    None
                };

                let reasoning_mapping = turn_reasoning
                    .map(|requested| {
                        crate::llm::reasoning_effort::ReasoningEffortMapper::resolve(
                            self.provider_client.as_ref(),
                            &turn_model,
                            requested,
                            self.config().agent.allow_reasoning_effort_downgrade,
                        )
                    })
                    .transpose();
                let reasoning_effort = match reasoning_mapping {
                    Ok(mapping) => mapping,
                    Err(error) => {
                        let message = error.to_string();
                        event_recorder.turn_blocked(vtcode_exec_events::TurnBlockedEvent {
                            completed_at: None,
                            message: message.clone(),
                            last_tool: None,
                            blocked_streak: 0,
                            blocked_total: 0,
                            consecutive_cap: 0,
                            total_cap: 0,
                            recovery_active: false,
                            usage: Some(runtime.state.stats.total_usage.clone()),
                        });
                        should_write_blocked_handoff = true;
                        runtime.state.outcome = TaskOutcome::failed(
                            message,
                            Vec::new(),
                            Some("Choose a supported reasoning effort or explicitly enable downgrade".to_string()),
                            None,
                        );
                        break;
                    }
                }
                .map(|mapping| {
                    if mapping.degraded() {
                        tracing::warn!(requested = %mapping.requested, effective = %mapping.effective,
                            model = %turn_model, "Harness reasoning effort explicitly downgraded");
                    }
                    mapping.effective
                })
                .filter(|effort| *effort != ReasoningEffortLevel::None);
                let reasoning_active = reasoning_effort.is_some_and(|effort| {
                    !matches!(effort, ReasoningEffortLevel::None | ReasoningEffortLevel::Unknown)
                });
                let capability_prefix_hash = crate::core::agent::hash_utils::PromptCapabilityIdentity::resolve(
                    self.provider_client.as_ref(),
                    &turn_model,
                    reasoning_effort,
                    prompt_bundle.tool_snapshot.epoch,
                )
                .prefix_hash(prompt_bundle.request_envelope.prefix_hash());

                // Reasoning-effort-change advisory (Phase E4): a mid-task
                // change to the reasoning effort alters the request prefix,
                // which invalidates the provider prompt cache for the next
                // request.
                if runtime.state.note_reasoning_effort_change(reasoning_effort) {
                    let message = "Reasoning effort changed mid-task; provider prompt cache \
                                    will be invalidated and the next request re-pays full \
                                    input cost."
                        .to_string();
                    tracing::warn!("{message}");
                    runtime.state.push_warning(message);
                }

                // Model-change advisory: prompt caches are unique per model,
                // so a mid-task switch rebuilds the cache at full input cost
                // even when the rest of the prefix is unchanged. Prefer
                // resolving the model up front; when a switch is unavoidable,
                // expect one full-price request before hits resume.
                if runtime.state.note_model_change(&turn_model) {
                    let message = "Model changed mid-task; provider prompt cache \
                                    will be invalidated and the next request re-pays full \
                                    input cost."
                        .to_string();
                    tracing::warn!("{message}");
                    runtime.state.push_warning(message);
                }

                let anthropic_shaped = matches!(provider_kind, ModelProvider::Anthropic | ModelProvider::Minimax);
                // Anthropic Messages (API 2026-10-21) rejects sampling even
                // when reasoning is off, so the drop is unconditional.
                let drop_sampling_params =
                    sampling_overrides.drops_sampling_params(anthropic_shaped, reasoning_active);
                let temperature = if drop_sampling_params {
                    None
                } else {
                    Some(sampling_overrides.temperature.unwrap_or(self.config().agent.temperature))
                };
                let mut top_p_override = sampling_overrides.top_p;
                let mut top_k_override = sampling_overrides.top_k;
                if drop_sampling_params {
                    // Keep the payload inside what our Anthropic reasoning
                    // validator accepts: `validate_reasoning_constraints`
                    // rejects any top_k and requires top_p in [0.95, 1.0]
                    // while extended thinking is active.
                    top_k_override = None;
                    top_p_override = top_p_override.filter(|value| *value >= 0.95);
                }

                self.tool_registry
                    .set_inspection_mode(self.config().agent.harness.inspection.mode);
                self.tool_registry.reconcile_source_inspection(&normalized_messages);
                let (request_messages, previous_response_id) = prepare_responses_request_messages(
                    &mut runtime.state.previous_response_chains,
                    &provider_name,
                    self.provider_client.supports_responses_compaction(&turn_model),
                    &turn_model,
                    &normalized_messages,
                );
                let request_messages = match request_messages {
                    std::borrow::Cow::Borrowed(_) => Arc::clone(&normalized_messages),
                    std::borrow::Cow::Owned(messages) => Arc::new(messages),
                };
                let request = build_harness_request_plan(HarnessRequestPlanInput {
                    messages: request_messages,
                    system_prompt: prompt_bundle.request_envelope.system_prompt(),
                    tools: (!planning_synthesis_turn && !prompt_bundle.request_envelope.ordered_tools().is_empty())
                        .then(|| prompt_bundle.request_envelope.ordered_tools()),
                    model: turn_model.clone(),
                    max_tokens,
                    temperature,
                    top_p: top_p_override,
                    top_k: top_k_override,
                    presence_penalty: if drop_sampling_params {
                        None
                    } else {
                        sampling_overrides.presence_penalty
                    },
                    frequency_penalty: if drop_sampling_params {
                        None
                    } else {
                        sampling_overrides.frequency_penalty
                    },
                    stream: self.provider_client.supports_streaming(),
                    tool_choice: if planning_synthesis_turn {
                        Some(ToolChoice::None)
                    } else if provider_name.eq_ignore_ascii_case("openai")
                        && !prompt_bundle.tool_snapshot.active_tool_names.is_empty()
                    {
                        Some(ToolChoice::allowed_tools_auto(prompt_bundle.tool_snapshot.active_tool_names.as_ref().clone()))
                    } else {
                        None
                    },
                    parallel_tool_config,
                    reasoning_effort,
                    verbosity: turn_verbosity,
                    metadata: None,
                    context_management: None,
                    previous_response_id,
                    // Keep the wire key stable per session. OpenAI routes by
                    // (prefix hash + key); the key must stay consistent across
                    // requests sharing a prefix ("Use prompt_cache_key
                    // consistently"). Per-turn suffixes (capability/catalog
                    // hashes) fragment routing buckets without benefit: distinct
                    // prefixes already route distinctly via their prefix hash,
                    // and a suffix that changes on tool-catalog churn busts an
                    // otherwise reusable routing affinity every few turns.
                    // Prefix identity stays tracked per request via
                    // `tool_catalog_hash` / `system_prompt_prefix_hash` below.
                    // OpenRouter/xAI receive namespaced session lineage for
                    // documented sticky routing even when the OpenAI cache
                    // block is off.
                    prompt_cache_key: build_session_affinity_prompt_cache_key(
                        &provider_name,
                        session_affinity_key_enabled(
                            &provider_name,
                            self.config().prompt_cache.enabled,
                            self.config().prompt_cache.providers.openai.enabled,
                        ),
                        &self.config().prompt_cache.providers.openai.prompt_cache_key_mode,
                        Some(&self.session_id),
                    ),
                    prompt_cache_profile: None,
                    tool_catalog_hash: prompt_bundle.request_envelope.catalog_hash(),
                    system_prompt_prefix_hash: Some(capability_prefix_hash),
                })
                .request;
                // Cheap pre-flight: catch malformed requests (empty system
                // prompt, no messages, duplicate tool names, missing required
                // properties) before paying for an API round-trip.
                self.validate_llm_request(&request)?;
                let previous_response_chain_present = request.previous_response_id.is_some();
                // O(1) Arc bump: keeps the sent history available for
                // set_previous_response_chain while the request still holds it
                // for providers that validate messages (e.g. MiMo) during
                // stream().
                let sent_messages = Arc::clone(&request.messages);
                // Compute timeout before the call to avoid simultaneous mutable/immutable
                // borrows of `self` (provider_client vs config).
                let streaming_timeout = self
                    .config()
                    .timeouts
                    .ceiling_duration(self.config().timeouts.streaming_ceiling_seconds);

                // Cache-gap advisory (Phase E1): warn once per gap when the
                // provider prompt cache has likely expired since the last
                // request, so this request may unexpectedly re-pay full
                // input cost.
                if let Some(threshold) = self
                    .config()
                    .prompt_cache
                    .gap_threshold_secs_for_route(&provider_name, &turn_model, self.provider_client.requested_prompt_cache_ttl(&turn_model))
                {
                    let threshold = std::time::Duration::from_secs(threshold);
                    if runtime.state.stats.total_usage.cached_input_tokens > 0
                        && let Some(elapsed) = runtime.state.cache_gap_exceeds(threshold)
                    {
                        let gap = crate::llm::request_gap::format_gap(elapsed);
                        let message = format!(
                            "~{gap} since the last request; the provider prompt cache may have expired, so this request may rebuild cached input."
                        );
                        tracing::warn!("{message}");
                        runtime.state.push_warning(message);
                    }
                }
                runtime.state.note_request_sent();

                let turn_output = match runtime
                    .run_turn_once(&mut self.provider_client, request, streaming_timeout).await {
                    Ok(output) => output,
                    Err(error) if self.tool_registry.is_planning_active() => {
                        if matches!(runtime.state.outcome, TaskOutcome::Cancelled) {
                            break;
                        }
                        super::tool_dispatch_common::drain_and_record_runtime_events(&mut runtime, &mut event_recorder);
                        planning_recovery.response_class = Some(PlanningResponseClass::ProviderFailure);
                        planning_failure_detail = Some(format!("Planning provider request failed: {error:#}"));
                        runtime.state.outcome = TaskOutcome::failed(
                            "Planning needs your input after a provider failure.".to_owned(), vec![], None, None);
                        break;
                    }
                    Err(error) => return Err(error),
                };
                super::tool_dispatch_common::drain_and_record_runtime_events(&mut runtime, &mut event_recorder);
                let response = turn_output.response;
                runtime.state.stop_reason = Some(stop_reason_from_finish_reason(&response.finish_reason));
                let refused = refusal::is_refusal(&response);

                // --- Progress stagnation detection ---
                // If the assistant produces near-identical responses across consecutive
                // turns (no tool calls, no progress), inject a nudge to break the loop.
                if !refused && !runtime.state.is_completed && runtime.state.record_progress_hash_and_check_stagnation()
                {
                    let nudge = "It looks like you're repeating the same response. \
                                 If you're stuck, try a different approach: break the \
                                 problem into smaller steps, use different tools, or \
                                 declare the task complete if you have enough information.";
                    self.record_diagnostic(&mut event_recorder, &Diagnostic {
                        severity: DiagnosticSeverity::Warning, code: "agent.stagnation".into(),
                        summary: "The agent is repeating a response. A recovery prompt was added.".into(),
                        agent_details: json!({"provider": provider_name, "model": turn_model, "turn": turn + 1, "recovery": nudge}).to_string(),
                        correlation_id: format!("agent:{}:stagnation", self.session_id), actions: vec![],
                    });
                    runtime.state.add_synthetic_user_message(nudge.into());
                }
                if !refused
                    && supports_responses_chaining(
                        &provider_name,
                        self.provider_client.supports_responses_compaction(&turn_model),
                    )
                {
                    runtime.state.set_previous_response_chain_shared(
                        &provider_name,
                        &turn_model,
                        response.request_id.as_deref(),
                        Arc::clone(&sent_messages),
                    );
                }
                let turn_cost = response.usage.as_ref().and_then(|usage| {
                    let normalized = crate::llm::usage_cost::normalized_turn_usage(&provider_name, usage);
                    crate::llm::usage_cost::estimate_session_costs(&provider_name, &turn_model, &normalized)
                });
                match session_costs.record(turn_cost) {
                    Some(estimate) => {
                        runtime.state.total_cost_usd = Some(estimate.effective_usd);
                        let threshold = self.config().agent.harness.budget_warning_threshold;
                        match crate::llm::usage_cost::BudgetStatus::classify(
                            estimate.raw_usd,
                            max_budget_usd,
                            threshold,
                        ) {
                            crate::llm::usage_cost::BudgetStatus::Exceeded { max, .. } => {
                                runtime.state.outcome = TaskOutcome::budget_limit_reached(max, estimate.raw_usd);
                                break;
                            }
                            crate::llm::usage_cost::BudgetStatus::Warning { max, .. } if !budget_warning_emitted => {
                                budget_warning_emitted = true;
                                warn!(
                                    provider = %self.config().agent.provider,
                                    model = %turn_model,
                                    cost_usd = estimate.raw_usd,
                                    max_budget_usd = max,
                                    "Session cost approaching budget limit"
                                );
                                runtime.state.push_warning(format!(
                                    "Session cost ${:.4} has reached {:.0}% of the ${max:.2} budget. {}",
                                    estimate.raw_usd,
                                    threshold * 100.0,
                                    runtime.state.stats.total_usage.cache_summary()
                                ));
                            }
                            _ => {}
                        }
                    }
                    None => {
                        runtime.state.total_cost_usd = None;
                        if max_budget_usd.is_some() {
                            runtime.state.outcome = TaskOutcome::failed(
                                format!(
                                    "Pricing or usage became unavailable for `{provider_name}/{turn_model}`; budget enforcement stopped execution"
                                ),
                                Vec::new(),
                                Some("Select a model route with complete pricing".to_string()),
                                None,
                            );
                            should_write_blocked_handoff = true;
                            break;
                        }
                    }
                }
                // A refusal is terminal for this prompt: idle recovery would
                // resend it and be refused again, and any partial output or
                // tool calls were cut off by the provider, so neither is
                // committed as an answer nor executed.
                if refused {
                    let reason = refusal::refusal_reason(&response);
                    discard_refused_assistant_message(&mut runtime.state);
                    // The refused response is not part of the kept history, so
                    // a stored continuation id would point past it.
                    runtime.state.clear_previous_response_chain_for(&provider_name, &turn_model);
                    self.record_diagnostic(&mut event_recorder, &Diagnostic {
                        severity: DiagnosticSeverity::Warning, code: "provider.refused".into(),
                        summary: "The provider declined this request.".into(),
                        agent_details: json!({"provider": provider_name, "model": turn_model, "turn": turn + 1, "reason": reason}).to_string(),
                        correlation_id: format!("agent:{}:refusal:{}", self.session_id, turn + 1), actions: vec![],
                    });
                    runtime.state.outcome = TaskOutcome::refused(reason);
                    break;
                }

                self.runner_println(format_args!(
                    "{} {} {} received response, processing...",
                    agent_prefix,
                    self.agent_type,
                    style("(RECV)").green().bold()
                ));

                self.warn_on_empty_response(
                    &agent_prefix,
                    response.content.as_deref().unwrap_or(""),
                    response.tool_calls.as_ref().is_some_and(|tool_calls| !tool_calls.is_empty()),
                    &mut event_recorder,
                );

                let response_text = response.content_string();
                if !response_text.trim().is_empty() {
                    self.emit_final_assistant_message(&self.agent_type, &response_text);
                }

                let mut effective_tool_calls = response.tool_calls.clone();
                let mut forced_continuation = false;

                match self
                    .handle_planning_response(PlanningResponse {
                        runtime: &mut runtime,
                        response: &response,
                        effective_tool_calls: &mut effective_tool_calls,
                        planning_recovery: &mut planning_recovery,
                        planning_failure_detail: &mut planning_failure_detail,
                        planning_synthesis_required: &mut planning_synthesis_required,
                        planning_synthesis_turn,
                        session_store_handle: &session_store_handle,
                        task,
                        provider_name: &provider_name,
                        turn_model: &turn_model,
                        turn,
                    })
                    .await?
                {
                    PlanningResponseFlow::FallThrough => {}
                    PlanningResponseFlow::ContinueLoop => continue,
                    PlanningResponseFlow::BreakLoop => break,
                }

                let is_gemini = matches!(provider_kind, ModelProvider::Gemini);

                // --- Confidence-based escalation gate + chain ---
                // Evaluate tool calls before dispatching them.  Implements the
                // escalation chain: re-plan → prompt user → abort with partial
                // results.  (Steps 1–2 already exist at the tool-exec level:
                // auto-retry and alternative-tool fallback are handled in
                // tool_exec.rs and execution_facade.rs.)
                if self
                    .run_escalation_gate(
                        &mut runtime,
                        &mut effective_tool_calls,
                        &mut event_recorder,
                        orchestration_enabled,
                        &provider_name,
                        &turn_model,
                        turn,
                        &mut should_write_blocked_handoff,
                        &mut forced_continuation,
                    )?
                    .is_break()
                {
                    break;
                }

                if self
                    .settle_coordinator_matrix(
                        &mut runtime,
                        &effective_tool_calls,
                        &mut forced_continuation,
                        &mut should_write_blocked_handoff,
                    )
                    .await?
                    .is_break()
                {
                    break;
                }

                match self
                    .assess_completion_gate(CompletionGate {
                        runtime: &mut runtime,
                        event_recorder: &mut event_recorder,
                        response: &response,
                        effective_tool_calls: &effective_tool_calls,
                        planning_recovery: &planning_recovery,
                        provider_name: &provider_name,
                        turn_model: &turn_model,
                        turn,
                        effective_task: &effective_task,
                        continuation_controller: &mut continuation_controller,
                        verification_recovery_used: &mut verification_recovery_used,
                        orchestration_enabled,
                        revision_rounds_used: &mut revision_rounds_used,
                        max_revision_rounds,
                        should_write_blocked_handoff: &mut should_write_blocked_handoff,
                        forced_continuation: &mut forced_continuation,
                    })
                    .await?
                {
                    CompletionGateFlow::FallThrough => {}
                    CompletionGateFlow::Break => break,
                    CompletionGateFlow::ReturnErr(err) => return Err(err),
                }

                if let Some(tool_calls) = effective_tool_calls
                    .as_ref()
                    .filter(|tool_calls| !tool_calls.is_empty())
                    .cloned()
                {
                    self.execute_tool_call_batches(
                        tool_calls,
                        &mut runtime,
                        &mut event_recorder,
                        &agent_prefix,
                        is_gemini,
                        previous_response_chain_present,
                    )
                    .await?;
                    super::tool_dispatch_common::drain_and_record_runtime_events(&mut runtime, &mut event_recorder);

                    if let Some(outcome) = self.tool_registry.matrix_worker_outcome() {
                        runtime.state.outcome = if outcome == crate::exec::events::matrix::MatrixOutcome::Success {
                            TaskOutcome::Success
                        } else {
                            TaskOutcome::failed(format!("Matrix worker reported {outcome:?}"), vec![], None, None)
                        };
                        runtime.state.is_completed = true;
                        break;
                    }

                    if self
                        .active_primary_agent
                        .as_ref()
                        .is_some_and(|agent| agent.identity.name == "coordinator")
                        && let Some(controller) = self.tool_registry.subagent_controller()
                        && let Some(snapshot) = controller.matrix_snapshot().await
                        && (matches!(
                            snapshot.lifecycle,
                            crate::exec::events::matrix::MatrixLifecycle::Running
                                | crate::exec::events::matrix::MatrixLifecycle::Verifying
                        ) || controller.matrix_is_driving())
                    {
                        // Headless sessions await owned work before another model turn;
                        // status polling must not exhaust the coordinator's loop budget.
                        let settled = loop {
                            tokio::select! {
                                settled = controller.wait_matrix_idle() => break settled.context("matrix disappeared before completion")?,
                                () = tokio::time::sleep(std::time::Duration::from_millis(100)) => {
                                    if matches!(runtime.poll_turn_control().await, RuntimeControl::StopRequested) {
                                        controller.cancel_matrix().await?;
                                        runtime.state.outcome = TaskOutcome::Cancelled;
                                    }
                                }
                            }
                        };
                        anyhow::ensure!(
                            !matches!(
                                settled.lifecycle,
                                crate::exec::events::matrix::MatrixLifecycle::Running
                                    | crate::exec::events::matrix::MatrixLifecycle::Verifying
                            ),
                            "matrix stopped without a durable result; reconcile owned cleanup before continuation"
                        );
                        if matches!(runtime.state.outcome, TaskOutcome::Cancelled) {
                            break;
                        }
                        runtime.state.add_synthetic_user_message(format!(
                            "Durable matrix results: {}. Report success only for Succeeded; other states require a coordinator decision.",
                            serde_json::to_string(&settled)?
                        ));
                    }
                }

                // Refresh tool definitions if the catalog was mutated during tool
                // execution (e.g. tools.load / tools.unload / skill activation).
                if self
                    .refresh_runtime_prompt_bundle_if_catalog_changed(&mut prompt_bundle, is_simple_task)
                    .await?
                {
                    tracing::debug!("runtime prompt bundle refreshed after tool catalog mutation");
                }

                // --- Emit tool latency events ---
                if !runtime.state.turn_tool_observations.is_empty() {
                    let observations = std::mem::take(&mut runtime.state.turn_tool_observations);
                    for observation in &observations {
                        event_recorder.record_tool_outcome(
                            &observation.tool_name,
                            observation.attempts,
                            observation.duration_ms,
                            observation.error_category.as_ref().map(vtcode_commons::ErrorCategory::as_str),
                        );
                    }
                }

                let had_effective_shell_tool_call = effective_tool_calls.as_ref().is_some_and(|calls| {
                    calls.iter().any(|call| {
                        call.function.as_ref().map(|function| function.name.as_str()) == Some(tools::UNIFIED_EXEC)
                    })
                });
                let had_tool_call = response.tool_calls.as_ref().is_some_and(|tool_calls| !tool_calls.is_empty())
                    || had_effective_shell_tool_call;

                if had_tool_call {
                    let loops = runtime.state.register_tool_loop();
                    if tool_loop_limit_reached(loops, runtime.state.constraints.max_tool_loops) {
                        let warning_message = format!(
                            "You have reached the tool-call iteration limit of {}. \
                             This typically means you are in a loop — repeatedly calling tools \
                             without making progress toward the task goal.\n\n\
                             To proceed:\n\
                             1. Review what you have already learned from previous tool outputs.\n\
                             2. Synthesize your findings into a concrete answer or implementation.\n\
                             3. If you need more information, use a different approach or tool.\n\
                             4. If you are truly stuck, explain what you have accomplished so far \
                             and what is blocking you.",
                            runtime.state.constraints.max_tool_loops
                        );
                        self.record_warning(
                            &agent_prefix,
                            &mut runtime.state,
                            &mut event_recorder,
                            warning_message.clone(),
                        );
                        runtime.state.add_synthetic_user_message(warning_message);
                        runtime.state.mark_tool_loop_limit_hit();
                        break;
                    }
                    runtime.state.consecutive_idle_turns = 0;
                } else {
                    runtime.state.reset_tool_loop_guard();
                    if forced_continuation {
                        runtime.state.consecutive_idle_turns = 0;
                    } else if !runtime.state.is_completed {
                        runtime.state.consecutive_idle_turns = runtime.state.consecutive_idle_turns.saturating_add(1);
                        let idle_turn_limit = self.config().agent.idle_turn_limit;
                        if runtime.state.consecutive_idle_turns >= idle_turn_limit {
                            let warning_message = format!(
                                "No tool calls or completion for {} consecutive turns. \
                                 The agent appears to be idle — it is responding without \
                                 taking actions or declaring the task complete.\n\n\
                                 To proceed:\n\
                                 1. Take concrete action using the available tools.\n\
                                 2. If you have enough information, present your solution.\n\
                                 3. If you are waiting for something, explain the situation.",
                                runtime.state.consecutive_idle_turns
                            );
                            self.record_warning(
                                &agent_prefix,
                                &mut runtime.state,
                                &mut event_recorder,
                                warning_message.clone(),
                            );
                            runtime.state.add_synthetic_user_message(warning_message);
                            runtime.state.outcome = TaskOutcome::StoppedNoAction;
                            break;
                        }
                    }
                }

                let should_continue = forced_continuation
                    || had_tool_call
                    || runtime.has_pending_follow_up_inputs()
                    || (!runtime.state.is_completed && (turn + 1) < self.max_turns);

                if !should_continue {
                    if runtime.state.is_completed {
                        runtime.state.outcome = TaskOutcome::Success;
                    } else if (turn + 1) >= self.max_turns {
                        runtime.state.outcome = TaskOutcome::turn_limit_reached(self.max_turns, turn + 1);
                    } else {
                        runtime.state.outcome = TaskOutcome::StoppedNoAction;
                    }
                    break;
                }
            }

            self.finalize_task_execution(TaskFinalization {
                runtime: &mut runtime,
                event_recorder: &mut event_recorder,
                planning_recovery: &mut planning_recovery,
                planning_failure_detail: &mut planning_failure_detail,
                planning_handoff_text,
                should_write_blocked_handoff: &mut should_write_blocked_handoff,
                session_store_handle: &session_store_handle,
                task,
                run_started_at,
                agent_prefix: &agent_prefix,
                effective_task: &effective_task,
                max_tool_loops,
                max_context_tokens,
                session_costs: &session_costs,
            })
            .await
        }
        .await;

        let result = match result {
            Ok((task_results, steering_receiver)) => {
                *self.steering_receiver.lock() = steering_receiver;
                Ok(task_results)
            }
            Err(err) => {
                event_recorder.thread_failed(
                    &self.session_id,
                    &format!("{err:#}"),
                    runtime.state.stats.turns_executed.max(1),
                );
                *self.steering_receiver.lock() = runtime.take_steering_receiver();
                Err(err)
            }
        };

        let persistence_result = if let Some(session_store_handle) = session_store_handle {
            session_store_handle
                .close()
                .await
                .context("authoritative session event persistence failed")
        } else {
            Ok(())
        };

        self.tool_registry.set_harness_task(None);
        match (result, persistence_result) {
            (Err(error), Err(persistence_error)) => Err(error
                .context(format!("failed to finalize canonical session after execution error: {persistence_error:#}"))),
            (Err(error), Ok(())) => Err(error),
            (Ok(_), Err(error)) => Err(error),
            (Ok(results), Ok(())) => Ok(results),
        }
    }
}

mod assessment;
mod dispatch_gates;
mod planning;
use planning::{PlanningResponse, PlanningResponseFlow};
mod finalize;
use assessment::{CompletionGate, CompletionGateFlow};
mod pre_request;
use finalize::TaskFinalization;

#[cfg(test)]
mod tests;
