use super::*;

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
