use anyhow::Result;

use crate::agent::runloop::slash_commands::SlashCommandOutcome;

use super::{SlashCommandContext, SlashCommandControl, handlers};

pub(super) async fn route_outcome(
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
        outcome @ (SlashCommandOutcome::ThemeChanged(_)
        | SlashCommandOutcome::StartThemePalette { .. }
        | SlashCommandOutcome::StartSessionPalette { .. }
        | SlashCommandOutcome::StartHistoryPicker
        | SlashCommandOutcome::StartFileBrowser { .. }
        | SlashCommandOutcome::StartStatuslineSetup { .. }
        | SlashCommandOutcome::StartTerminalTitleSetup
        | SlashCommandOutcome::StartModelSelection
        | SlashCommandOutcome::SetEffort { .. }
        | SlashCommandOutcome::ToggleIdeContext
        | SlashCommandOutcome::InitializeWorkspace { .. }
        | SlashCommandOutcome::ShowSettings
        | SlashCommandOutcome::ShowSettingsAtPath { .. }
        | SlashCommandOutcome::ShowHooks
        | SlashCommandOutcome::ShowMemoryConfig
        | SlashCommandOutcome::ShowPermissions
        | SlashCommandOutcome::ShowMemory) => route_ui_and_settings_outcome(outcome, ctx).await,
        outcome @ (SlashCommandOutcome::ClearScreen
        | SlashCommandOutcome::ClearConversation
        | SlashCommandOutcome::CompactConversation { .. }
        | SlashCommandOutcome::CopyLatestAssistantReply
        | SlashCommandOutcome::TriggerPromptSuggestions
        | SlashCommandOutcome::ToggleTasksPanel
        | SlashCommandOutcome::ShowJobsPanel
        | SlashCommandOutcome::ShowStatus
        | SlashCommandOutcome::Notify { .. }
        | SlashCommandOutcome::StopAgent
        | SlashCommandOutcome::ManageMcp { .. }
        | SlashCommandOutcome::StartDoctorInteractive
        | SlashCommandOutcome::RunDoctor { .. }
        | SlashCommandOutcome::Update { .. }
        | SlashCommandOutcome::StartTerminalSetup
        | SlashCommandOutcome::ManageLoop { .. }
        | SlashCommandOutcome::ManageSchedule { .. }
        | SlashCommandOutcome::ManageLocalServer { .. }) => {
            route_runtime_outcome(outcome, ctx).await
        }
        outcome @ (SlashCommandOutcome::NewSession
        | SlashCommandOutcome::OpenDocs
        | SlashCommandOutcome::OpenDonateLinks
        | SlashCommandOutcome::LaunchEditor { .. }
        | SlashCommandOutcome::LaunchGit
        | SlashCommandOutcome::ManageSkills { .. }
        | SlashCommandOutcome::ManageAgents { .. }
        | SlashCommandOutcome::ManageSubprocesses { .. }
        | SlashCommandOutcome::OpenRewindPicker
        | SlashCommandOutcome::RewindToTurn { .. }
        | SlashCommandOutcome::RewindLatest { .. }
        | SlashCommandOutcome::ShareLog { .. }
        | SlashCommandOutcome::Exit) => route_navigation_outcome(outcome, ctx).await,
        outcome @ (SlashCommandOutcome::TogglePlanningWorkflow { .. }
        | SlashCommandOutcome::OAuthLogin { .. }
        | SlashCommandOutcome::StartOAuthProviderPicker { .. }
        | SlashCommandOutcome::OAuthLogout { .. }
        | SlashCommandOutcome::RefreshOAuth { .. }
        | SlashCommandOutcome::ShowAuthStatus { .. }) => {
            route_mode_and_auth_outcome(outcome, ctx).await
        }
    }
}

