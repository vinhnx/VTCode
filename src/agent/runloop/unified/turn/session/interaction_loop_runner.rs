mod status_refresh;
mod support;

use anyhow::Result;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use tokio::task::JoinHandle;
use vtcode_core::hooks::SessionEndReason;
use vtcode_core::llm::provider as uni;
use vtcode_core::session::SessionId;
use vtcode_core::utils::ansi::MessageStyle;

use super::interaction_loop::{InteractionLoopContext, InteractionOutcome, InteractionState};
use crate::agent::runloop::model_picker::ModelPickerProgress;
use crate::agent::runloop::unified::display::display_user_message;
use crate::agent::runloop::unified::external_url_guard::ExternalUrlGuardContext;
use crate::agent::runloop::unified::inline_events::{
    InlineEventLoopResources, InlineInterruptCoordinator, InlineLoopAction, poll_inline_loop_action,
};
use crate::agent::runloop::unified::model_selection::{ModelSwitchCompactionTargets, finalize_model_selection};
use crate::agent::runloop::unified::palettes::ActivePalette;
use crate::agent::runloop::unified::session_setup::refresh_local_agents;
use crate::agent::runloop::unified::settings_interactive::{reload_state_from_disk, show_settings_palette};
use crate::agent::runloop::unified::state::is_follow_up_prompt_like;
use crate::agent::runloop::unified::turn::background_completion::{
    background_completion_thread_event, drain_background_completions, drain_exec_session_completions,
};
use crate::agent::runloop::unified::turn::session::{
    mcp_lifecycle, memory_prompt, slash_command_handler, tool_dispatch,
};
use status_refresh::{StatusRefreshContext, StatusRefreshReason, StatusRefreshRequest, refresh_interaction_ui};
pub(crate) use support::handle_select_primary_agent;
use support::{
    InlineLoopActionResolution, apply_live_theme_and_appearance, build_durable_scheduler_daemon,
    build_user_message_content, extract_recent_follow_up_hint, fallback_args_preview, replace_submitted_input_text,
    resolve_inline_loop_action, scheduler_enabled, selected_model_supports_image_input,
    stalled_follow_up_recovery_prompt, stalled_verification_resume_directive, submitted_images_are_unsupported,
    sync_mcp_approval_policy_for_context,
};
use vtcode_config::loader::SimpleConfigWatcher;

/// Shared by both repeated-follow-up directives: structured next-step fields
/// from tool results are the cheapest recovery path.
const REPEATED_FOLLOW_UP_TOOL_GUIDANCE: &str = "If a recent tool result or tool error provides `fallback_tool`, \
`fallback_tool_args`, `hint`, or `next_action`, start from that guidance; repeating the failing call returns the same \
failure.";

fn repeated_follow_up_directive(stalled: bool) -> String {
    let situation = if stalled {
        "The previous turn stalled or aborted, and the user has asked to continue more than once. Identify the likely \
         cause from recent errors, try one adjusted approach, then end with either a completion summary or a blocker \
         report naming the specific next action; asking the user to continue again would repeat the stall."
    } else {
        "The user has asked to continue more than once without a visible update. Make your next response a concrete \
         status update: completed work, current blocker, and the exact next action."
    };
    format!("{situation} {REPEATED_FOLLOW_UP_TOOL_GUIDANCE}")
}
const SCHEDULED_PROMPT_INACTIVITY_GRACE: Duration = Duration::from_secs(2);
const DURABLE_SCHEDULER_POLL_INTERVAL: Duration = Duration::from_secs(1);

fn idle_completion_outcome(
    action: &InlineLoopAction,
    ordinary_completions: usize,
    matrix_ready: bool,
) -> Option<InteractionOutcome> {
    (matches!(action, InlineLoopAction::Continue) && (ordinary_completions > 0 || matrix_ready))
        .then_some(InteractionOutcome::BackgroundCompletionReady)
}

