use anyhow::Result;
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;
use vtcode_core::llm::provider as uni;
use vtcode_core::ui::theme;
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_core::utils::session_archive::find_session_by_identifier;

use crate::agent::runloop::ResumeSession;
use crate::agent::runloop::model_picker::ModelPickerProgress;
use crate::agent::runloop::prompt::refine_and_enrich_prompt;
use crate::agent::runloop::tui_compat::{inline_theme_from_core_styles, to_tui_appearance};
use crate::agent::runloop::unified::display::display_user_message;
use crate::agent::runloop::unified::inline_events::{
    InlineEventLoopResources, InlineInterruptCoordinator, InlineLoopAction, poll_inline_loop_action,
};
use crate::agent::runloop::unified::model_selection::{
    finalize_model_selection, finalize_subagent_model_selection, finalize_team_model_selection,
};
use crate::agent::runloop::unified::state::ModelPickerTarget;
use crate::agent::runloop::unified::turn::session::{
    mcp_lifecycle, slash_command_handler, tool_dispatch,
};
use crate::hooks::lifecycle::SessionEndReason;
use vtcode::config_watcher::SimpleConfigWatcher;

use super::interaction_loop::{InteractionLoopContext, InteractionOutcome, InteractionState};
use super::interaction_loop_team::{direct_message_target, handle_team_switch, poll_team_mailbox};

const REPEATED_FOLLOW_UP_DIRECTIVE: &str = "User has asked to continue repeatedly. Do not keep exploring silently. In your next assistant response, provide a concrete status update: completed work, current blocker, and the exact next action. If a recent tool error provides a replacement tool (for example read_pty_session), use it directly instead of retrying the same failing call.";
const REPEATED_FOLLOW_UP_STALLED_DIRECTIVE: &str = "Previous turn stalled or aborted and the user asked to continue repeatedly. Recover autonomously without asking for more user prompts: identify the likely root cause from recent errors, execute exactly one adjusted strategy, and then provide either a completion summary or a final blocker review with specific next action. If the last tool error includes fallback_tool/fallback_tool_args, use that fallback first. Do not repeat a failing tool call when the error already provides the next tool to use.";
const FALLBACK_ARGS_PREVIEW_LIMIT: usize = 240;

#[derive(Debug, Deserialize)]
struct ToolErrorPayloadHint {
    #[serde(default)]
    fallback_tool: Option<String>,
    #[serde(default)]
    fallback_tool_args: Option<Value>,
    #[serde(default)]
    is_recoverable: Option<bool>,
}

fn extract_recent_fallback_hint(history: &[uni::Message]) -> Option<(String, Value)> {
    history.iter().rev().take(60).find_map(|message| {
        if !message.is_tool_response() {
            return None;
        }
        let content = message.get_text_content();
        let content_ref: &str = content.as_ref();
        if !content_ref.contains("\"fallback_tool\"")
            || !content_ref.contains("\"fallback_tool_args\"")
        {
            return None;
        }
        let parsed = serde_json::from_str::<ToolErrorPayloadHint>(content_ref).ok()?;
        let fallback_tool = parsed.fallback_tool?;
        let fallback_args = parsed.fallback_tool_args?;
        if parsed.is_recoverable == Some(false) {
            return None;
        }
        Some((fallback_tool, fallback_args))
    })
}

fn fallback_args_preview(args: &Value) -> String {
    let serialized =
        serde_json::to_string(args).unwrap_or_else(|_| "{\"action\":\"list\"}".to_string());
    let mut chars = serialized.chars();
    let mut preview = String::with_capacity(FALLBACK_ARGS_PREVIEW_LIMIT + 3);
    for _ in 0..FALLBACK_ARGS_PREVIEW_LIMIT {
        if let Some(ch) = chars.next() {
            preview.push(ch);
        } else {
            return serialized;
        }
    }
    if chars.next().is_some() {
        preview.push_str("...");
    }
    preview
}

fn append_file_reference_metadata(
    content: uni::MessageContent,
    input: &str,
    workspace: &Path,
) -> uni::MessageContent {
    let Some(metadata) = build_file_reference_metadata(input, workspace) else {
        return content;
    };

    match content {
        uni::MessageContent::Text(text) => uni::MessageContent::text(format!("{text}{metadata}")),
        uni::MessageContent::Parts(mut parts) => {
            parts.push(uni::ContentPart::text(metadata));
            uni::MessageContent::parts(parts)
        }
    }
}

