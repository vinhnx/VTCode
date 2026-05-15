use super::builtins::{DoctorCommand, parse_doctor_args, parse_effort_args, parse_update_args};
use super::{
    AgentManagerAction, CompactConversationCommand, ScheduleCommandAction, SessionLogExportFormat,
    SessionModeCommand, SlashCommandOutcome, SubprocessManagerAction, handle_slash_command,
};
use vtcode_core::config::types::ReasoningEffortLevel;
use vtcode_core::llm::provider::ResponsesCompactionOptions;
use vtcode_core::skills::command_skill_specs;
use vtcode_core::utils::ansi::AnsiRenderer;
use vtcode_tui::app::InlineHandle;

fn renderer_for_tests() -> AnsiRenderer {
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(tx), Default::default())
}

#[test]
fn parse_doctor_defaults_to_full_mode() {
    let mode = parse_doctor_args("", false).expect("should parse");
    assert!(matches!(mode, DoctorCommand::Run { quick: false }));
}

#[test]
fn parse_doctor_defaults_to_interactive_when_inline_ui_available() {
    let mode = parse_doctor_args("", true).expect("should parse");
    assert!(matches!(mode, DoctorCommand::Interactive));
}

#[test]
fn parse_doctor_quick_aliases() {
    let mode = parse_doctor_args("--quick", true).expect("should parse");
    assert!(matches!(mode, DoctorCommand::Run { quick: true }));

    let mode = parse_doctor_args("-q", true).expect("should parse");
    assert!(matches!(mode, DoctorCommand::Run { quick: true }));

    let mode = parse_doctor_args("quick", true).expect("should parse");
    assert!(matches!(mode, DoctorCommand::Run { quick: true }));
}

#[test]
fn parse_doctor_rejects_conflicting_flags() {
    let err = parse_doctor_args("--quick --full", true).expect_err("must reject");
    assert!(err.contains("either --quick or --full"));
}

#[test]
fn parse_update_rejects_conflicting_modes() {
    let err = parse_update_args("check install").expect_err("must reject");
    assert!(err.contains("either 'check' or 'install'"));
}

#[test]
fn parse_effort_defaults_to_picker_mode() {
    let parsed = parse_effort_args("").expect("should parse");
    assert_eq!(parsed, (None, false));
}

#[test]
fn parse_effort_supports_persist_flag_and_level() {
    let parsed = parse_effort_args("--persist xhigh").expect("should parse");
    assert_eq!(parsed, (Some(ReasoningEffortLevel::XHigh), true));

    let parsed = parse_effort_args("high persist").expect("should parse");
    assert_eq!(parsed, (Some(ReasoningEffortLevel::High), true));
}

#[test]
fn parse_effort_rejects_multiple_levels() {
    let err = parse_effort_args("low high").expect_err("must reject");
    assert!(err.contains("at most one effort level"));
}

#[tokio::test]
async fn stop_command_returns_local_stop_outcome() {
    let workspace = std::env::current_dir().expect("workspace");
    let mut renderer = renderer_for_tests();

    let outcome = handle_slash_command("stop", &mut renderer, &workspace)
        .await
        .expect("stop command should parse");

    assert!(matches!(outcome, SlashCommandOutcome::StopAgent));
}

#[tokio::test]
async fn pause_command_is_idle_noop() {
    let workspace = std::env::current_dir().expect("workspace");
    let mut renderer = renderer_for_tests();

    let outcome = handle_slash_command("pause", &mut renderer, &workspace)
        .await
        .expect("pause command should parse");

    assert!(matches!(outcome, SlashCommandOutcome::Handled));
}

#[tokio::test]
async fn share_defaults_to_json_and_html_export() {
    let workspace = std::env::current_dir().expect("workspace");
    let mut renderer = renderer_for_tests();

    let outcome = handle_slash_command("share", &mut renderer, &workspace)
        .await
        .expect("share command should parse");

    assert!(matches!(
        outcome,
        SlashCommandOutcome::ShareLog {
            format: SessionLogExportFormat::Both
        }
    ));
}

