use super::*;

#[test]
fn runtime_tool_guidance_uses_explicit_powershell_profile() {
    let tools = vec![
        TOOL_APPLY_PATCH.to_string(),
        TOOL_EXEC_COMMAND.to_string(),
        TOOL_CODE_SEARCH.to_string(),
    ];
    let guidelines =
        generate_runtime_tool_guidelines_for_profile(&tools, false, ResolvedShellPromptProfile::PowerShell);

    assert!(guidelines.contains("native PowerShell commands"));
    assert!(guidelines.contains("`Get-ChildItem`"));
    assert!(guidelines.contains("`Select-String`"));
    assert!(guidelines.contains("native PowerShell syntax"));
    assert!(!guidelines.contains("`ls`, `rg`, `find`, `cat`, `sed`, and `awk`"));
}
#[test]
fn runtime_tool_guidance_uses_explicit_unix_like_profile() {
    let tools = vec![
        TOOL_APPLY_PATCH.to_string(),
        TOOL_EXEC_COMMAND.to_string(),
        TOOL_CODE_SEARCH.to_string(),
    ];
    let guidelines = generate_runtime_tool_guidelines_for_profile(&tools, false, ResolvedShellPromptProfile::UnixLike);

    assert!(guidelines.contains("`ls`, `find`, `cat`, `sed`, and `awk` for repository browsing"));
    assert!(guidelines.contains("Prefer `code_search` over `rg`/`grep` for code"));
    assert!(guidelines.contains("Advanced `code_search` takes `query`"));
    assert!(guidelines.contains("literal smart-case"));
    assert!(guidelines.contains("shell-only tasks"));
    assert!(!guidelines.contains("native PowerShell commands"));
    assert!(!guidelines.contains("`Get-ChildItem`"));
}
#[test]
fn shell_browse_guidance_lists_rg_only_without_code_search() {
    // Asymmetric pair over `has_search`: without the dedicated search
    // tool the browse line must name `rg`; with it, `rg` drops out in
    // favor of the `code_search` preference suffix.
    let without_search = shell_browse_guidance(ResolvedShellPromptProfile::UnixLike, false);
    assert!(without_search.contains("`ls`, `rg`, `find`, `cat`, `sed`, and `awk` for repository browsing"));
    assert!(!without_search.contains("Prefer `code_search`"));

    let with_search = shell_browse_guidance(ResolvedShellPromptProfile::UnixLike, true);
    assert!(with_search.contains("`ls`, `find`, `cat`, `sed`, and `awk` for repository browsing"));
    assert!(!with_search.contains("`ls`, `rg`, `find`"));
    assert!(with_search.contains("Prefer `code_search` over `rg`/`grep` for code"));
}
#[test]
fn runtime_tool_prompt_sections_use_explicit_profile_for_active_tools() {
    let mut powershell_prompt = "Base prompt".to_string();
    let mut unix_prompt = "Base prompt".to_string();
    let snapshot = SessionToolCatalogSnapshot::new(
        7,
        9,
        false,
        false,
        Some(std::sync::Arc::new(vec![
            ToolDefinition::function(
                TOOL_EXEC_COMMAND.to_string(),
                "Shell".to_string(),
                serde_json::json!({"type": "object"}),
            ),
            ToolDefinition::function(
                TOOL_CODE_SEARCH.to_string(),
                "Bounded source search".to_string(),
                serde_json::json!({"type": "object"}),
            ),
        ])),
        false,
    );

    append_runtime_tool_prompt_sections_for_profile(
        &mut powershell_prompt,
        &snapshot,
        false,
        ResolvedShellPromptProfile::PowerShell,
    );
    append_runtime_tool_prompt_sections_for_profile(
        &mut unix_prompt,
        &snapshot,
        false,
        ResolvedShellPromptProfile::UnixLike,
    );

    assert!(powershell_prompt.contains("## Active Tools"));
    assert!(powershell_prompt.contains("`Get-ChildItem`"));
    assert!(powershell_prompt.contains("`Select-String`"));
    assert!(!powershell_prompt.contains("`ls`, `rg`, `find`, `cat`, `sed`, and `awk`"));

    assert!(unix_prompt.contains("## Active Tools"));
    assert!(unix_prompt.contains("`ls`, `find`, `cat`, `sed`, and `awk` for repository browsing"));
    assert!(unix_prompt.contains("Prefer `code_search` over `rg`/`grep` for code"));
    assert!(unix_prompt.contains("Advanced `code_search` takes `query`"));
    assert!(unix_prompt.contains("literal smart-case"));
    assert!(!unix_prompt.contains("`Get-ChildItem`"));
}
#[test]
fn runtime_tool_prompt_sections_include_catalog_metadata() {
    let mut prompt = "Base prompt".to_string();
    let snapshot = SessionToolCatalogSnapshot::new(
        7,
        9,
        true,
        false,
        Some(std::sync::Arc::new(vec![
            ToolDefinition::function(
                TOOL_EXEC_COMMAND.to_string(),
                "Search".to_string(),
                serde_json::json!({"type": "object"}),
            ),
            ToolDefinition::function(
                TOOL_APPLY_PATCH.to_string(),
                "File".to_string(),
                serde_json::json!({"type": "object"}),
            ),
        ])),
        false,
    );

    append_runtime_tool_prompt_sections_for_profile(
        &mut prompt,
        &snapshot,
        true,
        ShellPromptProfile::Auto.resolve_for_current_platform(),
    );

    assert!(prompt.contains("## Active Tools"));
    assert!(prompt.contains("[Runtime Tool Catalog]"));
    assert!(prompt.contains("catalog_tools: 2"));
    assert!(prompt.contains("currently_available_tools: exec_command, apply_patch"));
    assert!(prompt.contains("request_user_input_enabled: false"));
}
#[test]
fn runtime_tool_prompt_sections_replace_existing_runtime_sections() {
    let mut prompt = "Base prompt".to_string();
    let first = SessionToolCatalogSnapshot::new(
        1,
        2,
        false,
        false,
        Some(std::sync::Arc::new(vec![ToolDefinition::function(
            TOOL_EXEC_COMMAND.to_string(),
            "Search".to_string(),
            serde_json::json!({"type": "object"}),
        )])),
        false,
    );
    let second = SessionToolCatalogSnapshot::new(
        7,
        9,
        true,
        true,
        Some(std::sync::Arc::new(vec![ToolDefinition::function(
            TOOL_APPLY_PATCH.to_string(),
            "File".to_string(),
            serde_json::json!({"type": "object"}),
        )])),
        false,
    );

    let shell = ShellPromptProfile::Auto.resolve_for_current_platform();
    append_runtime_tool_prompt_sections_for_profile(&mut prompt, &first, true, shell);
    append_runtime_tool_prompt_sections_for_profile(&mut prompt, &second, true, shell);

    assert_eq!(prompt.matches("## Active Tools").count(), 1);
    assert_eq!(prompt.matches("[Runtime Tool Catalog]").count(), 1);
    assert!(prompt.contains("version: 7"));
    assert!(!prompt.contains("version: 1"));
    assert!(prompt.contains("request_user_input_enabled: true"));
    assert!(!prompt.contains("request_user_input_enabled: false"));
}
