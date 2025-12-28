use std::sync::Arc;

use anyhow::{Context, Result};
use serde_json;

use vtcode_core::commands::init::{GenerateAgentsFileStatus, generate_agents_file};
use vtcode_core::config::constants::tools as tools_consts;
use vtcode_core::core::decision_tracker::DecisionTracker;
use vtcode_core::llm::provider as uni;
use vtcode_core::ui::theme;
use vtcode_core::ui::tui::theme_from_styles;
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_core::utils::session_archive;
use vtcode_core::utils::transcript;

use crate::agent::runloop::model_picker::{ModelPickerStart, ModelPickerState};
use crate::agent::runloop::slash_commands::McpCommandAction;
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
use crate::agent::runloop::unified::turn::workspace::{
    bootstrap_config_files, build_workspace_index,
};
use crate::agent::runloop::unified::ui_interaction::display_session_status;
use crate::agent::runloop::unified::workspace_links::handle_workspace_directory_command;
use crate::hooks::lifecycle::SessionEndReason;
use webbrowser;

use super::{SlashCommandContext, SlashCommandControl};
use crate::agent::runloop::unified::turn::config_modal::load_config_modal_content;

pub async fn handle_debug_agent(ctx: SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    // Prefer tool-driven diagnostics when available
    if ctx.tool_registry.has_tool(tools_consts::AGENT_INFO).await {
        ctx.tool_registry
            .mark_tool_preapproved(tools_consts::AGENT_INFO);
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
        ctx.tool_registry,
        &name,
        Some(&args),
        ctx.renderer,
        ctx.handle,
        ctx.session,
        ctx.default_placeholder.clone(),
        ctx.ctrl_c_state,
        ctx.ctrl_c_notify,
        ctx.lifecycle_hooks,
        None, // justification from agent
        ctx.approval_recorder,
        Some(ctx.decision_ledger),
        Some(ctx.tool_permission_cache),
        ctx.vt_cfg
            .as_ref()
            .map(|cfg| cfg.security.hitl_notification_bell)
            .unwrap_or(true),
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

            if let Err(e) = ctx.tool_registry.register_tool(registration) {
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
                    ctx.conversation_history
                        .push(uni::Message::assistant(result_string));

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

pub async fn handle_exit(ctx: SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    ctx.renderer.line(MessageStyle::Info, "✓")?;
    Ok(SlashCommandControl::BreakWithReason(SessionEndReason::Exit))
}
