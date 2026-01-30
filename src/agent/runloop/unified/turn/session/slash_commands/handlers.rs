use std::sync::Arc;

use anyhow::{Context, Result};
use serde_json;

use vtcode_core::commands::init::{GenerateAgentsFileStatus, generate_agents_file};
use vtcode_core::config::constants::tools as tools_consts;
use vtcode_core::config::loader::{ConfigManager, VTCodeConfig};
use vtcode_core::config::types::EditingMode as ConfigEditingMode;
use vtcode_core::core::decision_tracker::DecisionTracker;
use vtcode_core::llm::provider as uni;
use vtcode_core::ui::theme;
use vtcode_core::ui::tui::theme_from_styles;
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_core::utils::session_archive;
use vtcode_core::utils::transcript;

use crate::agent::runloop::model_picker::{ModelPickerStart, ModelPickerState};
use crate::agent::runloop::slash_commands::{AgentCommandAction, McpCommandAction};
use crate::agent::runloop::unified::diagnostics::run_doctor_diagnostics;
use crate::agent::runloop::unified::display::persist_theme_preference;
use crate::agent::runloop::unified::mcp_support::{
    diagnose_mcp, display_mcp_config_summary, display_mcp_providers, display_mcp_status,
    display_mcp_tools, refresh_mcp_tools, render_mcp_config_edit_guidance,
    render_mcp_login_guidance, repair_mcp_runtime,
};
use crate::agent::runloop::unified::model_selection::finalize_model_selection;
use crate::agent::runloop::unified::palettes::{
    ActivePalette, apply_prompt_style, show_sessions_palette, show_theme_palette,
};
use crate::agent::runloop::unified::state::SessionStats;
use crate::agent::runloop::unified::tool_routing::{ToolPermissionFlow, ensure_tool_permission};
use crate::agent::runloop::unified::turn::utils::{
    enforce_history_limits, truncate_message_content,
};
use crate::agent::runloop::unified::turn::workspace::{
    bootstrap_config_files, build_workspace_index,
};
use crate::agent::runloop::unified::ui_interaction::display_session_status;
use crate::agent::runloop::unified::workspace_links::handle_workspace_directory_command;
use crate::hooks::lifecycle::SessionEndReason;
use webbrowser;

use super::{SlashCommandContext, SlashCommandControl};
use crate::agent::runloop::unified::turn::config_modal::load_config_modal_content;

fn persist_mode_settings(
    workspace: &std::path::Path,
    vt_cfg: &mut Option<VTCodeConfig>,
    editing_mode: Option<ConfigEditingMode>,
    autonomous_mode: Option<bool>,
) -> Result<()> {
    if editing_mode.is_none() && autonomous_mode.is_none() {
        return Ok(());
    }

    let mut manager = ConfigManager::load().with_context(|| {
        format!(
            "Failed to load configuration for workspace {}",
            workspace.display()
        )
    })?;
    let mut config = manager.config().clone();

    if let Some(mode) = editing_mode {
        config.agent.default_editing_mode = mode;
    }

    if let Some(enabled) = autonomous_mode {
        config.agent.autonomous_mode = enabled;
    }

    manager
        .save_config(&config)
        .context("Failed to persist mode settings")?;

    if let Some(cfg) = vt_cfg.as_mut() {
        if let Some(mode) = editing_mode {
            cfg.agent.default_editing_mode = mode;
        }
        if let Some(enabled) = autonomous_mode {
            cfg.agent.autonomous_mode = enabled;
        }
    }

    Ok(())
}

pub async fn handle_debug_agent(ctx: SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    // Prefer tool-driven diagnostics when available
    if ctx.tool_registry.has_tool(tools_consts::AGENT_INFO).await {
        ctx.tool_registry
            .mark_tool_preapproved(tools_consts::AGENT_INFO)
            .await;
        match ctx
            .tool_registry
            .execute_tool_ref(
                tools_consts::AGENT_INFO,
                &serde_json::json!({"mode": "debug"}),
            )
            .await
        {
            Ok(value) => {
                ctx.renderer
                    .line(MessageStyle::Info, "Debug information (tool):")?;
                ctx.renderer
                    .line(MessageStyle::Output, &serde_json::to_string_pretty(&value)?)?;
                return Ok(SlashCommandControl::Continue);
            }
            Err(err) => {
                ctx.renderer.line(
                    MessageStyle::Error,
                    &format!("Failed to invoke agent_info tool: {}", err),
                )?;
            }
        }
    }

    ctx.renderer
        .line(MessageStyle::Info, "Debug information:")?;
    ctx.renderer.line(
        MessageStyle::Output,
        &format!("  Current model: {}", ctx.config.model),
    )?;
    ctx.renderer.line(
        MessageStyle::Output,
        &format!("  Workspace: {}", ctx.config.workspace.display()),
    )?;
    ctx.renderer.line(
        MessageStyle::Output,
        &format!(
            "  Conversation history: {} messages",
            ctx.conversation_history.len()
        ),
    )?;
    ctx.renderer.line(
        MessageStyle::Output,
        &format!(
            "  Available tools: {} registered",
            ctx.tools.read().await.len()
        ),
    )?;
    // Show recent decisions
    let ledger = ctx.decision_ledger.read().await;
    if !ledger.get_decisions().is_empty() {
        ctx.renderer.line(
            MessageStyle::Output,
            &format!("  Recent decisions: {}", ledger.get_decisions().len()),
        )?;
        // Show last few decisions
        let recent = ledger.get_decisions().iter().rev().take(3);
        for (idx, decision) in recent.enumerate() {
            ctx.renderer.line(
                MessageStyle::Output,
                &format!("    {}: {:?}", idx + 1, decision.action),
            )?;
        }
    }
    ctx.renderer.line_if_not_empty(MessageStyle::Output)?;
    Ok(SlashCommandControl::Continue)
}

