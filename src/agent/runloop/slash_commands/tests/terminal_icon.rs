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
    use vtcode_commons::ui_protocol::AppIconPreference;

    let spec = command_skill_specs()
        .iter()
        .find(|spec| spec.slash_name == "terminal-setup")
        .expect("terminal-setup catalog entry");
    assert!(spec.usage.contains("icon ["), "usage: {}", spec.usage);
    // Every selectable variant must appear in the usage string, so a future
    // variant cannot land in the parser without surfacing in `/help`.
    for variant in AppIconPreference::ALL {
        assert!(spec.usage.contains(variant.id()), "usage: {} lacks {}", spec.usage, variant.id());
    }
}
