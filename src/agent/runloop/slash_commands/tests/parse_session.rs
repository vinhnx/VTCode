use super::*;

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
fn parse_doctor_defaults_to_checkup_modes() {
    let mode = parse_doctor_args("", false).expect("should parse");
    assert!(matches!(mode, DoctorCommand::Checkup(CheckupCommand::Run { quick: false })));

    let mode = parse_doctor_args("", true).expect("should parse");
    assert!(matches!(mode, DoctorCommand::Checkup(CheckupCommand::Interactive)));

    let mode = parse_doctor_args("--quick", false).expect("should parse");
    assert!(matches!(mode, DoctorCommand::Checkup(CheckupCommand::Run { quick: true })));
}
#[test]
fn parse_doctor_routes_prompt_audit() {
    let mode = parse_doctor_args("prompt-audit", false).expect("should parse");
    assert!(matches!(mode, DoctorCommand::PromptAudit));

    let mode = parse_doctor_args("PROMPT-AUDIT", false).expect("should parse case-insensitively");
    assert!(matches!(mode, DoctorCommand::PromptAudit));

    let err = parse_doctor_args("prompt-audit extra", false).expect_err("must reject");
    assert!(err.contains("/doctor prompt-audit"));

    let err = parse_doctor_args("--quick prompt-audit", false).expect_err("must reject");
    assert!(err.contains("/doctor prompt-audit"));

    let err = parse_doctor_args("--quick PROMPT-AUDIT", false).expect_err("must reject case-insensitively");
    assert!(err.contains("/doctor prompt-audit"));
}
#[test]
fn parse_doctor_usage_names_the_command() {
    let err = parse_doctor_args("--bogus", false).expect_err("must reject");
    assert!(err.contains("/doctor"));
    assert!(err.contains("prompt-audit"));
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
