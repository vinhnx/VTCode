use super::*;

#[test]
fn test_parallel_tool_call_guidance() {
    let tools = vec![
        TOOL_EXEC_COMMAND.to_string(),
        TOOL_CODE_SEARCH.to_string(),
        TOOL_APPLY_PATCH.to_string(),
    ];
    let guidelines = generate_tool_guidelines_for_profile(&tools, None, ResolvedShellPromptProfile::UnixLike, true);
    assert!(guidelines.contains("parallel"), "Should include parallel tool call guidance");
    assert!(guidelines.contains("inputs do not depend"), "Should mention independent inputs");
}
#[test]
fn test_read_only_batching_guidance_is_explicit() {
    let tools = vec![
        TOOL_CODE_SEARCH.to_string(),
        TOOL_READ_FILE.to_string(),
        TOOL_LIST_FILES.to_string(),
    ];
    let guidelines = generate_tool_guidelines_for_profile(&tools, None, ResolvedShellPromptProfile::UnixLike, true);

    assert!(guidelines.contains("Batch independent read-only calls"));
    assert!(guidelines.contains("`read_file` ranges"));
    assert!(guidelines.contains("serialize mutations"));
    // No exec tools in this profile: the verifier truthful-status rule is
    // exec-conditional and must not spend budget here.
    assert!(!guidelines.contains("Only terminal exit 0 clears verification"));
}
#[test]
fn execution_agents_can_suggest_planning_for_demanding_tasks() {
    let tools = vec![TOOL_START_PLANNING.to_string(), TOOL_EXEC_COMMAND.to_string()];
    let guidelines = generate_tool_guidelines_for_profile(&tools, None, ResolvedShellPromptProfile::UnixLike, true);

    assert_eq!(guidelines.matches(START_PLANNING_GUIDANCE_LINE).count(), 1);
    let minimal = generate_tool_guidelines_with_capabilities(
        &tools,
        None,
        ResolvedShellPromptProfile::UnixLike,
        ToolGuidanceProfile::Minimal,
        false,
    );
    assert_eq!(minimal.matches(START_PLANNING_GUIDANCE_LINE).count(), 1);
    // Without the tool, no profile mentions it.
    let without = generate_tool_guidelines_for_profile(
        &[TOOL_EXEC_COMMAND.to_string()],
        None,
        ResolvedShellPromptProfile::UnixLike,
        true,
    );
    assert!(!without.contains("start_planning"));
}
#[test]
fn markdown_validation_is_optional_in_planning_and_execution() {
    let tools = vec![TOOL_EXEC_COMMAND.to_string(), TOOL_READ_FILE.to_string()];
    for planning in [false, true] {
        let guidance =
            generate_runtime_tool_guidelines_for_profile(&tools, planning, ResolvedShellPromptProfile::UnixLike);
        assert_eq!(guidance.matches(OPTIONAL_MARKDOWN_VALIDATION_GUIDANCE).count(), 1);
        assert!(guidance.contains("report skipped"));
        assert!(guidance.contains("without installing tools"));
        assert!(guidance.contains("Lint errors remain failures"));
    }
}
#[test]
fn planning_workflow_runtime_guidance_keeps_exec_read_only() {
    let tools = vec![
        TOOL_APPLY_PATCH.to_string(),
        TOOL_EXEC_COMMAND.to_string(),
        TOOL_CODE_SEARCH.to_string(),
    ];
    let guidelines = generate_runtime_tool_guidelines_for_profile(&tools, true, ResolvedShellPromptProfile::UnixLike);

    assert!(guidelines.contains("Planning workflow active"));
    assert!(guidelines.contains("`exec_command` only for read-only verification"));
    assert!(!guidelines.contains("<proposed_plan>"));
    assert!(!guidelines.contains("Every implementation step"));
    assert!(guidelines.contains("omit unused filters"));
    assert!(!guidelines.contains("Inspect before edit"));
}
#[test]
fn minimal_planning_addendum_prefers_read_tools_over_shell_reads() {
    // Asymmetric pair over read-tool presence: with read tools the
    // inspection line names them and prefers them over shell reads;
    // without, only the generic small-inspection rule remains. Both
    // share the same git-history tail.
    let tail = "start git history with `git log --oneline`";
    let with_readers = ToolPresence::of(&[TOOL_READ_FILE.to_string(), TOOL_CODE_SEARCH.to_string()]);
    let mut guidance = String::new();
    append_minimal_planning_addendum(&mut guidance, &with_readers);
    assert!(guidance.contains("prefer `read_file`/`code_search` over `exec_command` shell reads"));
    assert!(guidance.contains(tail));

    let without_readers = ToolPresence::of(&[TOOL_EXEC_COMMAND.to_string()]);
    let mut bare = String::new();
    append_minimal_planning_addendum(&mut bare, &without_readers);
    assert!(!bare.contains("prefer `"));
    assert!(!bare.contains("over `exec_command` shell reads"));
    assert!(bare.contains(tail));
}
