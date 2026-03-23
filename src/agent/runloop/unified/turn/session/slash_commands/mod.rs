use std::sync::Arc;

use anyhow::Result;
use tokio::sync::{Notify, RwLock};

use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::core::decision_tracker::DecisionTracker;
use vtcode_core::hooks::SessionEndReason;
use vtcode_core::llm::provider as uni;
use vtcode_core::tools::ToolRegistry;
use vtcode_tui::app::{InlineHandle, InlineHeaderContext, InlineSession};

use crate::agent::runloop::mcp_events;
use crate::agent::runloop::model_picker::ModelPickerState;
pub(crate) use crate::agent::runloop::slash_commands::SlashCommandOutcome;
use crate::agent::runloop::unified::async_mcp_manager::AsyncMcpManager;
use crate::agent::runloop::unified::context_manager::ContextManager;
use crate::agent::runloop::unified::palettes::ActivePalette;
use crate::agent::runloop::unified::session_setup::IdeContextBridge;
use crate::agent::runloop::unified::state::{CtrlCState, SessionStats};
use crate::agent::runloop::unified::status_line::InputStatusState;
use crate::agent::runloop::unified::tool_catalog::ToolCatalogState;
use crate::agent::runloop::unified::workspace_links::LinkedDirectory;
use crate::agent::runloop::welcome::SessionBootstrap;
use vtcode_core::utils::ansi::AnsiRenderer;

mod handlers;

pub(crate) enum SlashCommandControl {
    Continue,
    SubmitPrompt(String),
    ReplaceInput(String),
    BreakWithReason(SessionEndReason),
}

pub(crate) struct SlashCommandContext<'a> {
    pub(crate) renderer: &'a mut AnsiRenderer,
    pub(crate) handle: &'a InlineHandle,
    pub(crate) session: &'a mut InlineSession,
    pub(crate) header_context: &'a mut InlineHeaderContext,
    pub(crate) ide_context_bridge: &'a mut Option<IdeContextBridge>,
    pub(crate) config: &'a mut CoreAgentConfig,
    pub(crate) vt_cfg: &'a mut Option<VTCodeConfig>,
    pub(crate) provider_client: &'a mut Box<dyn uni::LLMProvider>,
    pub(crate) session_bootstrap: &'a SessionBootstrap,
    pub(crate) model_picker_state: &'a mut Option<ModelPickerState>,
    pub(crate) palette_state: &'a mut Option<ActivePalette>,
    pub(crate) tool_registry: &'a mut ToolRegistry,
    pub(crate) conversation_history: &'a mut Vec<uni::Message>,
    pub(crate) decision_ledger: &'a Arc<RwLock<DecisionTracker>>,
    pub(crate) context_manager: &'a mut ContextManager,
    pub(crate) session_stats: &'a mut SessionStats,
    pub(crate) input_status_state: &'a mut InputStatusState,
    pub(crate) tools: &'a Arc<RwLock<Vec<uni::ToolDefinition>>>,
    pub(crate) tool_catalog: &'a Arc<ToolCatalogState>,
    pub(crate) async_mcp_manager: Option<&'a Arc<AsyncMcpManager>>,
    pub(crate) mcp_panel_state: &'a mut mcp_events::McpPanelState,
    pub(crate) linked_directories: &'a mut Vec<LinkedDirectory>,
    pub(crate) ctrl_c_state: &'a Arc<CtrlCState>,
    pub(crate) ctrl_c_notify: &'a Arc<Notify>,
    pub(crate) full_auto: bool,
    pub(crate) loaded_skills:
        &'a Arc<RwLock<hashbrown::HashMap<String, vtcode_core::skills::types::Skill>>>,
    pub(crate) checkpoint_manager: Option<&'a vtcode_core::core::agent::snapshots::SnapshotManager>,
}

