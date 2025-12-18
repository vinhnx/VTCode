use std::sync::Arc;

use anyhow::Result;
use tokio::sync::{Notify, RwLock};

use vtcode_core::commands::init::{GenerateAgentsFileStatus, generate_agents_file};
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::core::decision_tracker::DecisionTracker;
use vtcode_core::llm::provider as uni;
use vtcode_core::tools::ToolRegistry;

use vtcode_core::ui::theme;
use vtcode_core::ui::tui::{InlineHandle, InlineSession, theme_from_styles};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_core::utils::session_archive;
use vtcode_core::utils::transcript;

use super::super::workspace::{bootstrap_config_files, build_workspace_index};
use crate::agent::runloop::context::ContextTrimConfig;
use crate::agent::runloop::mcp_events;
use crate::agent::runloop::model_picker::{ModelPickerStart, ModelPickerState};
use crate::agent::runloop::slash_commands::McpCommandAction;
pub use crate::agent::runloop::slash_commands::SlashCommandOutcome;
use crate::agent::runloop::unified::async_mcp_manager::AsyncMcpManager;
use crate::agent::runloop::unified::context_manager::ContextManager;
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
use crate::agent::runloop::unified::state::{CtrlCState, SessionStats};
use crate::agent::runloop::unified::tool_routing::{ToolPermissionFlow, ensure_tool_permission};
use crate::agent::runloop::unified::ui_interaction::{display_session_status, display_token_cost};
use crate::agent::runloop::unified::workspace_links::{
    LinkedDirectory, handle_workspace_directory_command,
};
use crate::agent::runloop::welcome::SessionBootstrap;
use crate::hooks::lifecycle::{LifecycleHookEngine, SessionEndReason};
use vtcode_core::config::constants::tools as tools_consts;
use webbrowser;

use super::super::config_modal::{MODAL_CLOSE_HINT, load_config_modal_content};

pub enum SlashCommandControl {
    Continue,
    SubmitPrompt(String),
    BreakWithReason(SessionEndReason),
    BreakWithoutReason,
}

pub struct SlashCommandContext<'a> {
    pub renderer: &'a mut AnsiRenderer,
    pub handle: &'a InlineHandle,
    pub session: &'a mut InlineSession,
    pub config: &'a mut CoreAgentConfig,
    pub vt_cfg: &'a mut Option<VTCodeConfig>,
    pub provider_client: &'a mut Box<dyn uni::LLMProvider>,
    pub session_bootstrap: &'a SessionBootstrap,
    pub model_picker_state: &'a mut Option<ModelPickerState>,
    pub palette_state: &'a mut Option<ActivePalette>,
    pub tool_registry: &'a mut ToolRegistry,
    pub conversation_history: &'a mut Vec<uni::Message>,
    pub decision_ledger: &'a Arc<RwLock<DecisionTracker>>,
    pub pruning_ledger:
        &'a Arc<RwLock<vtcode_core::core::pruning_decisions::PruningDecisionLedger>>,
    pub context_manager: &'a mut ContextManager,
    pub session_stats: &'a mut SessionStats,
    pub tools: &'a Arc<RwLock<Vec<uni::ToolDefinition>>>,
    pub token_budget_enabled: bool,
    pub trim_config: &'a ContextTrimConfig,
    pub async_mcp_manager: Option<&'a Arc<AsyncMcpManager>>,
    pub mcp_panel_state: &'a mut mcp_events::McpPanelState,
    pub linked_directories: &'a mut Vec<LinkedDirectory>,
    pub ctrl_c_state: &'a Arc<CtrlCState>,
    pub ctrl_c_notify: &'a Arc<Notify>,
    pub default_placeholder: &'a Option<String>,
    pub lifecycle_hooks: Option<&'a LifecycleHookEngine>,
    pub full_auto: bool,
    pub approval_recorder: Option<&'a vtcode_core::tools::ApprovalRecorder>,
    pub tool_permission_cache: &'a Arc<RwLock<vtcode_core::acp::ToolPermissionCache>>,
    pub loaded_skills:
        &'a Arc<RwLock<std::collections::HashMap<String, vtcode_core::skills::types::Skill>>>,
}

