mod support;

use anyhow::Result;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use tokio::task::JoinHandle;
use vtcode_core::hooks::SessionEndReason;
use vtcode_core::llm::provider as uni;
use vtcode_core::session::SessionId;
use vtcode_core::utils::ansi::MessageStyle;

use crate::agent::runloop::model_picker::ModelPickerProgress;
use crate::agent::runloop::unified::display::display_user_message;
use crate::agent::runloop::unified::external_url_guard::ExternalUrlGuardContext;
use crate::agent::runloop::unified::inline_events::{
    InlineEventLoopResources, InlineInterruptCoordinator, poll_inline_loop_action,
};
use crate::agent::runloop::unified::model_selection::finalize_model_selection;
use crate::agent::runloop::unified::session_setup::{
    apply_ide_context_snapshot, ide_context_status_label_from_bridge,
};
use crate::agent::runloop::unified::state::ModelPickerTarget;
use crate::agent::runloop::unified::state::is_follow_up_prompt_like;
use crate::agent::runloop::unified::turn::session::{
    mcp_lifecycle, memory_prompt, slash_command_handler, tool_dispatch,
};
use vtcode_config::loader::SimpleConfigWatcher;

use super::interaction_loop::{InteractionLoopContext, InteractionOutcome, InteractionState};
use support::{
    InlineLoopActionResolution, apply_live_theme_and_appearance, build_durable_scheduler_daemon,
    build_user_message_content, extract_recent_follow_up_hint, fallback_args_preview,
    refresh_ide_context_before_user_turn, refresh_live_ide_context_update,
    resolve_inline_loop_action, scheduler_enabled, stalled_follow_up_recovery_prompt,
    sync_mcp_approval_policy_for_context,
};

const REPEATED_FOLLOW_UP_DIRECTIVE: &str = "User has asked to continue repeatedly. Do not keep exploring silently. In your next assistant response, provide a concrete status update: completed work, current blocker, and the exact next action. If a recent tool result or tool error already provides `fallback_tool`, `fallback_tool_args`, `hint`, or `next_action`, use that guidance directly instead of retrying the same failing call or asking for more follow-up.";
const REPEATED_FOLLOW_UP_STALLED_DIRECTIVE: &str = "Previous turn stalled or aborted and the user asked to continue repeatedly. Recover autonomously without asking for more user prompts: identify the likely root cause from recent errors, execute exactly one adjusted strategy, and then provide either a completion summary or a final blocker review with specific next action. If the last tool result or tool error includes `fallback_tool`, `fallback_tool_args`, `hint`, or `next_action`, use that guidance first. Do not repeat a failing tool call when the tool already provided the next step.";
const SCHEDULED_PROMPT_INACTIVITY_GRACE: Duration = Duration::from_secs(2);
const DURABLE_SCHEDULER_POLL_INTERVAL: Duration = Duration::from_secs(1);

