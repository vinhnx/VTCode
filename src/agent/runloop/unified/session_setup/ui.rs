use super::types::{SessionState, SessionUISetup};
use crate::agent::runloop::ResumeSession;
use crate::agent::runloop::ui::{build_inline_header_context, render_session_banner};
use crate::agent::runloop::unified::turn::utils::render_hook_messages;
use crate::agent::runloop::unified::turn::workspace::load_workspace_files;
use crate::agent::runloop::unified::{context_manager, palettes, state};
use crate::hooks::lifecycle::{LifecycleHookEngine, SessionEndReason, SessionStartTrigger};
use crate::ide_context::IdeContextBridge;
use anyhow::{Context, Result};
use chrono::Local;
use std::sync::Arc;
use tokio::sync::Notify;
use tracing::warn;
use vtcode_core::config::constants::ui;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::llm::provider as uni;
use vtcode_core::ui::theme;
use vtcode_core::ui::tui::{
    InlineEvent, InlineEventCallback, spawn_session_with_prompts, theme_from_styles,
};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_core::utils::formatting::indent_block;
use vtcode_core::utils::session_archive::{SessionArchive, SessionArchiveMetadata};
use vtcode_core::utils::transcript;

pub(crate) async fn initialize_session_ui(
    config: &CoreAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
    session_state: &mut SessionState,
    resume_state: Option<&ResumeSession>,
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
    let theme_spec = theme_from_styles(&active_styles);
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

    let pty_counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    session_state
        .tool_registry
        .set_active_pty_sessions(pty_counter.clone());

    let mut session = spawn_session_with_prompts(
        theme_spec.clone(),
        default_placeholder.clone(),
        config.ui_surface,
        inline_rows,
        Some(interrupt_callback),
        Some(pty_counter.clone()),
        vt_cfg
            .map(|cfg| cfg.ui.keyboard_protocol.clone())
            .unwrap_or_default(),
        Some(config.workspace.clone()),
    )
    .context("failed to launch inline session")?;
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
    let ui_redraw_batcher =
        crate::agent::runloop::unified::turn::utils::UIRedrawBatcher::with_auto_flush(
            handle.clone(),
        );

    let workspace_for_indexer = config.workspace.clone();
    let workspace_for_palette = config.workspace.clone();
    let handle_for_indexer = handle.clone();
    let _file_palette_task = tokio::spawn(async move {
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
    });

    transcript::clear();
    render_resume_state_if_present(&mut renderer, resume_state)?;

    let workspace_label = config
        .workspace
        .file_name()
        .and_then(|component| component.to_str())
        .map(|value| value.to_string())
        .unwrap_or_else(|| "workspace".to_string());
    let workspace_path = config.workspace.to_string_lossy().into_owned();
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

    let (session_archive, session_archive_error) = setup_session_archive(
        resume_state,
        workspace_label,
        workspace_path,
        config,
        provider_label,
    )
    .await;

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

    if let Some(mcp_manager) = &session_state.async_mcp_manager {
        let mcp_status = mcp_manager.get_status().await;
        if mcp_status.is_initializing() {
            renderer.line(
                MessageStyle::Info,
                "MCP is still initializing in the background...",
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

    if let Some(message) = session_archive_error {
        renderer.line(
            MessageStyle::Info,
            &format!("Session archiving disabled: {}", message),
        )?;
        renderer.line_if_not_empty(MessageStyle::Output)?;
    }

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
    })
}

fn render_resume_state_if_present(
    renderer: &mut AnsiRenderer,
    resume_state: Option<&ResumeSession>,
) -> Result<()> {
    let Some(session) = resume_state else {
        return Ok(());
    };

    let ended_local = session
        .snapshot
        .ended_at
        .with_timezone(&Local)
        .format("%Y-%m-%d %H:%M");
    let action = if session.is_fork {
        "Forking"
    } else {
        "Resuming"
    };
    renderer.line(
        MessageStyle::Info,
        &format!(
            "{} session {} · ended {} · {} messages",
            action,
            session.identifier,
            ended_local,
            session.message_count()
        ),
    )?;
    renderer.line(
        MessageStyle::Info,
        &format!("Previous archive: {}", session.path.display()),
    )?;
    if session.is_fork {
        renderer.line(MessageStyle::Info, "Starting independent forked session")?;
    }

    if !session.history.is_empty() {
        renderer.line(MessageStyle::Info, "Conversation history:")?;
        for (idx, msg) in session.history.iter().enumerate() {
            let (style, role_label) = match msg.role {
                uni::MessageRole::User => (MessageStyle::User, "You"),
                uni::MessageRole::Assistant => (MessageStyle::Response, "Assistant"),
                uni::MessageRole::Tool => (MessageStyle::ToolOutput, "Tool"),
                uni::MessageRole::System => (MessageStyle::Info, "System"),
            };
            let tool_suffix = msg
                .tool_call_id
                .as_ref()
                .map(|id| format!(" [tool_call_id: {}]", id))
                .unwrap_or_default();
            renderer.line(
                style,
                &format!("  [{}] {}{}:", idx + 1, role_label, tool_suffix),
            )?;
            match &msg.content {
                uni::MessageContent::Text(text) => {
                    let indented = indent_block(text, "  ");
                    renderer.line(style, &indented)?;
                }
                uni::MessageContent::Parts(parts) => {
                    renderer.line(style, &format!("  [content parts: {}]", parts.len()))?;
                }
            }
            if idx + 1 < session.history.len() {
                renderer.line(style, "")?;
            }
        }
    }
    renderer.line_if_not_empty(MessageStyle::Output)?;
    Ok(())
}

async fn setup_session_archive(
    resume_state: Option<&ResumeSession>,
    workspace_label: String,
    workspace_path: String,
    config: &CoreAgentConfig,
    provider_label: String,
) -> (Option<SessionArchive>, Option<String>) {
    let mut session_archive_error: Option<String> = None;
    let session_archive = if let Some(resume) = resume_state {
        if resume.is_fork {
            let custom_id = resume
                .identifier
                .strip_prefix("forked-")
                .map(|s| s.to_string());
            match SessionArchive::fork(&resume.snapshot, custom_id).await {
                Ok(archive) => Some(archive),
                Err(err) => {
                    session_archive_error = Some(err.to_string());
                    None
                }
            }
        } else {
            let archive_metadata = SessionArchiveMetadata::new(
                workspace_label,
                workspace_path,
                config.model.clone(),
                provider_label,
                config.theme.clone(),
                config.reasoning_effort.as_str().to_string(),
            );
            match SessionArchive::new(archive_metadata, None).await {
                Ok(archive) => Some(archive),
                Err(err) => {
                    session_archive_error = Some(err.to_string());
                    None
                }
            }
        }
    } else {
        let archive_metadata = SessionArchiveMetadata::new(
            workspace_label,
            workspace_path,
            config.model.clone(),
            provider_label,
            config.theme.clone(),
            config.reasoning_effort.as_str().to_string(),
        );
        match SessionArchive::new(archive_metadata, None).await {
            Ok(archive) => Some(archive),
            Err(err) => {
                session_archive_error = Some(err.to_string());
                None
            }
        }
    };

    (session_archive, session_archive_error)
}