pub async fn handle_outcome(
    outcome: SlashCommandOutcome,
    ctx: SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    match outcome {
        SlashCommandOutcome::SubmitPrompt { prompt } => {
            Ok(SlashCommandControl::SubmitPrompt(prompt))
        }
        SlashCommandOutcome::Handled => Ok(SlashCommandControl::Continue),
        SlashCommandOutcome::DebugAgent => {
            // Prefer tool-driven diagnostics when available
            if ctx.tool_registry.has_tool(tools_consts::DEBUG_AGENT).await {
                ctx.tool_registry
                    .mark_tool_preapproved(tools_consts::DEBUG_AGENT);
                match ctx
                    .tool_registry
                    .execute_tool_ref(tools_consts::DEBUG_AGENT, &serde_json::json!({}))
                    .await
                {
                    Ok(value) => {
                        ctx.renderer
                            .line(MessageStyle::Info, "Debug information (tool):")?;
                        ctx.renderer
                            .line(MessageStyle::Output, &value.to_string())?;
                        return Ok(SlashCommandControl::Continue);
                    }
                    Err(err) => {
                        ctx.renderer.line(
                            MessageStyle::Error,
                            &format!("Failed to invoke debug_agent tool: {}", err),
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
        SlashCommandOutcome::AnalyzeAgent => {
            // Prefer tool-driven analysis when available
            if ctx
                .tool_registry
                .has_tool(tools_consts::ANALYZE_AGENT)
                .await
            {
                ctx.tool_registry
                    .mark_tool_preapproved(tools_consts::ANALYZE_AGENT);
                match ctx
                    .tool_registry
                    .execute_tool_ref(tools_consts::ANALYZE_AGENT, &serde_json::json!({}))
                    .await
                {
                    Ok(value) => {
                        ctx.renderer
                            .line(MessageStyle::Info, "Agent analysis (tool):")?;
                        ctx.renderer
                            .line(MessageStyle::Output, &value.to_string())?;
                        return Ok(SlashCommandControl::Continue);
                    }
                    Err(err) => {
                        ctx.renderer.line(
                            MessageStyle::Error,
                            &format!("Failed to invoke analyze_agent tool: {}", err),
                        )?;
                    }
                }
            }

            ctx.renderer.line(MessageStyle::Info, "Agent analysis:")?;
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

            // Show current token budget status if enabled
            if ctx.token_budget_enabled {
                let token_budget = ctx.context_manager.token_budget();
                ctx.renderer.line(
                    MessageStyle::Output,
                    &format!(
                        "  Current token budget: {}/{}",
                        token_budget.get_stats().await.total_tokens,
                        ctx.trim_config.max_tokens
                    ),
                )?;
            }

            ctx.renderer.line_if_not_empty(MessageStyle::Output)?;
            Ok(SlashCommandControl::Continue)
        }
        SlashCommandOutcome::ThemeChanged(theme_id) => {
            persist_theme_preference(ctx.renderer, &theme_id).await?;
            let styles = theme::active_styles();
            ctx.handle.set_theme(theme_from_styles(&styles));
            apply_prompt_style(ctx.handle);
            Ok(SlashCommandControl::Continue)
        }
        SlashCommandOutcome::StartThemePalette { mode } => {
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
        SlashCommandOutcome::StartSessionsPalette { limit } => {
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

        SlashCommandOutcome::StartFileBrowser { initial_filter } => {
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

        SlashCommandOutcome::StartModelSelection => {
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
        SlashCommandOutcome::InitializeWorkspace { force } => {
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
        SlashCommandOutcome::GenerateAgentFile { overwrite } => {
            let workspace_path = ctx.config.workspace.clone();
            ctx.renderer.line(
                MessageStyle::Info,
                "Generating AGENTS.md guidance. This may take a moment...",
            )?;
            match generate_agents_file(ctx.tool_registry, workspace_path.as_path(), overwrite).await
            {
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
        SlashCommandOutcome::ShowConfig => {
            let workspace_path = ctx.config.workspace.clone();
            let vt_snapshot = ctx.vt_cfg.clone();
            match load_config_modal_content(workspace_path, vt_snapshot).await {
                Ok(content) => {
                    if ctx.renderer.prefers_untruncated_output() {
                        let mut modal_lines = Vec::new();
                        modal_lines.push(content.source_label.clone());
                        modal_lines.push(String::new());
                        modal_lines.extend(content.config_lines.clone());
                        modal_lines.push(String::new());
                        modal_lines.push(MODAL_CLOSE_HINT.to_string());
                        ctx.handle.close_modal();
                        ctx.handle
                            .show_modal(content.title.clone(), modal_lines, None);
                        ctx.renderer.line(
                            MessageStyle::Info,
                            &format!("Opened {} modal ({}).", content.title, content.source_label),
                        )?;
                        ctx.renderer.line(MessageStyle::Info, MODAL_CLOSE_HINT)?;
                    } else {
                        ctx.renderer
                            .line(MessageStyle::Info, &content.source_label)?;
                        for line in content.config_lines {
                            ctx.renderer.line(MessageStyle::Info, &line)?;
                        }
                    }
                }
                Err(err) => {
                    ctx.renderer.line(
                        MessageStyle::Error,
                        &format!("Failed to load configuration for display: {}", err),
                    )?;
                }
            }
            Ok(SlashCommandControl::Continue)
        }
        SlashCommandOutcome::ExecuteTool { name, args } => {
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
        SlashCommandOutcome::ClearConversation => {
            ctx.conversation_history.clear();
            *ctx.session_stats = SessionStats::default();
            {
                let mut ledger = ctx.decision_ledger.write().await;
                *ledger = DecisionTracker::new();
            }
            ctx.context_manager.reset_token_budget().await;
            transcript::clear();
            ctx.renderer.clear_screen();
            ctx.renderer.line(
                MessageStyle::Info,
                "Cleared conversation history and token statistics.",
            )?;
            ctx.renderer.line_if_not_empty(MessageStyle::Output)?;
            Ok(SlashCommandControl::Continue)
        }
        SlashCommandOutcome::ShowStatus => {
            let token_budget = ctx.context_manager.token_budget();
            let tool_count = ctx.tools.read().await.len();
            display_session_status(
                ctx.renderer,
                crate::agent::runloop::unified::ui_interaction::SessionStatusContext {
                    config: ctx.config,
                    message_count: ctx.conversation_history.len(),
                    stats: ctx.session_stats,
                    token_budget: token_budget.as_ref(),
                    token_budget_enabled: ctx.token_budget_enabled,
                    max_tokens: ctx.trim_config.max_tokens,
                    available_tools: tool_count,
                },
            )
            .await?;
            Ok(SlashCommandControl::Continue)
        }
        SlashCommandOutcome::ShowCost => {
            let token_budget = ctx.context_manager.token_budget();
            ctx.renderer
                .line(MessageStyle::Info, "Token usage summary:")?;
            display_token_cost(
                ctx.renderer,
                token_budget.as_ref(),
                ctx.token_budget_enabled,
                ctx.trim_config.max_tokens,
                "",
            )
            .await?;
            Ok(SlashCommandControl::Continue)
        }
        SlashCommandOutcome::ManageMcp { action } => {
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
                    render_mcp_config_edit_guidance(ctx.renderer, ctx.config.workspace.as_path())
                        .await?;
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
        SlashCommandOutcome::RunDoctor => {
            let provider_runtime = ctx.provider_client.name().to_string();
            run_doctor_diagnostics(
                ctx.renderer,
                ctx.config,
                ctx.vt_cfg.as_ref(),
                &provider_runtime,
                ctx.async_mcp_manager.map(|m| m.as_ref()),
                ctx.linked_directories,
            )
            .await?;
            ctx.renderer.line_if_not_empty(MessageStyle::Output)?;
            Ok(SlashCommandControl::Continue)
        }
        SlashCommandOutcome::ManageWorkspaceDirectories { command } => {
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
        SlashCommandOutcome::NewSession => {
            ctx.renderer
                .line(MessageStyle::Info, "Starting new session...")?;
            Ok(SlashCommandControl::BreakWithReason(
                SessionEndReason::NewSession,
            ))
        }
        SlashCommandOutcome::OpenDocs => {
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
        SlashCommandOutcome::ShowPruningReport => {
            ctx.renderer.line(MessageStyle::Info, "Pruning Report:")?;
            let ledger = ctx.pruning_ledger.read().await;
            let report = ledger.generate_report();

            // Display summary statistics
            ctx.renderer.line(
                MessageStyle::Output,
                &format!(
                    "  Total messages evaluated: {}",
                    report.statistics.total_messages_evaluated
                ),
            )?;
            ctx.renderer.line(
                MessageStyle::Output,
                &format!("  Messages kept: {}", report.statistics.messages_kept),
            )?;
            ctx.renderer.line(
                MessageStyle::Output,
                &format!("  Messages removed: {}", report.statistics.messages_removed),
            )?;
            ctx.renderer.line(
                MessageStyle::Output,
                &format!(
                    "  Retention ratio: {:.1}%",
                    report.message_retention_ratio * 100.0
                ),
            )?;
            ctx.renderer.line(
                MessageStyle::Output,
                &format!("  Semantic efficiency: {:.2}", report.semantic_efficiency),
            )?;

            // Display brief ledger summary
            let brief = ledger.render_ledger_brief(10);
            if !brief.is_empty() {
                ctx.renderer.line(MessageStyle::Output, "")?;
                ctx.renderer
                    .line(MessageStyle::Output, "Recent pruning decisions:")?;
                for line in brief.lines().take(10) {
                    ctx.renderer
                        .line(MessageStyle::Output, &format!("  {}", line))?;
                }
            }

            ctx.renderer.line_if_not_empty(MessageStyle::Output)?;
            Ok(SlashCommandControl::Continue)
        }
        SlashCommandOutcome::LaunchEditor { file } => {
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
            // Wait for pause to take effect. The event loop polls every 16ms, and might be
            // in the middle of a poll when we send the suspend message. Wait a bit longer
            // to ensure the pause flag is checked before the editor launches.
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

        SlashCommandOutcome::LaunchGit => {
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

        SlashCommandOutcome::ManageSkills { action } => {
            use crate::agent::runloop::handle_skill_command;
            use std::sync::Arc;
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
                SkillCommandOutcome::LoadSkill { skill } => {
                    let skill_name = skill.name().to_string();

                    // Create adapter and register as tool in tool registry
                    let adapter = SkillToolAdapter::new(skill.clone());
                    let adapter_arc = Arc::new(adapter);

                    // SAFETY: skill_name is converted to static for Tool trait.
                    // The ToolAdapter's name() method already returns 'static.
                    let name_static: &'static str = Box::leak(Box::new(skill_name.clone()));

                    let registration = ToolRegistration::from_tool(
                        name_static,
                        CapabilityLevel::Bash,
                        adapter_arc,
                    );

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

                    ctx.renderer.line(
                        MessageStyle::Info,
                        &format!("Loaded skill: {} - {}", skill.name(), skill.description()),
                    )?;
                    Ok(SlashCommandControl::Continue)
                }
                SkillCommandOutcome::UnloadSkill { name } => {
                    // Remove from loaded skills registry
                    ctx.loaded_skills.write().await.remove(&name);

                    // Unregister from tool registry (if support exists)
                    // Note: Current tool registry may not support dynamic unregistration
                    // This is a TODO for future enhancement

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
                        ctx.provider_client.as_ref(),
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

        SlashCommandOutcome::Exit => {
            ctx.renderer.line(MessageStyle::Info, "✓")?;
            Ok(SlashCommandControl::BreakWithReason(SessionEndReason::Exit))
        }
    }
}
