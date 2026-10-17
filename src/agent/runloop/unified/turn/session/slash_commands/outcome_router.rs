use anyhow::Result;

use crate::agent::runloop::slash_commands::SlashCommandOutcome;

use super::{SlashCommandContext, SlashCommandControl, handlers, secrets};

pub(super) async fn route_outcome(
    outcome: SlashCommandOutcome,
    ctx: SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    match outcome {
        SlashCommandOutcome::SubmitPrompt { prompt } => Ok(SlashCommandControl::SubmitPrompt(prompt)),
        SlashCommandOutcome::ReplaceInput { content } => Ok(SlashCommandControl::ReplaceInput(content)),
        SlashCommandOutcome::Handled => Ok(SlashCommandControl::Continue),
        SlashCommandOutcome::ThemeChanged(_)
        | SlashCommandOutcome::StartThemePalette { .. }
        | SlashCommandOutcome::StartSessionPalette { .. }
        | SlashCommandOutcome::StartHistoryPicker
        | SlashCommandOutcome::StartFileBrowser { .. }
        | SlashCommandOutcome::StartStatuslineSetup { .. }
        | SlashCommandOutcome::StartTerminalTitleSetup
        | SlashCommandOutcome::StartModelSelection
        | SlashCommandOutcome::ToggleFastMode { .. }
        | SlashCommandOutcome::StartModePalette
        | SlashCommandOutcome::SelectPrimaryAgent { .. }
        | SlashCommandOutcome::ToggleVimMode { .. }
        | SlashCommandOutcome::InitializeWorkspace { .. }
        | SlashCommandOutcome::ShowSettings
        | SlashCommandOutcome::ShowSettingsReset
        | SlashCommandOutcome::ShowSettingsAtPath { .. }
        | SlashCommandOutcome::ShowMemoryConfig
        | SlashCommandOutcome::ShowPermissions => route_ui_and_settings_outcome(outcome, ctx).await,
        outcome @ (SlashCommandOutcome::ClearScreen
        | SlashCommandOutcome::ClearConversation
        | SlashCommandOutcome::CompactConversation { .. }
        | SlashCommandOutcome::CopyLatestAssistantReply
        | SlashCommandOutcome::ToggleTasksPanel
        | SlashCommandOutcome::ShowJobsPanel
        | SlashCommandOutcome::ShowStatus
        | SlashCommandOutcome::ShowTranscriptStats
        | SlashCommandOutcome::Explain { .. }
        | SlashCommandOutcome::ExportTranscript { .. }
        | SlashCommandOutcome::ShowWebmcpStatus
        | SlashCommandOutcome::ShowWebmcpHelp
        | SlashCommandOutcome::StartWebmcp { .. }
        | SlashCommandOutcome::StopWebmcp
        | SlashCommandOutcome::ShowLogViewer { .. }
        | SlashCommandOutcome::Notify { .. }
        | SlashCommandOutcome::StopAgent
        | SlashCommandOutcome::ManageMcp { .. }
        | SlashCommandOutcome::StartCheckupInteractive
        | SlashCommandOutcome::RunCheckup { .. }
        | SlashCommandOutcome::Update { .. }
        | SlashCommandOutcome::StartTerminalSetup
        | SlashCommandOutcome::ManageLocalServer { .. }) => route_runtime_outcome(outcome, ctx).await,
        outcome @ (SlashCommandOutcome::NewSession
        | SlashCommandOutcome::OpenDocs
        | SlashCommandOutcome::OpenFeedback
        | SlashCommandOutcome::OpenDonateLinks
        | SlashCommandOutcome::ManageSkills { .. }
        | SlashCommandOutcome::ManagePlugins { .. }
        | SlashCommandOutcome::ManageAgents { .. }
        | SlashCommandOutcome::ManageSubprocesses { .. }
        | SlashCommandOutcome::OpenRewindPicker
        | SlashCommandOutcome::Redo
        | SlashCommandOutcome::RewindToTurn { .. }
        | SlashCommandOutcome::RewindLatest { .. }
        | SlashCommandOutcome::ShareLog { .. }
        | SlashCommandOutcome::Exit) => route_navigation_outcome(outcome, ctx).await,
        outcome @ (SlashCommandOutcome::TogglePlanningWorkflow { .. }
        | SlashCommandOutcome::OAuthLogin { .. }
        | SlashCommandOutcome::StartOAuthProviderPicker { .. }
        | SlashCommandOutcome::OAuthLogout { .. }
        | SlashCommandOutcome::RefreshOAuth { .. }
        | SlashCommandOutcome::ShowAuthStatus { .. }
        | SlashCommandOutcome::ManageSecrets { .. }) => route_mode_and_auth_outcome(outcome, ctx).await,
    }
}