fn build_file_reference_metadata(input: &str, workspace: &Path) -> Option<String> {
    let mut alias_to_full_path = BTreeMap::new();
    for at_match in vtcode_commons::at_pattern::find_at_patterns(input) {
        let alias = at_match.path.trim();
        if alias.is_empty() {
            continue;
        }
        if let Some(full_path) = resolve_full_path_for_alias(alias, workspace) {
            alias_to_full_path.insert(format!("@{alias}"), full_path);
        }
    }

    if alias_to_full_path.is_empty() {
        return None;
    }

    let mut metadata = String::from("\n\n[file_reference_metadata]\n");
    for (alias, full_path) in alias_to_full_path {
        metadata.push_str(&format!("{}={}\n", alias, full_path));
    }

    Some(metadata)
}

fn resolve_full_path_for_alias(alias: &str, workspace: &Path) -> Option<String> {
    let trimmed = alias.trim();
    if trimmed.is_empty()
        || trimmed.starts_with("http://")
        || trimmed.starts_with("https://")
        || trimmed.starts_with("data:")
        || !vtcode_commons::paths::is_safe_relative_path(trimmed)
    {
        return None;
    }

    let resolved =
        vtcode_commons::paths::resolve_workspace_path(workspace, Path::new(trimmed)).ok()?;
    Some(resolved.to_string_lossy().to_string())
}

fn apply_live_theme_and_appearance(
    handle: &vtcode_tui::InlineHandle,
    cfg: &vtcode_core::config::loader::VTCodeConfig,
) {
    let color_config = theme::ColorAccessibilityConfig {
        minimum_contrast: cfg.ui.minimum_contrast,
        bold_is_bright: cfg.ui.bold_is_bright,
        safe_colors_only: cfg.ui.safe_colors_only,
    };
    theme::set_color_accessibility_config(color_config);

    let selected = cfg.agent.theme.trim();
    let selected = if selected.is_empty() {
        theme::DEFAULT_THEME_ID
    } else {
        selected
    };
    if let Err(err) = theme::set_active_theme(selected) {
        tracing::warn!(
            theme = selected,
            error = %err,
            "Failed to activate configured theme; falling back to default"
        );
        let _ = theme::set_active_theme(theme::DEFAULT_THEME_ID);
    }

    let styles = theme::active_styles();
    handle.set_theme(inline_theme_from_core_styles(&styles));
    handle.set_appearance(to_tui_appearance(cfg));
    crate::agent::runloop::unified::palettes::apply_prompt_style(handle);
    handle.force_redraw();
}

