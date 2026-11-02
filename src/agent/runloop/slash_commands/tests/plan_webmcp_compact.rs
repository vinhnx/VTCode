use super::*;

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

    let connect = handle_slash_command("webmcp connect https://vtcode.vinhnx.chatgpt.site", &mut renderer, &workspace)
        .await
        .expect("webmcp connect should parse");
    assert!(matches!(
        connect,
        SlashCommandOutcome::StartWebmcp { ref origin, replace: false }
            if origin == "https://vtcode.vinhnx.chatgpt.site"
    ));

    let connect_replace =
        handle_slash_command("webmcp connect --replace http://localhost:5173", &mut renderer, &workspace)
            .await
            .expect("webmcp connect replacement should parse");
    assert!(matches!(
        connect_replace,
        SlashCommandOutcome::StartWebmcp { ref origin, replace: true } if origin == "http://localhost:5173"
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
        "webmcp connect",
        "webmcp connect https://a.test https://b.test",
        "webmcp connect --replace --replace https://a.test",
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

    let splash = handle_slash_command("splash", &mut renderer, &workspace)
        .await
        .expect("splash should parse");
    assert!(matches!(splash, SlashCommandOutcome::ShowSplash));

    let help = handle_slash_command("webmcp help", &mut renderer, &workspace)
        .await
        .expect("webmcp help should parse");
    assert!(matches!(help, SlashCommandOutcome::ShowWebmcpHelp));

    let unpair = handle_slash_command("webmcp unpair", &mut renderer, &workspace)
        .await
        .expect("webmcp unpair should parse");
    assert!(matches!(unpair, SlashCommandOutcome::StopWebmcp));

    let stop = handle_slash_command("webmcp stop", &mut renderer, &workspace)
        .await
        .expect("webmcp stop should parse");
    assert!(matches!(stop, SlashCommandOutcome::StopWebmcp));
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
