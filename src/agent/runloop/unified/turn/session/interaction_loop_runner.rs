use anyhow::Result;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use tokio::task::JoinHandle;
use vtcode_core::config::EditorToolConfig;
use vtcode_core::config::constants::tools as tool_names;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::core::threads::ArchivedSessionIntent;
use vtcode_core::hooks::SessionEndReason;
use vtcode_core::llm::provider as uni;
use vtcode_core::scheduler::{DurableTaskStore, SchedulerDaemon};
use vtcode_core::session::SessionId;
use vtcode_core::tools::continuation::{PtyContinuationArgs, ReadChunkContinuationArgs};
use vtcode_core::tools::terminal_app::{EditorLaunchConfig, TerminalAppLauncher};
use vtcode_core::ui::theme;
use vtcode_core::ui::{inline_theme_from_core_styles, to_tui_appearance};
use vtcode_core::utils::ansi::MessageStyle;

use crate::agent::runloop::model_picker::ModelPickerProgress;
use crate::agent::runloop::prompt::refine_and_enrich_prompt;
use crate::agent::runloop::unified::async_mcp_manager::{
    AsyncMcpManager, approval_policy_from_human_in_the_loop,
};
use crate::agent::runloop::unified::display::display_user_message;
use crate::agent::runloop::unified::external_url_guard::ExternalUrlGuardContext;
use crate::agent::runloop::unified::inline_events::{
    InlineEventLoopResources, InlineInterruptCoordinator, InlineLoopAction, poll_inline_loop_action,
};
use crate::agent::runloop::unified::interactive_features::{
    PromptSuggestionSource, generate_inline_prompt_suggestion,
};
use crate::agent::runloop::unified::model_selection::finalize_model_selection;
use crate::agent::runloop::unified::session_setup::{
    apply_ide_context_snapshot, ide_context_status_label_from_bridge,
};
use crate::agent::runloop::unified::state::ModelPickerTarget;
use crate::agent::runloop::unified::state::is_follow_up_prompt_like;
use crate::agent::runloop::unified::turn::session::{
    mcp_lifecycle, memory_prompt, slash_command_handler,
    slash_commands::run_with_event_loop_suspended, tool_dispatch,
};
use vtcode_config::loader::SimpleConfigWatcher;

use super::interaction_loop::{InteractionLoopContext, InteractionOutcome, InteractionState};

const REPEATED_FOLLOW_UP_DIRECTIVE: &str = "User has asked to continue repeatedly. Do not keep exploring silently. In your next assistant response, provide a concrete status update: completed work, current blocker, and the exact next action. If a recent tool result or tool error already provides `fallback_tool`, `fallback_tool_args`, `hint`, or `next_action`, use that guidance directly instead of retrying the same failing call or asking for more follow-up.";
const REPEATED_FOLLOW_UP_STALLED_DIRECTIVE: &str = "Previous turn stalled or aborted and the user asked to continue repeatedly. Recover autonomously without asking for more user prompts: identify the likely root cause from recent errors, execute exactly one adjusted strategy, and then provide either a completion summary or a final blocker review with specific next action. If the last tool result or tool error includes `fallback_tool`, `fallback_tool_args`, `hint`, or `next_action`, use that guidance first. Do not repeat a failing tool call when the tool already provided the next step.";
const FALLBACK_ARGS_PREVIEW_LIMIT: usize = 240;
const SCHEDULED_PROMPT_INACTIVITY_GRACE: Duration = Duration::from_secs(2);
const DURABLE_SCHEDULER_POLL_INTERVAL: Duration = Duration::from_secs(1);
const REVIEW_SCROLLBACK_EXIT_HINT: &str =
    "[Native scrollback view. Press Esc, q, or Alt+O to return to fullscreen.]";