pub(super) async fn run_interaction_loop_impl(
    ctx: &mut InteractionLoopContext<'_>,
    state: &mut InteractionState<'_>,
) -> Result<InteractionOutcome> {
    const MCP_REFRESH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);
    let mut live_reload_watcher = SimpleConfigWatcher::new(ctx.config.workspace.clone());
    live_reload_watcher.set_check_interval(1);
    live_reload_watcher.set_debounce_duration(200);

    loop {
        if live_reload_watcher.should_reload() {
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
            }
        }

        let spooled_count = ctx.tool_registry.spooled_files_count().await;
        crate::agent::runloop::unified::status_line::update_spooled_files_count(
            state.input_status_state,
            spooled_count,
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
        crate::agent::runloop::unified::status_line::update_team_status(
            state.input_status_state,
            ctx.session_stats,
        );

        if let Err(error) =
            crate::agent::runloop::unified::status_line::update_input_status_if_changed(
                ctx.handle,
                &ctx.config.workspace,
                &ctx.config.model,
                ctx.config.reasoning_effort.as_str(),
                ctx.vt_cfg.as_ref().map(|cfg| &cfg.ui.status_line),
                state.input_status_state,
            )
            .await
        {
            tracing::warn!("Failed to refresh status line: {}", error);
        }

        if let Err(error) = poll_team_mailbox(ctx).await {
            tracing::warn!("Failed to read team mailbox: {}", error);
        }

        if ctx.ctrl_c_state.is_exit_requested() {
            return Ok(InteractionOutcome::Exit {
                reason: SessionEndReason::Exit,
            });
        }

        let interrupts = InlineInterruptCoordinator::new(ctx.ctrl_c_state.as_ref());
        if let Some(mcp_manager) = ctx.async_mcp_manager {
            mcp_lifecycle::handle_mcp_updates(
                mcp_manager,
                ctx.tool_registry,
                ctx.tools,
                ctx.tool_catalog,
                ctx.renderer,
                state.mcp_catalog_initialized,
                state.last_mcp_refresh,
                state.last_known_mcp_tools,
                MCP_REFRESH_INTERVAL,
            )
            .await?;
        }

        let resources = InlineEventLoopResources {
            renderer: ctx.renderer,
            handle: ctx.handle,
            interrupts,
            ctrl_c_notice_displayed: state.ctrl_c_notice_displayed,
            default_placeholder: ctx.default_placeholder,
            queued_inputs: state.queued_inputs,
            model_picker_state: state.model_picker_state,
            palette_state: state.palette_state,
            config: ctx.config,
            vt_cfg: ctx.vt_cfg,
            provider_client: ctx.provider_client,
            session_bootstrap: ctx.session_bootstrap,
            full_auto: ctx.full_auto,
            team_active: ctx.session_stats.team_context.is_some(),
        };

        let mut input_owned =
            match poll_inline_loop_action(ctx.session, ctx.ctrl_c_notify, resources).await? {
                InlineLoopAction::Continue => continue,
                InlineLoopAction::Submit(text) => text,
                InlineLoopAction::ToggleDelegateMode => {
                    let enabled = ctx.session_stats.toggle_delegate_mode();
                    ctx.renderer.line(
                        MessageStyle::Info,
                        if enabled {
                            "Delegate mode enabled (coordination only)."
                        } else {
                            "Delegate mode disabled."
                        },
                    )?;
                    continue;
                }
                InlineLoopAction::SwitchTeammate(direction) => {
                    handle_team_switch(ctx, direction).await?;
                    continue;
                }
                InlineLoopAction::Exit(reason) => {
                    return Ok(InteractionOutcome::Exit { reason });
                }
                InlineLoopAction::PlanApproved {
                    auto_accept,
                    clear_context,
                } => {
                    ctx.renderer.line(
                        MessageStyle::Info,
                        if clear_context {
                            "Plan approved. Clearing context and auto-accepting edits..."
                        } else if auto_accept {
                            "Plan approved with auto-accept. Starting execution..."
                        } else {
                            "Plan approved. Starting execution with manual approval..."
                        },
                    )?;
                    return Ok(InteractionOutcome::PlanApproved {
                        auto_accept,
                        clear_context,
                    });
                }
                InlineLoopAction::PlanEditRequested => {
                    ctx.renderer.line(
                        MessageStyle::Info,
                        "Returning to plan mode. Continue refining your plan.",
                    )?;
                    continue;
                }
                InlineLoopAction::ResumeSession(session_id) => {
                    ctx.renderer.line(
                        MessageStyle::Info,
                        &format!("Loading session: {}", session_id),
                    )?;

                    match find_session_by_identifier(&session_id).await {
                        Ok(Some(listing)) => {
                            let resume = ResumeSession::from_listing(&listing, false);

                            ctx.renderer.line(
                                MessageStyle::Info,
                                &format!("Restarting with session: {}", session_id),
                            )?;
                            return Ok(InteractionOutcome::Resume {
                                resume_session: Box::new(resume),
                            });
                        }
                        Ok(None) => {
                            ctx.renderer.line(
                                MessageStyle::Error,
                                &format!("Session not found: {}", session_id),
                            )?;
                            continue;
                        }
                        Err(err) => {
                            ctx.renderer.line(
                                MessageStyle::Error,
                                &format!("Failed to load session: {}", err),
                            )?;
                            continue;
                        }
                    }
                }
                InlineLoopAction::DiffApproved | InlineLoopAction::DiffRejected => {
                    continue;
                }
            };

        if input_owned.is_empty() {
            continue;
        }

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

        if let Some(target) = direct_message_target(ctx.session_stats)
            && !input_owned.trim_start().starts_with('/')
            && let Some(team) = ctx.session_stats.team_state.as_mut()
        {
            team.send_message(&target, "lead", input_owned.clone(), None)
                .await?;
            ctx.renderer
                .line(MessageStyle::Info, &format!("Message sent to {}.", target))?;
            continue;
        }

        if let Some(hooks) = ctx.lifecycle_hooks {
            match hooks.run_user_prompt_submit(input_owned.as_str()).await {
                Ok(outcome) => {
                    crate::agent::runloop::unified::turn::utils::render_hook_messages(
                        ctx.renderer,
                        &outcome.messages,
                    )?;
                    if !outcome.allow_prompt {
                        ctx.handle.clear_input();
                        continue;
                    }
                    for context in outcome.additional_context {
                        if !context.trim().is_empty() {
                            ctx.conversation_history.push(uni::Message::system(context));
                        }
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
            let progress = picker.handle_input(ctx.renderer, input_owned.as_str())?;
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
                ModelPickerProgress::Completed(selection) => {
                    let Some(picker_state) = state.model_picker_state.take() else {
                        tracing::warn!(
                            "Model picker completed but state was missing; skipping completion flow"
                        );
                        continue;
                    };
                    let target = ctx.session_stats.model_picker_target;
                    ctx.session_stats.model_picker_target = ModelPickerTarget::Main;
                    match target {
                        ModelPickerTarget::Main => {
                            if let Err(err) = finalize_model_selection(
                                ctx.renderer,
                                &picker_state,
                                selection,
                                ctx.config,
                                ctx.vt_cfg,
                                ctx.provider_client,
                                ctx.session_bootstrap,
                                ctx.handle,
                                ctx.full_auto,
                            )
                            .await
                            {
                                ctx.renderer.line(
                                    MessageStyle::Error,
                                    &format!("Failed to apply model selection: {}", err),
                                )?;
                            }
                        }
                        ModelPickerTarget::SubagentDefault => {
                            if let Err(err) = finalize_subagent_model_selection(
                                ctx.renderer,
                                selection,
                                ctx.vt_cfg,
                                &ctx.config.workspace,
                            )
                            .await
                            {
                                ctx.renderer.line(
                                    MessageStyle::Error,
                                    &format!("Failed to set subagent model: {}", err),
                                )?;
                            }
                        }
                        ModelPickerTarget::TeamDefault => {
                            if let Err(err) = finalize_team_model_selection(
                                ctx.renderer,
                                selection,
                                ctx.vt_cfg,
                                &ctx.config.workspace,
                            )
                            .await
                            {
                                ctx.renderer.line(
                                    MessageStyle::Error,
                                    &format!("Failed to set team model: {}", err),
                                )?;
                            }
                        }
                    }
                    continue;
                }
            }
        }

        if ctx
            .session_stats
            .register_follow_up_prompt(input_owned.as_str())
        {
            if ctx.session_stats.turn_stalled() {
                let stall_reason = ctx
                    .session_stats
                    .turn_stall_reason()
                    .unwrap_or("Previous turn stalled without a detailed reason.")
                    .to_string();
                let fallback_hint = extract_recent_fallback_hint(ctx.conversation_history);
                if let Ok(mut detector) = ctx.autonomous_executor.loop_detector().write() {
                    detector.reset();
                } else {
                    tracing::warn!(
                        "Failed to reset loop detector during stalled follow-up recovery"
                    );
                }
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
                input_owned = if fallback_hint.is_some() {
                    format!(
                        "Continue autonomously from the last stalled turn. Stall reason: {}. Use the recovered fallback hint as the first adjusted strategy, then continue until you can provide a concrete conclusion and final review.",
                        stall_reason
                    )
                } else {
                    format!(
                        "Continue autonomously from the last stalled turn. Stall reason: {}. Keep working until you can provide a concrete conclusion and final review.",
                        stall_reason
                    )
                };
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
        {
            let mut direct_tool_ctx = tool_dispatch::DirectToolContext {
                interaction_ctx: ctx,
                input_status_state: state.input_status_state,
            };

            if let Some(outcome) =
                tool_dispatch::handle_direct_tool_execution(input, &mut direct_tool_ctx).await?
            {
                return Ok(outcome);
            }
        }

        let processed_content =
            match vtcode_core::utils::at_pattern::parse_at_patterns(input, &ctx.config.workspace)
                .await
            {
                Ok(content) => content,
                Err(e) => {
                    tracing::warn!("Failed to parse @ patterns: {}", e);
                    uni::MessageContent::text(input.to_string())
                }
            };

        let refined_content = match &processed_content {
            uni::MessageContent::Text(text) => {
                let refined_text =
                    refine_and_enrich_prompt(text, ctx.config, ctx.vt_cfg.as_ref()).await;
                uni::MessageContent::text(refined_text)
            }
            uni::MessageContent::Parts(parts) => {
                let mut refined_parts = Vec::new();
                for part in parts {
                    match part {
                        uni::ContentPart::Text { text } => {
                            let refined_text =
                                refine_and_enrich_prompt(text, ctx.config, ctx.vt_cfg.as_ref())
                                    .await;
                            refined_parts.push(uni::ContentPart::text(refined_text));
                        }
                        _ => refined_parts.push(part.clone()),
                    }
                }
                uni::MessageContent::parts(refined_parts)
            }
        };
        let refined_content =
            append_file_reference_metadata(refined_content, input, &ctx.config.workspace);

        display_user_message(ctx.renderer, input)?;

        let user_message = match refined_content {
            uni::MessageContent::Text(text) => uni::Message::user(text),
            uni::MessageContent::Parts(parts) => uni::Message::user_with_parts(parts),
        };

        ctx.conversation_history.push(user_message);
        return Ok(InteractionOutcome::Continue {
            input: input.to_string(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{
        append_file_reference_metadata, build_file_reference_metadata,
        extract_recent_fallback_hint, fallback_args_preview,
    };
    use std::fs;
    use tempfile::TempDir;
    use vtcode_core::llm::provider as uni;

    #[test]
    fn extract_recent_fallback_hint_reads_latest_recoverable_tool_payload() {
        let history = vec![
            uni::Message::tool_response(
                "call_1".to_string(),
                serde_json::json!({
                    "error": "older",
                    "is_recoverable": true,
                    "fallback_tool": "read_file",
                    "fallback_tool_args": {"path":"a.rs","offset":1,"limit":20}
                })
                .to_string(),
            ),
            uni::Message::tool_response(
                "call_2".to_string(),
                serde_json::json!({
                    "error": "newer",
                    "is_recoverable": true,
                    "fallback_tool": "task_tracker",
                    "fallback_tool_args": {"action":"list"}
                })
                .to_string(),
            ),
        ];

        let hint = extract_recent_fallback_hint(&history);
        assert_eq!(
            hint,
            Some((
                "task_tracker".to_string(),
                serde_json::json!({"action":"list"})
            ))
        );
    }

    #[test]
    fn extract_recent_fallback_hint_skips_non_recoverable_payloads() {
        let history = vec![uni::Message::tool_response(
            "call_1".to_string(),
            serde_json::json!({
                "error": "blocked",
                "is_recoverable": false,
                "fallback_tool": "read_file",
                "fallback_tool_args": {"path":"a.rs"}
            })
            .to_string(),
        )];

        assert!(extract_recent_fallback_hint(&history).is_none());
    }

    #[test]
    fn extract_recent_fallback_hint_skips_payloads_without_hint_fields() {
        let history = vec![
            uni::Message::tool_response(
                "call_1".to_string(),
                serde_json::json!({
                    "output": "very long output that should not be parsed for fallback hints"
                })
                .to_string(),
            ),
            uni::Message::tool_response(
                "call_2".to_string(),
                serde_json::json!({
                    "error": "newer",
                    "is_recoverable": true,
                    "fallback_tool": "task_tracker",
                    "fallback_tool_args": {"action":"list"}
                })
                .to_string(),
            ),
        ];
        let hint = extract_recent_fallback_hint(&history);
        assert_eq!(
            hint,
            Some((
                "task_tracker".to_string(),
                serde_json::json!({"action":"list"})
            ))
        );
    }

    #[test]
    fn fallback_args_preview_truncates_long_payloads() {
        let args = serde_json::json!({
            "action": "search",
            "query": "x".repeat(600)
        });
        let preview = fallback_args_preview(&args);
        assert!(preview.ends_with("..."));
        assert!(preview.len() <= 243);
    }

    #[test]
    fn build_file_reference_metadata_maps_aliases_to_full_paths() {
        let temp_dir = TempDir::new().expect("temp dir");
        let file_path = temp_dir.path().join("src").join("main.rs");
        fs::create_dir_all(file_path.parent().expect("parent")).expect("mkdir");
        fs::write(&file_path, "fn main() {}\n").expect("write file");

        let metadata =
            build_file_reference_metadata("check @src/main.rs and continue", temp_dir.path())
                .expect("metadata");

        assert!(metadata.contains("@src/main.rs="));
        assert!(metadata.contains("src/main.rs"));
    }

    #[test]
    fn append_file_reference_metadata_keeps_ui_alias_but_adds_full_path_context() {
        let temp_dir = TempDir::new().expect("temp dir");
        let file_path = temp_dir.path().join("README.md");
        fs::write(&file_path, "# test\n").expect("write file");

        let content = uni::MessageContent::text("check @README.md".to_string());
        let augmented =
            append_file_reference_metadata(content, "check @README.md", temp_dir.path());

        match augmented {
            uni::MessageContent::Text(text) => {
                assert!(text.contains("check @README.md"));
                assert!(text.contains("[file_reference_metadata]"));
                assert!(text.contains("@README.md="));
                assert!(text.contains("README.md"));
            }
            uni::MessageContent::Parts(_) => panic!("expected text content"),
        }
    }

    #[test]
    fn build_file_reference_metadata_ignores_non_file_aliases() {
        let temp_dir = TempDir::new().expect("temp dir");
        let metadata = build_file_reference_metadata("npm i @types/node", temp_dir.path());
        assert!(metadata.is_none());
    }

    #[test]
    fn build_file_reference_metadata_ignores_absolute_paths() {
        let temp_dir = TempDir::new().expect("temp dir");
        let metadata = build_file_reference_metadata("check @/tmp/example.rs", temp_dir.path());
        assert!(metadata.is_none());
    }
}
