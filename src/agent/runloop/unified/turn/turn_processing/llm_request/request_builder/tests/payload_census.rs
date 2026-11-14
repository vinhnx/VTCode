use super::*;

/// Full-request payload census for the harness audit: build a representative
/// first turn through the real `build_turn_request` path and pin the emitted
/// [`FirstCallComposition`] parts.
///
/// Fixture: Minimal-mode composed base (pinned), three stub tool definitions,
/// one user message, no skills/MCP/deferral. Numbers are printed for the
/// audit ledger (`--no-capture`); ceilings are regression tripwires with
/// generous headroom (~2x measured), not budgets. Per-provider formatted
/// tool bytes live with the tool-format fixture in `vtcode-core`, which is
/// where both the catalog and the formatters are reachable.
#[tokio::test]
async fn first_request_payload_census() {
    let mut backing = TestTurnProcessingBacking::new().await;
    let mut compose_config = VTCodeConfig::default();
    // Pin Minimal: the census tracks this profile's payload, not whatever the
    // configured default happens to be.
    compose_config.agent.system_prompt_mode = vtcode_core::config::types::SystemPromptMode::Minimal;
    let base = vtcode_core::prompts::system::compose_system_instruction_text(
        backing.workspace_path(),
        Some(&compose_config),
        None,
    )
    .await;
    backing.add_tool_definition(named_tool("exec_command")).await;
    backing.add_tool_definition(named_tool("apply_patch")).await;
    backing.add_tool_definition(named_tool("write_stdin")).await;
    let mut ctx = backing.turn_processing_context();
    ctx.context_manager.set_base_system_prompt(base);
    ctx.working_history.push(uni::Message::user("hello".to_string()));
    let snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
    let built = build_turn_request(&mut ctx, 1, "noop-model", &snapshot, Some(320), None, false)
        .await
        .expect("census request should build");

    let composition = ctx
        .session_stats
        .first_call_composition()
        .expect("first build must capture composition");
    let system_text = system_prompt_text(&built.request);
    let wire_tools = built.request.tools.as_deref().map(Vec::as_slice).unwrap_or(&[]);
    assert_eq!(composition.on_wire_tools, wire_tools.len(), "metric must match wire tools");
    assert_eq!(composition.on_wire_tools, 3, "fixture wires all three stub tools");
    assert!(composition.system_prompt_tokens > 0, "system part must be non-zero");
    assert!(composition.tool_schema_tokens > 0, "tool part must be non-zero");
    assert!(composition.message_history_tokens > 0, "history part must be non-zero");
    // Regression tripwires (~2x the measured census); see the audit ledger.
    assert!(composition.system_prompt_tokens <= 3_000, "system part regressed");
    assert!(composition.tool_schema_tokens <= 300, "tool part regressed");
    assert!(composition.message_history_tokens <= 50, "history part regressed");
    assert!(composition.fixed_overhead_tokens() <= 3_500, "fixed overhead regressed");

    let internal_tool_bytes: usize = wire_tools
        .iter()
        .map(|tool| serde_json::to_vec(tool).expect("serialize tool").len())
        .sum();
    eprintln!(
        "census: system {} bytes / {} tokens; tools {} bytes / {} tokens ({} on wire); history {} tokens; fixed overhead {}",
        system_text.len(),
        composition.system_prompt_tokens,
        internal_tool_bytes,
        composition.tool_schema_tokens,
        composition.on_wire_tools,
        composition.message_history_tokens,
        composition.fixed_overhead_tokens(),
    );
}