pub async fn handle_analyze_agent(ctx: SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    // For now, we'll show session metrics like before
    // In the future, this could be enhanced to do actual workspace analysis
    // similar to the CLI version

    ctx.renderer
        .line(MessageStyle::Info, "Agent behavior analysis:")?;
    ctx.renderer.line(
        MessageStyle::Output,
        "  Analyzing current AI behavior patterns...",
    )?;

    // Calculate some statistics
    let total_messages = ctx.conversation_history.len();
    let tool_calls: usize = ctx
        .conversation_history
        .iter()
        .filter(|msg| msg.role == uni::MessageRole::Assistant)
        .map(|msg| msg.tool_calls.as_ref().map_or(0, |calls| calls.len()))
        .sum();

    let user_messages = ctx
        .conversation_history
        .iter()
        .filter(|msg| msg.role == uni::MessageRole::User)
        .count();

    ctx.renderer.line(
        MessageStyle::Output,
        &format!("  Total messages in conversation: {}", total_messages),
    )?;
    ctx.renderer.line(
        MessageStyle::Output,
        &format!("  User messages: {}", user_messages),
    )?;
    ctx.renderer.line(
        MessageStyle::Output,
        &format!("  Assistant tool calls: {}", tool_calls),
    )?;

    if total_messages > 0 {
        let tool_call_ratio = (tool_calls as f64) / (total_messages as f64) * 100.0;
        ctx.renderer.line(
            MessageStyle::Output,
            &format!("  Tool usage ratio: {:.1}%", tool_call_ratio),
        )?;
    }

    // Show recent tool usage patterns
    let recent_tool_calls: Vec<String> = ctx
        .conversation_history
        .iter()
        .filter(|msg| msg.role == uni::MessageRole::Assistant)
        .flat_map(|msg| {
            msg.tool_calls
                .as_ref()
                .map(|calls| {
                    calls
                        .iter()
                        .filter_map(|call| call.function.as_ref())
                        .map(|f| f.name.clone())
                })
                .into_iter()
                .flatten()
        })
        .take(10)
        .collect();

    if !recent_tool_calls.is_empty() {
        ctx.renderer
            .line(MessageStyle::Output, "  Recent tool usage:")?;
        for tool_name in recent_tool_calls {
            ctx.renderer
                .line(MessageStyle::Output, &format!("    • {}", tool_name))?;
        }
    }

    ctx.renderer.line_if_not_empty(MessageStyle::Output)?;
    Ok(SlashCommandControl::Continue)
}

pub async fn handle_theme_changed(
    ctx: SlashCommandContext<'_>,
    theme_id: String,
) -> Result<SlashCommandControl> {
    persist_theme_preference(ctx.renderer, &theme_id).await?;
    let styles = theme::active_styles();
    ctx.handle.set_theme(theme_from_styles(&styles));
    apply_prompt_style(ctx.handle);
    Ok(SlashCommandControl::Continue)
}

pub async fn handle_start_theme_palette(
    ctx: SlashCommandContext<'_>,
    mode: crate::agent::runloop::slash_commands::ThemePaletteMode,
) -> Result<SlashCommandControl> {
    if ctx.model_picker_state.is_some() {
        ctx.renderer.line(
            MessageStyle::Error,
            "Close the active model picker before selecting a theme.",
        )?;
        return Ok(SlashCommandControl::Continue);
    }
    if ctx.palette_state.is_some() {
        ctx.renderer.line(
            MessageStyle::Error,
            "Another selection modal is already open. Press Esc to dismiss it before starting a new one.",
        )?;
        return Ok(SlashCommandControl::Continue);
    }
    if show_theme_palette(ctx.renderer, mode)? {
        *ctx.palette_state = Some(ActivePalette::Theme { mode });
    }
    Ok(SlashCommandControl::Continue)
}

pub async fn handle_start_sessions_palette(
    ctx: SlashCommandContext<'_>,
    limit: usize,
) -> Result<SlashCommandControl> {
    if ctx.model_picker_state.is_some() {
        ctx.renderer.line(
            MessageStyle::Error,
            "Close the active model picker before browsing sessions.",
        )?;
        return Ok(SlashCommandControl::Continue);
    }
    if ctx.palette_state.is_some() {
        ctx.renderer.line(
            MessageStyle::Error,
            "Another selection modal is already open. Press Esc to close it before continuing.",
        )?;
        return Ok(SlashCommandControl::Continue);
    }
    match session_archive::list_recent_sessions(limit).await {
        Ok(listings) => {
            if show_sessions_palette(ctx.renderer, &listings, limit)? {
                *ctx.palette_state = Some(ActivePalette::Sessions { listings, limit });
            }
        }
        Err(err) => {
            ctx.renderer.line(
                MessageStyle::Error,
                &format!("Failed to load session archives: {}", err),
            )?;
        }
    }
    Ok(SlashCommandControl::Continue)
}

pub async fn handle_start_file_browser(
    ctx: SlashCommandContext<'_>,
    initial_filter: Option<String>,
) -> Result<SlashCommandControl> {
    if ctx.model_picker_state.is_some() {
        ctx.renderer.line(
            MessageStyle::Error,
            "Close the active model picker before opening file browser.",
        )?;
        return Ok(SlashCommandControl::Continue);
    }
    if ctx.palette_state.is_some() {
        ctx.renderer.line(
            MessageStyle::Error,
            "Another selection modal is already open. Press Esc to dismiss it before starting a new one.",
        )?;
        return Ok(SlashCommandControl::Continue);
    }
    ctx.handle.force_redraw();
    if let Some(filter) = initial_filter {
        ctx.handle.set_input(format!("@{}", filter));
    } else {
        ctx.handle.set_input("@".to_string());
    }
    ctx.renderer.line(
        MessageStyle::Info,
        "File browser activated. Use arrow keys to navigate, Enter to select, Esc to close.",
    )?;
    Ok(SlashCommandControl::Continue)
}

