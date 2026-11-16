use super::*;

#[tokio::test]
async fn goal_bare_and_status_dispatch_to_status() {
    let workspace = tempfile::tempdir().expect("workspace");
    let mut renderer = renderer_for_tests();

    for input in ["goal", "goal status", "goal   STATUS  "] {
        let outcome = handle_slash_command(input, &mut renderer, workspace.path())
            .await
            .expect("goal command should parse");
        assert!(
            matches!(outcome, SlashCommandOutcome::GoalCommand { command: GoalSlashCommand::Status }),
            "{input:?} must dispatch to Status"
        );
    }
}

#[tokio::test]
async fn goal_verbs_dispatch_asymmetrically_from_objectives() {
    let workspace = tempfile::tempdir().expect("workspace");
    let mut renderer = renderer_for_tests();

    for (input, expected) in [
        ("goal pause", GoalSlashCommand::Pause),
        ("goal Pause", GoalSlashCommand::Pause),
        ("goal resume", GoalSlashCommand::Resume),
        ("goal clear", GoalSlashCommand::Clear),
        ("goal edit", GoalSlashCommand::Edit),
    ] {
        let outcome = handle_slash_command(input, &mut renderer, workspace.path())
            .await
            .expect("goal verb should parse");
        assert!(
            matches!(outcome, SlashCommandOutcome::GoalCommand { command } if command == expected),
            "{input:?} must dispatch to {expected:?}"
        );
    }

    for (input, limit) in [
        ("goal budget", None),
        ("goal budget 25000", Some(25_000)),
        ("goal budget 0", Some(0)),
    ] {
        let outcome = handle_slash_command(input, &mut renderer, workspace.path())
            .await
            .expect("goal budget should parse");
        assert!(
            matches!(outcome, SlashCommandOutcome::GoalCommand {
                command: GoalSlashCommand::Budget { limit: ref got }
            } if *got == limit),
            "{input:?} must dispatch to Budget({limit:?})"
        );
    }

    let outcome = handle_slash_command("goal budget soon", &mut renderer, workspace.path())
        .await
        .expect("invalid budget dispatches to an error outcome");
    assert!(
        matches!(outcome, SlashCommandOutcome::Handled),
        "malformed budget must render usage, not set an objective"
    );

    // Same leading words with more text are objectives, not verbs.
    for (input, objective) in [
        ("goal pause the deploy", "pause the deploy"),
        ("goal pausing everything", "pausing everything"),
        ("goal ship the release", "ship the release"),
    ] {
        let outcome = handle_slash_command(input, &mut renderer, workspace.path())
            .await
            .expect("goal objective should parse");
        assert!(
            matches!(outcome, SlashCommandOutcome::GoalCommand {
                command: GoalSlashCommand::Set { objective: ref got }
            } if got == objective),
            "{input:?} must dispatch to Set({objective:?})"
        );
    }
}