pub(super) async fn run_interaction_loop_impl(
    ctx: &mut InteractionLoopContext<'_>,
    state: &mut InteractionState<'_>,
) -> Result<InteractionOutcome> {
    const MCP_REFRESH_INTERVAL: Duration = Duration::from_secs(5);
    let mut last_input_activity = ctx.input_activity_counter.load(Ordering::Relaxed);
    let mut last_input_activity_at = Instant::now();
    let mut last_durable_scheduler_poll = Instant::now()
        .checked_sub(DURABLE_SCHEDULER_POLL_INTERVAL)
        .unwrap();
    let mut durable_scheduler_daemon = None;
    let mut last_durable_scheduler_error = None::<String>;
    let mut durable_scheduler_run = None::<JoinHandle<Result<usize>>>;
    let mut live_reload_watcher = SimpleConfigWatcher::new(ctx.config.workspace.clone());
    live_reload_watcher.set_check_interval(1);
    live_reload_watcher.set_debounce_duration(200);
    let mut last_status_refresh = Instant::now()
        .checked_sub(Duration::from_millis(500))
        .unwrap();
    const STATUS_REFRESH_INTERVAL: Duration = Duration::from_millis(200);

    loop {
        let mut workspace_config_reloaded = false;
        let should_refresh_status = last_status_refresh.elapsed() >= STATUS_REFRESH_INTERVAL;
        if should_refresh_status {
            last_status_refresh = Instant::now();
        }
        if should_refresh_status && live_reload_watcher.should_reload() {
            if let Err(err) = crate::agent::runloop::unified::turn::workspace::refresh_vt_config(
                &ctx.config.workspace,
                ctx.config,
                ctx.vt_cfg,
            )
            .await
            {
                tracing::warn!("Failed to live-reload workspace configuration: {}", err);
            } else if let Some(cfg) = ctx.vt_cfg.as_ref() {
                if let Err(err) =
                    crate::agent::runloop::unified::turn::workspace::apply_workspace_config_to_registry(
                        ctx.tool_registry,
                        cfg,
                    )
                {
                    tracing::warn!("Failed to apply live-reloaded workspace config: {}", err);
                }
                apply_live_theme_and_appearance(ctx.handle, cfg);
                sync_mcp_approval_policy_for_context(ctx);
                workspace_config_reloaded = true;
            }
        }

        if should_refresh_status {
            let live_ide_context = refresh_live_ide_context_update(ctx.ide_context_bridge);
            if live_ide_context.changed || workspace_config_reloaded {
                apply_ide_context_snapshot(
                    ctx.context_manager,
                    ctx.header_context,
                    ctx.handle,
                    ctx.config.workspace.as_path(),
                    ctx.vt_cfg.as_ref(),
                    live_ide_context.snapshot.clone(),
                );
            }
            crate::agent::runloop::unified::status_line::update_ide_context_source(
                state.input_status_state,
                ide_context_status_label_from_bridge(
                    ctx.context_manager,
                    ctx.config.workspace.as_path(),
                    ctx.vt_cfg.as_ref(),
                    ctx.ide_context_bridge.as_ref(),
                ),
            );

            let spooled_count = ctx.tool_registry.spooled_files_count().await;
            crate::agent::runloop::unified::status_line::update_spooled_files_count(
                state.input_status_state,
                spooled_count,
            );
            let local_agent_count =
                if let Some(controller) = ctx.tool_registry.subagent_controller() {
                    let entries = controller.status_entries().await;
                    crate::agent::runloop::ui::sync_active_subagent_badges(
                        ctx.header_context,
                        ctx.handle,
                        &entries,
                    );
                    let delegated_count = entries
                        .iter()
                        .filter(|entry| !entry.status.is_terminal())
                        .count();
                    let background_count = controller
                        .background_status_entries()
                        .await
                        .into_iter()
                        .filter(|entry| {
                            matches!(
                                entry.status,
                                vtcode_core::subagents::BackgroundSubprocessStatus::Starting
                                    | vtcode_core::subagents::BackgroundSubprocessStatus::Running
                            ) || (entry.desired_enabled
                                && matches!(
                                    entry.status,
                                    vtcode_core::subagents::BackgroundSubprocessStatus::Error
                                ))
                        })
                        .count();
                    delegated_count + background_count
                } else {
                    crate::agent::runloop::ui::sync_active_subagent_badges(
                        ctx.header_context,
                        ctx.handle,
                        &[],
                    );
                    0
                };
            crate::agent::runloop::unified::status_line::update_thread_context(
                state.input_status_state,
                ctx.active_thread_label,
                local_agent_count,
            );
            let context_limit_tokens = ctx
                .provider_client
                .effective_context_size(&ctx.config.model);
            let context_used_tokens = ctx.context_manager.current_token_usage();
            crate::agent::runloop::unified::status_line::update_context_budget(
                state.input_status_state,
                context_used_tokens,
                context_limit_tokens,
            );

            // Track running cost, cache hit, and balance for visible auto status components.
            let model = &ctx.config.model;
            let status = &mut state.input_status_state;
            let status_config = ctx.vt_cfg.as_ref().map(|cfg| &cfg.ui.status_line);
            status.show_costs =
                crate::agent::runloop::unified::status_line::status_line_shows_auto_components(
                    status_config,
                ) && matches!(ctx.provider_client.name(), "deepseek" | "openai");
            status.cost_usd = ctx.session_stats.total_cost_usd();
            let usage = ctx.session_stats.total_usage();
            let total_cache = usage.cached_input_tokens + usage.cache_creation_tokens;
            status.cache_hit_pct = (total_cache > 0)
                .then(|| (usage.cached_input_tokens as f64 / total_cache as f64) * 100.0);

            if let Err(error) =
                crate::agent::runloop::unified::status_line::update_input_status_if_changed(
                    ctx.handle,
                    &ctx.config.workspace,
                    model,
                    ctx.config.reasoning_effort.as_str(),
                    status_config,
                    status,
                )
                .await
            {
                tracing::warn!("Failed to refresh status line: {}", error);
            }

            // Periodically fetch account balance for providers that support it
            if status.show_costs {
                crate::agent::runloop::unified::status_line::refresh_balance_info(
                    ctx.provider_client.as_ref(),
                    ctx.handle,
                    &ctx.config.workspace,
                    model,
                    ctx.config.reasoning_effort.as_str(),
                    status_config,
                    status,
                )
                .await;
            } else {
                status.balance = None;
                status.last_balance_refresh = None;
            }
            ctx.handle.set_terminal_title_items(
                ctx.vt_cfg
                    .as_ref()
                    .and_then(|cfg| cfg.ui.terminal_title.items.clone()),
            );
            ctx.handle
                .set_terminal_title_thread_label(state.input_status_state.thread_context.clone());
            ctx.handle.set_terminal_title_git_branch(
                state
                    .input_status_state
                    .git_summary
                    .as_ref()
                    .map(|summary| summary.branch.clone())
                    .filter(|branch| !branch.trim().is_empty()),
            );

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
            return Ok(InteractionOutcome::Exit {
                reason: SessionEndReason::Exit,
            });
        }

        let interrupts = InlineInterruptCoordinator::new(ctx.ctrl_c_state.as_ref());
        let use_unicode = ctx.renderer.should_use_unicode_formatting();
        let idle_wake_delay = STATUS_REFRESH_INTERVAL.saturating_sub(last_status_refresh.elapsed());
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
            conversation_history_len: ctx.conversation_history.len(),
            idle_wake_delay,
        };

        let inline_action =
            poll_inline_loop_action(ctx.session, ctx.ctrl_c_notify, resources).await?;
        sync_mcp_approval_policy_for_context(ctx);

        let current_input_activity = ctx.input_activity_counter.load(Ordering::Relaxed);
        if current_input_activity != last_input_activity {
            last_input_activity = current_input_activity;
            last_input_activity_at = Instant::now();
        }

        if durable_scheduler_run
            .as_ref()
            .is_some_and(JoinHandle::is_finished)
        {
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
                        tracing::warn!(
                            "Durable scheduler poll failed in interactive session: {}",
                            error
                        );
                        ctx.renderer.line(
                            MessageStyle::Warning,
                            &format!("Durable scheduler poll failed: {}", error),
                        )?;
                        last_durable_scheduler_error = Some(error);
                    }
                }
                Err(err) => {
                    let error = err.to_string();
                    if last_durable_scheduler_error.as_deref() != Some(error.as_str()) {
                        tracing::warn!(
                            "Durable scheduler background task failed in interactive session: {}",
                            error
                        );
                        ctx.renderer.line(
                            MessageStyle::Warning,
                            &format!("Durable scheduler task failed: {}", error),
                        )?;
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
                            tracing::warn!(
                                "Failed to initialize durable scheduler in interactive session: {}",
                                error
                            );
                            last_durable_scheduler_error = Some(error);
                        }
                    }
                }
            }

            if let Some(daemon) = durable_scheduler_daemon.clone() {
                durable_scheduler_run =
                    Some(tokio::spawn(
                        async move { daemon.run_due_tasks_once().await },
                    ));
            }
        }

        if scheduler_enabled(ctx)
            && state.queued_inputs.is_empty()
            && last_input_activity_at.elapsed() >= SCHEDULED_PROMPT_INACTIVITY_GRACE
        {
            let due = ctx
                .tool_registry
                .collect_due_session_prompts(chrono::Utc::now())
                .await?;
            for task in due {
                state.queued_inputs.push_back(task.prompt);
                ctx.renderer.line(
                    MessageStyle::Info,
                    &format!(
                        "Scheduled task {} ({}) is ready to run.",
                        task.id, task.name
                    ),
                )?;
            }
        }

        let mut input_owned = match resolve_inline_loop_action(ctx, state, inline_action).await? {
            InlineLoopActionResolution::ContinueLoop => continue,
            InlineLoopActionResolution::Submit(text) => text,
            InlineLoopActionResolution::Outcome(outcome) => return Ok(outcome),
        };

        if input_owned.is_empty() {
            continue;
        }

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
            ctx.renderer.line(
                MessageStyle::Error,
                &format!("Failed to reload configuration: {}", err),
            )?;
        }

        if let Some(cfg) = ctx.vt_cfg.as_ref()
            && let Err(err) =
                crate::agent::runloop::unified::turn::workspace::apply_workspace_config_to_registry(
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
                ctx.renderer
                    .line(MessageStyle::Error, &format!("MCP Error: {}", error_msg))?;
                ctx.renderer.line(
                    MessageStyle::Info,
                    "Use /mcp to check status or update your vtcode.toml configuration.",
                )?;
            }
        }

        if let Some(next_placeholder) = ctx.follow_up_placeholder.take() {
            ctx.handle.set_placeholder(Some(next_placeholder.clone()));
            *ctx.default_placeholder = Some(next_placeholder);
        }

        match slash_command_handler::handle_input_commands(input_owned.as_str(), ctx, state).await?
        {
            slash_command_handler::CommandProcessingResult::Outcome(outcome) => return Ok(outcome),
            slash_command_handler::CommandProcessingResult::ContinueLoop => continue,
            slash_command_handler::CommandProcessingResult::UpdateInput(new_input) => {
                input_owned = new_input;
            }
            slash_command_handler::CommandProcessingResult::NotHandled => {}
        }

        let turn_id = SessionId::new().0;

        if let Some(hooks) = ctx.lifecycle_hooks {
            match hooks
                .run_user_prompt_submit(&turn_id, input_owned.as_str())
                .await
            {
                Ok(outcome) => {
                    crate::agent::runloop::unified::turn::utils::render_hook_messages(
                        ctx.renderer,
                        &outcome.messages,
                    )?;
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
                    ctx.renderer.line(
                        MessageStyle::Error,
                        &format!("Failed to run prompt hooks: {}", err),
                    )?;
                }
            }
        }

        if let Some(picker) = state.model_picker_state.as_mut() {
            let progress = picker
                .handle_input(
                    ctx.renderer,
                    input_owned.as_str(),
                    ExternalUrlGuardContext::new(
                        ctx.handle,
                        ctx.session,
                        ctx.ctrl_c_state,
                        ctx.ctrl_c_notify,
                    ),
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
                    return Ok(InteractionOutcome::Exit {
                        reason: SessionEndReason::Exit,
                    });
                }
                ModelPickerProgress::Completed(selection) => {
                    let Some(picker_state) = state.model_picker_state.take() else {
                        tracing::warn!(
                            "Model picker completed but state was missing; skipping completion flow"
                        );
                        continue;
                    };
                    let target = ctx.session_stats.model_picker_target;
                    ctx.session_stats.model_picker_target = ModelPickerTarget::Main;
                    if target == ModelPickerTarget::Main
                        && let Err(err) = finalize_model_selection(
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
                            ctx.conversation_history.len(),
                        )
                        .await
                    {
                        ctx.renderer.line(
                            MessageStyle::Error,
                            &format!("Failed to apply model selection: {}", err),
                        )?;
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

            if let Some(outcome) = tool_dispatch::handle_direct_tool_execution(
                input_owned.as_str(),
                &mut direct_tool_ctx,
            )
            .await?
            {
                return Ok(outcome);
            }
        }

        if let Some(outcome) =
            memory_prompt::handle_memory_prompt(input_owned.as_str(), ctx, state).await?
        {
            return Ok(outcome);
        }

        let follow_up_action = ctx
            .session_stats
            .register_follow_up_prompt(input_owned.as_str());
        if follow_up_action.should_force_autonomous_response() {
            if follow_up_action.is_stalled_recovery() {
                let stall_reason = follow_up_action
                    .stall_reason()
                    .unwrap_or("Previous turn stalled without a detailed reason.")
                    .to_string();
                let fallback_hint = extract_recent_follow_up_hint(ctx.conversation_history);
                ctx.conversation_history.push(uni::Message::system(
                    REPEATED_FOLLOW_UP_STALLED_DIRECTIVE.to_string(),
                ));
                if let Some((tool, args)) = fallback_hint.as_ref() {
                    let args_preview = fallback_args_preview(args);
                    ctx.conversation_history.push(uni::Message::system(format!(
                        "Recovered fallback hint from recent tool error: call tool '{}' with args {} as the first adjusted strategy.",
                        tool, args_preview
                    )));
                }
                ctx.session_stats.suppress_next_follow_up_prompt();
                ctx.conversation_history.push(uni::Message::system(
                    stalled_follow_up_recovery_prompt(&stall_reason, fallback_hint.is_some()),
                ));
                ctx.renderer.line(
                    MessageStyle::Info,
                    "Repeated follow-up after stalled turn detected; enforcing autonomous recovery and conclusion.",
                )?;
            } else {
                let directive = REPEATED_FOLLOW_UP_DIRECTIVE;
                ctx.conversation_history
                    .push(uni::Message::system(directive.to_string()));
                ctx.renderer.line(
                    MessageStyle::Info,
                    "Repeated follow-up detected; forcing a concrete status/conclusion.",
                )?;
            }
        }
        let input = input_owned.as_str();

        let refined_content = build_user_message_content(ctx, input).await;
        refresh_ide_context_before_user_turn(ctx, state.input_status_state);

        display_user_message(ctx.renderer, input)?;

        let user_message = match refined_content {
            uni::MessageContent::Text(text) => uni::Message::user(text),
            uni::MessageContent::Parts(parts) => uni::Message::user_with_parts(parts),
        };

        let prompt_message_index = ctx.conversation_history.len();
        ctx.conversation_history.push(user_message);
        return Ok(InteractionOutcome::Continue {
            input: input.to_string(),
            prompt_message_index: Some(prompt_message_index),
            turn_id,
        });
    }
}