pub async fn handle_start_model_selection(
    ctx: SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    if ctx.model_picker_state.is_some() {
        ctx.renderer.line(
            MessageStyle::Error,
            "A model picker session is already active. Complete or type 'cancel' to exit it before starting another.",
        )?;
        return Ok(SlashCommandControl::Continue);
    }
    let reasoning = ctx
        .vt_cfg
        .as_ref()
        .map(|cfg| cfg.agent.reasoning_effort)
        .unwrap_or(ctx.config.reasoning_effort);
    let workspace_hint = Some(ctx.config.workspace.clone());
    match ModelPickerState::new(ctx.renderer, reasoning, workspace_hint).await {
        Ok(ModelPickerStart::InProgress(picker)) => {
            *ctx.model_picker_state = Some(picker);
        }
        Ok(ModelPickerStart::Completed { state, selection }) => {
            if let Err(err) = finalize_model_selection(
                ctx.renderer,
                &state,
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
        Err(err) => {
            ctx.renderer.line(
                MessageStyle::Error,
                &format!("Failed to start model picker: {}", err),
            )?;
        }
    }
    Ok(SlashCommandControl::Continue)
}

pub async fn handle_initialize_workspace(
    ctx: SlashCommandContext<'_>,
    force: bool,
) -> Result<SlashCommandControl> {
    let workspace_path = ctx.config.workspace.clone();
    let workspace_label = workspace_path.display().to_string();
    ctx.renderer.line(
        MessageStyle::Info,
        &format!(
            "Initializing vtcode configuration in {}...",
            workspace_label
        ),
    )?;
    let created_files = match bootstrap_config_files(workspace_path.clone(), force).await {
        Ok(files) => files,
        Err(err) => {
            ctx.renderer.line(
                MessageStyle::Error,
                &format!("Failed to initialize configuration: {}", err),
            )?;
            return Ok(SlashCommandControl::Continue);
        }
    };
    if created_files.is_empty() {
        ctx.renderer.line(
            MessageStyle::Info,
            "Existing configuration detected; no files were changed.",
        )?;
    } else {
        ctx.renderer.line(
            MessageStyle::Info,
            &format!(
                "Created {}: {}",
                if created_files.len() == 1 {
                    "file"
                } else {
                    "files"
                },
                created_files.join(", "),
            ),
        )?;
    }
    ctx.renderer.line(
        MessageStyle::Info,
        "Indexing workspace context (this may take a moment)...",
    )?;
    match build_workspace_index(workspace_path).await {
        Ok(()) => {
            ctx.renderer.line(
                MessageStyle::Info,
                "Workspace indexing complete. Stored under .vtcode/index.",
            )?;
        }
        Err(err) => {
            ctx.renderer.line(
                MessageStyle::Error,
                &format!("Failed to index workspace: {}", err),
            )?;
        }
    }
    Ok(SlashCommandControl::Continue)
}

pub async fn handle_generate_agent_file(
    ctx: SlashCommandContext<'_>,
    overwrite: bool,
) -> Result<SlashCommandControl> {
    let workspace_path = ctx.config.workspace.clone();
    ctx.renderer.line(
        MessageStyle::Info,
        "Generating AGENTS.md guidance. This may take a moment...",
    )?;
    match generate_agents_file(ctx.tool_registry, workspace_path.as_path(), overwrite).await {
        Ok(report) => match report.status {
            GenerateAgentsFileStatus::Created => {
                ctx.renderer.line(
                    MessageStyle::Info,
                    &format!("Created AGENTS.md at {}", report.path.display()),
                )?;
            }
            GenerateAgentsFileStatus::Overwritten => {
                ctx.renderer.line(
                    MessageStyle::Info,
                    &format!("Overwrote existing AGENTS.md at {}", report.path.display()),
                )?;
            }
            GenerateAgentsFileStatus::SkippedExisting => {
                ctx.renderer.line(
                    MessageStyle::Info,
                    &format!(
                        "AGENTS.md already exists at {}. Use /generate-agent-file --force to regenerate it.",
                        report.path.display()
                    ),
                )?;
            }
        },
        Err(err) => {
            ctx.renderer.line(
                MessageStyle::Error,
                &format!("Failed to generate AGENTS.md guidance: {}", err),
            )?;
        }
    }
    Ok(SlashCommandControl::Continue)
}

pub async fn handle_show_config(ctx: SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    if ctx.renderer.supports_inline_ui() {
        ctx.handle.open_config_palette();
    } else {
        let workspace_path = ctx.config.workspace.clone();
        let vt_snapshot = ctx.vt_cfg.clone();
        match load_config_modal_content(workspace_path, vt_snapshot).await {
            Ok(content) => {
                ctx.renderer
                    .line(MessageStyle::Info, &content.source_label)?;
                for line in &content.config_lines {
                    ctx.renderer.line(MessageStyle::Info, line)?;
                }
            }
            Err(err) => {
                ctx.renderer.line(
                    MessageStyle::Error,
                    &format!("Failed to load configuration for display: {}", err),
                )?;
            }
        }
    }
    Ok(SlashCommandControl::Continue)
}

pub async fn handle_execute_tool(
    ctx: SlashCommandContext<'_>,
    name: String,
    args: serde_json::Value,
) -> Result<SlashCommandControl> {
    match ensure_tool_permission(
        crate::agent::runloop::unified::tool_routing::ToolPermissionsContext {
            tool_registry: ctx.tool_registry,
            renderer: ctx.renderer,
            handle: ctx.handle,
            session: ctx.session,
            default_placeholder: ctx.default_placeholder.clone(),
            ctrl_c_state: ctx.ctrl_c_state,
            ctrl_c_notify: ctx.ctrl_c_notify,
            hooks: ctx.lifecycle_hooks,
            justification: None,
            approval_recorder: ctx.approval_recorder,
            decision_ledger: Some(ctx.decision_ledger),
            tool_permission_cache: Some(ctx.tool_permission_cache),
            hitl_notification_bell: ctx
                .vt_cfg
                .as_ref()
                .map(|cfg| cfg.security.hitl_notification_bell)
                .unwrap_or(true),
            autonomous_mode: ctx.session_stats.is_autonomous_mode(),
            human_in_the_loop: ctx
                .vt_cfg
                .as_ref()
                .map(|cfg| cfg.security.human_in_the_loop)
                .unwrap_or(true),
        },
        &name,
        Some(&args),
    )
    .await
    {
        Ok(ToolPermissionFlow::Approved) => Ok(SlashCommandControl::Continue),
        Ok(ToolPermissionFlow::Denied) => Ok(SlashCommandControl::Continue),
        Ok(ToolPermissionFlow::Exit) => {
            Ok(SlashCommandControl::BreakWithReason(SessionEndReason::Exit))
        }
        Ok(ToolPermissionFlow::Interrupted) => Ok(SlashCommandControl::BreakWithoutReason),
        Err(err) => {
            ctx.renderer.line(
                MessageStyle::Error,
                &format!("Failed to evaluate policy for tool '{}': {}", name, err),
            )?;
            Ok(SlashCommandControl::Continue)
        }
    }
}

pub async fn handle_clear_conversation(
    ctx: SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    ctx.conversation_history.clear();
    *ctx.session_stats = SessionStats::default();
    {
        let mut ledger = ctx.decision_ledger.write().await;
        *ledger = DecisionTracker::new();
    }
    transcript::clear();
    ctx.renderer.clear_screen();
    ctx.renderer
        .line(MessageStyle::Info, "Cleared conversation history.")?;
    ctx.renderer.line_if_not_empty(MessageStyle::Output)?;
    Ok(SlashCommandControl::Continue)
}

pub async fn handle_show_status(ctx: SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    let tool_count = ctx.tools.read().await.len();
    display_session_status(
        ctx.renderer,
        crate::agent::runloop::unified::ui_interaction::SessionStatusContext {
            config: ctx.config,
            message_count: ctx.conversation_history.len(),
            stats: ctx.session_stats,
            available_tools: tool_count,
        },
    )
    .await?;
    Ok(SlashCommandControl::Continue)
}

pub async fn handle_manage_mcp(
    ctx: SlashCommandContext<'_>,
    action: McpCommandAction,
) -> Result<SlashCommandControl> {
    let manager = ctx.async_mcp_manager.map(|m| m.as_ref());
    match action {
        McpCommandAction::Overview => {
            display_mcp_status(
                ctx.renderer,
                ctx.session_bootstrap,
                ctx.tool_registry,
                manager,
                ctx.mcp_panel_state,
            )
            .await?;
        }
        McpCommandAction::ListProviders => {
            display_mcp_providers(ctx.renderer, ctx.session_bootstrap, manager).await?;
        }
        McpCommandAction::ListTools => {
            display_mcp_tools(ctx.renderer, ctx.tool_registry).await?;
        }
        McpCommandAction::RefreshTools => {
            refresh_mcp_tools(ctx.renderer, ctx.tool_registry).await?;
        }
        McpCommandAction::ShowConfig => {
            display_mcp_config_summary(
                ctx.renderer,
                ctx.vt_cfg.as_ref(),
                ctx.session_bootstrap,
                manager,
            )
            .await?;
        }
        McpCommandAction::EditConfig => {
            render_mcp_config_edit_guidance(ctx.renderer, ctx.config.workspace.as_path()).await?;
        }
        McpCommandAction::Repair => {
            repair_mcp_runtime(
                ctx.renderer,
                manager,
                ctx.tool_registry,
                ctx.vt_cfg.as_ref(),
            )
            .await?;
        }
        McpCommandAction::Diagnose => {
            diagnose_mcp(
                ctx.renderer,
                ctx.vt_cfg.as_ref(),
                ctx.session_bootstrap,
                manager,
                ctx.tool_registry,
                ctx.mcp_panel_state,
            )
            .await?;
        }
        McpCommandAction::Login(name) => {
            render_mcp_login_guidance(ctx.renderer, name, true)?;
        }
        McpCommandAction::Logout(name) => {
            render_mcp_login_guidance(ctx.renderer, name, false)?;
        }
    }
    ctx.renderer.line_if_not_empty(MessageStyle::Output)?;
    Ok(SlashCommandControl::Continue)
}

pub async fn handle_run_doctor(ctx: SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    let provider_runtime = ctx.provider_client.name().to_string();
    run_doctor_diagnostics(
        ctx.renderer,
        ctx.config,
        ctx.vt_cfg.as_ref(),
        &provider_runtime,
        ctx.async_mcp_manager.map(|m| m.as_ref()),
        ctx.linked_directories,
        Some(ctx.loaded_skills),
    )
    .await?;
    ctx.renderer.line_if_not_empty(MessageStyle::Output)?;
    Ok(SlashCommandControl::Continue)
}

pub async fn handle_start_terminal_setup(
    ctx: SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    let vt_cfg = ctx
        .vt_cfg
        .as_ref()
        .context("VT Code configuration not available")?;
    vtcode_core::terminal_setup::run_terminal_setup_wizard(ctx.renderer, vt_cfg).await?;
    ctx.renderer.line_if_not_empty(MessageStyle::Output)?;
    Ok(SlashCommandControl::Continue)
}

pub async fn handle_manage_workspace_directories(
    ctx: SlashCommandContext<'_>,
    command: crate::agent::runloop::slash_commands::WorkspaceDirectoryCommand,
) -> Result<SlashCommandControl> {
    handle_workspace_directory_command(
        ctx.renderer,
        &ctx.config.workspace,
        command,
        ctx.linked_directories,
    )
    .await?;
    ctx.renderer.line_if_not_empty(MessageStyle::Output)?;
    Ok(SlashCommandControl::Continue)
}

pub async fn handle_new_session(ctx: SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    ctx.renderer
        .line(MessageStyle::Info, "Starting new session...")?;
    Ok(SlashCommandControl::BreakWithReason(
        SessionEndReason::NewSession,
    ))
}

pub async fn handle_open_docs(ctx: SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    const DOCS_URL: &str = "https://deepwiki.com/vinhnx/vtcode";
    match webbrowser::open(DOCS_URL) {
        Ok(_) => {
            ctx.renderer.line(
                MessageStyle::Info,
                &format!("Opening documentation in browser: {}", DOCS_URL),
            )?;
        }
        Err(err) => {
            ctx.renderer.line(
                MessageStyle::Error,
                &format!("Failed to open browser: {}", err),
            )?;
            ctx.renderer
                .line(MessageStyle::Info, &format!("Please visit: {}", DOCS_URL))?;
        }
    }
    ctx.renderer.line_if_not_empty(MessageStyle::Output)?;
    Ok(SlashCommandControl::Continue)
}

pub async fn handle_launch_editor(
    ctx: SlashCommandContext<'_>,
    file: Option<String>,
) -> Result<SlashCommandControl> {
    use std::path::PathBuf;
    use vtcode_core::tools::terminal_app::TerminalAppLauncher;

    let launcher = TerminalAppLauncher::new(ctx.config.workspace.clone());

    ctx.renderer.line(
        MessageStyle::Info,
        if file.is_some() {
            "Launching editor..."
        } else {
            "Launching editor with current input..."
        },
    )?;

    let file_path = file.as_ref().map(|f| {
        let path = PathBuf::from(f);
        if path.is_absolute() {
            path
        } else {
            ctx.config.workspace.join(path)
        }
    });

    // Pause event loop to prevent it from reading input while editor is running.
    // This prevents stdin conflicts between the TUI event loop and the external editor.
    ctx.handle.suspend_event_loop();
    // Wait for pause to take effect
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    match launcher.launch_editor(file_path) {
        Ok(Some(edited_content)) => {
            // User edited temp file, replace input with edited content
            ctx.handle.set_input(edited_content);
            ctx.handle.force_redraw(); // Force redraw to clear any artifacts
            ctx.renderer.line(
                MessageStyle::Info,
                "Editor closed. Input updated with edited content.",
            )?;
        }
        Ok(None) => {
            // User edited existing file
            ctx.handle.force_redraw(); // Force redraw to clear any artifacts
            ctx.renderer.line(MessageStyle::Info, "Editor closed.")?;
        }
        Err(err) => {
            ctx.handle.force_redraw(); // Force redraw even on error
            ctx.renderer.line(
                MessageStyle::Error,
                &format!("Failed to launch editor: {}", err),
            )?;
        }
    }

    // Resume event loop to process input again
    ctx.handle.resume_event_loop();

    ctx.renderer.line_if_not_empty(MessageStyle::Output)?;
    Ok(SlashCommandControl::Continue)
}

pub async fn handle_launch_git(ctx: SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    use vtcode_core::tools::terminal_app::TerminalAppLauncher;

    let launcher = TerminalAppLauncher::new(ctx.config.workspace.clone());

    ctx.renderer
        .line(MessageStyle::Info, "Launching git interface (lazygit)...")?;

    // Suspend TUI event loop to prevent input stealing
    ctx.handle.suspend_event_loop();
    // Give a small moment for the suspend command to propagate to the TUI thread
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    match launcher.launch_git_interface() {
        Ok(_) => {
            ctx.handle.force_redraw(); // Force redraw to clear any artifacts
            ctx.renderer
                .line(MessageStyle::Info, "Git interface closed.")?;
        }
        Err(err) => {
            ctx.handle.force_redraw(); // Force redraw even on error
            ctx.renderer.line(
                MessageStyle::Error,
                &format!("Failed to launch git interface: {}", err),
            )?;
        }
    }

    // Resume TUI event loop
    ctx.handle.resume_event_loop();

    ctx.renderer.line_if_not_empty(MessageStyle::Output)?;
    Ok(SlashCommandControl::Continue)
}

pub async fn handle_manage_skills(
    ctx: SlashCommandContext<'_>,
    action: crate::agent::runloop::SkillCommandAction,
) -> Result<SlashCommandControl> {
    use crate::agent::runloop::handle_skill_command;
    use vtcode_core::config::types::CapabilityLevel;
    use vtcode_core::skills::executor::SkillToolAdapter;
    use vtcode_core::tools::ToolRegistration;

    let outcome = handle_skill_command(action, ctx.config.workspace.clone()).await?;

    use crate::agent::runloop::SkillCommandOutcome;
    match outcome {
        SkillCommandOutcome::Handled { message } => {
            ctx.renderer.line(MessageStyle::Info, &message)?;
            Ok(SlashCommandControl::Continue)
        }
        SkillCommandOutcome::LoadSkill { skill, message } => {
            let skill_name = skill.name().to_string();

            // Create adapter and register as tool in tool registry
            let adapter = SkillToolAdapter::new(skill.clone());
            let adapter_arc = Arc::new(adapter);

            let name_static: &'static str = Box::leak(Box::new(skill_name.clone()));

            let registration =
                ToolRegistration::from_tool(name_static, CapabilityLevel::Bash, adapter_arc);

            if let Err(e) = ctx.tool_registry.register_tool(registration).await {
                ctx.renderer.line(
                    MessageStyle::Error,
                    &format!("Failed to register skill as tool: {}", e),
                )?;
                return Ok(SlashCommandControl::Continue);
            }

            // Store in session loaded skills registry
            ctx.loaded_skills
                .write()
                .await
                .insert(skill_name.clone(), skill.clone());

            ctx.renderer.line(MessageStyle::Info, &message)?;
            Ok(SlashCommandControl::Continue)
        }
        SkillCommandOutcome::UnloadSkill { name } => {
            // Remove from loaded skills registry
            ctx.loaded_skills.write().await.remove(&name);

            ctx.renderer
                .line(MessageStyle::Info, &format!("Unloaded skill: {}", name))?;
            Ok(SlashCommandControl::Continue)
        }
        SkillCommandOutcome::UseSkill { skill, input } => {
            // Phase 5: Execute skill with LLM sub-call support
            use vtcode_core::skills::execute_skill_with_sub_llm;

            let skill_name = skill.name().to_string();
            let available_tools = ctx.tools.read().await.clone();
            let model = ctx.config.model.clone();

            // Execute skill with LLM sub-calls
            match execute_skill_with_sub_llm(
                &skill,
                input,
                ctx.provider_client.as_ref(), // deref Box to &dyn
                ctx.tool_registry,
                available_tools,
                model,
            )
            .await
            {
                Ok(result) => {
                    // Display result to user
                    ctx.renderer.line(MessageStyle::Output, &result)?;

                    // Add to conversation history for context
                    ctx.conversation_history.push(uni::Message::user(format!(
                        "/skills use {} [executed]",
                        skill_name
                    )));

                    let result_string: String = result;
                    let limited = truncate_message_content(&result_string);
                    ctx.conversation_history
                        .push(uni::Message::assistant(limited));
                    enforce_history_limits(ctx.conversation_history);

                    Ok(SlashCommandControl::Continue)
                }
                Err(e) => {
                    ctx.renderer.line(
                        MessageStyle::Error,
                        &format!("Failed to execute skill: {}", e),
                    )?;
                    Ok(SlashCommandControl::Continue)
                }
            }
        }
        SkillCommandOutcome::Error { message } => {
            ctx.renderer.line(MessageStyle::Error, &message)?;
            Ok(SlashCommandControl::Continue)
        }
    }
}

pub async fn handle_rewind_to_turn(
    ctx: SlashCommandContext<'_>,
    turn: usize,
    scope: vtcode_core::core::agent::snapshots::RevertScope,
) -> Result<SlashCommandControl> {
    // Check if checkpoint manager is available
    if let Some(manager) = ctx.checkpoint_manager {
        // Attempt to restore the snapshot
        match manager.restore_snapshot(turn, scope).await {
            Ok(Some(restored)) => {
                // Update conversation history if scope includes conversation
                if scope.includes_conversation() {
                    *ctx.conversation_history = restored
                        .conversation
                        .iter()
                        .map(uni::Message::from)
                        .collect();
                    ctx.renderer.line(
                        MessageStyle::Info,
                        &format!(
                            "Restored conversation history from turn {} ({} messages)",
                            turn,
                            restored.conversation.len()
                        ),
                    )?;
                }

                // Report code changes if scope includes code
                if scope.includes_code() {
                    ctx.renderer.line(
                        MessageStyle::Info,
                        &format!("Applied code changes from turn {}", turn),
                    )?;
                }

                ctx.renderer.line(
                    MessageStyle::Info,
                    &format!(
                        "Successfully rewound to turn {} with scope {:?}",
                        turn, scope
                    ),
                )?;
            }
            Ok(None) => {
                ctx.renderer.line(
                    MessageStyle::Error,
                    &format!("No checkpoint found for turn {}", turn),
                )?;
            }
            Err(err) => {
                ctx.renderer.line(
                    MessageStyle::Error,
                    &format!("Failed to restore checkpoint for turn {}: {}", turn, err),
                )?;
            }
        }
    } else {
        // Fallback to CLI command guidance if checkpoint manager is not available
        ctx.renderer.line(
            MessageStyle::Info,
            &format!("Rewinding to turn {} with scope {:?}...", turn, scope),
        )?;

        ctx.renderer.line(
            MessageStyle::Info,
            &format!(
                "Use: `vtcode revert --turn {} --partial {}` from command line",
                turn,
                match scope {
                    vtcode_core::core::agent::snapshots::RevertScope::Conversation =>
                        "conversation",
                    vtcode_core::core::agent::snapshots::RevertScope::Code => "code",
                    vtcode_core::core::agent::snapshots::RevertScope::Both => "both",
                }
            ),
        )?;

        ctx.renderer.line(
            MessageStyle::Info,
            "Note: In-chat rewind requires access to the checkpoint manager.",
        )?;
    }

    Ok(SlashCommandControl::Continue)
}

pub async fn handle_exit(ctx: SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    ctx.renderer.line(MessageStyle::Info, "✓")?;
    Ok(SlashCommandControl::BreakWithReason(SessionEndReason::Exit))
}

pub async fn handle_manage_agents(
    ctx: SlashCommandContext<'_>,
    action: AgentCommandAction,
) -> Result<SlashCommandControl> {
    match action {
        AgentCommandAction::List => {
            ctx.renderer
                .line(MessageStyle::Info, "Built-in Subagents")?;
            ctx.renderer.line(MessageStyle::Output, "")?;
            ctx.renderer.line(
                MessageStyle::Output,
                "  explore         - Fast read-only codebase search (haiku)",
            )?;
            ctx.renderer.line(
                MessageStyle::Output,
                "  plan            - Research specialist for planning mode (sonnet)",
            )?;
            ctx.renderer.line(
                MessageStyle::Output,
                "  general         - Multi-step tasks with full capabilities (sonnet)",
            )?;
            ctx.renderer.line(
                MessageStyle::Output,
                "  code-reviewer   - Code quality and security review",
            )?;
            ctx.renderer.line(
                MessageStyle::Output,
                "  debugger        - Error investigation and fixes",
            )?;
            ctx.renderer.line(MessageStyle::Output, "")?;
            ctx.renderer.line(
                MessageStyle::Info,
                "Custom Subagents (project: .vtcode/agents/ | user: ~/.vtcode/agents/)",
            )?;
            ctx.renderer.line(MessageStyle::Output, "")?;

            // Load and display custom agents from .vtcode/agents/ and ~/.vtcode/agents/
            let mut custom_agents = Vec::new();

            // 1. Check project agents (.vtcode/agents/)
            let project_agents_dir = ctx.config.workspace.join(".vtcode/agents");
            if project_agents_dir.exists()
                && let Ok(entries) = std::fs::read_dir(project_agents_dir)
            {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file()
                        && path.extension().and_then(|s| s.to_str()) == Some("md")
                        && let Some(name) = path.file_stem().and_then(|s| s.to_str())
                    {
                        custom_agents.push(format!("  {: <15} - (project)", name));
                    }
                }
            }

            // 2. Check user agents (~/.vtcode/agents/)
            if let Some(home) = dirs::home_dir() {
                let user_agents_dir = home.join(".vtcode/agents");
                if user_agents_dir.exists()
                    && let Ok(entries) = std::fs::read_dir(user_agents_dir)
                {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.is_file()
                            && path.extension().and_then(|s| s.to_str()) == Some("md")
                            && !custom_agents.iter().any(|a| {
                                a.contains(path.file_stem().and_then(|s| s.to_str()).unwrap_or(""))
                            })
                            && let Some(name) = path.file_stem().and_then(|s| s.to_str())
                        {
                            custom_agents.push(format!("  {: <15} - (user)", name));
                        }
                    }
                }
            }

            if custom_agents.is_empty() {
                ctx.renderer.line(
                    MessageStyle::Output,
                    "  Use /agents create to add a custom agent",
                )?;
            } else {
                for agent in custom_agents {
                    ctx.renderer.line(MessageStyle::Output, &agent)?;
                }
            }
            ctx.renderer.line(MessageStyle::Output, "")?;
            ctx.renderer.line(
                MessageStyle::Info,
                "More info: https://code.claude.com/docs/en/sub-agents",
            )?;
            Ok(SlashCommandControl::Continue)
        }
        AgentCommandAction::Create => {
            ctx.renderer.line(
                MessageStyle::Info,
                "Creating a new subagent interactively...",
            )?;
            ctx.renderer.line(MessageStyle::Output, "")?;
            ctx.renderer.line(
                MessageStyle::Output,
                "Use Claude to generate a subagent configuration:",
            )?;
            ctx.renderer.line(MessageStyle::Output, "")?;
            ctx.renderer.line(
                MessageStyle::Output,
                "  > I need a subagent that [describe what it should do]",
            )?;
            ctx.renderer.line(MessageStyle::Output, "")?;
            ctx.renderer
                .line(MessageStyle::Output, "Or edit manually:")?;
            ctx.renderer.line(MessageStyle::Output, "")?;
            ctx.renderer
                .line(MessageStyle::Output, "  mkdir -p .vtcode/agents")?;
            ctx.renderer.line(
                MessageStyle::Output,
                "  Create a .md file with YAML frontmatter in that directory",
            )?;
            ctx.renderer.line(MessageStyle::Output, "")?;
            ctx.renderer.line(
                MessageStyle::Info,
                "For format details: https://code.claude.com/docs/en/sub-agents#file-format",
            )?;
            Ok(SlashCommandControl::Continue)
        }
        AgentCommandAction::Edit(agent_name) => {
            ctx.renderer.line(
                MessageStyle::Info,
                &format!("Editing subagent: {}", agent_name),
            )?;
            ctx.renderer.line(MessageStyle::Output, "")?;
            ctx.renderer.line(
                MessageStyle::Output,
                "Edit the agent configuration file manually:",
            )?;
            ctx.renderer.line(MessageStyle::Output, "")?;
            ctx.renderer.line(
                MessageStyle::Output,
                "  Project agents:  .vtcode/agents/{}.md",
            )?;
            ctx.renderer.line(
                MessageStyle::Output,
                "  User agents:     ~/.vtcode/agents/{}.md",
            )?;
            ctx.renderer.line(MessageStyle::Output, "")?;
            ctx.renderer.line(
                MessageStyle::Info,
                "Or use /edit command to open in your editor",
            )?;
            Ok(SlashCommandControl::Continue)
        }
        AgentCommandAction::Delete(agent_name) => {
            ctx.renderer.line(
                MessageStyle::Info,
                &format!("Deleting subagent: {}", agent_name),
            )?;
            ctx.renderer.line(MessageStyle::Output, "")?;
            ctx.renderer
                .line(MessageStyle::Output, "Remove the agent configuration file:")?;
            ctx.renderer.line(MessageStyle::Output, "")?;
            ctx.renderer.line(
                MessageStyle::Output,
                &format!("  rm .vtcode/agents/{}.md", agent_name),
            )?;
            ctx.renderer.line(
                MessageStyle::Output,
                &format!("  # or ~/.vtcode/agents/{}.md for user agents", agent_name),
            )?;
            ctx.renderer.line(MessageStyle::Output, "")?;
            ctx.renderer.line(
                MessageStyle::Info,
                "Changes take effect on next session start",
            )?;
            Ok(SlashCommandControl::Continue)
        }
        AgentCommandAction::Help => {
            ctx.renderer
                .line(MessageStyle::Info, "Subagent Management")?;
            ctx.renderer.line(MessageStyle::Output, "")?;
            ctx.renderer.line(
                MessageStyle::Output,
                "Usage: /agents [list|create|edit|delete] [options]",
            )?;
            ctx.renderer.line(MessageStyle::Output, "")?;
            ctx.renderer.line(
                MessageStyle::Output,
                "  /agents              List all available subagents",
            )?;
            ctx.renderer.line(
                MessageStyle::Output,
                "  /agents create       Create a new subagent interactively",
            )?;
            ctx.renderer.line(
                MessageStyle::Output,
                "  /agents edit NAME    Edit an existing subagent",
            )?;
            ctx.renderer.line(
                MessageStyle::Output,
                "  /agents delete NAME  Delete a subagent",
            )?;
            ctx.renderer.line(MessageStyle::Output, "")?;
            ctx.renderer.line(
                MessageStyle::Info,
                "Documentation: https://code.claude.com/docs/en/sub-agents",
            )?;
            Ok(SlashCommandControl::Continue)
        }
    }
}