#[cfg_attr(feature = "profiling", hotpath::measure)]
pub(super) async fn run_interaction_loop_impl(
    ctx: &mut InteractionLoopContext<'_>,
    state: &mut InteractionState<'_>,
) -> Result<InteractionOutcome> {
    const MCP_REFRESH_INTERVAL: Duration = Duration::from_secs(5);
    let mut last_input_activity = ctx.input_activity_counter.load(Ordering::Relaxed);
    let mut last_input_activity_at = Instant::now();
    let mut last_durable_scheduler_poll = Instant::now()
        .checked_sub(DURABLE_SCHEDULER_POLL_INTERVAL)
        .unwrap_or_else(Instant::now);
    let mut durable_scheduler_daemon = None;
    let mut last_durable_scheduler_error = None::<String>;
    let mut durable_scheduler_run = None::<JoinHandle<Result<usize>>>;
    let mut live_reload_watcher = SimpleConfigWatcher::new_with_user_config_paths(ctx.config.workspace.clone());
    live_reload_watcher.set_check_interval(1);
    live_reload_watcher.set_debounce_duration(200);
    if let Some(initial_config) = ctx.vt_cfg.as_ref() {
        live_reload_watcher.set_last_known_config(initial_config.clone());
    }
    let mut last_status_refresh = Instant::now()
        .checked_sub(Duration::from_millis(500))
        .unwrap_or_else(Instant::now);
    const STATUS_REFRESH_INTERVAL: Duration = Duration::from_millis(200);

    loop {
        let mut workspace_config_reloaded = false;
        let should_refresh_status = last_status_refresh.elapsed() >= STATUS_REFRESH_INTERVAL;
        if should_refresh_status {
            last_status_refresh = Instant::now();
        }
        if should_refresh_status && live_reload_watcher.should_reload() {
            let reloaded = live_reload_watcher.load_config();
            if let Some(error) = live_reload_watcher.take_reload_error() {
                ctx.renderer.line(
                    MessageStyle::Warning,
                    &format!("Configuration reload rejected; keeping the last valid configuration: {error}"),
                )?;
            } else if let Some(reloaded) = reloaded {
                if let Err(err) = crate::agent::runloop::unified::turn::workspace::apply_workspace_config_snapshot(
                    reloaded, ctx.config, ctx.vt_cfg,
                ) {
                    tracing::warn!("Failed to apply live-reloaded workspace config: {}", err);
                } else if let Some(cfg) = ctx.vt_cfg.as_ref() {
                    if let Err(err) =
                        crate::agent::runloop::unified::turn::workspace::apply_workspace_config_to_registry(
                            ctx.tool_registry,
                            cfg,
                        )
                    {
                        tracing::warn!("Failed to apply live-reloaded workspace config: {}", err);
                    }
                    apply_live_theme_and_appearance(ctx.handle, cfg, ctx.session_bootstrap);
                    ctx.renderer
                        .set_show_diagnostics_in_transcript(cfg.ui.show_diagnostics_in_transcript);
                    ctx.renderer.set_tool_display_mode(cfg.ui.tool_display_mode);
                    vtcode_ui::tui::panic_hook::set_show_diagnostics(cfg.ui.show_diagnostics_in_transcript);
                    ctx.config.reasoning_effort = cfg.agent.reasoning_effort;
                    ctx.config.theme.clone_from(&cfg.agent.theme);
                    *ctx.permissions_state.write().await = cfg.permissions.clone();
                    sync_mcp_approval_policy_for_context(ctx);
                    if let Some(ActivePalette::Settings { state: palette_state, .. }) = state.palette_state.as_mut() {
                        let selected = palette_state.selection_for_view(palette_state.view_path.as_deref());
                        if let Err(err) = reload_state_from_disk(palette_state) {
                            ctx.renderer.line(
                                MessageStyle::Warning,
                                &format!("Settings palette kept its last valid values after reload failure: {err:#}"),
                            )?;
                        } else {
                            show_settings_palette(ctx.renderer, palette_state.as_ref(), selected)?;
                        }
                    }
                    workspace_config_reloaded = true;
                }
            }
        }

        if should_refresh_status {
            let status_refresh_request = StatusRefreshRequest {
                reason: if workspace_config_reloaded {
                    StatusRefreshReason::ConfigurationReloaded
                } else {
                    StatusRefreshReason::Cadence
                },
            };
            {
                let status_refresh_context = StatusRefreshContext::from_loop(ctx);
                refresh_interaction_ui(status_refresh_context, state, status_refresh_request).await;
            }

            if let Some(mcp_manager) = ctx.async_mcp_manager {
                mcp_lifecycle::handle_mcp_updates(
                    mcp_manager,
                    ctx.tool_registry,
                    ctx.tools,
                    ctx.tool_catalog,
                    ctx.config,
                    ctx.vt_cfg.as_ref(),
                    &**ctx.provider_client,
                    ctx.vt_cfg
                        .as_ref()
                        .map(|cfg| cfg.agent.tool_documentation_mode)
                        .unwrap_or_default(),
                    ctx.renderer,
                    state.mcp_catalog_initialized,
                    state.last_mcp_refresh,
                    state.last_known_mcp_tools,
                    state.pending_mcp_refresh,
                    MCP_REFRESH_INTERVAL,
                )
                .await?;
            }
        } // end should_refresh_status

        if ctx.ctrl_c_state.is_exit_requested() {
            return Ok(InteractionOutcome::Exit { reason: SessionEndReason::Exit });
        }

        // This loop runs only at the idle input boundary. Reevaluate the live
        // provider and mode after settings/auth changes, with a session latch.
        let applicable = crate::agent::runloop::unified::auto_permission::decisions_suggestion_applicable(
            ctx.renderer.supports_inline_ui(),
            true,
            ctx.tool_registry.is_planning_active(),
            ctx.provider_client.as_ref(),
            ctx.vt_cfg.as_ref(),
        );
        if ctx.session_stats.take_decisions_probe_suggestion(applicable) {
            ctx.renderer
                .line(MessageStyle::Info, crate::agent::runloop::unified::auto_permission::DECISIONS_SUGGESTION)?;
        }

        let interrupts = InlineInterruptCoordinator::new(ctx.ctrl_c_state.as_ref());
        let use_unicode = ctx.renderer.should_use_unicode_formatting();
        let idle_wake_delay = STATUS_REFRESH_INTERVAL.saturating_sub(last_status_refresh.elapsed());
        let harness_snapshot = ctx.tool_registry.harness_context_snapshot();
        let resources = InlineEventLoopResources {
            renderer: ctx.renderer,
            handle: ctx.handle,
            interrupts,
            ctrl_c_notice_displayed: state.ctrl_c_notice_displayed,
            default_placeholder: ctx.default_placeholder,
            queued_inputs: state.queued_inputs,
            prefer_latest_queued_input_once: state.prefer_latest_queued_input_once,
            model_picker_state: state.model_picker_state,
            palette_state: state.palette_state,
            config: ctx.config,
            vt_cfg: ctx.vt_cfg,
            provider_client: ctx.provider_client,
            ctrl_c_state: ctx.ctrl_c_state,
            ctrl_c_notify: ctx.ctrl_c_notify,
            session_bootstrap: ctx.session_bootstrap,
            full_auto: ctx.full_auto,
            startup_update_notice_rx: ctx.startup_update_notice_rx,
            header_context: ctx.header_context,
            use_unicode,
            conversation_history: ctx.conversation_history,
            session_stats: ctx.session_stats,
            context_manager: ctx.context_manager,
            session_id: &harness_snapshot.session_id,
            thread_id: ctx.thread_id,
            lifecycle_hooks: ctx.lifecycle_hooks.as_ref(),
            harness_emitter: ctx.harness_emitter,
            editor_open_sender: ctx.editor_open_sender,
            editor_open_dispatcher: ctx.editor_open_dispatcher.clone(),
            exec_sessions: Some(ctx.tool_registry.exec_session_manager()),
            background_completion_notify: ctx.background_completion_notify.clone(),
            exec_completion_notify: ctx.exec_completion_notify.clone(),
            webmcp_prompt_receiver: ctx.webmcp_prompt_receiver,
            idle_wake_delay,
        };

        let inline_action = poll_inline_loop_action(ctx.session, ctx.ctrl_c_notify, resources).await?;
        sync_mcp_approval_policy_for_context(ctx);

        if matches!(&inline_action, InlineLoopAction::Continue) {
            let completion_drain = drain_background_completions(
                ctx.background_completion_receiver.as_mut(),
                ctx.pending_background_completions,
            );
            let exec_completion_drain = drain_exec_session_completions(
                ctx.exec_completion_receiver.as_mut(),
                ctx.pending_background_completions,
            );
            if completion_drain.lagged
                && let Some(controller) = ctx.tool_registry.subagent_controller()
                && let Err(error) = controller.refresh_background_processes().await
            {
                tracing::warn!(error = %error, "Failed to reconcile lagged background completions");
            }
            for event in completion_drain.events.iter().chain(exec_completion_drain.events.iter()) {
                let style = if matches!(event.status, vtcode_core::subagents::BackgroundSubprocessStatus::Error) {
                    MessageStyle::Error
                } else {
                    MessageStyle::Info
                };
                let detail = event
                    .summary
                    .as_deref()
                    .or(event.error.as_deref())
                    .unwrap_or("no summary recorded");
                let exit = event.exit_code.map(|code| format!(" (exit {code})")).unwrap_or_default();
                ctx.renderer.line(
                    style,
                    &format!("Background task {} {}: {}{}", event.task_id, event.status.as_str(), detail, exit),
                )?;
                if let Some(emitter) = ctx.harness_emitter {
                    let _ = emitter.emit(background_completion_thread_event(event));
                }
            }
            let controller = ctx.tool_registry.subagent_controller();
            if let Some(outcome) = idle_completion_outcome(
                &inline_action,
                completion_drain.added.saturating_add(exec_completion_drain.added),
                controller.as_ref().is_some_and(|controller| controller.has_matrix_completion()),
            ) {
                if let Err(error) =
                    refresh_local_agents(ctx.handle, controller.as_ref(), ctx.tool_registry.exec_session_manager())
                        .await
                {
                    tracing::warn!(%error, "Failed to synchronize Local Agents after background completion");
                }
                return Ok(outcome);
            }
        }

        let current_input_activity = ctx.input_activity_counter.load(Ordering::Relaxed);
        if current_input_activity != last_input_activity {
            last_input_activity = current_input_activity;
            last_input_activity_at = Instant::now();
        }

        if durable_scheduler_run.as_ref().is_some_and(JoinHandle::is_finished) {
            let Some(task) = durable_scheduler_run.take() else {
                tracing::debug!("Durable scheduler task finished but handle was already consumed");
                continue;
            };
            let result = task.await;
            match result {
                Ok(Ok(triggered)) => {
                    last_durable_scheduler_error = None;
                    if triggered > 0 {
                        ctx.renderer.line(
                            MessageStyle::Info,
                            &format!(
                                "Triggered {triggered} durable scheduled task{}.",
                                if triggered == 1 { "" } else { "s" }
                            ),
                        )?;
                    }
                }
                Ok(Err(err)) => {
                    let error = err.to_string();
                    if last_durable_scheduler_error.as_deref() != Some(error.as_str()) {
                        tracing::warn!("Durable scheduler poll failed in interactive session: {}", error);
                        ctx.renderer
                            .line(MessageStyle::Warning, &format!("Durable scheduler poll failed: {error}"))?;
                        last_durable_scheduler_error = Some(error);
                    }
                }
                Err(err) => {
                    let error = err.to_string();
                    if last_durable_scheduler_error.as_deref() != Some(error.as_str()) {
                        tracing::warn!("Durable scheduler background task failed in interactive session: {}", error);
                        ctx.renderer
                            .line(MessageStyle::Warning, &format!("Durable scheduler task failed: {error}"))?;
                        last_durable_scheduler_error = Some(error);
                    }
                }
            }
        }

        if scheduler_enabled(ctx)
            && durable_scheduler_run.is_none()
            && last_durable_scheduler_poll.elapsed() >= DURABLE_SCHEDULER_POLL_INTERVAL
        {
            last_durable_scheduler_poll = Instant::now();

            if durable_scheduler_daemon.is_none() {
                match build_durable_scheduler_daemon() {
                    Ok(daemon) => durable_scheduler_daemon = Some(daemon),
                    Err(err) => {
                        let error = err.to_string();
                        if last_durable_scheduler_error.as_deref() != Some(error.as_str()) {
                            tracing::warn!("Failed to initialize durable scheduler in interactive session: {}", error);
                            last_durable_scheduler_error = Some(error);
                        }
                    }
                }
            }

            if let Some(daemon) = durable_scheduler_daemon.clone() {
                durable_scheduler_run = Some(tokio::spawn(async move { daemon.run_due_tasks_once().await }));
            }
        }

        if scheduler_enabled(ctx)
            && state.queued_inputs.is_empty()
            && last_input_activity_at.elapsed() >= SCHEDULED_PROMPT_INACTIVITY_GRACE
        {
            let due = ctx.tool_registry.collect_due_session_prompts(chrono::Utc::now()).await?;
            for task in due {
                state
                    .queued_inputs
                    .push_back(crate::agent::runloop::unified::inline_events::QueuedInput::new(
                        task.prompt.into(),
                        Some(ctx.active_primary_agent.active().display_name.clone()),
                    ));
                ctx.renderer.line(
                    MessageStyle::Info,
                    &format!("Scheduled task {} ({}) is ready to run.", task.id, task.name),
                )?;
            }
            if !state.queued_inputs.is_empty() {
                // Direct pushes bypass `InlineQueueState::sync_handle_queue`,
                // so mirror the authoritative FIFO to the TUI overlay
                // immediately instead of waiting for the next queue op.
                ctx.handle
                    .set_queued_inputs(state.queued_inputs.iter().map(|queued| queued.display_label()).collect());
            }
        }

        let (mut submitted_input, process_slash_commands) =
            match resolve_inline_loop_action(ctx, state, inline_action).await? {
                InlineLoopActionResolution::ContinueLoop => continue,
                InlineLoopActionResolution::Submit(input) => (input, true),
                InlineLoopActionResolution::SubmitPrompt(input) => (input, false),
                InlineLoopActionResolution::Outcome(outcome) => return Ok(outcome),
            };
        let mut input_owned = submitted_input.text.clone();

        if submitted_input.is_empty() {
            continue;
        }

        let submission_progress = ctx
            .handle
            .begin_progress(vtcode_commons::ui_protocol::ProgressPhase::PreparingContext);
        tracing::debug!(target: "vtcode.response_latency", operation_id = submission_progress.operation().id(), "submission accepted");

        // A fresh submitted input starts a new turn. Clear any stale local cancel
        // latch left behind by a prior interrupted turn so permission modals and
        // the provider stream don't inherit a spurious "interrupted" state.
        ctx.ctrl_c_state.reset();

        if let Err(err) = crate::agent::runloop::unified::turn::workspace::refresh_vt_config(
            &ctx.config.workspace,
            ctx.config,
            ctx.vt_cfg,
        )
        .await
        {
            tracing::warn!("Failed to refresh workspace configuration: {}", err);
            ctx.renderer
                .line(MessageStyle::Error, &format!("Failed to reload configuration: {err}"))?;
        }

        if let Some(cfg) = ctx.vt_cfg.as_ref()
            && let Err(err) = crate::agent::runloop::unified::turn::workspace::apply_workspace_config_to_registry(
                ctx.tool_registry,
                cfg,
            )
        {
            tracing::warn!("Failed to apply workspace configuration to tools: {}", err);
        }
        sync_mcp_approval_policy_for_context(ctx);

        if let Some(mcp_manager) = ctx.async_mcp_manager {
            let mcp_status = mcp_manager.get_status().await;
            if mcp_status.is_error()
                && let Some(error_msg) = mcp_status.get_error_message()
            {
                ctx.renderer.line(MessageStyle::Error, &format!("MCP Error: {error_msg}"))?;
                ctx.renderer
                    .line(MessageStyle::Info, "Use /mcp to check status or update your vtcode.toml configuration.")?;
            }
        }

        if let Some(next_placeholder) = ctx.follow_up_placeholder.take() {
            ctx.handle.set_placeholder(Some(next_placeholder.clone()));
            *ctx.default_placeholder = Some(next_placeholder);
        } else if state.input_status_state.is_blocked {
            state.input_status_state.is_blocked = false;
            ctx.handle.set_placeholder(ctx.default_placeholder.clone());
            ctx.handle.set_activity_state(vtcode_commons::ui_protocol::ActivityState::Idle);
        }

        if process_slash_commands {
            match slash_command_handler::handle_input_commands(input_owned.as_str(), ctx, state).await? {
                slash_command_handler::CommandProcessingResult::Outcome(outcome) => return Ok(outcome),
                slash_command_handler::CommandProcessingResult::ContinueLoop => continue,
                slash_command_handler::CommandProcessingResult::UpdateInput(new_input) => {
                    replace_submitted_input_text(&mut submitted_input, new_input);
                    input_owned.clone_from(&submitted_input.text);
                }
                slash_command_handler::CommandProcessingResult::NotHandled => {}
            }
        }

        if submitted_images_are_unsupported(
            &submitted_input,
            selected_model_supports_image_input(
                &ctx.config.provider,
                &ctx.config.model,
                ctx.provider_client.supports_vision(&ctx.config.model),
            ),
            &ctx.config.workspace,
        ) {
            ctx.renderer.line(
                MessageStyle::Warning,
                "The selected model does not support image input. Choose a vision-capable model or remove image attachments before submitting.",
            )?;
            ctx.handle.restore_input_draft(submitted_input);
            continue;
        }

        let turn_id = SessionId::generate().into_inner();

        if let Some(hooks) = ctx.lifecycle_hooks.as_ref() {
            match hooks.run_user_prompt_submit(&turn_id, input_owned.as_str()).await {
                Ok(outcome) => {
                    crate::agent::runloop::unified::turn::utils::render_hook_messages(ctx.renderer, &outcome.messages)?;
                    crate::agent::runloop::unified::turn::utils::append_additional_context(
                        ctx.conversation_history,
                        outcome.additional_context,
                    );
                    if !outcome.allow_prompt {
                        ctx.handle.clear_input();
                        continue;
                    }
                }
                Err(err) => {
                    ctx.renderer
                        .line(MessageStyle::Error, &format!("Failed to run prompt hooks: {err}"))?;
                }
            }
        }

        if let Some(picker) = state.model_picker_state.as_mut() {
            let progress = picker
                .handle_input(
                    ctx.renderer,
                    input_owned.as_str(),
                    ExternalUrlGuardContext::new(ctx.handle, ctx.session, ctx.ctrl_c_state, ctx.ctrl_c_notify),
                )
                .await?;
            match progress {
                ModelPickerProgress::InProgress => continue,
                ModelPickerProgress::NeedsRefresh => {
                    picker.refresh_dynamic_models(ctx.renderer).await?;
                    continue;
                }
                ModelPickerProgress::Cancelled => {
                    *state.model_picker_state = None;
                    continue;
                }
                ModelPickerProgress::Exit => {
                    *state.model_picker_state = None;
                    return Ok(InteractionOutcome::Exit { reason: SessionEndReason::Exit });
                }
                ModelPickerProgress::Completed(selection) => {
                    let Some(picker_state) = state.model_picker_state.take() else {
                        tracing::warn!("Model picker completed but state was missing; skipping completion flow");
                        continue;
                    };
                    let env_key_for_recovery = Some(selection.env_key.clone());
                    let harness_snapshot = ctx.tool_registry.harness_context_snapshot();
                    if let Err(err) = finalize_model_selection(
                        ctx.renderer,
                        &picker_state,
                        selection,
                        ctx.config,
                        ctx.vt_cfg,
                        ctx.provider_client,
                        ctx.session_bootstrap,
                        ctx.handle,
                        ctx.header_context,
                        ctx.full_auto,
                        ModelSwitchCompactionTargets {
                            history: ctx.conversation_history,
                            session_stats: ctx.session_stats,
                            context_manager: ctx.context_manager,
                            session_id: &harness_snapshot.session_id,
                            thread_id: ctx.thread_id,
                            lifecycle_hooks: ctx.lifecycle_hooks.as_ref(),
                            harness_emitter: ctx.harness_emitter,
                        },
                    )
                    .await
                    {
                        ctx.renderer
                            .line(MessageStyle::Error, &format!("Failed to apply model selection: {err}"))?;
                        if let Some(env_key) = &env_key_for_recovery
                            && !env_key.is_empty()
                        {
                            ctx.renderer.line(
                                MessageStyle::Info,
                                        &format!(
                                            "Recovery: set {} in your shell environment, or run `/secret add {}` in a session to store it securely.",
                                            env_key,
                                            ctx.config.provider,
                                        ),
                            )?;
                        }
                    }
                    continue;
                }
            }
        }

        let recent_follow_up_hint = if is_follow_up_prompt_like(input_owned.as_str()) {
            extract_recent_follow_up_hint(ctx.conversation_history)
        } else {
            None
        };

        if let Some((tool_name, tool_args)) = recent_follow_up_hint {
            let mut direct_tool_ctx = tool_dispatch::DirectToolContext {
                interaction_ctx: ctx,
                input_status_state: state.input_status_state,
            };
            if let Some(outcome) = tool_dispatch::execute_direct_tool_call(
                input_owned.as_str(),
                &tool_name,
                tool_args,
                false,
                &mut direct_tool_ctx,
            )
            .await?
            {
                return Ok(outcome);
            }
        }

        {
            let mut direct_tool_ctx = tool_dispatch::DirectToolContext {
                interaction_ctx: ctx,
                input_status_state: state.input_status_state,
            };

            if let Some(outcome) =
                tool_dispatch::handle_direct_tool_execution(input_owned.as_str(), &mut direct_tool_ctx).await?
            {
                return Ok(outcome);
            }
        }

        if let Some(outcome) = memory_prompt::handle_memory_prompt(input_owned.as_str(), ctx, state).await? {
            return Ok(outcome);
        }

        let follow_up_action = ctx.session_stats.register_follow_up_prompt(input_owned.as_str());
        if follow_up_action.should_force_autonomous_response() {
            if follow_up_action.is_stalled_recovery() {
                let stall_reason = follow_up_action
                    .stall_reason()
                    .unwrap_or("Previous turn stalled without a detailed reason.")
                    .to_string();
                let fallback_hint = extract_recent_follow_up_hint(ctx.conversation_history);
                // A stall with the verification gate still pending is not a
                // generic stuck turn: concluding would abandon unverified work
                // the user asked to continue. Resume verifier-first with the
                // configured-or-detected project command instead of the
                // generic conclude-oriented directive. Check both the live
                // snapshot and the stall reason text: the snapshot can be
                // lost across compaction/model-switch while the blocked
                // reason still names the pending gate — fail closed toward
                // verifier-first recovery rather than conclusion. Match the
                // existing helper convention (lowercase `contains` in
                // `tracker_auto_continue_is_recoverable_block`): stall reasons
                // may be compound/enriched with varying case.
                let verification_stalled = ctx.session_stats.verification_snapshot().0
                    || stall_reason.to_ascii_lowercase().contains("verification is still pending");
                if verification_stalled {
                    let verifier =
                        crate::agent::runloop::unified::turn::tool_outcomes::helpers::resolve_harness_verifier_command(
                            ctx.vt_cfg.as_ref(),
                            ctx.config.workspace.as_path(),
                            ctx.conversation_history,
                        );
                    ctx.conversation_history
                        .push(uni::Message::system(stalled_verification_resume_directive(verifier.as_deref())));
                } else {
                    ctx.conversation_history
                        .push(uni::Message::system(repeated_follow_up_directive(true)));
                }
                if let Some((tool, args)) = fallback_hint.as_ref() {
                    let args_preview = fallback_args_preview(args);
                    ctx.conversation_history.push(uni::Message::system(format!(
                        "Recovered fallback hint from recent tool error: call tool '{tool}' with args {args_preview} as the first adjusted strategy."
                    )));
                }
                ctx.session_stats.suppress_next_follow_up_prompt();
                ctx.conversation_history
                    .push(uni::Message::system(stalled_follow_up_recovery_prompt(
                        &stall_reason,
                        fallback_hint.is_some(),
                    )));
                ctx.renderer.line(
                    MessageStyle::Info,
                    if verification_stalled {
                        "Repeated follow-up after verification stall detected; resuming verifier-first recovery."
                    } else {
                        "Repeated follow-up after stalled turn detected; enforcing autonomous recovery and conclusion."
                    },
                )?;
            } else {
                ctx.conversation_history
                    .push(uni::Message::system(repeated_follow_up_directive(false)));
                ctx.renderer
                    .line(MessageStyle::Info, "Repeated follow-up detected; forcing a concrete status/conclusion.")?;
            }
        }
        submitted_input.text = input_owned;
        let input = submitted_input.text.as_str();

        let refined_content = build_user_message_content(ctx, &submitted_input).await;

        display_user_message(ctx.renderer, input)?;

        let user_message = match refined_content {
            uni::MessageContent::Text(text) => uni::Message::user(text),
            uni::MessageContent::Parts(parts) => uni::Message::user_with_parts(parts),
        };

        let prompt_message_index = ctx.conversation_history.len();
        ctx.conversation_history.push(user_message);
        submission_progress.transfer();
        return Ok(InteractionOutcome::Continue {
            input: input.to_string(),
            prompt_message_index: Some(prompt_message_index),
            turn_id,
        });
    }
}

#[cfg(test)]
mod tests;
