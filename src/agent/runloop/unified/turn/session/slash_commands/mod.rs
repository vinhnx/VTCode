use std::sync::Arc;

use anyhow::Result;
use tokio::sync::{Notify, RwLock};

// use vtcode_core::commands::init::{GenerateAgentsFileStatus, generate_agents_file};
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::core::decision_tracker::DecisionTracker;
use vtcode_core::llm::provider as uni;
use vtcode_core::tools::ToolRegistry;

// use vtcode_core::ui::theme;
use vtcode_core::utils::ansi::AnsiRenderer;
use vtcode_tui::{InlineHandle, InlineSession};
// use vtcode_core::utils::session_archive;
// use vtcode_core::utils::transcript;

// use super::super::workspace::{bootstrap_config_files, build_workspace_index};
use crate::agent::runloop::mcp_events;
use crate::agent::runloop::model_picker::ModelPickerState;
// use crate::agent::runloop::slash_commands::McpCommandAction;
pub use crate::agent::runloop::slash_commands::SlashCommandOutcome;
use crate::agent::runloop::unified::async_mcp_manager::AsyncMcpManager;
use crate::agent::runloop::unified::context_manager::ContextManager;
use crate::agent::runloop::unified::palettes::ActivePalette;
use crate::agent::runloop::unified::state::{CtrlCState, SessionStats};
use crate::agent::runloop::unified::workspace_links::LinkedDirectory;
use crate::agent::runloop::welcome::SessionBootstrap;
use crate::hooks::lifecycle::{LifecycleHookEngine, SessionEndReason};

pub mod handlers;

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
    #[allow(dead_code)]
    pub context_manager: &'a mut ContextManager,
    pub session_stats: &'a mut SessionStats,
    pub tools: &'a Arc<RwLock<Vec<uni::ToolDefinition>>>,
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
    pub checkpoint_manager: Option<&'a vtcode_core::core::agent::snapshots::SnapshotManager>,
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
        SlashCommandOutcome::ThemeChanged(theme_id) => {
            handlers::handle_theme_changed(ctx, theme_id).await
        }
        SlashCommandOutcome::StartThemePalette { mode } => {
            handlers::handle_start_theme_palette(ctx, mode).await
        }
        SlashCommandOutcome::StartSessionsPalette { limit } => {
            handlers::handle_start_sessions_palette(ctx, limit).await
        }
        SlashCommandOutcome::StartFileBrowser { initial_filter } => {
            handlers::handle_start_file_browser(ctx, initial_filter).await
        }
        SlashCommandOutcome::StartModelSelection => {
            handlers::handle_start_model_selection(ctx).await
        }
        SlashCommandOutcome::InitializeWorkspace { force } => {
            handlers::handle_initialize_workspace(ctx, force).await
        }
        SlashCommandOutcome::GenerateAgentFile { overwrite } => {
            handlers::handle_generate_agent_file(ctx, overwrite).await
        }
        SlashCommandOutcome::ShowConfig => handlers::handle_show_config(ctx).await,
        SlashCommandOutcome::ExecuteTool { name, args } => {
            handlers::handle_execute_tool(ctx, name, args).await
        }
        SlashCommandOutcome::ClearScreen => handlers::handle_clear_screen(ctx).await,
        SlashCommandOutcome::ClearConversation => handlers::handle_clear_conversation(ctx).await,
        SlashCommandOutcome::CopyLatestAssistantReply => {
            handlers::handle_copy_latest_assistant_reply(ctx).await
        }
        SlashCommandOutcome::ShowStatus => handlers::handle_show_status(ctx).await,
        SlashCommandOutcome::ManageMcp { action } => handlers::handle_manage_mcp(ctx, action).await,

        SlashCommandOutcome::RunDoctor => handlers::handle_run_doctor(ctx).await,
        SlashCommandOutcome::StartTerminalSetup => handlers::handle_start_terminal_setup(ctx).await,
        SlashCommandOutcome::ManageWorkspaceDirectories { command } => {
            handlers::handle_manage_workspace_directories(ctx, command).await
        }
        SlashCommandOutcome::NewSession => handlers::handle_new_session(ctx).await,
        SlashCommandOutcome::OpenDocs => handlers::handle_open_docs(ctx).await,
        SlashCommandOutcome::LaunchEditor { file } => {
            handlers::handle_launch_editor(ctx, file).await
        }
        SlashCommandOutcome::LaunchGit => handlers::handle_launch_git(ctx).await,
        SlashCommandOutcome::ManageSkills { action } => {
            handlers::handle_manage_skills(ctx, action).await
        }
        SlashCommandOutcome::ManageAgents { action } => {
            handlers::handle_manage_agents(ctx, action).await
        }
        SlashCommandOutcome::ManageTeams { action } => {
            handlers::handle_manage_teams(ctx, action).await
        }
        SlashCommandOutcome::ManageSubagentConfig { action } => {
            handlers::handle_manage_subagent_config(ctx, action).await
        }
        SlashCommandOutcome::RewindToTurn { turn, scope } => {
            handlers::handle_rewind_to_turn(ctx, turn, scope).await
        }
        SlashCommandOutcome::RewindLatest { scope } => {
            handlers::handle_rewind_latest(ctx, scope).await
        }
        SlashCommandOutcome::TogglePlanMode { enable, prompt } => {
            let control = handlers::handle_toggle_plan_mode(ctx, enable).await?;
            if matches!(control, SlashCommandControl::Continue)
                && let Some(prompt) = prompt
            {
                return Ok(SlashCommandControl::SubmitPrompt(prompt));
            }
            Ok(control)
        }
        SlashCommandOutcome::ToggleAutonomous { enable } => {
            handlers::handle_toggle_autonomous_mode(ctx, enable).await
        }
        SlashCommandOutcome::CycleMode => handlers::handle_cycle_mode(ctx).await,
        SlashCommandOutcome::OAuthLogin { provider } => {
            handlers::handle_oauth_login(ctx, provider).await
        }
        SlashCommandOutcome::OAuthLogout { provider } => {
            handlers::handle_oauth_logout(ctx, provider).await
        }
        SlashCommandOutcome::ShowAuthStatus { provider } => {
            handlers::handle_show_auth_status(ctx, provider).await
        }
        SlashCommandOutcome::ShareLog { format } => handlers::handle_share_log(ctx, format).await,
        SlashCommandOutcome::Exit => handlers::handle_exit(ctx).await,
    }
}