pub async fn handle_toggle_plan_mode(
    ctx: SlashCommandContext<'_>,
    enable: Option<bool>,
) -> Result<SlashCommandControl> {
    use vtcode_core::ui::tui::EditingMode;

    let current = ctx.session_stats.is_plan_mode();
    let new_state = match enable {
        Some(value) => value,
        None => !current,
    };

    if new_state == current {
        ctx.renderer.line(
            MessageStyle::Info,
            if current {
                "Plan Mode is already enabled (read-only: no edits or commands)."
            } else {
                "Plan Mode is already disabled."
            },
        )?;
        return Ok(SlashCommandControl::Continue);
    }

    ctx.session_stats.set_plan_mode(new_state);

    // Update header display to show editing mode indicator
    let new_mode = if new_state {
        EditingMode::Plan
    } else {
        EditingMode::Edit
    };
    ctx.handle.set_editing_mode(new_mode);

    if new_state {
        ctx.tool_registry.enable_plan_mode();
        ctx.session_stats.switch_to_planner();
        ctx.renderer.line(
            MessageStyle::Info,
            "Plan Mode enabled (planner profile active)",
        )?;
        ctx.renderer.line(
            MessageStyle::Output,
            "  The agent will only read/analyze the codebase and produce step-by-step plans.",
        )?;
        ctx.renderer.line(
            MessageStyle::Output,
            "  File edits, commands, and tests are disabled until you run /plan off.",
        )?;
        ctx.renderer.line(MessageStyle::Output, "")?;
        ctx.renderer.line(
            MessageStyle::Info,
            "Allowed tools: read_file, list_files, grep_file, code_intelligence, ask_user_question, request_user_input",
        )?;
    } else {
        ctx.tool_registry.disable_plan_mode();
        ctx.session_stats.switch_to_coder();
        ctx.renderer.line(
            MessageStyle::Info,
            "Edit Mode enabled (coder profile active)",
        )?;
        ctx.renderer.line(
            MessageStyle::Output,
            "  Mutating tools (edits, commands, tests) are now allowed, subject to normal permissions.",
        )?;
    }

    let persisted_mode = if new_state {
        ConfigEditingMode::Plan
    } else {
        ConfigEditingMode::Edit
    };
    if let Err(err) = persist_mode_settings(
        ctx.config.workspace.as_path(),
        ctx.vt_cfg,
        Some(persisted_mode),
        None,
    ) {
        ctx.renderer.line(
            MessageStyle::Error,
            &format!("Failed to persist plan mode preference: {}", err),
        )?;
    }

    Ok(SlashCommandControl::Continue)
}