#[tokio::test]
async fn share_alias_routes_html_export() {
    let workspace = std::env::current_dir().expect("workspace");
    let mut renderer = renderer_for_tests();

    let outcome = handle_slash_command("share html", &mut renderer, &workspace)
        .await
        .expect("share alias should parse");

    assert!(matches!(
        outcome,
        SlashCommandOutcome::ShareLog {
            format: SessionLogExportFormat::Html
        }
    ));
}

#[tokio::test]
async fn removed_share_log_command_no_longer_resolves() {
    let workspace = std::env::current_dir().expect("workspace");
    let mut renderer = renderer_for_tests();

    let outcome = handle_slash_command("share-log html", &mut renderer, &workspace)
        .await
        .expect("removed command should fall through");

    assert!(matches!(
        outcome,
        SlashCommandOutcome::SubmitPrompt { ref prompt } if prompt == "/share-log html"
    ));
}

#[tokio::test]
async fn subprocess_alias_matches_plural_command() {
    let workspace = std::env::current_dir().expect("workspace");
    let mut renderer = renderer_for_tests();

    let outcome = handle_slash_command("subprocess inspect child-1", &mut renderer, &workspace)
        .await
        .expect("subprocess alias should parse");

    assert!(matches!(
        outcome,
        SlashCommandOutcome::ManageSubprocesses {
            action: SubprocessManagerAction::Inspect { ref id }
        } if id == "child-1"
    ));
}

#[tokio::test]
async fn ide_command_returns_toggle_outcome() {
    let workspace = std::env::current_dir().expect("workspace");
    let mut renderer = renderer_for_tests();

    let outcome = handle_slash_command("ide", &mut renderer, &workspace)
        .await
        .expect("ide command should parse");

    assert!(matches!(outcome, SlashCommandOutcome::ToggleIdeContext));
}

#[tokio::test]
async fn effort_command_returns_set_effort_outcome() {
    let workspace = std::env::current_dir().expect("workspace");
    let mut renderer = renderer_for_tests();

    let outcome = handle_slash_command("effort --persist high", &mut renderer, &workspace)
        .await
        .expect("effort command should parse");

    assert!(matches!(
        outcome,
        SlashCommandOutcome::SetEffort {
            level: Some(ReasoningEffortLevel::High),
            persist: true,
        }
    ));
}

#[tokio::test]
async fn memory_command_returns_memory_outcome() {
    let workspace = std::env::current_dir().expect("workspace");
    let mut renderer = renderer_for_tests();

    let outcome = handle_slash_command("memory", &mut renderer, &workspace)
        .await
        .expect("memory command should parse");

    assert!(matches!(outcome, SlashCommandOutcome::ShowMemory));
}

#[tokio::test]
async fn notify_command_uses_default_message() {
    let workspace = std::env::current_dir().expect("workspace");
    let mut renderer = renderer_for_tests();

    let outcome = handle_slash_command("notify", &mut renderer, &workspace)
        .await
        .expect("notify command should parse");

    assert!(matches!(
        outcome,
        SlashCommandOutcome::Notify { ref message }
        if message == "Manual notification from /notify"
    ));
}

#[tokio::test]
async fn notify_command_preserves_custom_message() {
    let workspace = std::env::current_dir().expect("workspace");
    let mut renderer = renderer_for_tests();

    let outcome = handle_slash_command("notify build finished", &mut renderer, &workspace)
        .await
        .expect("notify command should parse");

    assert!(matches!(
        outcome,
        SlashCommandOutcome::Notify { ref message }
        if message == "build finished"
    ));
}

