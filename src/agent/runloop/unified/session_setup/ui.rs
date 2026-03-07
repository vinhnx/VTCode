use super::types::{BackgroundTaskGuard, SessionState, SessionUISetup};
use crate::agent::runloop::ResumeSession;
use crate::agent::runloop::tui_compat::{
    inline_theme_from_core_styles, to_tui_appearance, to_tui_keyboard_protocol,
    to_tui_slash_commands, to_tui_surface,
};
use crate::agent::runloop::ui::{build_inline_header_context, render_session_banner};
use crate::agent::runloop::unified::reasoning::{
    model_supports_reasoning, resolve_reasoning_visibility,
};
use crate::agent::runloop::unified::turn::utils::render_hook_messages;
use crate::agent::runloop::unified::turn::workspace::load_workspace_files;
use crate::agent::runloop::unified::{context_manager, palettes, state};
use crate::hooks::lifecycle::{LifecycleHookEngine, SessionEndReason, SessionStartTrigger};
use crate::ide_context::IdeContextBridge;
use anyhow::{Context, Result};
use chrono::Local;
use hashbrown::HashMap;
use std::sync::Arc;
use tokio::sync::Notify;
use tracing::warn;
use vtcode_core::config::constants::ui;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::llm::provider as uni;
use vtcode_core::notifications::set_global_terminal_focused;
use vtcode_core::ui::slash::SLASH_COMMANDS;
use vtcode_core::ui::theme;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_core::utils::session_archive::SessionArchive;
use vtcode_core::utils::transcript;
use vtcode_tui::{
    FocusChangeCallback, InlineEvent, InlineEventCallback, SessionOptions,
    spawn_session_with_options,
};