pub async fn handle_toggle_autonomous_mode(
    ctx: SlashCommandContext<'_>,
    enable: Option<bool>,
) -> Result<SlashCommandControl> {
    let current = ctx.session_stats.is_autonomous_mode();
    let new_state = match enable {
        Some(value) => value,
        None => !current,
    };

    if new_state == current {
        ctx.renderer.line(
            MessageStyle::Info,
            if current {
                "Autonomous Mode is already enabled."
            } else {
                "Autonomous Mode is already disabled."
            },
        )?;
        return Ok(SlashCommandControl::Continue);
    }

    ctx.session_stats.set_autonomous_mode(new_state);
    ctx.handle.set_autonomous_mode(new_state);

    if new_state {
        ctx.renderer
            .line(MessageStyle::Info, "Autonomous Mode enabled")?;
        ctx.renderer.line(
            MessageStyle::Output,
            "  The agent will work more autonomously with fewer confirmation prompts.",
        )?;
        ctx.renderer.line(
            MessageStyle::Output,
            "  Safe tools (read/search) are auto-approved. Use with caution.",
        )?;
    } else {
        ctx.renderer
            .line(MessageStyle::Info, "Autonomous Mode disabled")?;
        ctx.renderer.line(
            MessageStyle::Output,
            "  Standard human-in-the-loop prompts are now active for all mutating actions.",
        )?;
    }

    if let Err(err) = persist_mode_settings(
        ctx.config.workspace.as_path(),
        ctx.vt_cfg,
        None,
        Some(new_state),
    ) {
        ctx.renderer.line(
            MessageStyle::Error,
            &format!("Failed to persist autonomous mode preference: {}", err),
        )?;
    }

    Ok(SlashCommandControl::Continue)
}