#[derive(Debug, Deserialize)]
struct ToolErrorPayloadHint {
    #[serde(default)]
    fallback_tool: Option<String>,
    #[serde(default)]
    fallback_tool_args: Option<Value>,
    #[serde(default)]
    is_recoverable: Option<bool>,
}

fn extract_recent_follow_up_hint(history: &[uni::Message]) -> Option<(String, Value)> {
    let mut saw_trailing_tool = false;
    for message in history.iter().rev().take(60) {
        if message.is_tool_response() {
            saw_trailing_tool = true;
        } else if message.role == uni::MessageRole::System {
            continue;
        } else {
            break;
        }

        if !message.is_tool_response() {
            continue;
        }

        let content = message.get_text_content();
        let Some(parsed) = serde_json::from_str::<Value>(content.as_ref()).ok() else {
            continue;
        };
        let Some(obj) = parsed.as_object() else {
            continue;
        };

        if let Some(next_continue) = obj
            .get("next_continue_args")
            .and_then(PtyContinuationArgs::from_value)
        {
            return Some((
                tool_names::UNIFIED_EXEC.to_string(),
                json!({
                    "action": "continue",
                    "session_id": next_continue.session_id
                }),
            ));
        }

        if let Some(next_read) = obj
            .get("next_read_args")
            .and_then(ReadChunkContinuationArgs::from_value)
        {
            return Some((
                tool_names::UNIFIED_FILE.to_string(),
                json!({
                    "action": "read",
                    "path": next_read.path,
                    "offset": next_read.offset,
                    "limit": next_read.limit
                }),
            ));
        }

        let content_ref: &str = content.as_ref();
        if content_ref.contains("\"fallback_tool\"")
            && content_ref.contains("\"fallback_tool_args\"")
        {
            let Some(parsed) = serde_json::from_str::<ToolErrorPayloadHint>(content_ref).ok()
            else {
                continue;
            };
            let Some(fallback_tool) = parsed.fallback_tool else {
                continue;
            };
            let Some(fallback_args) = parsed.fallback_tool_args else {
                continue;
            };
            if parsed.is_recoverable == Some(false) {
                continue;
            }
            return Some((fallback_tool, fallback_args));
        }
    }

    if saw_trailing_tool {
        tracing::debug!("No continuation hint found in trailing tool responses");
    }

    None
}

fn review_editor_launch_config(editor_config: &EditorToolConfig) -> EditorLaunchConfig {
    EditorLaunchConfig {
        preferred_editor: (!editor_config.preferred_editor.trim().is_empty())
            .then(|| editor_config.preferred_editor.clone()),
        wait_for_editor: true,
    }
}

async fn open_transcript_review_in_editor(
    ctx: &mut InteractionLoopContext<'_>,
    text: String,
) -> Result<()> {
    let editor_config = ctx
        .vt_cfg
        .as_ref()
        .map(|config| config.tools.editor.clone())
        .unwrap_or_default();
    if !editor_config.enabled {
        ctx.renderer.line(
            MessageStyle::Warning,
            "External editor is disabled (`tools.editor.enabled = false`).",
        )?;
        return Ok(());
    }

    let mut temp = tempfile::NamedTempFile::new()?;
    temp.write_all(text.as_bytes())?;
    temp.flush()?;
    let (_, path) = temp.keep()?;
    let launcher = TerminalAppLauncher::new(ctx.config.workspace.clone());
    let launch_config = review_editor_launch_config(&editor_config);
    let result = run_with_event_loop_suspended(ctx.handle, editor_config.suspend_tui, || {
        launcher.launch_editor_with_config(Some(path.clone()), launch_config)
    })
    .await;
    let cleanup = fs::remove_file(&path);
    ctx.handle.force_redraw();

    match result {
        Ok(_) => {
            ctx.renderer
                .line(MessageStyle::Info, "Transcript review opened in editor.")?;
        }
        Err(err) => {
            ctx.renderer.line(
                MessageStyle::Error,
                &format!("Failed to open transcript review in editor: {err}"),
            )?;
        }
    }

    if let Err(err) = cleanup {
        tracing::debug!(%err, path = %path.display(), "failed to remove transcript review temp file");
    }

    Ok(())
}

