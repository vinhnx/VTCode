use super::*;

/// Universal rules have one home in Runtime Guidance (or the shared
/// contract). Compose every static profile with each tool-guidance variant
/// and the Harness Limits section, and check each rule marker lands once.
#[test]
fn universal_rules_have_one_home_across_composed_prompt_sections() {
    use crate::config::types::SystemPromptMode;
    use crate::prompts::harness_limits::upsert_harness_limits_section;
    use crate::prompts::static_prompts::static_profile_prompt;
    use crate::prompts::system::PLANNING_WORKFLOW_READ_ONLY_NOTICE_LINE;

    let shell = ResolvedShellPromptProfile::UnixLike;
    let execution_tools = [
        TOOL_EXEC_COMMAND,
        TOOL_WRITE_STDIN,
        TOOL_APPLY_PATCH,
        TOOL_CODE_SEARCH,
        TOOL_TASK_TRACKER,
        TOOL_START_PLANNING,
        TOOL_REQUEST_USER_INPUT,
    ]
    .map(str::to_owned)
    .to_vec();
    let planning_tools = [
        TOOL_EXEC_COMMAND,
        TOOL_WRITE_STDIN,
        TOOL_CODE_SEARCH,
        TOOL_GREP_FILE,
        TOOL_TASK_TRACKER,
        TOOL_REQUEST_USER_INPUT,
    ]
    .map(str::to_owned)
    .to_vec();
    let mut minimal_planning =
        generate_tool_guidelines_with_capabilities(&planning_tools, None, shell, ToolGuidanceProfile::Minimal, false);
    append_minimal_planning_addendum(&mut minimal_planning, &ToolPresence::of(&planning_tools));
    let variants = [
        ("default", generate_tool_guidelines_for_profile(&execution_tools, None, shell, true), false),
        (
            "minimal",
            generate_tool_guidelines_with_capabilities(
                &execution_tools,
                None,
                shell,
                ToolGuidanceProfile::Minimal,
                true,
            ),
            false,
        ),
        ("default planning", generate_runtime_tool_guidelines_for_profile(&planning_tools, true, shell), true),
        ("minimal planning", minimal_planning, true),
    ];
    // Each marker names one universal rule that lives in Runtime Guidance
    // or the shared contract and must not be restated by tool sections.
    let markers = [
        "never claim a check passed",
        "Diagnose failures; change approach",
        "Use returned `next_wait_args`",
        "Treat empty searches as evidence",
        "Check optional tools once",
        "small non-overlapping ranges",
        "accumulated output never exhausts tool access",
        "additional_permissions",
        "bypass safeguards",
        "Delegate only sizeable",
        "Across compaction",
    ];

    for mode in [
        SystemPromptMode::Default,
        SystemPromptMode::Minimal,
        SystemPromptMode::Lightweight,
        SystemPromptMode::Specialized,
    ] {
        for (variant, guidance, planning) in &variants {
            let mut prompt = static_profile_prompt(mode).to_owned();
            if *planning {
                prompt.push_str("\n\n");
                prompt.push_str(PLANNING_WORKFLOW_READ_ONLY_NOTICE_LINE);
            }
            prompt.push_str(guidance);
            upsert_harness_limits_section(&mut prompt, 32, 600, 2);
            for marker in markers {
                assert_eq!(
                    prompt.matches(marker).count(),
                    1,
                    "{mode:?} with {variant} tool guidance should state {marker:?} exactly once"
                );
            }
            let start_planning_mentions = prompt.matches("start_planning").count();
            assert_eq!(start_planning_mentions, usize::from(!*planning), "{mode:?} with {variant} tool guidance");
        }
    }
}
#[test]
fn documentation_profile_respects_context_tokens_and_known_cost() {
    assert_eq!(ToolGuidanceProfile::resolve(32_000, 100, 1000, None, None), ToolGuidanceProfile::Minimal);
    assert_eq!(ToolGuidanceProfile::resolve(1_000_000, 100, 1000, None, Some(0.0)), ToolGuidanceProfile::Default);
    assert_eq!(ToolGuidanceProfile::resolve(1_000_000, 1001, 1000, None, None), ToolGuidanceProfile::Minimal);
    assert_eq!(
        ToolGuidanceProfile::resolve(1_000_000, 1000, 2000, Some(0.00001), Some(0.001)),
        ToolGuidanceProfile::Minimal
    );
}
#[test]
fn minimal_and_default_tool_guidance_snapshots() {
    let tools = vec![TOOL_READ_FILE.to_owned()];
    let minimal = generate_tool_guidelines_with_capabilities(
        &tools,
        None,
        ResolvedShellPromptProfile::UnixLike,
        ToolGuidanceProfile::Minimal,
        false,
    );
    assert_eq!(
        minimal,
        "\n\n## Active Tools\n- `verify: [skip Markdown lint if unavailable]`: report skipped; review diff/links without installing tools. Lint errors remain failures.\n- Capabilities: read-only. Analyze and search, but do not modify files or run shell commands.\n- Use available read-only repository tools for browsing; do not modify files."
    );
    let default = generate_tool_guidelines_with_capabilities(
        &tools,
        None,
        ResolvedShellPromptProfile::UnixLike,
        ToolGuidanceProfile::Default,
        false,
    );
    assert_eq!(
        default,
        "\n\n## Active Tools\n- `verify: [skip Markdown lint if unavailable]`: report skipped; review diff/links without installing tools. Lint errors remain failures.\n- Capabilities: read-only. Analyze and search, but do not modify files or run shell commands.\n- Use available read-only repository tools for browsing; do not modify files.\n- Batch independent read-only calls; use bounded `read_file` ranges, order dependencies, serialize mutations; narrow the range on `line_truncated`; prefer wider windows and reuse evidence over overlapping slices."
    );
}