async fn route_ui_and_settings_outcome(
    outcome: SlashCommandOutcome,
    ctx: SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    match outcome {
        SlashCommandOutcome::ThemeChanged(theme_id) => handlers::handle_theme_changed(ctx, theme_id).await,
        SlashCommandOutcome::StartThemePalette { mode } => handlers::handle_start_theme_palette(ctx, mode).await,
        SlashCommandOutcome::StartSessionPalette { mode, limit, show_all } => {
            handlers::handle_start_session_palette(ctx, mode, limit, show_all).await
        }
        SlashCommandOutcome::StartHistoryPicker => handlers::handle_start_history_picker(ctx).await,
        SlashCommandOutcome::StartFileBrowser { initial_filter } => {
            handlers::handle_start_file_browser(ctx, initial_filter).await
        }
        SlashCommandOutcome::StartStatuslineSetup { instructions } => {
            handlers::handle_start_statusline_setup(ctx, instructions).await
        }
        SlashCommandOutcome::StartTerminalTitleSetup => handlers::handle_start_terminal_title_setup(ctx).await,
        SlashCommandOutcome::StartModelSelection => handlers::handle_start_model_selection(ctx).await,
        SlashCommandOutcome::ToggleFastMode { enable } => handlers::handle_toggle_fast_mode(ctx, enable).await,
        SlashCommandOutcome::StartModePalette => handlers::handle_start_mode_palette(ctx).await,
        SlashCommandOutcome::SelectPrimaryAgent { name } => {
            handlers::handle_select_primary_agent_from_slash(ctx, &name).await
        }
        SlashCommandOutcome::ToggleVimMode { enable } => handlers::handle_toggle_vim_mode(ctx, enable).await,
        SlashCommandOutcome::InitializeWorkspace { force } => handlers::handle_initialize_workspace(ctx, force).await,
        SlashCommandOutcome::ShowSettings => handlers::handle_show_settings(ctx).await,
        SlashCommandOutcome::ShowSettingsReset => handlers::handle_show_settings_reset(ctx).await,
        SlashCommandOutcome::ShowSettingsAtPath { path } => {
            handlers::handle_show_settings_at_path(ctx, Some(&path)).await
        }
        SlashCommandOutcome::ShowMemoryConfig => handlers::handle_show_memory_config(ctx).await,
        SlashCommandOutcome::ShowPermissions => handlers::handle_show_permissions(ctx).await,
        _ => anyhow::bail!("unexpected ui/settings outcome"),
    }
}

