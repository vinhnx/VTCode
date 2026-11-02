use super::*;

#[test]
fn test_guidelines_stay_compact() {
    let tools = vec![
        TOOL_EXEC_COMMAND.to_string(),
        TOOL_CODE_SEARCH.to_string(),
        "read_file".to_string(),
        TOOL_LIST_FILES.to_string(),
        "apply_patch".to_string(),
    ];
    let guidelines = generate_tool_guidelines_for_profile(&tools, None, ResolvedShellPromptProfile::UnixLike, true);
    assert!(guidelines.contains("Batch independent read-only calls"));
    assert!(guidelines.contains("code_search"));
    // Exec-capable profiles describe the fail-closed verifier pipeline
    // contract, including the terminal outcome and rejected shell forms.
    assert!(guidelines.contains("static read-only filtering pipelines use fail-closed `pipefail`"));
    assert!(guidelines.contains("Only terminal exit 0 clears verification"));
    assert!(guidelines.contains("Recheck after `--fix`"));
    assert!(guidelines.contains("dynamic syntax, mutating tails, `;`, and `||` do not qualify"));
    assert!(!guidelines.contains("elided"));
    assert!(guidelines.contains("max_output_tokens"));
    assert!(guidelines.contains("Build and Auto share tools and safety gates"));
    let approx_tokens = vtcode_commons::estimate_tokens(&guidelines);
    // The batching, bounded-diff, and verifier-discipline guardrails are
    // intentionally part of the compact shared prompt. Raised from 500 so
    // the verifier rule can state its reason (a visible exit status).
    // Raised to 580 for the Sonnet 5.5 low-effort real-check rule
    // (syntax-only/failed-to-start do not count; project package manager,
    // never sudo; state which check was skipped and why).
    // Raised to 640 for wider-window reuse and code_search unavailable
    // no-retry guardrails (session-20261015 overlapping reads + identical
    // retries).
    // Universal source-inspection guidance lives in Runtime Guidance;
    // both densities retain patch-specific permission and bounded-diff rules.
    assert!(approx_tokens < 640, "got ~{approx_tokens} tokens");
}
#[test]
fn deferred_tools_section_caps_groups_and_truncates_descriptions() {
    use crate::llm::provider::ToolNamespace;

    fn deferred_mcp_tool(server: &str, tool: &str, description: &str) -> ToolDefinition {
        let mut definition = ToolDefinition::function(
            format!("mcp__{server}__{tool}"),
            "deferred".to_string(),
            serde_json::json!({"type": "object"}),
        );
        definition.namespace = Some(ToolNamespace {
            name: server.to_string(),
            description: description.to_string(),
        });
        definition.defer_loading = Some(true);
        definition
    }

    let mut tools = Vec::new();
    for index in 0..10 {
        tools.push(deferred_mcp_tool(&format!("server-{index:02}"), "search", "Tools provided by MCP server"));
    }
    // One group with an unbounded server-provided description.
    tools.push(deferred_mcp_tool("server-long", "search", &"d".repeat(500)));

    let mut prompt = "Base prompt".to_string();
    append_deferred_tools_prompt_section(&mut prompt, &tools);

    assert!(prompt.contains("[Deferred Tools]"));
    assert!(prompt.contains("server-00 (1 tools)"));
    assert!(prompt.contains("server-07 (1 tools)"));
    assert!(!prompt.contains("server-08 (1 tools)"), "groups past the cap must collapse into overflow");
    assert!(!prompt.contains("server-09 (1 tools)"));
    assert!(
        prompt.contains("(+3 more deferred groups available"),
        "10 named groups + 1 long group over the 8-group cap overflows by 3"
    );
    assert!(!prompt.contains(&"d".repeat(121)), "group descriptions must stay truncated");
    assert!(prompt.contains("Use `search_tools` to find a deferred capability"));

    // Idempotent: re-appending replaces the section instead of duplicating it.
    append_deferred_tools_prompt_section(&mut prompt, &tools);
    assert_eq!(prompt.matches("[Deferred Tools]").count(), 1);
}
#[test]
fn deferred_tools_section_omits_empty_and_fully_loaded_groups() {
    let mut prompt = "Base prompt".to_string();
    append_deferred_tools_prompt_section(&mut prompt, &[]);
    assert!(!prompt.contains("[Deferred Tools]"));

    let loaded = ToolDefinition::function(
        "exec_command".to_string(),
        "Shell".to_string(),
        serde_json::json!({"type": "object"}),
    );
    let mut prompt = "Base prompt".to_string();
    append_deferred_tools_prompt_section(&mut prompt, &[loaded]);
    assert!(!prompt.contains("[Deferred Tools]"));
}
#[test]
fn truncate_deferred_description_keeps_boundaries_and_char_safety() {
    // Asymmetric boundaries: exactly at the cap stays verbatim, one over
    // truncates with an ellipsis. Unicode stays on char boundaries.
    let short = "Tools provided by MCP server";
    assert_eq!(truncate_deferred_description(short), short);

    let empty = "";
    assert_eq!(truncate_deferred_description(empty), "");

    let exact: String = "a".repeat(DEFERRED_TOOLS_MAX_DESC_CHARS);
    assert_eq!(truncate_deferred_description(&exact), exact);

    let one_over: String = "a".repeat(DEFERRED_TOOLS_MAX_DESC_CHARS + 1);
    let truncated = truncate_deferred_description(&one_over);
    assert!(truncated.ends_with("..."));
    assert!(!truncated.contains(&"a".repeat(DEFERRED_TOOLS_MAX_DESC_CHARS + 1)));
    assert_eq!(truncated.chars().count(), DEFERRED_TOOLS_MAX_DESC_CHARS + 3);

    // Multi-byte chars at the cut point must not panic or split bytes.
    let unicode_exact = format!("{}🚀", "a".repeat(DEFERRED_TOOLS_MAX_DESC_CHARS - 1));
    assert_eq!(unicode_exact.chars().count(), DEFERRED_TOOLS_MAX_DESC_CHARS);
    assert_eq!(truncate_deferred_description(&unicode_exact), unicode_exact);

    let unicode_over = format!("{}🚀b", "a".repeat(DEFERRED_TOOLS_MAX_DESC_CHARS - 1));
    let unicode_truncated = truncate_deferred_description(&unicode_over);
    assert!(unicode_truncated.ends_with("..."));
    assert!(unicode_truncated.contains("🚀"));

    // Trailing blanks inside the window are trimmed before the ellipsis.
    let padded = format!("{}   {}", "a".repeat(DEFERRED_TOOLS_MAX_DESC_CHARS - 4), "b".repeat(10));
    let padded_truncated = truncate_deferred_description(&padded);
    assert!(padded_truncated.ends_with("..."));
    assert!(!padded_truncated.contains(" ..."));
}