async fn route_ui_and_settings_outcome(
    outcome: SlashCommandOutcome,
    ctx: SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    match outcome {
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
        SlashCommandOutcome::StartStatuslineSetup { instructions } => {
            handlers::handle_start_statusline_setup(ctx, instructions).await
        }
        SlashCommandOutcome::StartTerminalTitleSetup => {
            handlers::handle_start_terminal_title_setup(ctx).await
        }
        SlashCommandOutcome::StartModelSelection => {
            handlers::handle_start_model_selection(ctx).await
        }
        SlashCommandOutcome::SetEffort { level, persist } => {
            handlers::handle_set_effort(ctx, level, persist).await
        }
        SlashCommandOutcome::ToggleIdeContext => handlers::handle_toggle_ide_context(ctx).await,
        SlashCommandOutcome::InitializeWorkspace { force } => {
            handlers::handle_initialize_workspace(ctx, force).await
        }
        SlashCommandOutcome::ShowSettings => handlers::handle_show_settings(ctx).await,
        SlashCommandOutcome::ShowSettingsAtPath { path } => {
            handlers::handle_show_settings_at_path(ctx, Some(&path)).await
        }
        SlashCommandOutcome::ShowHooks => handlers::handle_show_hooks(ctx).await,
        SlashCommandOutcome::ShowMemoryConfig => handlers::handle_show_memory_config(ctx).await,
        SlashCommandOutcome::ShowPermissions => handlers::handle_show_permissions(ctx).await,
        SlashCommandOutcome::ShowMemory => handlers::handle_show_memory(ctx).await,
        _ => unreachable!("unexpected ui/settings outcome"),
    }
}

async fn route_runtime_outcome(
    outcome: SlashCommandOutcome,
    ctx: SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    match outcome {
        SlashCommandOutcome::ClearScreen => handlers::handle_clear_screen(ctx).await,
        SlashCommandOutcome::ClearConversation => handlers::handle_clear_conversation(ctx).await,
        SlashCommandOutcome::CompactConversation { command } => {
            handlers::handle_compact_conversation(ctx, command).await
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
        SlashCommandOutcome::Notify { message } => handlers::handle_notify(ctx, message).await,
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
        SlashCommandOutcome::ManageLoop { command } => {
            handlers::handle_manage_loop(ctx, command).await
        }
        SlashCommandOutcome::ManageSchedule { action } => {
            handlers::handle_manage_schedule(ctx, action).await
        }
        SlashCommandOutcome::ManageLocalServer { action } => {
            handlers::handle_manage_local_server(ctx, action).await
        }
        _ => unreachable!("unexpected runtime outcome"),
    }
}

async fn route_navigation_outcome(
    outcome: SlashCommandOutcome,
    ctx: SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    match outcome {
        SlashCommandOutcome::NewSession => handlers::handle_new_session(ctx).await,
        SlashCommandOutcome::OpenDocs => handlers::handle_open_docs(ctx).await,
        SlashCommandOutcome::OpenDonateLinks => handlers::handle_open_donate_links(ctx).await,
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
        SlashCommandOutcome::ManageSubprocesses { action } => {
            handlers::handle_manage_subprocesses(ctx, action).await
        }
        SlashCommandOutcome::OpenRewindPicker => handlers::handle_open_rewind_picker(ctx).await,
        SlashCommandOutcome::RewindToTurn { turn, scope } => {
            handlers::handle_rewind_to_turn(ctx, turn, scope).await
        }
        SlashCommandOutcome::RewindLatest { scope } => {
            handlers::handle_rewind_latest(ctx, scope).await
        }
        SlashCommandOutcome::ShareLog { format } => handlers::handle_share_log(ctx, format).await,
        SlashCommandOutcome::Exit => handlers::handle_exit(ctx).await,
        _ => unreachable!("unexpected navigation outcome"),
    }
}

async fn route_mode_and_auth_outcome(
    outcome: SlashCommandOutcome,
    ctx: SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    match outcome {
        SlashCommandOutcome::TogglePlanningWorkflow { enable, prompt } => {
            let control = handlers::handle_toggle_planning_workflow(ctx, enable).await?;
            if matches!(control, SlashCommandControl::Continue)
                && let Some(prompt) = prompt
            {
                return Ok(SlashCommandControl::SubmitPrompt(prompt));
            }
            Ok(control)
        }
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
        _ => unreachable!("unexpected mode/auth outcome"),
    }
}
