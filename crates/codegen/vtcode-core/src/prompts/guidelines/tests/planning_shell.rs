use super::*;

#[test]
fn test_edit_workflow_guidance() {
    let tools = vec![TOOL_APPLY_PATCH.to_string()];
    let guidelines = guidelines_for(&tools, None);
    // The `apply_patch` envelope and "keep patches small" rule are documented
    // by the tool's own description, which reaches the model with the tool
    // definitions. The guidelines section keeps only the cross-tool rule.
    assert!(guidelines.contains("diff stays bounded"));
    assert!(!guidelines.contains("Use `apply_patch`"));
    // Completion-as-checkpoint guidance lives in the operating profiles;
    // the guidelines section no longer repeats it.
    assert!(!guidelines.contains("verification resolved"));
}
#[test]
fn test_vt_code_guidance_omits_task_tracker() {
    let tools = vec![
        TOOL_EXEC_COMMAND.to_string(),
        TOOL_WRITE_STDIN.to_string(),
        TOOL_APPLY_PATCH.to_string(),
    ];
    let guidelines = generate_tool_guidelines_for_profile(&tools, None, ResolvedShellPromptProfile::UnixLike, true);

    assert!(guidelines.contains("exec_command.cmd"));
    // The example commands, the three-background-process cap, `monitor_filter`,
    // and `monitor_digest` are documented by the `exec_command` schema, which
    // reaches the model with the tool definitions; the guidelines section
    // must not restate them.
    for command in ["ls", "rg", "find", "cat", "sed", "awk"] {
        assert!(
            !guidelines.contains(&format!("`{command}`")),
            "{command} is documented by the exec_command schema, not the prompt"
        );
    }
    assert!(!guidelines.contains("At most three live background processes"));
    assert!(!guidelines.contains("`monitor_filter`"));
    assert!(!guidelines.contains("`monitor_digest`"));
    assert!(!guidelines.contains("task_tracker"));
    assert!(!guidelines.contains("list_files"));
    assert!(!guidelines.contains("read_file"));
}
#[test]
fn task_tracker_guidance_explains_action_aware_indices() {
    let guidelines = generate_runtime_tool_guidelines_for_profile(
        &[TOOL_TASK_TRACKER.to_string()],
        true,
        ResolvedShellPromptProfile::UnixLike,
    );

    assert!(guidelines.contains(&format!("\n{PLANNING_TASK_TRACKER_LINE}")));
    assert!(PLANNING_TASK_TRACKER_LINE.contains("keep blockers and verification open"));
    // The tracker call shapes (list/update/bulk, index vs. index_path vs.
    // items) and the planning index rule are documented by the tool
    // description and parameter schema, which reach the model with the tool
    // definitions. The prompt must not restate them.
    for shape in [
        r#"{"action":"list"}"#,
        r#"{"action":"update","index":1,"status":"in_progress"}"#,
        r#"{"action":"update","items":["[x] Done","[ ] Remaining"]}"#,
    ] {
        assert!(!PLANNING_TASK_TRACKER_LINE.contains(shape), "{shape} belongs to the tool schema");
    }
    assert!(!PLANNING_TASK_TRACKER_LINE.contains("index 0 is invalid while planning"));
    // The planning sidecar rejects index 0, so no planning line may present
    // it as a valid checklist-completion index.
    let line = PLANNING_TASK_TRACKER_LINE;
    assert!(line.contains("`scope_review` checks cannot satisfy functional `verify` checks"));
    assert!(line.len() <= 570, "keep tracker guidance bounded");
    assert!(!line.contains("reserved"), "{line}");
    assert!(!line.contains("index: 0"), "{line}");
}
#[test]
fn unix_like_guidance_makes_command_reuse_explicit() {
    let tools = vec![TOOL_EXEC_COMMAND.to_string(), TOOL_WRITE_STDIN.to_string()];
    let guidelines = generate_tool_guidelines_for_profile(&tools, None, ResolvedShellPromptProfile::UnixLike, true);

    assert!(guidelines.contains("one-shot `exec_command` calls"));
    assert!(guidelines.contains("`!!`, `!$`, `!ssh`, or `fc`"));
    assert!(guidelines.contains("write full command arguments explicitly"));
    assert!(guidelines.contains("conversation or tool results"));
    assert!(guidelines.contains("Bash `histverify`"));
    assert!(guidelines.contains("zsh `HIST_VERIFY`"));
    // Session-id reuse, `background: true`, and the cross-turn resume hint are
    // documented by the `exec_command`/`write_stdin` descriptions.
    assert!(!guidelines.contains("existing `session_id`"));
    assert!(!guidelines.contains("`Exec session resume:`"));
    // No `code_search` in this profile: the search-preference clause
    // must not spend budget naming an unavailable tool.
    assert!(!guidelines.contains("Prefer `code_search` over `rg`"));
}
#[test]
fn write_stdin_guidance_advertises_cross_turn_resume_hint() {
    let tools = vec![TOOL_WRITE_STDIN.to_string()];
    let default_guidance =
        generate_tool_guidelines_for_profile(&tools, None, ResolvedShellPromptProfile::UnixLike, true);
    // The cross-turn resume hint (invariant #22) is part of the
    // `write_stdin` description, which reaches the model with the tool
    // definitions, so the guidelines section does not repeat it.
    assert!(!default_guidance.contains("`Exec session resume:`"));
    assert!(!default_guidance.contains("prior turn ended mid-run"));
    assert!(!default_guidance.contains("if missing, recover prior output"));
    assert!(!default_guidance.contains("rerun only for fresh results"));

    let minimal = generate_tool_guidelines_with_capabilities(
        &tools,
        None,
        ResolvedShellPromptProfile::UnixLike,
        ToolGuidanceProfile::Minimal,
        false,
    );
    assert!(!minimal.contains("`Exec session resume:`"));
    assert!(!minimal.contains("prior turn ended mid-run"));
}
#[test]
fn powershell_guidance_uses_native_command_examples() {
    let tools = vec![
        TOOL_EXEC_COMMAND.to_string(),
        TOOL_CODE_SEARCH.to_string(),
        TOOL_APPLY_PATCH.to_string(),
    ];
    let guidelines = generate_tool_guidelines_for_profile(&tools, None, ResolvedShellPromptProfile::PowerShell, true);

    assert!(guidelines.contains("native PowerShell syntax"));
    assert!(guidelines.contains("Prefer `code_search` over `Select-String` for code"));
    assert!(guidelines.contains("literal and smart-case"));
    // The native command examples are documented by the exec_command schema,
    // and the code_search filter shape by the code_search schema.
    assert!(!guidelines.contains("`Get-ChildItem`"));
    // `Select-String` appears only in the code_search preference clause.
    assert!(guidelines.contains("Prefer `code_search` over `Select-String` for code"));
    assert!(!guidelines.contains("omit unused filters"));
    assert!(!guidelines.contains("`ls`, `rg`, `find`, `cat`, `sed`, and `awk`"));
    assert!(!guidelines.contains("shell history expansion"));
    assert!(!guidelines.contains("histverify"));
    assert!(!guidelines.contains("HIST_VERIFY"));
}
#[test]
fn shell_profile_prompt_keeps_policy_and_syntax_separate() {
    let unix = render_shell_profile_guidance(ResolvedShellPromptProfile::UnixLike);
    assert!(unix.contains("Active shell profile: `unix_like`"));
    assert!(unix.contains("does not rewrite GNU flags for macOS BSD tools"));
    // Session-vtcode-20261111T131200Z: `${PIPESTATUS[0]}` expanded empty under
    // zsh and hid the exit code. The profile must name the login-shell
    // dialect hazard and the portable alternative.
    assert!(unix.contains("user's login shell, which may be zsh"));
    assert!(unix.contains("${PIPESTATUS[...]}"));
    assert!(unix.contains("harness-reported exit status"));
    assert!(unix.contains("controls prompt examples and expected command syntax only"));
    assert!(unix.contains("does not translate GNU-to-BSD"));

    let powershell = render_shell_profile_guidance(ResolvedShellPromptProfile::PowerShell);
    assert!(powershell.contains("Active shell profile: `powershell`"));
    assert!(powershell.contains("WSL"));
    assert!(powershell.contains("Unix-like workflows"));
    assert!(powershell.contains("PowerShell-to-Unix"));
}
#[test]
fn test_harness_browse_tool_guidance() {
    let tools = vec![TOOL_LIST_FILES.to_string(), TOOL_READ_FILE.to_string()];
    let guidelines = guidelines_for(&tools, None);
    assert!(guidelines.contains("available read-only repository tools"));
    assert!(guidelines.contains("bounded `read_file` ranges"));
    assert!(!guidelines.contains("list_files"));
    assert!(!guidelines.contains("offset"));
    assert!(!guidelines.contains("per_page"));
}
#[test]
fn test_canonical_browse_tool_guidance_prefers_public_tools() {
    let tools = vec![
        TOOL_CODE_SEARCH.to_string(),
        TOOL_LIST_FILES.to_string(),
        "read_file".to_string(),
    ];
    let guidelines = guidelines_for(&tools, None);
    assert!(guidelines.contains("available read-only repository tools"));
    assert!(guidelines.contains("code_search"));
    assert!(guidelines.contains("bounded `read_file` ranges"));
}