#[tokio::test]
async fn hooks_command_returns_hooks_outcome() {
    let workspace = std::env::current_dir().expect("workspace");
    let mut renderer = renderer_for_tests();

    let outcome = handle_slash_command("hooks", &mut renderer, &workspace)
        .await
        .expect("hooks command should parse");

    assert!(matches!(outcome, SlashCommandOutcome::ShowHooks));
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
async fn ide_command_rejects_arguments() {
    let workspace = std::env::current_dir().expect("workspace");
    let mut renderer = renderer_for_tests();

    let outcome = handle_slash_command("ide extra", &mut renderer, &workspace)
        .await
        .expect("ide command should parse");

    assert!(matches!(outcome, SlashCommandOutcome::Handled));
}

#[tokio::test]
async fn agent_command_opens_active_agents_inspector() {
    let workspace = std::env::current_dir().expect("workspace");
    let mut renderer = renderer_for_tests();

    let outcome = handle_slash_command("agent", &mut renderer, &workspace)
        .await
        .expect("agent command should parse");

    assert!(matches!(
        outcome,
        SlashCommandOutcome::ManageAgents {
            action: AgentManagerAction::Threads
        }
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

    let create_default = handle_slash_command("agents create", &mut renderer, &workspace)
        .await
        .expect("agents create should parse");
    assert!(matches!(
        create_default,
        SlashCommandOutcome::ManageAgents {
            action: AgentManagerAction::Create {
                scope: None,
                name: None,
            }
        }
    ));

    let create_project = handle_slash_command("agents create project", &mut renderer, &workspace)
        .await
        .expect("agents create project should parse");
    assert!(matches!(
        create_project,
        SlashCommandOutcome::ManageAgents {
            action: AgentManagerAction::Create {
                scope: Some(super::AgentDefinitionScope::Project),
                name: None,
            }
        }
    ));

    let create_named =
        handle_slash_command("agents create project reviewer", &mut renderer, &workspace)
            .await
            .expect("agents create project <name> should parse");
    assert!(matches!(
        create_named,
        SlashCommandOutcome::ManageAgents {
            action: AgentManagerAction::Create {
                scope: Some(super::AgentDefinitionScope::Project),
                name: Some(ref name),
            }
        } if name == "reviewer"
    ));

    let edit_default = handle_slash_command("agents edit", &mut renderer, &workspace)
        .await
        .expect("agents edit should parse");
    assert!(matches!(
        edit_default,
        SlashCommandOutcome::ManageAgents {
            action: AgentManagerAction::Edit { name: None }
        }
    ));

    let edit_named = handle_slash_command("agents edit reviewer", &mut renderer, &workspace)
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

    let toggle = handle_slash_command("subprocesses toggle", &mut renderer, &workspace)
        .await
        .expect("toggle command should parse");
    assert!(matches!(
        toggle,
        SlashCommandOutcome::ManageSubprocesses {
            action: SubprocessManagerAction::ToggleDefault
        }
    ));

    let refresh = handle_slash_command("subprocesses refresh", &mut renderer, &workspace)
        .await
        .expect("refresh command should parse");
    assert!(matches!(
        refresh,
        SlashCommandOutcome::ManageSubprocesses {
            action: SubprocessManagerAction::Refresh
        }
    ));
}

#[tokio::test]
async fn subprocesses_command_supports_direct_actions() {
    let workspace = std::env::current_dir().expect("workspace");
    let mut renderer = renderer_for_tests();

    let inspect = handle_slash_command("subprocesses inspect bg-1", &mut renderer, &workspace)
        .await
        .expect("inspect command should parse");
    assert!(matches!(
        inspect,
        SlashCommandOutcome::ManageSubprocesses {
            action: SubprocessManagerAction::Inspect { ref id }
        } if id == "bg-1"
    ));

    let stop = handle_slash_command("subprocesses stop bg-1", &mut renderer, &workspace)
        .await
        .expect("stop command should parse");
    assert!(matches!(
        stop,
        SlashCommandOutcome::ManageSubprocesses {
            action: SubprocessManagerAction::Stop { ref id }
        } if id == "bg-1"
    ));

    let cancel = handle_slash_command("subprocesses cancel bg-1", &mut renderer, &workspace)
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

    assert!(matches!(
        outcome,
        SlashCommandOutcome::StartTerminalTitleSetup
    ));
}

#[tokio::test]
async fn interactive_mode_commands_parse_to_expected_outcomes() {
    let workspace = std::env::current_dir().expect("workspace");
    let mut renderer = renderer_for_tests();

    let suggest = handle_slash_command("suggest", &mut renderer, &workspace)
        .await
        .expect("suggest should parse");
    assert!(matches!(
        suggest,
        SlashCommandOutcome::TriggerPromptSuggestions
    ));

    let tasks = handle_slash_command("tasks", &mut renderer, &workspace)
        .await
        .expect("tasks should parse");
    assert!(matches!(tasks, SlashCommandOutcome::ToggleTasksPanel));

    let jobs = handle_slash_command("jobs", &mut renderer, &workspace)
        .await
        .expect("jobs should parse");
    assert!(matches!(jobs, SlashCommandOutcome::ShowJobsPanel));

    let mode = handle_slash_command("mode", &mut renderer, &workspace)
        .await
        .expect("mode should parse");
    assert!(matches!(mode, SlashCommandOutcome::StartModeSelection));

    let auto_mode = handle_slash_command("mode auto", &mut renderer, &workspace)
        .await
        .expect("mode auto should parse");
    assert!(matches!(
        auto_mode,
        SlashCommandOutcome::SetMode {
            mode: SessionModeCommand::Auto
        }
    ));

    let cycle = handle_slash_command("mode cycle", &mut renderer, &workspace)
        .await
        .expect("mode cycle should parse");
    assert!(matches!(cycle, SlashCommandOutcome::CycleMode));
}

#[tokio::test]
async fn schedule_commands_parse_to_interactive_and_direct_outcomes() {
    let workspace = std::env::current_dir().expect("workspace");
    let mut renderer = renderer_for_tests();

    let interactive = handle_slash_command("schedule", &mut renderer, &workspace)
        .await
        .expect("schedule should parse");
    assert!(matches!(
        interactive,
        SlashCommandOutcome::ManageSchedule {
            action: ScheduleCommandAction::Interactive
        }
    ));

    let browse = handle_slash_command("schedule list", &mut renderer, &workspace)
        .await
        .expect("schedule list should parse");
    assert!(matches!(
        browse,
        SlashCommandOutcome::ManageSchedule {
            action: ScheduleCommandAction::Browse
        }
    ));

    let create_interactive = handle_slash_command("schedule create", &mut renderer, &workspace)
        .await
        .expect("schedule create should parse");
    assert!(matches!(
        create_interactive,
        SlashCommandOutcome::ManageSchedule {
            action: ScheduleCommandAction::CreateInteractive
        }
    ));

    let delete_interactive = handle_slash_command("schedule delete", &mut renderer, &workspace)
        .await
        .expect("schedule delete should parse");
    assert!(matches!(
        delete_interactive,
        SlashCommandOutcome::ManageSchedule {
            action: ScheduleCommandAction::DeleteInteractive
        }
    ));

    let delete_direct = handle_slash_command("schedule delete deadbeef", &mut renderer, &workspace)
        .await
        .expect("schedule delete id should parse");
    assert!(matches!(
        delete_direct,
        SlashCommandOutcome::ManageSchedule {
            action: ScheduleCommandAction::Delete { ref id }
        } if id == "deadbeef"
    ));

    let create_direct = handle_slash_command(
        "schedule create --prompt \"check the deployment\" --every 10m",
        &mut renderer,
        &workspace,
    )
    .await
    .expect("schedule create with flags should parse");
    assert!(matches!(
        create_direct,
        SlashCommandOutcome::ManageSchedule {
            action: ScheduleCommandAction::Create { ref input }
        } if input.prompt.as_deref() == Some("check the deployment")
            && input.every.as_deref() == Some("10m")
    ));
}

#[tokio::test]
async fn compact_commands_parse_to_automatic_and_direct_outcomes() {
    let workspace = std::env::current_dir().expect("workspace");
    let mut renderer = renderer_for_tests();

    let automatic = handle_slash_command("compact", &mut renderer, &workspace)
        .await
        .expect("compact should parse");
    assert!(matches!(
        automatic,
        SlashCommandOutcome::CompactConversation {
            command: CompactConversationCommand::Run { ref options }
        } if *options == ResponsesCompactionOptions::default()
    ));

    let edit_prompt = handle_slash_command("compact edit-prompt", &mut renderer, &workspace)
        .await
        .expect("compact edit-prompt should parse");
    assert!(matches!(
        edit_prompt,
        SlashCommandOutcome::CompactConversation {
            command: CompactConversationCommand::EditDefaultPrompt
        }
    ));

    let reset_prompt = handle_slash_command("compact reset-prompt", &mut renderer, &workspace)
        .await
        .expect("compact reset-prompt should parse");
    assert!(matches!(
        reset_prompt,
        SlashCommandOutcome::CompactConversation {
            command: CompactConversationCommand::ResetDefaultPrompt
        }
    ));

    let direct = handle_slash_command(
        "compact --instructions \"keep decisions\" --include reasoning.encrypted_content --store",
        &mut renderer,
        &workspace,
    )
    .await
    .expect("compact flags should parse");
    assert!(matches!(
        direct,
        SlashCommandOutcome::CompactConversation {
            command: CompactConversationCommand::Run { ref options }
        } if options.instructions.as_deref() == Some("keep decisions")
            && options.responses_include.as_deref()
                == Some(&["reasoning.encrypted_content".to_string()][..])
            && options.response_store == Some(true)
    ));
}

#[tokio::test]
async fn prompt_template_invocation_replaces_editor_input() {
    let workspace = tempfile::TempDir::new().expect("workspace");
    let template_dir = workspace.path().join(".vtcode/prompts/templates");
    std::fs::create_dir_all(&template_dir).expect("template dir");
    std::fs::write(
        template_dir.join("review-template.md"),
        "---\ndescription: Review template\n---\nReview $1 against $2.\nArgs: $@",
    )
    .expect("template");

    let mut renderer = renderer_for_tests();
    let outcome = handle_slash_command(
        "review-template src/lib.rs main",
        &mut renderer,
        workspace.path(),
    )
    .await
    .expect("review template should parse");

    assert!(matches!(
        outcome,
        SlashCommandOutcome::ReplaceInput { ref content }
            if content == "Review src/lib.rs against main.\nArgs: src/lib.rs main"
    ));
}

#[tokio::test]
async fn prompt_template_invocation_preserves_quoted_arguments() {
    let workspace = tempfile::TempDir::new().expect("workspace");
    let template_dir = workspace.path().join(".vtcode/prompts/templates");
    std::fs::create_dir_all(&template_dir).expect("template dir");
    std::fs::write(
        template_dir.join("rename-template.md"),
        "---\ndescription: Rename template\n---\nRename $1 to $2",
    )
    .expect("template");

    let mut renderer = renderer_for_tests();
    let outcome = handle_slash_command(
        r#"rename-template "src/old name.rs" "src/new name.rs""#,
        &mut renderer,
        workspace.path(),
    )
    .await
    .expect("rename template should parse");

    assert!(matches!(
        outcome,
        SlashCommandOutcome::ReplaceInput { ref content }
            if content == "Rename src/old name.rs to src/new name.rs"
    ));
}

#[tokio::test]
async fn built_in_slash_command_beats_same_named_template() {
    let workspace = tempfile::TempDir::new().expect("workspace");
    let template_dir = workspace.path().join(".vtcode/prompts/templates");
    std::fs::create_dir_all(&template_dir).expect("template dir");
    std::fs::write(
        template_dir.join("help.md"),
        "---\ndescription: shadow help\n---\nThis should not run.",
    )
    .expect("template");

    let mut renderer = renderer_for_tests();
    let outcome = handle_slash_command("help", &mut renderer, workspace.path())
        .await
        .expect("help should parse");

    assert!(matches!(outcome, SlashCommandOutcome::Handled));
}

#[tokio::test]
async fn review_slash_routes_through_cmd_review_skill() {
    let workspace = tempfile::TempDir::new().expect("workspace");
    let mut renderer = renderer_for_tests();

    let outcome = handle_slash_command("review --last-diff", &mut renderer, workspace.path())
        .await
        .expect("review should parse");

    assert!(matches!(
        outcome,
        SlashCommandOutcome::ManageSkills {
            action: crate::agent::runloop::SkillCommandAction::Use { ref name, ref input }
        } if name == "cmd-review" && input == "--last-diff"
    ));
}

#[tokio::test]
async fn command_alias_typo_routes_through_cmd_command_skill() {
    let workspace = tempfile::TempDir::new().expect("workspace");
    let mut renderer = renderer_for_tests();

    let outcome = handle_slash_command("comman cargo check", &mut renderer, workspace.path())
        .await
        .expect("comman alias should parse");

    assert!(matches!(
        outcome,
        SlashCommandOutcome::ManageSkills {
            action: crate::agent::runloop::SkillCommandAction::Use { ref name, ref input }
        } if name == "cmd-command" && input == "cargo check"
    ));
}

#[tokio::test]
async fn analyze_slash_routes_normalized_scope_through_cmd_analyze_skill() {
    let workspace = tempfile::TempDir::new().expect("workspace");
    let mut renderer = renderer_for_tests();

    let outcome = handle_slash_command("analyze SECURITY", &mut renderer, workspace.path())
        .await
        .expect("analyze should parse");

    assert!(matches!(
        outcome,
        SlashCommandOutcome::ManageSkills {
            action: crate::agent::runloop::SkillCommandAction::Use { ref name, ref input }
        } if name == "cmd-analyze" && input == "security"
    ));
}

#[tokio::test]
async fn invalid_analyze_scope_is_handled_locally() {
    let workspace = tempfile::TempDir::new().expect("workspace");
    let mut renderer = renderer_for_tests();

    let outcome = handle_slash_command("analyze nope", &mut renderer, workspace.path())
        .await
        .expect("analyze should parse");

    assert!(matches!(outcome, SlashCommandOutcome::Handled));
}

#[tokio::test]
async fn unknown_slash_command_falls_back_to_normal_prompt_submission() {
    let workspace = tempfile::TempDir::new().expect("workspace");
    let mut renderer = renderer_for_tests();

    let outcome = handle_slash_command(
        "totally-unknown keep this raw",
        &mut renderer,
        workspace.path(),
    )
    .await
    .expect("unknown slash should pass through");

    assert!(matches!(
        outcome,
        SlashCommandOutcome::SubmitPrompt { ref prompt }
            if prompt == "/totally-unknown keep this raw"
    ));
}

#[tokio::test]
async fn permissions_slash_command_opens_permissions_view() {
    let workspace = tempfile::TempDir::new().expect("workspace");
    let mut renderer = renderer_for_tests();

    let outcome = handle_slash_command("permissions", &mut renderer, workspace.path())
        .await
        .expect("permissions should parse");

    assert!(matches!(outcome, SlashCommandOutcome::ShowPermissions));
}

#[tokio::test]
async fn init_slash_command_parses_force_flag() {
    let workspace = tempfile::TempDir::new().expect("workspace");
    let mut renderer = renderer_for_tests();

    let outcome = handle_slash_command("init --force", &mut renderer, workspace.path())
        .await
        .expect("init should parse");

    assert!(matches!(
        outcome,
        SlashCommandOutcome::InitializeWorkspace { force: true }
    ));
}

#[tokio::test]
async fn every_registered_slash_command_resolves_without_prompt_fallback() {
    let workspace = tempfile::TempDir::new().expect("workspace");

    for spec in command_skill_specs() {
        let mut renderer = renderer_for_tests();
        let outcome = handle_slash_command(spec.slash_name, &mut renderer, workspace.path())
            .await
            .unwrap_or_else(|error| panic!("{} should parse: {error}", spec.slash_name));

        assert!(
            !matches!(outcome, SlashCommandOutcome::SubmitPrompt { .. }),
            "/{} fell through to plain prompt submission",
            spec.slash_name
        );
    }
}