pub async fn handle_cycle_mode(ctx: SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    use vtcode_core::ui::tui::EditingMode;

    let new_mode = ctx.session_stats.cycle_mode();
    ctx.handle.set_editing_mode(new_mode);

    // Handle registry state based on new mode
    if new_mode == EditingMode::Plan {
        ctx.tool_registry.enable_plan_mode();
    } else {
        ctx.tool_registry.disable_plan_mode();
    }

    match new_mode {
        EditingMode::Edit => {
            ctx.renderer
                .line(MessageStyle::Info, "Switched to Edit Mode")?;
            ctx.renderer.line(
                MessageStyle::Output,
                "  Full tool access with standard confirmation prompts.",
            )?;
        }
        EditingMode::Plan => {
            ctx.renderer
                .line(MessageStyle::Info, "Switched to Plan Mode")?;
            ctx.renderer.line(
                MessageStyle::Output,
                "  Read-only mode for analysis and planning. Mutating tools disabled.",
            )?;
        }
    }

    let persisted_mode = match new_mode {
        EditingMode::Plan => ConfigEditingMode::Plan,
        EditingMode::Edit => ConfigEditingMode::Edit,
    };
    if let Err(err) = persist_mode_settings(
        ctx.config.workspace.as_path(),
        ctx.vt_cfg,
        Some(persisted_mode),
        None,
    ) {
        ctx.renderer.line(
            MessageStyle::Error,
            &format!("Failed to persist editing mode preference: {}", err),
        )?;
    }

    Ok(SlashCommandControl::Continue)
}
