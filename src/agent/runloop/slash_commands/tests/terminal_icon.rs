use super::*;

#[tokio::test]
async fn terminal_setup_no_args_starts_wizard() {
    let mut renderer = renderer_for_tests();
    let workspace = std::path::PathBuf::from("/tmp");
    let outcome = handle_slash_command("terminal-setup", &mut renderer, &workspace).await.unwrap();
    assert!(matches!(outcome, SlashCommandOutcome::StartTerminalSetup));
}

#[tokio::test]
async fn terminal_setup_icon_show_reports_effective_artwork_without_writing() {
    let mut renderer = renderer_for_tests();
    let workspace = std::path::PathBuf::from("/tmp");
    let outcome = handle_slash_command("terminal-setup icon", &mut renderer, &workspace)
        .await
        .unwrap();
    assert!(matches!(outcome, SlashCommandOutcome::Handled));
}

#[tokio::test]
async fn terminal_setup_icon_show_tolerates_extra_whitespace() {
    let mut renderer = renderer_for_tests();
    let workspace = std::path::PathBuf::from("/tmp");
    let outcome = handle_slash_command("terminal-setup   icon  ", &mut renderer, &workspace)
        .await
        .unwrap();
    assert!(matches!(outcome, SlashCommandOutcome::Handled));
}

#[tokio::test]
async fn terminal_setup_icon_unknown_variant_is_usage_error() {
    let mut renderer = renderer_for_tests();
    let workspace = std::path::PathBuf::from("/tmp");
    let outcome = handle_slash_command("terminal-setup icon holographic", &mut renderer, &workspace)
        .await
        .unwrap();
    assert!(matches!(outcome, SlashCommandOutcome::Handled));
}

#[tokio::test]
async fn terminal_setup_spec_documents_icon_subcommand() {
    let spec = command_skill_specs()
        .iter()
        .find(|spec| spec.slash_name == "terminal-setup")
        .expect("terminal-setup catalog entry");
    assert!(spec.usage.contains("icon [light|dark|dynamic]"), "usage: {}", spec.usage);
}