async fn route_runtime_outcome(
    outcome: SlashCommandOutcome,
    ctx: SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    match outcome {
        SlashCommandOutcome::ClearScreen => handlers::handle_clear_screen(ctx).await,
        SlashCommandOutcome::ClearConversation => handlers::handle_clear_conversation(ctx).await,
        SlashCommandOutcome::ShowTranscriptStats => handlers::handle_show_transcript_stats(ctx).await,
        SlashCommandOutcome::Explain { args } => super::explain::handle_explain(ctx, &args).await,
        SlashCommandOutcome::ExportTranscript { path } => handlers::handle_export_transcript(ctx, path).await,
        SlashCommandOutcome::CompactConversation { command } => {
            handlers::handle_compact_conversation(ctx, command).await
        }
        SlashCommandOutcome::CopyLatestAssistantReply => handlers::handle_copy_latest_assistant_reply(ctx).await,
        SlashCommandOutcome::ToggleTasksPanel => handlers::handle_toggle_tasks_panel(ctx).await,
        SlashCommandOutcome::ShowJobsPanel => handlers::handle_show_jobs_panel(ctx).await,
        SlashCommandOutcome::ShowStatus => handlers::handle_show_status(ctx).await,
        SlashCommandOutcome::ShowWebmcpStatus => handlers::handle_show_webmcp_status(ctx).await,
        SlashCommandOutcome::ShowWebmcpHelp => handlers::handle_show_webmcp_help(ctx).await,
        SlashCommandOutcome::StartWebmcp { origin, replace } => {
            handlers::handle_start_webmcp(ctx, origin, replace).await
        }
        SlashCommandOutcome::StopWebmcp => handlers::handle_stop_webmcp(ctx).await,
        SlashCommandOutcome::ShowLogViewer { format, scope, save } => {
            handlers::handle_show_log_viewer(ctx, format, scope, save).await
        }
        SlashCommandOutcome::Notify { message } => handlers::handle_notify(ctx, message).await,
        SlashCommandOutcome::StopAgent => handlers::handle_stop_agent(ctx).await,
        SlashCommandOutcome::ManageMcp { action } => handlers::handle_manage_mcp(ctx, action).await,
        SlashCommandOutcome::StartCheckupInteractive => handlers::handle_start_checkup_interactive(ctx).await,
        SlashCommandOutcome::RunCheckup { quick } => handlers::handle_run_checkup(ctx, quick).await,
        SlashCommandOutcome::Update { check_only, install, force } => {
            handlers::handle_update(ctx, check_only, install, force).await
        }
        SlashCommandOutcome::StartTerminalSetup => handlers::handle_start_terminal_setup(ctx).await,
        SlashCommandOutcome::ManageLocalServer { action } => handlers::handle_manage_local_server(ctx, action).await,
        _ => anyhow::bail!("unexpected runtime outcome"),
    }
}

async fn route_navigation_outcome(
    outcome: SlashCommandOutcome,
    ctx: SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    match outcome {
        SlashCommandOutcome::NewSession => handlers::handle_new_session(ctx).await,
        SlashCommandOutcome::OpenDocs => handlers::handle_open_docs(ctx).await,
        SlashCommandOutcome::OpenFeedback => handlers::handle_open_feedback(ctx).await,
        SlashCommandOutcome::OpenDonateLinks => handlers::handle_open_donate_links(ctx).await,
        SlashCommandOutcome::ManageSkills { action } => handlers::handle_manage_skills(ctx, action).await,
        SlashCommandOutcome::ManagePlugins { action } => handlers::handle_manage_plugins(ctx, action).await,
        SlashCommandOutcome::ManageAgents { action } => handlers::handle_manage_agents(ctx, action).await,
        SlashCommandOutcome::ManageSubprocesses { action } => handlers::handle_manage_subprocesses(ctx, action).await,
        SlashCommandOutcome::OpenRewindPicker => handlers::handle_open_rewind_picker(ctx).await,
        SlashCommandOutcome::Redo => handlers::handle_redo(ctx).await,
        SlashCommandOutcome::RewindToTurn { turn, scope } => handlers::handle_rewind_to_turn(ctx, turn, scope).await,
        SlashCommandOutcome::RewindLatest { scope } => handlers::handle_rewind_latest(ctx, scope).await,
        SlashCommandOutcome::ShareLog { format } => handlers::handle_share_log(ctx, format).await,
        SlashCommandOutcome::Exit => handlers::handle_exit(ctx).await,
        _ => anyhow::bail!("unexpected navigation outcome"),
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
        SlashCommandOutcome::OAuthLogin { provider } => handlers::handle_oauth_login(ctx, provider).await,
        SlashCommandOutcome::StartOAuthProviderPicker { action } => {
            handlers::handle_start_oauth_provider_picker(ctx, action).await
        }
        SlashCommandOutcome::OAuthLogout { provider } => handlers::handle_oauth_logout(ctx, provider).await,
        SlashCommandOutcome::RefreshOAuth { provider } => handlers::handle_refresh_oauth(ctx, provider).await,
        SlashCommandOutcome::ShowAuthStatus { provider } => handlers::handle_show_auth_status(ctx, provider).await,
        SlashCommandOutcome::ManageSecrets { action } => secrets::handle_manage_secrets(ctx, action).await,
        _ => anyhow::bail!("unexpected mode/auth outcome"),
    }
}