pub(crate) async fn initialize_session_ui(
    config: &CoreAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
    session_state: &mut SessionState,
    resume_state: Option<&ResumeSession>,
    session_archive: Option<SessionArchive>,
    full_auto: bool,
    skip_confirmations: bool,
) -> Result<SessionUISetup> {
    let session_trigger = if resume_state.is_some() {
        SessionStartTrigger::Resume
    } else {
        SessionStartTrigger::Startup
    };
    let lifecycle_hooks = if let Some(vt) = vt_cfg {
        LifecycleHookEngine::new(config.workspace.clone(), &vt.hooks, session_trigger)?
    } else {
        None
    };

    let context_manager = context_manager::ContextManager::new(
        session_state.base_system_prompt.clone(),
        (),
        session_state.loaded_skills.clone(),
        vt_cfg.map(|cfg| cfg.agent.clone()),
    );

    let active_styles = theme::active_styles();
    let theme_spec = inline_theme_from_core_styles(&active_styles);
    let default_placeholder = session_state
        .session_bootstrap
        .placeholder
        .clone()
        .or_else(|| Some(ui::CHAT_INPUT_PLACEHOLDER_BOOTSTRAP.to_string()));
    let follow_up_placeholder = if session_state.session_bootstrap.placeholder.is_none() {
        Some(ui::CHAT_INPUT_PLACEHOLDER_FOLLOW_UP.to_string())
    } else {
        None
    };
    let inline_rows = vt_cfg
        .as_ref()
        .map(|cfg| cfg.ui.inline_viewport_rows)
        .unwrap_or(ui::DEFAULT_INLINE_VIEWPORT_ROWS);

    unsafe {
        std::env::set_var("VTCODE_TUI_MODE", "1");
    }

    let ctrl_c_state = Arc::new(state::CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());
    let interrupt_callback: InlineEventCallback = {
        let state = ctrl_c_state.clone();
        let notify = ctrl_c_notify.clone();
        Arc::new(move |event: &InlineEvent| {
            if matches!(event, InlineEvent::Interrupt) {
                let _ = state.register_signal();
                notify.notify_waiters();
            }
        })
    };
    let focus_callback: FocusChangeCallback = Arc::new(set_global_terminal_focused);

    let pty_counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    session_state
        .tool_registry
        .set_active_pty_sessions(pty_counter.clone());

    let mut session = spawn_session_with_options(
        theme_spec.clone(),
        SessionOptions {
            placeholder: default_placeholder.clone(),
            surface_preference: to_tui_surface(config.ui_surface),
            inline_rows,
            event_callback: Some(interrupt_callback),
            focus_callback: Some(focus_callback),
            active_pty_sessions: Some(pty_counter.clone()),
            keyboard_protocol: vt_cfg
                .map(|cfg| to_tui_keyboard_protocol(cfg.ui.keyboard_protocol.clone()))
                .unwrap_or_default(),
            workspace_root: Some(config.workspace.clone()),
            slash_commands: to_tui_slash_commands(SLASH_COMMANDS.as_slice()),
            appearance: vt_cfg.map(to_tui_appearance),
            app_name: "VT Code".to_string(),
            non_interactive_hint: Some(
                "Use `vtcode ask \"your prompt\"` for non-interactive input.".to_string(),
            ),
        },
    )
    .context("failed to launch inline session")?;
    set_global_terminal_focused(true);
    if skip_confirmations {
        session.set_skip_confirmations(true);
    }

    let handle = session.clone_inline_handle();
    let highlight_config = vt_cfg
        .as_ref()
        .map(|cfg| cfg.syntax_highlighting.clone())
        .unwrap_or_default();

    transcript::set_inline_handle(Arc::new(handle.clone()));
    let mut ide_context_bridge = IdeContextBridge::from_env();
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), highlight_config);
    let supports_reasoning =
        model_supports_reasoning(&*session_state.provider_client, &config.model);
    renderer.set_reasoning_visible(resolve_reasoning_visibility(vt_cfg, supports_reasoning));
    if let Some(cfg) = vt_cfg {
        renderer.set_screen_reader_mode(cfg.ui.screen_reader_mode);
        renderer.set_show_diagnostics_in_transcript(cfg.ui.show_diagnostics_in_transcript);
    }
    let ui_redraw_batcher =
        crate::agent::runloop::unified::turn::utils::UIRedrawBatcher::with_auto_flush(
            handle.clone(),
        );

    let workspace_for_indexer = config.workspace.clone();
    let workspace_for_palette = config.workspace.clone();
    let handle_for_indexer = handle.clone();
    let file_palette_task_guard = BackgroundTaskGuard::new(tokio::spawn(async move {
        match load_workspace_files(workspace_for_indexer).await {
            Ok(files) => {
                if !files.is_empty() {
                    handle_for_indexer.load_file_palette(files, workspace_for_palette);
                } else {
                    tracing::debug!("No files found in workspace for file palette");
                }
            }
            Err(err) => {
                tracing::warn!("Failed to load workspace files for file palette: {}", err);
            }
        }
    }));

    transcript::clear();
    render_resume_state_if_present(&mut renderer, resume_state, supports_reasoning)?;

    let provider_label = if config.provider.trim().is_empty() {
        session_state.provider_client.name().to_string()
    } else {
        config.provider.clone()
    };
    let header_provider_label = provider_label.clone();

    let mut checkpoint_config =
        vtcode_core::core::agent::snapshots::SnapshotConfig::new(config.workspace.clone());
    checkpoint_config.enabled = config.checkpointing_enabled;
    checkpoint_config.storage_dir = config.checkpointing_storage_dir.clone();
    checkpoint_config.max_snapshots = config.checkpointing_max_snapshots;
    checkpoint_config.max_age_days = config.checkpointing_max_age_days;
    let checkpoint_manager =
        match vtcode_core::core::agent::snapshots::SnapshotManager::new(checkpoint_config) {
            Ok(manager) => Some(manager),
            Err(err) => {
                warn!("Failed to initialize checkpoint manager: {}", err);
                None
            }
        };

    if let (Some(hooks), Some(archive)) = (&lifecycle_hooks, session_archive.as_ref()) {
        hooks
            .update_transcript_path(Some(archive.path().to_path_buf()))
            .await;
    }

    if let Some(hooks) = &lifecycle_hooks {
        match hooks.run_session_start().await {
            Ok(outcome) => {
                render_hook_messages(&mut renderer, &outcome.messages)?;
                for context in outcome.additional_context {
                    if !context.trim().is_empty() {
                        session_state
                            .conversation_history
                            .push(uni::Message::system(context));
                    }
                }
            }
            Err(err) => {
                renderer.line(
                    MessageStyle::Error,
                    &format!("Failed to run session start hooks: {}", err),
                )?;
            }
        }
    }

    if full_auto && let Some(allowlist) = session_state.full_auto_allowlist.as_ref() {
        if allowlist.is_empty() {
            renderer.line(
                MessageStyle::Info,
                "Full-auto mode enabled with no tool permissions; tool calls will be skipped.",
            )?;
        } else {
            renderer.line(
                MessageStyle::Info,
                &format!(
                    "Full-auto mode enabled. Permitted tools: {}",
                    allowlist.join(", ")
                ),
            )?;
        }
    }

    handle.set_theme(theme_spec.clone());
    palettes::apply_prompt_style(&handle);
    handle.set_placeholder(default_placeholder.clone());

    let reasoning_label = vt_cfg
        .as_ref()
        .map(|cfg| cfg.agent.reasoning_effort.as_str().to_string())
        .unwrap_or_else(|| config.reasoning_effort.as_str().to_string());
    render_session_banner(
        &mut renderer,
        config,
        &session_state.session_bootstrap,
        &config.model,
        &reasoning_label,
    )?;

    if let Some(bridge) = ide_context_bridge.as_mut() {
        match bridge.snapshot() {
            Ok(Some(context)) => session_state
                .conversation_history
                .push(uni::Message::system(context)),
            Ok(None) => {}
            Err(err) => warn!("Failed to update IDE context snapshot: {}", err),
        }
    }

    let mode_label = match (config.ui_surface, full_auto) {
        (vtcode_core::config::types::UiSurfacePreference::Inline, true) => "auto".to_string(),
        (vtcode_core::config::types::UiSurfacePreference::Inline, false) => "inline".to_string(),
        (vtcode_core::config::types::UiSurfacePreference::Alternate, _) => "alt".to_string(),
        (vtcode_core::config::types::UiSurfacePreference::Auto, true) => "auto".to_string(),
        (vtcode_core::config::types::UiSurfacePreference::Auto, false) => "std".to_string(),
    };
    let header_context = build_inline_header_context(
        config,
        &session_state.session_bootstrap,
        header_provider_label,
        config.model.clone(),
        mode_label,
        reasoning_label.clone(),
    )
    .await?;
    handle.set_header_context(header_context);

    let next_checkpoint_turn = checkpoint_manager
        .as_ref()
        .and_then(|manager| manager.next_turn_number().ok())
        .unwrap_or(1);

    Ok(SessionUISetup {
        renderer,
        session,
        handle,
        ctrl_c_state,
        ctrl_c_notify,
        checkpoint_manager,
        session_archive,
        lifecycle_hooks,
        session_end_reason: SessionEndReason::Completed,
        context_manager,
        default_placeholder,
        follow_up_placeholder,
        next_checkpoint_turn,
        ui_redraw_batcher,
        file_palette_task_guard,
    })
}