fn show_transcript_review_in_scrollback(text: &str, mouse_capture: bool) -> Result<()> {
    use ratatui::crossterm::{
        event::{
            self, DisableBracketedPaste, DisableFocusChange, DisableMouseCapture,
            EnableBracketedPaste, EnableFocusChange, EnableMouseCapture, Event, KeyCode,
            KeyEventKind, KeyModifiers,
        },
        execute,
        terminal::{Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
    };

    let mut stderr = io::stderr();
    execute!(stderr, LeaveAlternateScreen)?;
    if mouse_capture {
        let _ = execute!(stderr, DisableMouseCapture);
    }
    let _ = execute!(stderr, DisableFocusChange, DisableBracketedPaste);
    write!(stderr, "{text}")?;
    if !text.ends_with('\n') {
        writeln!(stderr)?;
    }
    writeln!(stderr)?;
    writeln!(stderr, "{REVIEW_SCROLLBACK_EXIT_HINT}")?;
    stderr.flush()?;

    loop {
        match event::read()? {
            Event::Key(key)
                if matches!(key.kind, KeyEventKind::Press)
                    && (matches!(
                        key.code,
                        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q')
                    ) || (key.modifiers.contains(KeyModifiers::ALT)
                        && matches!(key.code, KeyCode::Char('o') | KeyCode::Char('O')))) =>
            {
                break;
            }
            _ => {}
        }
    }

    execute!(stderr, EnterAlternateScreen, Clear(ClearType::All))?;
    let _ = execute!(stderr, EnableBracketedPaste, EnableFocusChange);
    if mouse_capture {
        let _ = execute!(stderr, EnableMouseCapture);
    }
    stderr.flush()?;
    Ok(())
}

async fn open_transcript_review_scrollback(
    ctx: &mut InteractionLoopContext<'_>,
    text: String,
) -> Result<()> {
    let mouse_capture = ctx
        .vt_cfg
        .as_ref()
        .map(|config| config.ui.fullscreen.mouse_capture)
        .unwrap_or(true);
    let result = run_with_event_loop_suspended(ctx.handle, true, || {
        show_transcript_review_in_scrollback(&text, mouse_capture)
    })
    .await;
    ctx.handle.force_redraw();
    if let Err(err) = result {
        ctx.renderer.line(
            MessageStyle::Error,
            &format!("Failed to open transcript in native scrollback: {err}"),
        )?;
    }
    Ok(())
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

fn stalled_follow_up_recovery_prompt(stall_reason: &str, has_fallback_hint: bool) -> String {
    if has_fallback_hint {
        format!(
            "Continue autonomously from the last stalled turn. Stall reason: {}. Use the recovered fallback hint as the first adjusted strategy, then continue until you can provide a concrete conclusion and final review.",
            stall_reason
        )
    } else {
        format!(
            "Continue autonomously from the last stalled turn. Stall reason: {}. Keep working until you can provide a concrete conclusion and final review.",
            stall_reason
        )
    }
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

fn append_agent_reference_metadata(
    content: uni::MessageContent,
    selected_agents: &[String],
) -> uni::MessageContent {
    if selected_agents.is_empty() {
        return content;
    }

    let mut metadata = String::from("\n\n[agent_reference_metadata]\n");
    for mention in selected_agents {
        metadata.push_str(&format!("selected=@agent-{mention}\n"));
    }

    match content {
        uni::MessageContent::Text(text) => uni::MessageContent::text(format!("{text}{metadata}")),
        uni::MessageContent::Parts(mut parts) => {
            parts.push(uni::ContentPart::text(metadata));
            uni::MessageContent::parts(parts)
        }
    }
}

fn supports_native_openai_file_inputs(
    provider_name: &str,
    model_supports_responses_compaction: bool,
) -> bool {
    provider_name.eq_ignore_ascii_case("openai") && model_supports_responses_compaction
}

async fn handle_inline_prompt_suggestion_request(
    ctx: &mut InteractionLoopContext<'_>,
    state: &mut InteractionState<'_>,
    draft: &str,
) -> Result<()> {
    let Some(suggestion) = generate_inline_prompt_suggestion(
        ctx.provider_client.as_ref(),
        ctx.config,
        ctx.vt_cfg.as_ref(),
        &ctx.config.workspace,
        ctx.conversation_history,
        ctx.session_stats,
        ctx.tool_registry,
        draft,
    )
    .await
    else {
        ctx.handle.clear_inline_prompt_suggestion();
        ctx.renderer.line(
            MessageStyle::Info,
            "No inline prompt suggestion is available for the current draft.",
        )?;
        return Ok(());
    };

    if suggestion.source == PromptSuggestionSource::Llm
        && ctx
            .vt_cfg
            .as_ref()
            .map(|cfg| cfg.agent.prompt_suggestions.show_cost_notice)
            .unwrap_or(true)
        && !*state.inline_prompt_cost_notice_shown
    {
        ctx.renderer.line(
            MessageStyle::Info,
            "Inline prompt suggestions may use tokens when VT Code calls your configured LLM provider.",
        )?;
        *state.inline_prompt_cost_notice_shown = true;
    }

    ctx.handle.set_inline_prompt_suggestion(
        suggestion.prompt,
        suggestion.source == PromptSuggestionSource::Llm,
    );
    Ok(())
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
    handle: &vtcode_tui::app::InlineHandle,
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

fn sync_mcp_approval_policy(
    async_mcp_manager: Option<&AsyncMcpManager>,
    vt_cfg: Option<&VTCodeConfig>,
) {
    let (Some(mcp_manager), Some(cfg)) = (async_mcp_manager, vt_cfg) else {
        return;
    };
    let desired_policy = approval_policy_from_human_in_the_loop(cfg.security.human_in_the_loop);
    if mcp_manager.approval_policy() != desired_policy {
        mcp_manager.set_approval_policy(desired_policy);
    }
}

fn sync_mcp_approval_policy_for_context(ctx: &InteractionLoopContext<'_>) {
    sync_mcp_approval_policy(ctx.async_mcp_manager.as_deref(), ctx.vt_cfg.as_ref());
}

fn scheduler_enabled(ctx: &InteractionLoopContext<'_>) -> bool {
    let enabled = ctx
        .vt_cfg
        .as_ref()
        .map(|cfg| cfg.automation.scheduled_tasks.enabled)
        .unwrap_or(false);
    vtcode_core::scheduler::scheduled_tasks_enabled(enabled)
}

fn build_durable_scheduler_daemon() -> Result<SchedulerDaemon> {
    let store = DurableTaskStore::new_default()?;
    let executable = std::env::current_exe()?;
    Ok(SchedulerDaemon::new(store, executable))
}

#[derive(Default)]
struct LiveIdeContextUpdate {
    snapshot: Option<vtcode_core::EditorContextSnapshot>,
    changed: bool,
}

fn refresh_live_ide_context_update(
    ide_context_bridge: &mut Option<
        crate::agent::runloop::unified::session_setup::IdeContextBridge,
    >,
) -> LiveIdeContextUpdate {
    let Some(bridge) = ide_context_bridge.as_mut() else {
        return LiveIdeContextUpdate::default();
    };

    match bridge.refresh() {
        Ok((snapshot, refresh_state)) => LiveIdeContextUpdate {
            snapshot,
            changed: refresh_state.changed,
        },
        Err(err) => {
            tracing::warn!(
                error = %err,
                "Failed to refresh IDE context during live UI update"
            );
            LiveIdeContextUpdate {
                snapshot: bridge.snapshot().cloned(),
                changed: false,
            }
        }
    }
}

async fn try_resume_archived_session(
    renderer: &mut vtcode_core::utils::ansi::AnsiRenderer,
    session_id: &str,
    intent: ArchivedSessionIntent,
    loading_message: &str,
    success_message: &str,
) -> Result<Option<InteractionOutcome>> {
    renderer.line(
        MessageStyle::Info,
        &format!("{loading_message}: {session_id}"),
    )?;

    match crate::agent::agents::load_resume_session(session_id, intent).await {
        Ok(Some(resume)) => {
            renderer.line(
                MessageStyle::Info,
                &format!("{success_message}: {session_id}"),
            )?;
            Ok(Some(InteractionOutcome::Resume {
                resume_session: Box::new(resume),
            }))
        }
        Ok(None) => {
            renderer.line(
                MessageStyle::Error,
                &format!("Session not found: {}", session_id),
            )?;
            Ok(None)
        }
        Err(err) => {
            renderer.line(
                MessageStyle::Error,
                &format!("Failed to load session: {}", err),
            )?;
            Ok(None)
        }
    }
}

pub(super) async fn run_interaction_loop_impl(
    ctx: &mut InteractionLoopContext<'_>,
    state: &mut InteractionState<'_>,
) -> Result<InteractionOutcome> {
    const MCP_REFRESH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);
    let mut last_input_activity = ctx.input_activity_counter.load(Ordering::Relaxed);
    let mut last_input_activity_at = Instant::now();
    let mut last_durable_scheduler_poll = Instant::now() - DURABLE_SCHEDULER_POLL_INTERVAL;
    let mut durable_scheduler_daemon = None;
    let mut last_durable_scheduler_error = None::<String>;
    let mut durable_scheduler_run = None::<JoinHandle<Result<usize>>>;
    let mut live_reload_watcher = SimpleConfigWatcher::new(ctx.config.workspace.clone());
    live_reload_watcher.set_check_interval(1);
    live_reload_watcher.set_debounce_duration(200);
    let mut last_status_refresh = Instant::now() - Duration::from_millis(500);
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
            .is_some_and(tokio::task::JoinHandle::is_finished)
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

        let mut input_owned = match inline_action {
            InlineLoopAction::Continue => continue,
            InlineLoopAction::Submit(text) => text,
            InlineLoopAction::RequestInlinePromptSuggestion(draft) => {
                handle_inline_prompt_suggestion_request(ctx, state, &draft).await?;
                continue;
            }
            InlineLoopAction::OpenTranscriptReviewInEditor(text) => {
                open_transcript_review_in_editor(ctx, text).await?;
                continue;
            }
            InlineLoopAction::OpenTranscriptReviewScrollback(text) => {
                open_transcript_review_scrollback(ctx, text).await?;
                continue;
            }
            InlineLoopAction::Exit(reason) => {
                return Ok(InteractionOutcome::Exit { reason });
            }
            InlineLoopAction::PlanApproved { auto_accept } => {
                let mode = if auto_accept {
                    "auto-accept edits"
                } else {
                    "manual edit approvals"
                };
                let message = format!(
                    "Plan approved. Switching to Edit Mode and starting execution ({mode})."
                );
                ctx.renderer.line(MessageStyle::Info, &message)?;
                return Ok(InteractionOutcome::PlanApproved { auto_accept });
            }
            InlineLoopAction::PlanEditRequested => {
                ctx.renderer.line(
                    MessageStyle::Info,
                    "Staying in Plan Mode. Continue refining the plan.",
                )?;
                continue;
            }
            InlineLoopAction::ResumeSession(session_id) => {
                if let Some(outcome) = try_resume_archived_session(
                    ctx.renderer,
                    &session_id,
                    ArchivedSessionIntent::ResumeInPlace,
                    "Loading session",
                    "Restarting with session",
                )
                .await?
                {
                    return Ok(outcome);
                }
                continue;
            }
            InlineLoopAction::ForkSession {
                session_id,
                summarize,
            } => {
                if let Some(outcome) = try_resume_archived_session(
                    ctx.renderer,
                    &session_id,
                    ArchivedSessionIntent::ForkNewArchive {
                        custom_suffix: None,
                        summarize,
                    },
                    "Loading session for fork",
                    "Restarting from fork source",
                )
                .await?
                {
                    return Ok(outcome);
                }
                continue;
            }
            InlineLoopAction::DiffApproved | InlineLoopAction::DiffRejected => {
                continue;
            }
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

        let allow_structured_non_image_file_inputs = supports_native_openai_file_inputs(
            &ctx.config.provider,
            ctx.provider_client
                .supports_responses_compaction(&ctx.config.model),
        );
        let processed_content =
            match vtcode_core::utils::at_pattern::parse_at_patterns_with_options(
                input,
                &ctx.config.workspace,
                vtcode_core::utils::at_pattern::AtPatternOptions {
                    allow_local_non_image_file_inputs: allow_structured_non_image_file_inputs,
                    allow_remote_non_image_file_inputs: allow_structured_non_image_file_inputs,
                },
            )
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
        let selected_agents = if let Some(controller) = ctx.tool_registry.subagent_controller() {
            controller.set_turn_delegation_hints_from_input(input).await
        } else {
            Vec::new()
        };
        let refined_content =
            append_file_reference_metadata(refined_content, input, &ctx.config.workspace);
        let refined_content =
            append_agent_reference_metadata(refined_content, selected_agents.as_slice());

        let latest_editor_snapshot = if let Some(bridge) = ctx.ide_context_bridge.as_mut() {
            match bridge.refresh() {
                Ok((snapshot, _)) => snapshot,
                Err(err) => {
                    tracing::warn!(error = %err, "Failed to refresh IDE context before user turn");
                    bridge.snapshot().cloned()
                }
            }
        } else {
            None
        };
        apply_ide_context_snapshot(
            ctx.context_manager,
            ctx.header_context,
            ctx.handle,
            ctx.config.workspace.as_path(),
            ctx.vt_cfg.as_ref(),
            latest_editor_snapshot,
        );
        crate::agent::runloop::unified::status_line::update_ide_context_source(
            state.input_status_state,
            ide_context_status_label_from_bridge(
                ctx.context_manager,
                ctx.config.workspace.as_path(),
                ctx.vt_cfg.as_ref(),
                ctx.ide_context_bridge.as_ref(),
            ),
        );

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

#[cfg(test)]
mod tests {
    use super::{
        append_agent_reference_metadata, append_file_reference_metadata,
        build_file_reference_metadata, extract_recent_follow_up_hint, fallback_args_preview,
        refresh_live_ide_context_update, stalled_follow_up_recovery_prompt,
        supports_native_openai_file_inputs, tool_names,
    };
    use crate::agent::runloop::unified::context_manager::ContextManager;
    use crate::agent::runloop::unified::session_setup::{
        IdeContextBridge, ide_context_status_label_from_bridge,
    };
    use hashbrown::HashMap;
    use std::fs;
    use std::sync::Arc;
    use tempfile::TempDir;
    use tokio::sync::RwLock;
    use vtcode_core::llm::provider as uni;

    #[test]
    fn extract_recent_follow_up_hint_reads_latest_recoverable_tool_payload() {
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

        let hint = extract_recent_follow_up_hint(&history);
        assert_eq!(
            hint,
            Some((
                "task_tracker".to_string(),
                serde_json::json!({"action":"list"})
            ))
        );
    }

    #[test]
    fn extract_recent_follow_up_hint_skips_non_recoverable_payloads() {
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

        assert!(extract_recent_follow_up_hint(&history).is_none());
    }

    #[test]
    fn supports_native_openai_file_inputs_requires_openai_provider_and_responses_support() {
        assert!(supports_native_openai_file_inputs("openai", true));
        assert!(!supports_native_openai_file_inputs("openai", false));
        assert!(!supports_native_openai_file_inputs("anthropic", true));
        assert!(!supports_native_openai_file_inputs("mycorp", true));
    }

    #[test]
    fn extract_recent_follow_up_hint_prefers_latest_hint() {
        let history = vec![
            uni::Message::tool_response(
                "call_1".to_string(),
                serde_json::json!({
                    "next_continue_args": {
                        "session_id": "run-42"
                    }
                })
                .to_string(),
            ),
            uni::Message::tool_response(
                "call_2".to_string(),
                serde_json::json!({
                    "fallback_tool": "task_tracker",
                    "fallback_tool_args": {"action":"list"},
                    "is_recoverable": true
                })
                .to_string(),
            ),
        ];
        let hint = extract_recent_follow_up_hint(&history);
        assert_eq!(
            hint,
            Some((
                "task_tracker".to_string(),
                serde_json::json!({"action":"list"})
            ))
        );
    }

    #[test]
    fn extract_recent_follow_up_hint_skips_stale_tool_hints_after_assistant_reply() {
        let history = vec![
            uni::Message::tool_response(
                "call_1".to_string(),
                serde_json::json!({
                    "next_continue_args": {
                        "session_id": "run-123"
                    }
                })
                .to_string(),
            ),
            uni::Message::assistant("All done.".to_string()),
        ];

        assert!(extract_recent_follow_up_hint(&history).is_none());
    }

    #[test]
    fn extract_recent_follow_up_hint_keeps_hint_when_system_note_follows_tool() {
        let history = vec![
            uni::Message::tool_response(
                "call_1".to_string(),
                serde_json::json!({
                    "fallback_tool": "task_tracker",
                    "fallback_tool_args": {"action":"list"},
                    "is_recoverable": true
                })
                .to_string(),
            ),
            uni::Message::system("Tool blocked; try fallback.".to_string()),
        ];

        let hint = extract_recent_follow_up_hint(&history);
        assert_eq!(
            hint,
            Some((
                "task_tracker".to_string(),
                serde_json::json!({"action":"list"})
            ))
        );
    }

    #[test]
    fn extract_recent_follow_up_hint_ignores_next_action_when_fallback_exists() {
        let history = vec![uni::Message::tool_response(
            "call_1".to_string(),
            serde_json::json!({
                "error": "Tool preflight validation failed: x",
                "is_recoverable": true,
                "fallback_tool": "task_tracker",
                "fallback_tool_args": {"action":"list"},
                "next_action": "Retry with fallback_tool_args."
            })
            .to_string(),
        )];

        let hint = extract_recent_follow_up_hint(&history);
        assert_eq!(
            hint,
            Some((
                "task_tracker".to_string(),
                serde_json::json!({"action":"list"})
            ))
        );
    }

    #[test]
    fn extract_recent_follow_up_hint_does_not_promote_next_action_only_payload() {
        let history = vec![uni::Message::tool_response(
            "call_1".to_string(),
            serde_json::json!({
                "error": "boom",
                "is_recoverable": true,
                "next_action": "Try an alternative tool or narrower scope."
            })
            .to_string(),
        )];

        assert!(extract_recent_follow_up_hint(&history).is_none());
    }

    #[test]
    fn extract_recent_follow_up_hint_reads_next_continue_args() {
        let history = vec![uni::Message::tool_response(
            "call_1".to_string(),
            serde_json::json!({
                "next_continue_args": {
                    "session_id": "run-123"
                }
            })
            .to_string(),
        )];

        let hint = extract_recent_follow_up_hint(&history);
        assert_eq!(
            hint,
            Some((
                tool_names::UNIFIED_EXEC.to_string(),
                serde_json::json!({
                    "action": "continue",
                    "session_id": "run-123"
                })
            ))
        );
    }

    #[test]
    fn extract_recent_follow_up_hint_reads_next_read_args() {
        let history = vec![uni::Message::tool_response(
            "call_1".to_string(),
            serde_json::json!({
                "next_read_args": {
                    "path": ".vtcode/context/tool_outputs/out.txt",
                    "offset": 41,
                    "limit": 40
                }
            })
            .to_string(),
        )];

        let hint = extract_recent_follow_up_hint(&history);
        assert_eq!(
            hint,
            Some((
                tool_names::UNIFIED_FILE.to_string(),
                serde_json::json!({
                    "action": "read",
                    "path": ".vtcode/context/tool_outputs/out.txt",
                    "offset": 41,
                    "limit": 40
                })
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
    fn stalled_follow_up_recovery_prompt_mentions_fallback_without_replacing_user_input() {
        let prompt = stalled_follow_up_recovery_prompt("turn blocked", true);

        assert!(prompt.contains("Stall reason: turn blocked"));
        assert!(prompt.contains("Use the recovered fallback hint"));
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
    fn append_agent_reference_metadata_adds_selected_agent_hint() {
        let content = uni::MessageContent::text("use rust-engineer agent".to_string());
        let augmented = append_agent_reference_metadata(content, &[String::from("rust-engineer")]);

        match augmented {
            uni::MessageContent::Text(text) => {
                assert!(text.contains("use rust-engineer agent"));
                assert!(text.contains("[agent_reference_metadata]"));
                assert!(text.contains("selected=@agent-rust-engineer"));
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

    #[test]
    fn refresh_live_ide_context_update_tracks_active_file_changes() {
        let temp_dir = TempDir::new().expect("temp dir");
        let workspace = temp_dir.path();
        fs::create_dir_all(workspace.join(".vtcode")).expect("create ide context dir");
        let context_manager = ContextManager::new(
            "sys".into(),
            (),
            Arc::new(RwLock::new(HashMap::new())),
            None,
        );

        let snapshot_path = workspace.join(".vtcode/ide-context.json");
        let mut bridge = Some(IdeContextBridge::new(workspace));

        fs::write(
            &snapshot_path,
            serde_json::json!({
                "version": 1,
                "provider_family": "vscode_compatible",
                "editor_name": "VS Code",
                "workspace_root": workspace,
                "active_file": {
                    "path": workspace.join("src/alpha.rs"),
                    "language_id": "rust",
                    "line_range": { "start": 1, "end": 8 },
                    "dirty": false,
                    "truncated": false
                }
            })
            .to_string(),
        )
        .expect("write alpha snapshot");

        let first = refresh_live_ide_context_update(&mut bridge);
        assert!(first.changed);
        assert_eq!(
            ide_context_status_label_from_bridge(
                &context_manager,
                workspace,
                None,
                bridge.as_ref()
            )
            .as_deref(),
            Some("IDE Context (VS Code): src/alpha.rs")
        );

        fs::write(
            &snapshot_path,
            serde_json::json!({
                "version": 1,
                "provider_family": "vscode_compatible",
                "editor_name": "VS Code",
                "workspace_root": workspace,
                "active_file": {
                    "path": workspace.join("src/beta.rs"),
                    "language_id": "rust",
                    "line_range": { "start": 3, "end": 12 },
                    "dirty": false,
                    "truncated": false
                }
            })
            .to_string(),
        )
        .expect("write beta snapshot");

        let second = refresh_live_ide_context_update(&mut bridge);
        assert!(second.changed);
        assert_eq!(
            ide_context_status_label_from_bridge(
                &context_manager,
                workspace,
                None,
                bridge.as_ref()
            )
            .as_deref(),
            Some("IDE Context (VS Code): src/beta.rs")
        );
    }
}