pub(crate) async fn handle_outcome(
    outcome: SlashCommandOutcome,
    ctx: SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    match outcome {
        SlashCommandOutcome::SubmitPrompt { prompt } => {
            Ok(SlashCommandControl::SubmitPrompt(prompt))
        }
        SlashCommandOutcome::ReplaceInput { content } => {
            Ok(SlashCommandControl::ReplaceInput(content))
        }
        SlashCommandOutcome::Handled => Ok(SlashCommandControl::Continue),
        SlashCommandOutcome::ThemeChanged(theme_id) => {
            handlers::handle_theme_changed(ctx, theme_id).await
        }
        SlashCommandOutcome::StartThemePalette { mode } => {
            handlers::handle_start_theme_palette(ctx, mode).await
        }
        SlashCommandOutcome::StartSessionPalette {
            mode,
            limit,
            show_all,
        } => handlers::handle_start_session_palette(ctx, mode, limit, show_all).await,
        SlashCommandOutcome::StartHistoryPicker => handlers::handle_start_history_picker(ctx).await,
        SlashCommandOutcome::StartFileBrowser { initial_filter } => {
            handlers::handle_start_file_browser(ctx, initial_filter).await
        }
        SlashCommandOutcome::ToggleVimMode { enable } => {
            handlers::handle_toggle_vim_mode(ctx, enable).await
        }
        SlashCommandOutcome::StartStatuslineSetup { instructions } => {
            handlers::handle_start_statusline_setup(ctx, instructions).await
        }
        SlashCommandOutcome::StartModelSelection => {
            handlers::handle_start_model_selection(ctx).await
        }
        SlashCommandOutcome::ToggleIdeContext => handlers::handle_toggle_ide_context(ctx).await,
        SlashCommandOutcome::InitializeWorkspace { force } => {
            handlers::handle_initialize_workspace(ctx, force).await
        }
        SlashCommandOutcome::ShowSettings => handlers::handle_show_settings(ctx).await,
        SlashCommandOutcome::ShowPermissions => handlers::handle_show_permissions(ctx).await,
        SlashCommandOutcome::ClearScreen => handlers::handle_clear_screen(ctx).await,
        SlashCommandOutcome::ClearConversation => handlers::handle_clear_conversation(ctx).await,
        SlashCommandOutcome::CompactConversation => {
            handlers::handle_compact_conversation(ctx).await
        }
        SlashCommandOutcome::CopyLatestAssistantReply => {
            handlers::handle_copy_latest_assistant_reply(ctx).await
        }
        SlashCommandOutcome::TriggerPromptSuggestions => {
            handlers::handle_trigger_prompt_suggestions(ctx).await
        }
        SlashCommandOutcome::ToggleTasksPanel => handlers::handle_toggle_tasks_panel(ctx).await,
        SlashCommandOutcome::ShowJobsPanel => handlers::handle_show_jobs_panel(ctx).await,
        SlashCommandOutcome::ShowStatus => handlers::handle_show_status(ctx).await,
        SlashCommandOutcome::StopAgent => handlers::handle_stop_agent(ctx).await,
        SlashCommandOutcome::ManageMcp { action } => handlers::handle_manage_mcp(ctx, action).await,

        SlashCommandOutcome::StartDoctorInteractive => {
            handlers::handle_start_doctor_interactive(ctx).await
        }
        SlashCommandOutcome::RunDoctor { quick } => handlers::handle_run_doctor(ctx, quick).await,
        SlashCommandOutcome::Update {
            check_only,
            install,
            force,
        } => handlers::handle_update(ctx, check_only, install, force).await,
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
        SlashCommandOutcome::OpenRewindPicker => handlers::handle_open_rewind_picker(ctx).await,
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
        SlashCommandOutcome::StartModeSelection => handlers::handle_start_mode_selection(ctx).await,
        SlashCommandOutcome::SetMode { mode } => handlers::handle_set_mode(ctx, mode).await,
        SlashCommandOutcome::CycleMode => handlers::handle_cycle_mode(ctx).await,
        SlashCommandOutcome::OAuthLogin { provider } => {
            handlers::handle_oauth_login(ctx, provider).await
        }
        SlashCommandOutcome::StartOAuthProviderPicker { action } => {
            handlers::handle_start_oauth_provider_picker(ctx, action).await
        }
        SlashCommandOutcome::OAuthLogout { provider } => {
            handlers::handle_oauth_logout(ctx, provider).await
        }
        SlashCommandOutcome::RefreshOAuth { provider } => {
            handlers::handle_refresh_oauth(ctx, provider).await
        }
        SlashCommandOutcome::ShowAuthStatus { provider } => {
            handlers::handle_show_auth_status(ctx, provider).await
        }
        SlashCommandOutcome::ShareLog { format } => handlers::handle_share_log(ctx, format).await,
        SlashCommandOutcome::Exit => handlers::handle_exit(ctx).await,
    }
}
