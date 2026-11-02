use super::*;

#[tokio::test]
async fn notify_command_uses_default_message() {
    let workspace = std::env::current_dir().expect("workspace");
    let mut renderer = renderer_for_tests();

    let outcome = handle_slash_command("config notify", &mut renderer, &workspace)
        .await
        .expect("notify command should parse");

    assert!(matches!(
        outcome,
        SlashCommandOutcome::Notify { ref message }
        if message == "Manual notification from /config notify"
    ));
}
#[tokio::test]
async fn notify_command_preserves_custom_message() {
    let workspace = std::env::current_dir().expect("workspace");
    let mut renderer = renderer_for_tests();

    let outcome = handle_slash_command("config notify build finished", &mut renderer, &workspace)
        .await
        .expect("notify command should parse");

    assert!(matches!(
        outcome,
        SlashCommandOutcome::Notify { ref message }
        if message == "build finished"
    ));
}
#[tokio::test]
async fn config_memory_opens_memory_controls() {
    let workspace = std::env::current_dir().expect("workspace");
    let mut renderer = renderer_for_tests();

    let outcome = handle_slash_command("config memory", &mut renderer, &workspace)
        .await
        .expect("config memory command should parse");

    assert!(matches!(outcome, SlashCommandOutcome::ShowMemoryConfig));
}
#[tokio::test]
async fn config_model_opens_model_settings_tree() {
    let workspace = std::env::current_dir().expect("workspace");
    let mut renderer = renderer_for_tests();

    let outcome = handle_slash_command("config model", &mut renderer, &workspace)
        .await
        .expect("config model command should parse");

    assert!(matches!(
        outcome,
        SlashCommandOutcome::ShowSettingsAtPath { ref path } if path == "model"
    ));
}
#[tokio::test]
async fn config_reset_opens_confirmed_settings_reset_flow() {
    let workspace = std::env::current_dir().expect("workspace");
    let mut renderer = renderer_for_tests();

    let outcome = handle_slash_command("config reset", &mut renderer, &workspace)
        .await
        .expect("config reset command should parse");

    assert!(matches!(outcome, SlashCommandOutcome::ShowSettingsReset));
}
#[tokio::test]
async fn settings_alias_accepts_config_reset() {
    let workspace = std::env::current_dir().expect("workspace");
    let mut renderer = renderer_for_tests();

    let outcome = handle_slash_command("settings reset", &mut renderer, &workspace)
        .await
        .expect("settings alias should parse");

    assert!(matches!(outcome, SlashCommandOutcome::ShowSettingsReset));
}
#[tokio::test]
async fn config_reset_rejects_extra_arguments() {
    let workspace = std::env::current_dir().expect("workspace");
    let mut renderer = renderer_for_tests();

    let outcome = handle_slash_command("config reset now", &mut renderer, &workspace)
        .await
        .expect("invalid config reset should be handled");

    assert!(matches!(outcome, SlashCommandOutcome::Handled));
}
#[tokio::test]
async fn config_ide_rejects_arguments() {
    let workspace = std::env::current_dir().expect("workspace");
    let mut renderer = renderer_for_tests();

    let outcome = handle_slash_command("config ide extra", &mut renderer, &workspace)
        .await
        .expect("ide command should parse");

    assert!(matches!(outcome, SlashCommandOutcome::Handled));
}
#[tokio::test]
async fn agent_command_opens_manager_and_inspector() {
    let workspace = std::env::current_dir().expect("workspace");
    let mut renderer = renderer_for_tests();

    let default = handle_slash_command("agent", &mut renderer, &workspace)
        .await
        .expect("agent command should parse");
    assert!(matches!(default, SlashCommandOutcome::ManageAgents { action: AgentManagerAction::List }));

    let threads = handle_slash_command("agent threads", &mut renderer, &workspace)
        .await
        .expect("agent threads should parse");
    assert!(matches!(threads, SlashCommandOutcome::ManageAgents { action: AgentManagerAction::Threads }));
}
#[tokio::test]
async fn primary_agent_command_is_not_registered() {
    let workspace = std::env::current_dir().expect("workspace");
    let mut renderer = renderer_for_tests();

    let outcome = handle_slash_command("primary-agent list", &mut renderer, &workspace)
        .await
        .expect("primary-agent should fall through");

    assert!(matches!(
        outcome,
        SlashCommandOutcome::SubmitPrompt { ref prompt } if prompt == "/primary-agent list"
    ));
}
#[tokio::test]
async fn agent_command_supports_direct_inspect_and_close() {
    let workspace = std::env::current_dir().expect("workspace");
    let mut renderer = renderer_for_tests();

    let inspect = handle_slash_command("agent inspect thread-1", &mut renderer, &workspace)
        .await
        .expect("agent inspect should parse");
    assert!(matches!(
        inspect,
        SlashCommandOutcome::ManageAgents {
            action: AgentManagerAction::Inspect { ref id }
        } if id == "thread-1"
    ));

    let close = handle_slash_command("agent close thread-1", &mut renderer, &workspace)
        .await
        .expect("agent close should parse");
    assert!(matches!(
        close,
        SlashCommandOutcome::ManageAgents {
            action: AgentManagerAction::Close { ref id }
        } if id == "thread-1"
    ));
}
#[tokio::test]
async fn agents_create_and_edit_commands_parse_guided_forms() {
    let workspace = std::env::current_dir().expect("workspace");
    let mut renderer = renderer_for_tests();

    let create_default = handle_slash_command("agent create", &mut renderer, &workspace)
        .await
        .expect("agents create should parse");
    assert!(matches!(
        create_default,
        SlashCommandOutcome::ManageAgents {
            action: AgentManagerAction::Create { scope: None, name: None }
        }
    ));

    let create_project = handle_slash_command("agent create project", &mut renderer, &workspace)
        .await
        .expect("agents create project should parse");
    assert!(matches!(
        create_project,
        SlashCommandOutcome::ManageAgents {
            action: AgentManagerAction::Create {
                scope: Some(super::super::AgentDefinitionScope::Project),
                name: None,
            }
        }
    ));

    let create_named = handle_slash_command("agent create project reviewer", &mut renderer, &workspace)
        .await
        .expect("agents create project <name> should parse");
    assert!(matches!(
        create_named,
        SlashCommandOutcome::ManageAgents {
            action: AgentManagerAction::Create {
                scope: Some(super::super::AgentDefinitionScope::Project),
                name: Some(ref name),
            }
        } if name == "reviewer"
    ));

    let edit_default = handle_slash_command("agent edit", &mut renderer, &workspace)
        .await
        .expect("agents edit should parse");
    assert!(matches!(
        edit_default,
        SlashCommandOutcome::ManageAgents { action: AgentManagerAction::Edit { name: None } }
    ));

    let edit_named = handle_slash_command("agent edit reviewer", &mut renderer, &workspace)
        .await
        .expect("agents edit <name> should parse");
    assert!(matches!(
        edit_named,
        SlashCommandOutcome::ManageAgents {
            action: AgentManagerAction::Edit { name: Some(ref name) }
        } if name == "reviewer"
    ));
}
#[tokio::test]
async fn subprocesses_command_supports_toggle_and_refresh() {
    let workspace = std::env::current_dir().expect("workspace");
    let mut renderer = renderer_for_tests();

    let toggle = handle_slash_command("config subprocess toggle", &mut renderer, &workspace)
        .await
        .expect("toggle command should parse");
    assert!(matches!(
        toggle,
        SlashCommandOutcome::ManageSubprocesses { action: SubprocessManagerAction::ToggleDefault }
    ));

    let refresh = handle_slash_command("config subprocess refresh", &mut renderer, &workspace)
        .await
        .expect("refresh command should parse");
    assert!(matches!(
        refresh,
        SlashCommandOutcome::ManageSubprocesses { action: SubprocessManagerAction::Refresh }
    ));
}
#[tokio::test]
async fn subprocesses_command_supports_direct_actions() {
    let workspace = std::env::current_dir().expect("workspace");
    let mut renderer = renderer_for_tests();

    let inspect = handle_slash_command("config subprocess inspect bg-1", &mut renderer, &workspace)
        .await
        .expect("inspect command should parse");
    assert!(matches!(
        inspect,
        SlashCommandOutcome::ManageSubprocesses {
            action: SubprocessManagerAction::Inspect { ref id }
        } if id == "bg-1"
    ));

    let stop = handle_slash_command("config subprocess stop bg-1", &mut renderer, &workspace)
        .await
        .expect("stop command should parse");
    assert!(matches!(
        stop,
        SlashCommandOutcome::ManageSubprocesses {
            action: SubprocessManagerAction::Stop { ref id }
        } if id == "bg-1"
    ));

    let cancel = handle_slash_command("config subprocess cancel bg-1", &mut renderer, &workspace)
        .await
        .expect("cancel command should parse");
    assert!(matches!(
        cancel,
        SlashCommandOutcome::ManageSubprocesses {
            action: SubprocessManagerAction::Cancel { ref id }
        } if id == "bg-1"
    ));
}
#[tokio::test]
async fn statusline_command_parses_optional_instructions() {
    let workspace = std::env::current_dir().expect("workspace");
    let mut renderer = renderer_for_tests();

    let outcome = handle_slash_command("statusline show cwd and branch", &mut renderer, &workspace)
        .await
        .expect("statusline should parse");

    assert!(matches!(
        outcome,
        SlashCommandOutcome::StartStatuslineSetup {
            instructions: Some(ref text)
        } if text == "show cwd and branch"
    ));
}
#[tokio::test]
async fn title_command_is_interactive_only() {
    let workspace = std::env::current_dir().expect("workspace");
    let mut renderer = renderer_for_tests();

    let outcome = handle_slash_command("title", &mut renderer, &workspace)
        .await
        .expect("title should parse");

    assert!(matches!(outcome, SlashCommandOutcome::StartTerminalTitleSetup));
}
#[tokio::test]
async fn interactive_mode_commands_parse_to_expected_outcomes() {
    let workspace = std::env::current_dir().expect("workspace");
    let mut renderer = renderer_for_tests();

    let tasks = handle_slash_command("config tasks", &mut renderer, &workspace)
        .await
        .expect("tasks should parse");
    assert!(matches!(tasks, SlashCommandOutcome::ToggleTasksPanel));

    let jobs = handle_slash_command("config jobs", &mut renderer, &workspace)
        .await
        .expect("jobs should parse");
    assert!(matches!(jobs, SlashCommandOutcome::ShowJobsPanel));

    let mode_no_args = handle_slash_command("mode", &mut renderer, &workspace)
        .await
        .expect("mode should parse");
    assert!(matches!(mode_no_args, SlashCommandOutcome::StartModePalette));

    let mode_with_arg = handle_slash_command("mode auto", &mut renderer, &workspace)
        .await
        .expect("mode auto should parse");
    assert!(matches!(
        mode_with_arg,
        SlashCommandOutcome::SelectPrimaryAgent { ref name } if name == "auto"
    ));
}
