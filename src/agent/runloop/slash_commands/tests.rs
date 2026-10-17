use super::builtins::{CheckupCommand, parse_checkup_args, parse_update_args};
use super::{
    AgentManagerAction, CompactConversationCommand, SecretCommandAction, SessionLogExportFormat, SlashCommandOutcome,
    SubprocessManagerAction, handle_slash_command,
};
use vtcode_core::compaction::ManualCompactionOptions;
use vtcode_core::config::types::ReasoningEffortLevel;
use vtcode_core::skills::command_skill_specs;
use vtcode_core::utils::ansi::AnsiRenderer;
use vtcode_ui::tui::app::InlineHandle;

fn renderer_for_tests() -> AnsiRenderer {
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(tx), Default::default())
}

#[test]
fn parse_checkup_defaults_to_full_mode() {
    let mode = parse_checkup_args("", false).expect("should parse");
    assert!(matches!(mode, CheckupCommand::Run { quick: false }));
}

#[test]
fn parse_checkup_defaults_to_interactive_when_inline_ui_available() {
    let mode = parse_checkup_args("", true).expect("should parse");
    assert!(matches!(mode, CheckupCommand::Interactive));
}

#[test]
fn parse_checkup_quick_aliases() {
    let mode = parse_checkup_args("--quick", true).expect("should parse");
    assert!(matches!(mode, CheckupCommand::Run { quick: true }));

    let mode = parse_checkup_args("-q", true).expect("should parse");
    assert!(matches!(mode, CheckupCommand::Run { quick: true }));

    let mode = parse_checkup_args("quick", true).expect("should parse");
    assert!(matches!(mode, CheckupCommand::Run { quick: true }));
}

#[test]
fn parse_checkup_rejects_conflicting_flags() {
    let err = parse_checkup_args("--quick --full", true).expect_err("must reject");
    assert!(err.contains("either --quick or --full"));
}

#[test]
fn parse_update_rejects_conflicting_modes() {
    let err = parse_update_args("check install").expect_err("must reject");
    assert!(err.contains("either 'check' or 'install'"));
}