fn render_resume_state_if_present(
    renderer: &mut AnsiRenderer,
    resume_state: Option<&ResumeSession>,
    supports_reasoning: bool,
) -> Result<()> {
    let Some(session) = resume_state else {
        return Ok(());
    };

    let ended_local = session
        .snapshot()
        .ended_at
        .with_timezone(&Local)
        .format("%Y-%m-%d %H:%M");
    let action = if session.is_fork() {
        "Forking"
    } else {
        "Resuming"
    };
    renderer.line(
        MessageStyle::Info,
        &format!(
            "{} session {} · ended {} · {} messages",
            action,
            session.identifier(),
            ended_local,
            session.message_count()
        ),
    )?;
    renderer.line(
        MessageStyle::Info,
        &format!("Previous archive: {}", session.path().display()),
    )?;
    if session.is_fork() {
        renderer.line(MessageStyle::Info, "Starting independent forked session")?;
    }

    if !session.history().is_empty() {
        renderer.line(MessageStyle::Info, "Conversation history:")?;
        let lines = build_structured_resume_lines(session.history(), supports_reasoning);
        render_resume_lines(renderer, &lines)?;
    } else if !session.snapshot().transcript.is_empty() {
        renderer.line(
            MessageStyle::Info,
            "Conversation history (legacy transcript):",
        )?;
        let lines = build_legacy_resume_lines(&session.snapshot().transcript);
        render_resume_lines(renderer, &lines)?;
    }
    renderer.line_if_not_empty(MessageStyle::Output)?;
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ResumeRenderLine {
    style: MessageStyle,
    text: String,
}

impl ResumeRenderLine {
    fn new(style: MessageStyle, text: impl Into<String>) -> Self {
        Self {
            style,
            text: text.into(),
        }
    }
}

fn render_resume_lines(renderer: &mut AnsiRenderer, lines: &[ResumeRenderLine]) -> Result<()> {
    for line in lines {
        renderer.line(line.style, &line.text)?;
    }
    Ok(())
}

fn build_structured_resume_lines(
    history: &[uni::Message],
    supports_reasoning: bool,
) -> Vec<ResumeRenderLine> {
    let mut lines = Vec::new();
    let mut tool_name_by_call_id: HashMap<String, String> = HashMap::new();

    for (index, message) in history.iter().enumerate() {
        if index > 0 {
            push_resume_spacing(&mut lines);
        }
        match message.role {
            uni::MessageRole::User => {
                push_content_lines(&mut lines, MessageStyle::User, &message.content);
            }
            uni::MessageRole::Assistant => {
                let mut rendered_any = false;

                if let Some(tool_calls) = &message.tool_calls {
                    for tool_call in tool_calls {
                        rendered_any = true;
                        let tool_name = tool_call
                            .function
                            .as_ref()
                            .map(|function| function.name.clone())
                            .unwrap_or_else(|| "unknown".to_string());
                        if !tool_call.id.trim().is_empty() {
                            tool_name_by_call_id.insert(tool_call.id.clone(), tool_name.clone());
                        }

                        lines.push(ResumeRenderLine::new(
                            MessageStyle::Tool,
                            format_resume_tool_header(&tool_name, Some(tool_call.id.as_str())),
                        ));

                        if let Some(function) = &tool_call.function {
                            let args_block = format_tool_arguments_for_resume(&function.arguments);
                            if !args_block.is_empty() {
                                lines.push(ResumeRenderLine::new(
                                    MessageStyle::ToolDetail,
                                    args_block,
                                ));
                            }
                        } else if let Some(text) = tool_call.text.as_deref()
                            && !text.trim().is_empty()
                        {
                            lines.push(ResumeRenderLine::new(
                                MessageStyle::ToolDetail,
                                text.trim().to_string(),
                            ));
                        }
                    }
                }

                let reasoning_text = if supports_reasoning {
                    message
                        .reasoning
                        .as_deref()
                        .map(str::trim)
                        .filter(|text| !text.is_empty())
                        .map(str::to_string)
                        .or_else(|| {
                            message
                                .reasoning_details
                                .as_deref()
                                .and_then(
                                    vtcode_core::llm::providers::common::extract_reasoning_text_from_detail_values,
                                )
                        })
                } else {
                    None
                };

                if let Some(reasoning) = reasoning_text {
                    rendered_any = true;
                    lines.push(ResumeRenderLine::new(MessageStyle::Reasoning, reasoning));
                }

                if let Some(content) = project_content_text(&message.content) {
                    rendered_any = true;
                    lines.push(ResumeRenderLine::new(MessageStyle::Response, content));
                }

                if !rendered_any {
                    lines.push(ResumeRenderLine::new(
                        MessageStyle::Response,
                        "Assistant: [no content]",
                    ));
                }
            }
            uni::MessageRole::Tool => {
                let call_id = message.tool_call_id.as_deref();
                let tool_name = call_id
                    .and_then(|id| tool_name_by_call_id.get(id))
                    .cloned()
                    .or_else(|| message.origin_tool.clone())
                    .unwrap_or_else(|| "tool".to_string());
                lines.push(ResumeRenderLine::new(
                    MessageStyle::Tool,
                    format_resume_tool_header(&tool_name, call_id),
                ));
                push_content_lines(&mut lines, MessageStyle::ToolOutput, &message.content);
            }
            uni::MessageRole::System => {
                lines.push(ResumeRenderLine::new(MessageStyle::Info, "System:"));
                push_content_lines(&mut lines, MessageStyle::Info, &message.content);
            }
        }
    }

    lines
}

fn format_resume_tool_header(tool_name: &str, tool_call_id: Option<&str>) -> String {
    match tool_call_id {
        Some(id) if !id.trim().is_empty() && tool_name.trim().eq_ignore_ascii_case("tool") => {
            format!("Tool [tool_call_id: {}]:", id)
        }
        Some(id) if !id.trim().is_empty() => {
            format!("Tool {} [tool_call_id: {}]:", tool_name, id)
        }
        _ if tool_name.trim().eq_ignore_ascii_case("tool") => "Tool:".to_string(),
        _ => format!("Tool {}:", tool_name),
    }
}

fn format_tool_arguments_for_resume(arguments: &str) -> String {
    let trimmed = arguments.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    match serde_json::from_str::<serde_json::Value>(trimmed) {
        Ok(value) => serde_json::to_string_pretty(&value)
            .map(|pretty| format!("```json\n{}\n```", pretty))
            .unwrap_or_else(|_| format!("```json\n{}\n```", trimmed)),
        Err(_) => format!("```text\n{}\n```", trimmed),
    }
}

fn push_resume_spacing(lines: &mut Vec<ResumeRenderLine>) {
    if lines.last().is_none_or(|line| !line.text.is_empty()) {
        lines.push(ResumeRenderLine::new(MessageStyle::Info, ""));
    }
}

fn push_content_lines(
    lines: &mut Vec<ResumeRenderLine>,
    style: MessageStyle,
    content: &uni::MessageContent,
) {
    if let Some(projected) = project_content_text(content) {
        lines.push(ResumeRenderLine::new(style, projected));
    } else {
        lines.push(ResumeRenderLine::new(style, "[no textual content]"));
    }
}

fn project_content_text(content: &uni::MessageContent) -> Option<String> {
    match content {
        uni::MessageContent::Text(text) => (!text.trim().is_empty()).then(|| text.clone()),
        uni::MessageContent::Parts(parts) => {
            let mut fragments = Vec::new();
            for part in parts {
                match part {
                    uni::ContentPart::Text { text } => {
                        if !text.trim().is_empty() {
                            fragments.push(text.clone());
                        }
                    }
                    uni::ContentPart::Image { mime_type, .. } => {
                        fragments.push(format!("[image content: {}]", mime_type));
                    }
                    uni::ContentPart::File {
                        filename,
                        file_id,
                        file_url,
                        ..
                    } => {
                        if let Some(name) = filename {
                            fragments.push(format!("[file attachment: {}]", name));
                        } else if let Some(id) = file_id {
                            fragments.push(format!("[file attachment id: {}]", id));
                        } else if let Some(url) = file_url {
                            fragments.push(format!("[file attachment url: {}]", url));
                        } else {
                            fragments.push("[file attachment]".to_string());
                        }
                    }
                }
            }

            (!fragments.is_empty()).then(|| fragments.join("\n"))
        }
    }
}

fn build_legacy_resume_lines(transcript: &[String]) -> Vec<ResumeRenderLine> {
    transcript
        .iter()
        .map(|line| ResumeRenderLine::new(infer_legacy_line_style(line), line.clone()))
        .collect()
}

fn infer_legacy_line_style(line: &str) -> MessageStyle {
    let trimmed = line.trim_start();
    if trimmed.is_empty() {
        return MessageStyle::Info;
    }

    if trimmed.contains("You:") {
        return MessageStyle::User;
    }
    if trimmed.contains("Assistant:") {
        return MessageStyle::Response;
    }
    if trimmed.contains("System:") {
        return MessageStyle::Info;
    }
    if trimmed.contains("Tool ")
        || trimmed.contains("[tool_call_id:")
        || trimmed.contains("\"tool_call_id\"")
    {
        return MessageStyle::ToolOutput;
    }
    MessageStyle::Info
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structured_resume_lines_preserve_tool_context() {
        let mut assistant =
            uni::Message::assistant("cargo fmt completed successfully.".to_string());
        assistant.reasoning = Some("Need to run formatter before checks.".to_string());
        assistant.tool_calls = Some(vec![uni::ToolCall::function(
            "call_123".to_string(),
            "unified_exec".to_string(),
            "{\"cmd\":\"cargo fmt\"}".to_string(),
        )]);

        let mut tool_response =
            uni::Message::tool_response("call_123".to_string(), "{\"exit_code\":0}".to_string());
        tool_response.origin_tool = Some("unified_exec".to_string());

        let history = vec![
            uni::Message::user("run cargo fmt".to_string()),
            assistant,
            tool_response,
        ];

        let lines = build_structured_resume_lines(&history, true);

        assert!(lines.iter().any(|line| {
            line.style == MessageStyle::User && line.text.contains("run cargo fmt")
        }));
        assert!(!lines.iter().any(|line| line.text == "You:"));
        assert!(!lines.iter().any(|line| line.text == "Assistant:"));
        assert!(lines.iter().any(|line| {
            line.style == MessageStyle::Tool
                && line
                    .text
                    .contains("Tool unified_exec [tool_call_id: call_123]:")
        }));
        assert!(lines.iter().any(|line| {
            line.style == MessageStyle::ToolDetail && line.text.starts_with("```json")
        }));
        assert!(lines.iter().any(|line| {
            line.style == MessageStyle::ToolOutput && line.text.contains("\"exit_code\":0")
        }));
    }

    #[test]
    fn legacy_style_inference_maps_common_prefixes() {
        assert_eq!(infer_legacy_line_style("  [1] You:"), MessageStyle::User);
        assert_eq!(
            infer_legacy_line_style("  [5] Assistant:"),
            MessageStyle::Response
        );
        assert_eq!(
            infer_legacy_line_style("System: startup"),
            MessageStyle::Info
        );
        assert_eq!(
            infer_legacy_line_style("Tool [tool_call_id: call_1]:"),
            MessageStyle::ToolOutput
        );
    }

    #[test]
    fn structured_resume_lines_fallback_to_reasoning_details() {
        let assistant =
            uni::Message::assistant("done".to_string()).with_reasoning_details(Some(vec![
                serde_json::json!(r#"{"type":"reasoning.text","text":"detail trace"}"#),
            ]));
        let lines = build_structured_resume_lines(&[assistant], true);
        assert!(lines.iter().any(|line| {
            line.style == MessageStyle::Reasoning && line.text.contains("detail trace")
        }));
    }

    #[test]
    fn structured_resume_lines_hide_reasoning_when_unsupported() {
        let mut assistant = uni::Message::assistant("done".to_string());
        assistant.reasoning = Some("trace".to_string());
        let lines = build_structured_resume_lines(&[assistant], false);
        assert!(
            !lines
                .iter()
                .any(|line| line.style == MessageStyle::Reasoning)
        );
    }
}