#[tokio::test]
async fn checkpoint_navigation_commands_dispatch_locally() {
    let workspace = tempfile::tempdir().expect("workspace");
    let mut renderer = renderer_for_tests();

    let outcome = handle_slash_command("redo", &mut renderer, workspace.path())
        .await
        .expect("checkpoint command should parse");
    assert!(matches!(outcome, SlashCommandOutcome::Redo), "redo must dispatch locally");

    let outcome = handle_slash_command("rewind-recover", &mut renderer, workspace.path())
        .await
        .expect("removed rewind-recover should parse");
    assert!(
        matches!(
            outcome,
            SlashCommandOutcome::SubmitPrompt { ref prompt } if prompt == "/rewind-recover"
        ),
        "removed rewind-recover must fall through; /rewind auto-recovers instead"
    );

    let outcome = handle_slash_command("rewind", &mut renderer, workspace.path())
        .await
        .expect("rewind command should parse");
    assert!(matches!(outcome, SlashCommandOutcome::OpenRewindPicker));
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
async fn donate_command_is_handled() {
    let workspace = std::env::current_dir().expect("workspace");
    let mut renderer = renderer_for_tests();

    let outcome = handle_slash_command("donate", &mut renderer, &workspace)
        .await
        .expect("donate command should parse");

    assert!(matches!(outcome, SlashCommandOutcome::OpenDonateLinks));
}

#[tokio::test]
async fn feedback_command_opens_the_github_issue_form() {
    let workspace = std::env::current_dir().expect("workspace");
    let mut renderer = renderer_for_tests();

    let outcome = handle_slash_command("feedback", &mut renderer, &workspace)
        .await
        .expect("feedback command should parse");

    assert!(matches!(outcome, SlashCommandOutcome::OpenFeedback));
}

#[tokio::test]
async fn share_defaults_to_json_and_html_export() {
    let workspace = std::env::current_dir().expect("workspace");
    let mut renderer = renderer_for_tests();

    let outcome = handle_slash_command("share", &mut renderer, &workspace)
        .await
        .expect("share command should parse");

    assert!(matches!(outcome, SlashCommandOutcome::ShareLog { format: SessionLogExportFormat::Both }));
}

#[tokio::test]
async fn share_alias_routes_html_export() {
    let workspace = std::env::current_dir().expect("workspace");
    let mut renderer = renderer_for_tests();

    let outcome = handle_slash_command("share html", &mut renderer, &workspace)
        .await
        .expect("share alias should parse");

    assert!(matches!(outcome, SlashCommandOutcome::ShareLog { format: SessionLogExportFormat::Html }));
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
async fn removed_effort_command_falls_through_to_prompt() {
    let workspace = std::env::current_dir().expect("workspace");
    let mut renderer = renderer_for_tests();

    let outcome = handle_slash_command("effort --persist high", &mut renderer, &workspace)
        .await
        .expect("effort command should parse");

    assert!(matches!(
        outcome,
        SlashCommandOutcome::SubmitPrompt { ref prompt } if prompt == "/effort --persist high"
    ));
}

#[tokio::test]
async fn removed_memory_command_falls_through_to_prompt() {
    let workspace = std::env::current_dir().expect("workspace");
    let mut renderer = renderer_for_tests();

    let outcome = handle_slash_command("memory", &mut renderer, &workspace)
        .await
        .expect("memory command should parse");

    assert!(matches!(
        outcome,
        SlashCommandOutcome::SubmitPrompt { ref prompt } if prompt == "/memory"
    ));
}

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
                scope: Some(super::AgentDefinitionScope::Project),
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
                scope: Some(super::AgentDefinitionScope::Project),
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

#[tokio::test]
async fn plan_command_still_routes_to_planning_workflow() {
    let workspace = std::env::current_dir().expect("workspace");
    let mut renderer = renderer_for_tests();

    let toggle = handle_slash_command("plan", &mut renderer, &workspace)
        .await
        .expect("plan should parse");
    assert!(matches!(toggle, SlashCommandOutcome::TogglePlanningWorkflow { enable: None, prompt: None }));

    let with_prompt = handle_slash_command("plan implement auth checks", &mut renderer, &workspace)
        .await
        .expect("plan prompt should parse");
    assert!(matches!(
        with_prompt,
        SlashCommandOutcome::TogglePlanningWorkflow {
            enable: Some(true),
            prompt: Some(ref prompt)
        } if prompt == "implement auth checks"
    ));
}

#[tokio::test]
async fn webmcp_pair_command_starts_an_active_bridge() {
    let workspace = std::env::current_dir().expect("workspace");
    let mut renderer = renderer_for_tests();

    let outcome = handle_slash_command("webmcp pair http://localhost:5173", &mut renderer, &workspace)
        .await
        .expect("webmcp pair should parse");

    assert!(matches!(
        outcome,
        SlashCommandOutcome::StartWebmcp { ref origin, replace: false } if origin == "http://localhost:5173"
    ));

    let replacement =
        handle_slash_command("webmcp pair --replace --origin http://localhost:5173", &mut renderer, &workspace)
            .await
            .expect("webmcp replacement should parse");
    assert!(matches!(
        replacement,
        SlashCommandOutcome::StartWebmcp { ref origin, replace: true } if origin == "http://localhost:5173"
    ));

    for rejected in [
        "webmcp pair",
        "webmcp pair   ",
        "webmcp pair http://a http://b",
        "webmcp pair --origin --replace",
        "webmcp pair --replace --replace http://localhost:5173",
    ] {
        let outcome = handle_slash_command(rejected, &mut renderer, &workspace)
            .await
            .expect("malformed webmcp pair should still parse");
        assert!(matches!(outcome, SlashCommandOutcome::Handled));
    }
}

#[tokio::test]
async fn webmcp_status_and_unpair_commands_route_to_runtime_handlers() {
    let workspace = std::env::current_dir().expect("workspace");
    let mut renderer = renderer_for_tests();

    let status = handle_slash_command("webmcp", &mut renderer, &workspace)
        .await
        .expect("webmcp status should parse");
    assert!(matches!(status, SlashCommandOutcome::ShowWebmcpStatus));

    let help = handle_slash_command("webmcp help", &mut renderer, &workspace)
        .await
        .expect("webmcp help should parse");
    assert!(matches!(help, SlashCommandOutcome::ShowWebmcpHelp));

    let unpair = handle_slash_command("webmcp unpair", &mut renderer, &workspace)
        .await
        .expect("webmcp unpair should parse");
    assert!(matches!(unpair, SlashCommandOutcome::StopWebmcp));
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
            command: CompactConversationCommand::Run {
                ref options,
                native_only: false,
            }
        } if *options == ManualCompactionOptions::default()
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

    // Dropped OpenAI-only flags (`--include`, `--store`) are now rejected with a
    // deprecation message instead of being parsed into provider-specific options.
    let rejected = handle_slash_command(
        "compact --instructions \"keep decisions\" --include reasoning.encrypted_content --store",
        &mut renderer,
        &workspace,
    )
    .await
    .expect("rejected compact should still produce an outcome");
    assert!(matches!(rejected, SlashCommandOutcome::Handled));

    let direct = handle_slash_command(
        "compact --instructions \"keep decisions\" --max-output-tokens 128 --reasoning-effort minimal --verbosity high --native-only",
        &mut renderer,
        &workspace,
    )
    .await
    .expect("compact flags should parse");
    assert!(matches!(
        direct,
        SlashCommandOutcome::CompactConversation {
            command: CompactConversationCommand::Run {
                ref options,
                native_only: true,
            }
        } if options.instructions.as_deref() == Some("keep decisions")
            && options.max_output_tokens == Some(128)
            && options.reasoning_effort == Some(ReasoningEffortLevel::Minimal)
            && options.verbosity == Some(vtcode_core::config::types::VerbosityLevel::High)
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
    let outcome = handle_slash_command("review-template src/lib.rs main", &mut renderer, workspace.path())
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
    std::fs::write(template_dir.join("rename-template.md"), "---\ndescription: Rename template\n---\nRename $1 to $2")
        .expect("template");

    let mut renderer = renderer_for_tests();
    let outcome =
        handle_slash_command(r#"rename-template "src/old name.rs" "src/new name.rs""#, &mut renderer, workspace.path())
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
    std::fs::write(template_dir.join("help.md"), "---\ndescription: shadow help\n---\nThis should not run.")
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
async fn review_slash_routes_natural_language_through_cmd_review_skill() {
    let workspace = tempfile::TempDir::new().expect("workspace");
    let mut renderer = renderer_for_tests();

    let input = "Review the full diff and nearby code for correctness, regressions";
    let outcome = handle_slash_command(&format!("review {input}"), &mut renderer, workspace.path())
        .await
        .expect("natural-language review should parse");

    assert!(matches!(
        outcome,
        SlashCommandOutcome::ManageSkills {
            action: crate::agent::runloop::SkillCommandAction::Use { ref name, input: ref skill_input }
        } if name == "cmd-review" && skill_input == input
    ));
}

#[tokio::test]
async fn removed_continue_edit_analyze_fall_through_to_prompt() {
    let workspace = tempfile::TempDir::new().expect("workspace");

    for removed in [
        "continue",
        "continue --all",
        "edit",
        "edit src/main.rs",
        "analyze",
        "analyze security",
    ] {
        let mut renderer = renderer_for_tests();
        let outcome = handle_slash_command(removed, &mut renderer, workspace.path())
            .await
            .unwrap_or_else(|error| panic!("/{removed} should parse: {error}"));

        assert!(
            matches!(
                outcome,
                SlashCommandOutcome::SubmitPrompt { ref prompt } if prompt == &format!("/{removed}")
            ),
            "/{removed} must fall through to plain prompt submission",
        );
    }
}

#[tokio::test]
async fn unknown_slash_command_falls_back_to_normal_prompt_submission() {
    let workspace = tempfile::TempDir::new().expect("workspace");
    let mut renderer = renderer_for_tests();

    let outcome = handle_slash_command("totally-unknown keep this raw", &mut renderer, workspace.path())
        .await
        .expect("unknown slash should pass through");

    assert!(matches!(
        outcome,
        SlashCommandOutcome::SubmitPrompt { ref prompt }
            if prompt == "/totally-unknown keep this raw"
    ));
}

#[tokio::test]
async fn config_permissions_opens_permissions_view() {
    let workspace = tempfile::TempDir::new().expect("workspace");
    for command in ["config permissions", "permissions"] {
        let mut renderer = renderer_for_tests();
        let outcome = handle_slash_command(command, &mut renderer, workspace.path())
            .await
            .unwrap_or_else(|error| panic!("{command} should parse: {error}"));

        assert!(matches!(outcome, SlashCommandOutcome::ShowPermissions), "{command} must open permissions");
    }
}

#[tokio::test]
async fn init_slash_command_parses_force_flag() {
    let workspace = tempfile::TempDir::new().expect("workspace");
    let mut renderer = renderer_for_tests();

    let outcome = handle_slash_command("init --force", &mut renderer, workspace.path())
        .await
        .expect("init should parse");

    assert!(matches!(outcome, SlashCommandOutcome::InitializeWorkspace { force: true }));
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

#[tokio::test]
async fn slash_command_aliases_resolve_to_canonical_backends() {
    let workspace = tempfile::TempDir::new().expect("workspace");

    for alias in [
        "settings",
        "setttings",
        "context",
        "subprocess",
        "subprocesses",
        "models",
        "doctor",
        "permissions",
        "tasks",
        "jobs",
        "log",
        "notify",
        "checkup",
    ] {
        let mut renderer = renderer_for_tests();
        let outcome = handle_slash_command(alias, &mut renderer, workspace.path())
            .await
            .unwrap_or_else(|error| panic!("/{alias} should parse: {error}"));

        assert!(
            !matches!(outcome, SlashCommandOutcome::SubmitPrompt { .. }),
            "/{alias} fell through to plain prompt submission",
        );
    }
}

#[tokio::test]
async fn config_consolidated_sections_match_hidden_aliases() {
    let workspace = tempfile::TempDir::new().expect("workspace");

    for (canonical, alias) in [
        ("config permissions", "permissions"),
        ("config tasks", "tasks"),
        ("config jobs", "jobs"),
        ("config log", "log"),
        ("config subprocess", "subprocess"),
        ("config notify hello", "notify hello"),
        ("config checkup --quick", "checkup --quick"),
        ("config checkup", "doctor"),
    ] {
        let mut canonical_renderer = renderer_for_tests();
        let canonical_outcome = handle_slash_command(canonical, &mut canonical_renderer, workspace.path())
            .await
            .unwrap_or_else(|error| panic!("{canonical} should parse: {error}"));
        let mut alias_renderer = renderer_for_tests();
        let alias_outcome = handle_slash_command(alias, &mut alias_renderer, workspace.path())
            .await
            .unwrap_or_else(|error| panic!("/{alias} should parse: {error}"));

        assert_eq!(
            std::mem::discriminant(&canonical_outcome),
            std::mem::discriminant(&alias_outcome),
            "/{alias} must match /{canonical}"
        );
    }
}

// -- /local command tests --

use super::LocalServerAction;

#[tokio::test]
async fn local_no_args_returns_interactive() {
    let mut renderer = renderer_for_tests();
    let workspace = std::path::PathBuf::from("/tmp");
    let outcome = handle_slash_command("local", &mut renderer, &workspace).await.unwrap();
    assert!(matches!(outcome, SlashCommandOutcome::ManageLocalServer { action: LocalServerAction::Interactive }));
}

#[tokio::test]
async fn local_status_all() {
    let mut renderer = renderer_for_tests();
    let workspace = std::path::PathBuf::from("/tmp");
    let outcome = handle_slash_command("local status", &mut renderer, &workspace).await.unwrap();
    assert!(matches!(
        outcome,
        SlashCommandOutcome::ManageLocalServer {
            action: LocalServerAction::Status { provider: None }
        }
    ));
}

#[tokio::test]
async fn local_status_provider() {
    let mut renderer = renderer_for_tests();
    let workspace = std::path::PathBuf::from("/tmp");
    let outcome = handle_slash_command("local status ollama", &mut renderer, &workspace)
        .await
        .unwrap();
    assert!(matches!(
        outcome,
        SlashCommandOutcome::ManageLocalServer {
            action: LocalServerAction::Status {
                provider: Some(ref p)
            }
        } if p == "ollama"
    ));
}

#[tokio::test]
async fn local_start_provider() {
    let mut renderer = renderer_for_tests();
    let workspace = std::path::PathBuf::from("/tmp");
    let outcome = handle_slash_command("local start lm-studio", &mut renderer, &workspace)
        .await
        .unwrap();
    assert!(matches!(
        outcome,
        SlashCommandOutcome::ManageLocalServer {
            action: LocalServerAction::Start {
                provider: Some(ref p)
            }
        } if p == "lmstudio"
    ));
}

#[tokio::test]
async fn local_stop_llamacpp() {
    let mut renderer = renderer_for_tests();
    let workspace = std::path::PathBuf::from("/tmp");
    let outcome = handle_slash_command("local stop llama.cpp", &mut renderer, &workspace)
        .await
        .unwrap();
    assert!(matches!(
        outcome,
        SlashCommandOutcome::ManageLocalServer {
            action: LocalServerAction::Stop {
                provider: Some(ref p)
            }
        } if p == "llamacpp"
    ));
}

#[tokio::test]
async fn local_troubleshoot_no_provider() {
    let mut renderer = renderer_for_tests();
    let workspace = std::path::PathBuf::from("/tmp");
    let outcome = handle_slash_command("local troubleshoot", &mut renderer, &workspace)
        .await
        .unwrap();
    assert!(matches!(
        outcome,
        SlashCommandOutcome::ManageLocalServer {
            action: LocalServerAction::Troubleshoot { provider: None }
        }
    ));
}

#[tokio::test]
async fn local_configure_provider() {
    let mut renderer = renderer_for_tests();
    let workspace = std::path::PathBuf::from("/tmp");
    let outcome = handle_slash_command("local configure ollama", &mut renderer, &workspace)
        .await
        .unwrap();
    assert!(matches!(
        outcome,
        SlashCommandOutcome::ManageLocalServer {
            action: LocalServerAction::Configure {
                provider: Some(ref p)
            }
        } if p == "ollama"
    ));
}

#[tokio::test]
async fn local_provider_name_only() {
    let mut renderer = renderer_for_tests();
    let workspace = std::path::PathBuf::from("/tmp");
    let outcome = handle_slash_command("local ollama", &mut renderer, &workspace).await.unwrap();
    assert!(matches!(
        outcome,
        SlashCommandOutcome::ManageLocalServer {
            action: LocalServerAction::Provider {
                name: ref n
            }
        } if n == "ollama"
    ));
}

#[tokio::test]
async fn local_provider_then_action() {
    let mut renderer = renderer_for_tests();
    let workspace = std::path::PathBuf::from("/tmp");
    let outcome = handle_slash_command("local ollama status", &mut renderer, &workspace)
        .await
        .unwrap();
    assert!(matches!(
        outcome,
        SlashCommandOutcome::ManageLocalServer {
            action: LocalServerAction::Status {
                provider: Some(ref p)
            }
        } if p == "ollama"
    ));
}

#[tokio::test]
async fn local_help_renders_usage() {
    let mut renderer = renderer_for_tests();
    let workspace = std::path::PathBuf::from("/tmp");
    let outcome = handle_slash_command("local help", &mut renderer, &workspace).await.unwrap();
    assert!(matches!(outcome, SlashCommandOutcome::Handled));
}

#[tokio::test]
async fn secret_no_args_is_interactive() {
    let mut renderer = renderer_for_tests();
    let workspace = std::path::PathBuf::from("/tmp");
    let outcome = handle_slash_command("secret", &mut renderer, &workspace).await.unwrap();
    assert!(matches!(outcome, SlashCommandOutcome::ManageSecrets { action: SecretCommandAction::Interactive }));
}

#[tokio::test]
async fn secret_list() {
    let mut renderer = renderer_for_tests();
    let workspace = std::path::PathBuf::from("/tmp");
    let outcome = handle_slash_command("secret list", &mut renderer, &workspace).await.unwrap();
    assert!(matches!(outcome, SlashCommandOutcome::ManageSecrets { action: SecretCommandAction::List }));
}

#[tokio::test]
async fn secret_status_no_provider() {
    let mut renderer = renderer_for_tests();
    let workspace = std::path::PathBuf::from("/tmp");
    let outcome = handle_slash_command("secret status", &mut renderer, &workspace).await.unwrap();
    assert!(matches!(
        outcome,
        SlashCommandOutcome::ManageSecrets {
            action: SecretCommandAction::Status { provider: None, .. }
        }
    ));
}

#[tokio::test]
async fn secret_status_with_provider() {
    let mut renderer = renderer_for_tests();
    let workspace = std::path::PathBuf::from("/tmp");
    let outcome = handle_slash_command("secret status anthropic MIMO_TOKEN_PLAN_KEY", &mut renderer, &workspace)
        .await
        .unwrap();
    assert!(matches!(
        outcome,
        SlashCommandOutcome::ManageSecrets {
            action: SecretCommandAction::Status {
                provider: Some(ref p),
                key_name: Some(ref key_name),
            }
        } if p == "anthropic" && key_name == "MIMO_TOKEN_PLAN_KEY"
    ));
}

#[tokio::test]
async fn secret_add_requires_provider() {
    let mut renderer = renderer_for_tests();
    let workspace = std::path::PathBuf::from("/tmp");
    let outcome = handle_slash_command("secret add", &mut renderer, &workspace).await.unwrap();
    assert!(matches!(outcome, SlashCommandOutcome::Handled));
}

#[tokio::test]
async fn secret_add_provider() {
    let mut renderer = renderer_for_tests();
    let workspace = std::path::PathBuf::from("/tmp");
    let outcome = handle_slash_command("secret add mycorp --key-name MYCORP_BILLING_KEY", &mut renderer, &workspace)
        .await
        .unwrap();
    assert!(matches!(
        outcome,
        SlashCommandOutcome::ManageSecrets {
            action: SecretCommandAction::Add {
                ref provider,
                key_name: Some(ref key_name),
            }
        } if provider == "mycorp" && key_name == "MYCORP_BILLING_KEY"
    ));
}

#[tokio::test]
async fn secret_add_provider_accepts_equals_key_name() {
    let mut renderer = renderer_for_tests();
    let workspace = std::path::PathBuf::from("/tmp");
    let outcome = handle_slash_command("secret add mycorp --env=MYCORP_BILLING_KEY", &mut renderer, &workspace)
        .await
        .unwrap();
    assert!(matches!(
        outcome,
        SlashCommandOutcome::ManageSecrets {
            action: SecretCommandAction::Add {
                ref provider,
                key_name: Some(ref key_name),
            }
        } if provider == "mycorp" && key_name == "MYCORP_BILLING_KEY"
    ));
}

#[tokio::test]
async fn secret_delete_requires_provider() {
    let mut renderer = renderer_for_tests();
    let workspace = std::path::PathBuf::from("/tmp");
    let outcome = handle_slash_command("secret delete", &mut renderer, &workspace).await.unwrap();
    assert!(matches!(outcome, SlashCommandOutcome::Handled));
}

#[tokio::test]
async fn secret_delete_provider() {
    let mut renderer = renderer_for_tests();
    let workspace = std::path::PathBuf::from("/tmp");
    let outcome = handle_slash_command("secret delete openai OPENAI_BILLING_KEY", &mut renderer, &workspace)
        .await
        .unwrap();
    assert!(matches!(
        outcome,
        SlashCommandOutcome::ManageSecrets {
            action: SecretCommandAction::Delete {
                ref provider,
                key_name: Some(ref key_name),
            }
        } if provider == "openai" && key_name == "OPENAI_BILLING_KEY"
    ));
}

#[tokio::test]
async fn secret_unknown_provider_returns_manage_secrets() {
    let mut renderer = renderer_for_tests();
    let workspace = std::path::PathBuf::from("/tmp");
    let outcome = handle_slash_command("secret add notaprovider", &mut renderer, &workspace)
        .await
        .unwrap();
    assert!(matches!(
        outcome,
        SlashCommandOutcome::ManageSecrets {
            action: SecretCommandAction::Add { ref provider, .. }
        } if provider == "notaprovider"
    ));
}

#[tokio::test]
async fn secret_help_renders_usage() {
    let mut renderer = renderer_for_tests();
    let workspace = std::path::PathBuf::from("/tmp");
    let outcome = handle_slash_command("secret help", &mut renderer, &workspace).await.unwrap();
    assert!(matches!(outcome, SlashCommandOutcome::Handled));
}

#[tokio::test]
async fn secret_migrate_no_provider() {
    let mut renderer = renderer_for_tests();
    let workspace = std::path::PathBuf::from("/tmp");
    let outcome = handle_slash_command("secret migrate", &mut renderer, &workspace).await.unwrap();
    assert!(matches!(
        outcome,
        SlashCommandOutcome::ManageSecrets {
            action: SecretCommandAction::Migrate { provider: None }
        }
    ));
}

#[tokio::test]
async fn secret_migrate_with_provider() {
    let mut renderer = renderer_for_tests();
    let workspace = std::path::PathBuf::from("/tmp");
    let outcome = handle_slash_command("secret migrate openai", &mut renderer, &workspace)
        .await
        .unwrap();
    assert!(matches!(
        outcome,
        SlashCommandOutcome::ManageSecrets {
            action: SecretCommandAction::Migrate { provider: Some(ref p) }
        } if p == "openai"
    ));
}

#[tokio::test]
async fn vim_command_toggles_and_sets_mode() {
    let workspace = std::path::PathBuf::from("/tmp");

    let mut renderer = renderer_for_tests();
    let outcome = handle_slash_command("vim", &mut renderer, &workspace).await.unwrap();
    assert!(matches!(outcome, SlashCommandOutcome::ToggleVimMode { enable: None }));

    let mut renderer = renderer_for_tests();
    let outcome = handle_slash_command("vim on", &mut renderer, &workspace).await.unwrap();
    assert!(matches!(outcome, SlashCommandOutcome::ToggleVimMode { enable: Some(true) }));

    let mut renderer = renderer_for_tests();
    let outcome = handle_slash_command("vim off", &mut renderer, &workspace).await.unwrap();
    assert!(matches!(outcome, SlashCommandOutcome::ToggleVimMode { enable: Some(false) }));

    let mut renderer = renderer_for_tests();
    let outcome = handle_slash_command("vim bogus", &mut renderer, &workspace).await.unwrap();
    assert!(matches!(outcome, SlashCommandOutcome::Handled));
}

#[tokio::test]
async fn fast_command_catalog_and_parser() {
    let spec = command_skill_specs()
        .iter()
        .find(|spec| spec.slash_name == "fast")
        .expect("fast catalog entry");
    assert_eq!(spec.usage, "/fast [on|off]");
    let workspace = tempfile::tempdir().unwrap();
    let mut renderer = renderer_for_tests();
    for (input, expected) in [
        ("fast", None),
        ("  FAST  ", None),
        ("fast on", Some(true)),
        ("fast OFF", Some(false)),
        ("fast \t On \n", Some(true)),
    ] {
        let outcome = handle_slash_command(input, &mut renderer, workspace.path()).await.unwrap();
        assert!(matches!(outcome, SlashCommandOutcome::ToggleFastMode { enable } if enable == expected), "{input}");
    }
    for input in [
        "fast true",
        "fast toggle",
        "fast on off",
        "fast on extra",
        "fast --help",
    ] {
        let outcome = handle_slash_command(input, &mut renderer, workspace.path()).await.unwrap();
        assert!(matches!(outcome, SlashCommandOutcome::Handled), "{input}");
    }
    let outcome = super::dispatch::execute_command_skill_by_name("fast", "on", &mut renderer, workspace.path())
        .await
        .unwrap();
    assert!(matches!(outcome, SlashCommandOutcome::ToggleFastMode { enable: Some(true) }));
}
