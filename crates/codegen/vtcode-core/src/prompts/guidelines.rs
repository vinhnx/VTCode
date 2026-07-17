use std::collections::BTreeSet;
use std::fmt::Write as _;

use crate::config::constants::tools;
use crate::config::types::{CapabilityLevel, ResolvedShellPromptProfile, ShellPromptProfile};
use crate::core::agent::harness_kernel::SessionToolCatalogSnapshot;
use crate::llm::provider::ToolDefinition;
use crate::prompts::sections::SectionBoundaryMode;
use crate::tools::registry::tool_groups;

const TOOL_EXEC_COMMAND: &str = tools::EXEC_COMMAND;
const TOOL_WRITE_STDIN: &str = tools::WRITE_STDIN;
const TOOL_CODE_SEARCH: &str = tools::CODE_SEARCH;
const TOOL_READ_FILE: &str = tools::READ_FILE;
const TOOL_LIST_FILES: &str = tools::LIST_FILES;
const TOOL_APPLY_PATCH: &str = tools::APPLY_PATCH;
const TOOL_REQUEST_USER_INPUT: &str = tools::REQUEST_USER_INPUT;
const TOOL_TASK_TRACKER: &str = tools::TASK_TRACKER;

/// Generate compact cross-tool guidance based on the tools available in the session.
pub fn generate_tool_guidelines(
    available_tools: &[String],
    capability_level: Option<CapabilityLevel>,
) -> String {
    generate_tool_guidelines_for_profile(
        available_tools,
        capability_level,
        ShellPromptProfile::Auto.resolve_for_current_platform(),
    )
}

/// Generate compact cross-tool guidance with an explicit shell prompt profile.
pub fn generate_tool_guidelines_for_profile(
    available_tools: &[String],
    capability_level: Option<CapabilityLevel>,
    shell_profile: ResolvedShellPromptProfile,
) -> String {
    let has_exec = available_tools.iter().any(|tool| tool == TOOL_EXEC_COMMAND);
    let has_stdin = available_tools.iter().any(|tool| tool == TOOL_WRITE_STDIN);
    let has_search = available_tools.iter().any(|tool| tool == TOOL_CODE_SEARCH);
    let has_read_file = available_tools.iter().any(|tool| tool == TOOL_READ_FILE);
    let has_list_files = available_tools.iter().any(|tool| tool == TOOL_LIST_FILES);
    let has_apply_patch = available_tools.iter().any(|tool| tool == TOOL_APPLY_PATCH);

    let mut lines = Vec::new();
    if let Some(mode_line) = capability_mode_line(capability_level, has_exec, has_apply_patch) {
        lines.push(mode_line.to_string());
    }
    if let Some(browse_guidance) = browse_tool_guidance(
        has_exec,
        has_search,
        has_list_files,
        has_read_file,
        shell_profile,
    ) {
        lines.push(browse_guidance);
    }
    if has_apply_patch {
        lines.push(
            "- Use `apply_patch` for file edits after inspection; keep patches small.".to_string(),
        );
    }
    if has_exec {
        lines.push(shell_task_guidance(shell_profile).to_string());
    }
    if has_stdin {
        lines.push("- Use `write_stdin` only with an active exec_command session.".to_string());
    }
    if has_exec || has_apply_patch {
        lines.push("- Completion is a checkpoint: keep verification resolved.".to_string());
    }
    if has_search {
        lines.push(code_search_guidance(has_exec, shell_profile).to_string());
    }
    if has_apply_patch || has_exec {
        lines.push("- If calls repeat, re-plan instead of retrying.".to_string());
    }
    if has_search || has_exec {
        lines.push(
            "- Run independent tools in parallel when their inputs do not depend on each other."
                .to_string(),
        );
    }

    if lines.is_empty() {
        return String::new();
    }

    format!("\n\n## Active Tools\n{}", lines.join("\n"))
}

pub fn append_runtime_tool_prompt_sections(
    prompt: &mut String,
    tool_snapshot: &SessionToolCatalogSnapshot,
    include_catalog_metadata: bool,
) {
    append_runtime_tool_prompt_sections_for_profile(
        prompt,
        tool_snapshot,
        include_catalog_metadata,
        ShellPromptProfile::Auto.resolve_for_current_platform(),
    );
}

pub fn append_runtime_tool_prompt_sections_for_profile(
    prompt: &mut String,
    tool_snapshot: &SessionToolCatalogSnapshot,
    include_catalog_metadata: bool,
    shell_profile: ResolvedShellPromptProfile,
) {
    remove_prompt_section(prompt, "## Active Tools");
    remove_prompt_section(prompt, "[Runtime Tool Catalog]");
    while prompt.ends_with('\n') {
        prompt.pop();
    }

    let available_tools = snapshot_tool_names(tool_snapshot);
    let guidelines = generate_runtime_tool_guidelines_for_profile(
        &available_tools,
        tool_snapshot.planning_active,
        shell_profile,
    );
    if !guidelines.is_empty() {
        append_prompt_block(prompt, guidelines.trim_start_matches('\n'));
    }

    if include_catalog_metadata && tool_snapshot.snapshot.is_some() {
        let active_tools = if tool_snapshot.active_tool_names.is_empty() {
            "none".to_string()
        } else {
            tool_snapshot.active_tool_names.join(", ")
        };
        let catalog_metadata = format!(
            "[Runtime Tool Catalog]\n- version: {}\n- epoch: {}\n- catalog_tools: {}\n- available_tools: {}\n- currently_available_tools: {}\n- request_user_input_enabled: {}",
            tool_snapshot.version,
            tool_snapshot.epoch,
            tool_snapshot.catalog_tools(),
            tool_snapshot.available_tools(),
            active_tools,
            tool_snapshot.request_user_input_enabled,
        );
        append_prompt_block(prompt, &catalog_metadata);
    }
}

/// Append a compact summary of tools omitted from a client-local wire payload.
pub fn append_deferred_tools_prompt_section(prompt: &mut String, tools: &[ToolDefinition]) {
    remove_prompt_section(prompt, "[Deferred Tools]");

    let mut lines: Vec<String> = tool_groups(tools)
        .into_iter()
        .filter(|group| group.deferred_count > 0)
        .map(|group| {
            format!(
                "- {} ({} tools): {}",
                group.name,
                group.deferred_count,
                group.description.unwrap_or_default()
            )
        })
        .collect();

    let unnamespaced_deferred = tools
        .iter()
        .filter(|tool| tool.namespace.is_none() && tool.defer_loading == Some(true))
        .count();
    if unnamespaced_deferred > 0 {
        lines.push(format!(
            "- {unnamespaced_deferred} additional deferred tools"
        ));
    }

    if lines.is_empty() {
        return;
    }

    let section = format!(
        "[Deferred Tools]\n{}\nUse the relevant discovery tool to load a deferred capability before calling it.",
        lines.join("\n")
    );
    append_prompt_block(prompt, &section);
}

fn append_prompt_block(prompt: &mut String, block: &str) {
    if block.is_empty() {
        return;
    }

    if prompt.is_empty() {
        prompt.push_str(block);
    } else {
        let _ = write!(prompt, "\n\n{block}");
    }
}

fn remove_prompt_section(prompt: &mut String, section_header: &str) {
    while let Some((section_start, section_end)) =
        find_prompt_section_bounds(prompt, section_header)
    {
        prompt.replace_range(section_start..section_end, "");
    }
}

fn find_prompt_section_bounds(prompt: &str, section_header: &str) -> Option<(usize, usize)> {
    crate::prompts::sections::find_prompt_section_bounds(
        prompt,
        section_header,
        SectionBoundaryMode::BracketOrMarkdown,
    )
}

fn generate_runtime_tool_guidelines_for_profile(
    available_tools: &[String],
    planning_active: bool,
    shell_profile: ResolvedShellPromptProfile,
) -> String {
    if !planning_active {
        return generate_tool_guidelines_for_profile(available_tools, None, shell_profile);
    }

    let has_exec = available_tools.iter().any(|tool| tool == TOOL_EXEC_COMMAND);
    let has_search = available_tools.iter().any(|tool| tool == TOOL_CODE_SEARCH);
    let has_read_file = available_tools.iter().any(|tool| tool == TOOL_READ_FILE);
    let has_list_files = available_tools.iter().any(|tool| tool == TOOL_LIST_FILES);
    let has_request_user_input = available_tools
        .iter()
        .any(|tool| tool == TOOL_REQUEST_USER_INPUT);
    let has_task_tracker = available_tools
        .iter()
        .any(|tool| matches!(tool.as_str(), TOOL_TASK_TRACKER));

    let mut lines =
        vec!["- Planning workflow active: stay within the read-safe tool list.".to_string()];
    if let Some(browse_guidance) = browse_tool_guidance(
        has_exec,
        has_search,
        has_list_files,
        has_read_file,
        shell_profile,
    ) {
        lines.push(browse_guidance);
    }
    if has_exec {
        lines.push(
            "- In Planning workflow, use `exec_command` only for read-only verification."
                .to_string(),
        );
    }
    if has_task_tracker {
        lines.push("- Keep `task_tracker` updated as you refine the plan.".to_string());
        lines.push(
            "- Keep blockers and verification open in `task_tracker` until resolved.".to_string(),
        );
    }
    if has_request_user_input {
        lines.push(
            "- Use `request_user_input` only for material blockers that remain after repository exploration."
                .to_string(),
        );
    }
    if has_search || has_exec {
        lines.push(
            "- If calls repeat without progress, tighten the plan instead of retrying identically."
                .to_string(),
        );
    }

    format!("\n\n## Active Tools\n{}", lines.join("\n"))
}

fn snapshot_tool_names(tool_snapshot: &SessionToolCatalogSnapshot) -> Vec<String> {
    tool_snapshot
        .active_tool_names
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn browse_tool_guidance(
    has_exec: bool,
    has_search: bool,
    has_list_files: bool,
    has_read_file: bool,
    shell_profile: ResolvedShellPromptProfile,
) -> Option<String> {
    if has_exec {
        return Some(shell_browse_guidance(shell_profile).to_string());
    }

    if !(has_search || has_list_files || has_read_file) {
        return None;
    }

    Some(
        "- Use available read-only repository tools for browsing; do not modify files.".to_string(),
    )
}

pub fn render_shell_profile_guidance(shell_profile: ResolvedShellPromptProfile) -> String {
    match shell_profile {
        ResolvedShellPromptProfile::UnixLike => {
            "## Shell Profile\n- Active shell profile: `unix_like`. Use Unix-like command syntax in `exec_command.cmd`, for example `ls`, `rg`, `find`, `cat`, `sed`, and `awk`.\n- On macOS, write BSD-compatible flags for BSD tools. VT Code does not rewrite GNU flags for macOS BSD tools.\n- The shell profile controls prompt examples and expected command syntax only; command policy, sandboxing, and approvals remain separate runtime checks.\n- VT Code does not translate GNU-to-BSD, BSD-to-GNU, Unix-to-PowerShell, or PowerShell-to-Unix command flags.".to_string()
        }
        ResolvedShellPromptProfile::PowerShell => {
            "## Shell Profile\n- Active shell profile: `powershell`. Use native PowerShell syntax in `exec_command.cmd`, for example `Get-ChildItem`, `Select-String`, `Get-Content`, and `Where-Object`.\n- On native Windows, use WSL when you need Unix-like workflows or Unix command examples.\n- The shell profile controls prompt examples and expected command syntax only; command policy, sandboxing, and approvals remain separate runtime checks.\n- VT Code does not translate GNU-to-BSD, BSD-to-GNU, Unix-to-PowerShell, or PowerShell-to-Unix command flags.".to_string()
        }
    }
}

fn shell_browse_guidance(shell_profile: ResolvedShellPromptProfile) -> &'static str {
    match shell_profile {
        ResolvedShellPromptProfile::UnixLike => {
            "- Use `exec_command.cmd` with `ls`, `rg`, `find`, `cat`, `sed`, and `awk` for repository browsing."
        }
        ResolvedShellPromptProfile::PowerShell => {
            "- Use `exec_command.cmd` with native PowerShell commands such as `Get-ChildItem`, `Select-String`, `Get-Content`, and `Where-Object` for repository browsing."
        }
    }
}

fn shell_task_guidance(shell_profile: ResolvedShellPromptProfile) -> &'static str {
    match shell_profile {
        ResolvedShellPromptProfile::UnixLike => {
            "- Use `exec_command.cmd` for build tools, test tools, `git diff -- <path>`, and shell-only tasks."
        }
        ResolvedShellPromptProfile::PowerShell => {
            "- Use `exec_command.cmd` for build tools, test tools, `git diff -- <path>`, and shell-only tasks using native PowerShell syntax."
        }
    }
}

fn code_search_guidance(has_exec: bool, shell_profile: ResolvedShellPromptProfile) -> &'static str {
    match (has_exec, shell_profile) {
        (true, ResolvedShellPromptProfile::UnixLike) => {
            "- Advanced `code_search` takes `query` plus optional `path`, `file_types`, `result_types`, and `max_results`; results are recognised definitions, exact syntactic usages that are not resolved references, literal text, and matching paths. Queries use literal smart-case. If results are truncated, narrow a filter in another call. Use `exec_command` or a specialised skill for arbitrary syntax-pattern work."
        }
        (true, ResolvedShellPromptProfile::PowerShell) => {
            "- Advanced `code_search` takes `query` plus optional `path`, `file_types`, `result_types`, and `max_results`; results are recognised definitions, exact syntactic usages that are not resolved references, literal text, and matching paths. Queries use literal smart-case. If results are truncated, narrow a filter in another call. Use `exec_command` or a specialised skill for arbitrary syntax-pattern work."
        }
        (false, _) => {
            "- Advanced `code_search` takes `query` plus optional `path`, `file_types`, `result_types`, and `max_results`; results are recognised definitions, exact syntactic usages that are not resolved references, literal text, and matching paths. Queries use literal smart-case. If results are truncated, narrow a filter in another call."
        }
    }
}

fn capability_mode_line(
    capability_level: Option<CapabilityLevel>,
    has_exec: bool,
    has_file: bool,
) -> Option<&'static str> {
    match capability_level {
        Some(CapabilityLevel::Basic) => Some(
            "- Capabilities: limited. Ask the user to enable more capabilities if file work is required.",
        ),
        Some(CapabilityLevel::FileReading | CapabilityLevel::FileListing) => Some(
            "- Capabilities: read-only. Analyze and search, but do not modify files or run shell commands.",
        ),
        _ if !has_exec && !has_file => Some(
            "- Capabilities: read-only. Analyze and search, but do not modify files or run shell commands.",
        ),
        _ => None,
    }
}

/// Infer capability level from available tools.
pub fn infer_capability_level(available_tools: &[String]) -> CapabilityLevel {
    let has_search = available_tools.iter().any(|t| t == TOOL_CODE_SEARCH);
    let has_edit = available_tools.iter().any(|t| t == TOOL_APPLY_PATCH);
    let has_read = has_edit || available_tools.iter().any(|t| t == TOOL_READ_FILE);
    let has_list = has_search || available_tools.iter().any(|t| t == TOOL_LIST_FILES);
    let has_exec = available_tools.iter().any(|t| t == TOOL_EXEC_COMMAND);

    if has_search {
        CapabilityLevel::CodeSearch
    } else if has_edit {
        CapabilityLevel::Editing
    } else if has_exec {
        CapabilityLevel::Bash
    } else if has_list {
        CapabilityLevel::FileListing
    } else if has_read {
        CapabilityLevel::FileReading
    } else {
        CapabilityLevel::Basic
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_only_capability_detection() {
        let tools = vec![TOOL_CODE_SEARCH.to_string()];
        let guidelines = generate_tool_guidelines(&tools, None);
        assert!(guidelines.contains("Capabilities: read-only"));
        assert!(guidelines.contains("do not modify files"));
    }

    #[test]
    fn test_tool_preference_guidance() {
        let tools = vec![TOOL_EXEC_COMMAND.to_string(), TOOL_CODE_SEARCH.to_string()];
        let guidelines = generate_tool_guidelines_for_profile(
            &tools,
            None,
            ResolvedShellPromptProfile::UnixLike,
        );
        assert!(guidelines.contains("Advanced `code_search` takes `query`"));
        assert!(guidelines.contains("literal smart-case"));
        assert!(guidelines.contains("exact syntactic usages"));
        assert!(guidelines.contains("git diff -- <path>"));
        assert!(guidelines.contains("build tools"));
        assert!(guidelines.contains("test tools"));
        assert!(guidelines.contains("Completion is a checkpoint"));
    }

    #[test]
    fn test_edit_workflow_guidance() {
        let tools = vec![TOOL_APPLY_PATCH.to_string()];
        let guidelines = generate_tool_guidelines(&tools, None);
        assert!(guidelines.contains("Use `apply_patch`"));
        assert!(guidelines.contains("patches small"));
        assert!(guidelines.contains("verification resolved"));
    }

    #[test]
    fn test_codex_default_guidance_omits_task_tracker() {
        let tools = vec![
            TOOL_EXEC_COMMAND.to_string(),
            TOOL_WRITE_STDIN.to_string(),
            TOOL_APPLY_PATCH.to_string(),
        ];
        let guidelines = generate_tool_guidelines_for_profile(
            &tools,
            None,
            ResolvedShellPromptProfile::UnixLike,
        );

        assert!(guidelines.contains("exec_command.cmd"));
        for command in ["ls", "rg", "find", "cat", "sed", "awk"] {
            assert!(
                guidelines.contains(&format!("`{command}`")),
                "{command} should be shown as an exec_command.cmd example"
            );
        }
        assert!(guidelines.contains("`write_stdin`"));
        assert!(guidelines.contains("`apply_patch`"));
        assert!(!guidelines.contains("task_tracker"));
        assert!(!guidelines.contains("list_files"));
        assert!(!guidelines.contains("read_file"));
    }

    #[test]
    fn powershell_guidance_uses_native_command_examples() {
        let tools = vec![
            TOOL_EXEC_COMMAND.to_string(),
            TOOL_CODE_SEARCH.to_string(),
            TOOL_APPLY_PATCH.to_string(),
        ];
        let guidelines = generate_tool_guidelines_for_profile(
            &tools,
            None,
            ResolvedShellPromptProfile::PowerShell,
        );

        assert!(guidelines.contains("native PowerShell commands"));
        assert!(guidelines.contains("`Get-ChildItem`"));
        assert!(guidelines.contains("`Select-String`"));
        assert!(guidelines.contains("native PowerShell syntax"));
        assert!(guidelines.contains("Advanced `code_search` takes `query`"));
        assert!(guidelines.contains("literal smart-case"));
        assert!(!guidelines.contains("`ls`, `rg`, `find`, `cat`, `sed`, and `awk`"));
    }

    #[test]
    fn shell_profile_prompt_keeps_policy_and_syntax_separate() {
        let unix = render_shell_profile_guidance(ResolvedShellPromptProfile::UnixLike);
        assert!(unix.contains("Active shell profile: `unix_like`"));
        assert!(unix.contains("does not rewrite GNU flags for macOS BSD tools"));
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
        let guidelines = generate_tool_guidelines(&tools, None);
        assert!(guidelines.contains("available read-only repository tools"));
        assert!(!guidelines.contains("read_file"));
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
        let guidelines = generate_tool_guidelines(&tools, None);
        assert!(guidelines.contains("available read-only repository tools"));
        assert!(guidelines.contains("code_search"));
        assert!(!guidelines.contains("read_file"));
    }

    #[test]
    fn test_capability_basic_guidance() {
        let tools = vec![];
        let guidelines = generate_tool_guidelines(&tools, Some(CapabilityLevel::Basic));
        assert!(guidelines.contains("Capabilities: limited"));
        assert!(guidelines.contains("enable more capabilities"));
    }

    #[test]
    fn test_capability_file_reading_guidance() {
        let tools = vec![TOOL_APPLY_PATCH.to_string()];
        let guidelines = generate_tool_guidelines(&tools, Some(CapabilityLevel::FileReading));
        assert!(guidelines.contains("Capabilities: read-only"));
        assert!(guidelines.contains("do not modify"));
    }

    #[test]
    fn test_full_capabilities_no_special_guidance() {
        let tools = vec![
            TOOL_APPLY_PATCH.to_string(),
            TOOL_EXEC_COMMAND.to_string(),
            TOOL_CODE_SEARCH.to_string(),
        ];
        let guidelines = generate_tool_guidelines_for_profile(
            &tools,
            Some(CapabilityLevel::Editing),
            ResolvedShellPromptProfile::UnixLike,
        );

        assert!(!guidelines.contains("Capabilities: limited"));
        assert!(!guidelines.contains("Capabilities: read-only"));
    }

    #[test]
    fn test_empty_tools_shows_read_only_capabilities() {
        let tools = vec![];
        let guidelines = generate_tool_guidelines(&tools, None);
        assert!(guidelines.contains("Capabilities: read-only"));
    }

    #[test]
    fn test_planning_workflow_guidance_keeps_verification_open() {
        let tools = vec![
            TOOL_EXEC_COMMAND.to_string(),
            TOOL_TASK_TRACKER.to_string(),
            TOOL_CODE_SEARCH.to_string(),
        ];
        let guidelines = generate_runtime_tool_guidelines_for_profile(
            &tools,
            true,
            ResolvedShellPromptProfile::UnixLike,
        );
        assert!(guidelines.contains("Keep `task_tracker` updated"));
        assert!(guidelines.contains("blockers and verification open"));
    }

    #[test]
    fn test_capability_inference_precedence() {
        let tools = vec![TOOL_APPLY_PATCH.to_string(), TOOL_CODE_SEARCH.to_string()];
        assert_eq!(infer_capability_level(&tools), CapabilityLevel::CodeSearch);

        let tools = vec![TOOL_EXEC_COMMAND.to_string(), TOOL_APPLY_PATCH.to_string()];
        assert_eq!(infer_capability_level(&tools), CapabilityLevel::Editing);
    }

    #[test]
    fn test_capability_inference_variants() {
        let tools = vec![TOOL_APPLY_PATCH.to_string()];
        assert_eq!(infer_capability_level(&tools), CapabilityLevel::Editing);

        let tools = vec![TOOL_EXEC_COMMAND.to_string()];
        assert_eq!(infer_capability_level(&tools), CapabilityLevel::Bash);

        let tools = vec![TOOL_CODE_SEARCH.to_string()];
        assert_eq!(infer_capability_level(&tools), CapabilityLevel::CodeSearch);

        let tools = vec![TOOL_LIST_FILES.to_string()];
        assert_eq!(infer_capability_level(&tools), CapabilityLevel::FileListing);

        let tools = vec!["read_file".to_string()];
        assert_eq!(infer_capability_level(&tools), CapabilityLevel::FileReading);

        let tools = vec!["unknown_tool".to_string()];
        assert_eq!(infer_capability_level(&tools), CapabilityLevel::Basic);
    }

    #[test]
    fn test_guidelines_stay_compact() {
        let tools = vec![
            TOOL_EXEC_COMMAND.to_string(),
            TOOL_CODE_SEARCH.to_string(),
            "read_file".to_string(),
            TOOL_LIST_FILES.to_string(),
            "apply_patch".to_string(),
        ];
        let guidelines = generate_tool_guidelines_for_profile(
            &tools,
            None,
            ResolvedShellPromptProfile::UnixLike,
        );
        assert!(!guidelines.contains("read_file"));
        assert!(!guidelines.contains("list_files"));
        assert!(guidelines.contains("code_search"));
        let approx_tokens = guidelines.len() / 4;
        assert!(approx_tokens < 240, "got ~{approx_tokens} tokens");
    }

    #[test]
    fn test_parallel_tool_call_guidance() {
        let tools = vec![
            TOOL_EXEC_COMMAND.to_string(),
            TOOL_CODE_SEARCH.to_string(),
            TOOL_APPLY_PATCH.to_string(),
        ];
        let guidelines = generate_tool_guidelines_for_profile(
            &tools,
            None,
            ResolvedShellPromptProfile::UnixLike,
        );
        assert!(
            guidelines.contains("parallel"),
            "Should include parallel tool call guidance"
        );
        assert!(
            guidelines.contains("inputs do not depend"),
            "Should mention independent inputs"
        );
    }

    #[test]
    fn planning_workflow_runtime_guidance_keeps_exec_read_only() {
        let tools = vec![
            TOOL_APPLY_PATCH.to_string(),
            TOOL_EXEC_COMMAND.to_string(),
            TOOL_CODE_SEARCH.to_string(),
        ];
        let guidelines = generate_runtime_tool_guidelines_for_profile(
            &tools,
            true,
            ResolvedShellPromptProfile::UnixLike,
        );

        assert!(guidelines.contains("Planning workflow active"));
        assert!(guidelines.contains("`exec_command` only for read-only verification"));
        assert!(!guidelines.contains("Inspect before edit"));
    }

    #[test]
    fn runtime_tool_guidance_uses_explicit_powershell_profile() {
        let tools = vec![
            TOOL_APPLY_PATCH.to_string(),
            TOOL_EXEC_COMMAND.to_string(),
            TOOL_CODE_SEARCH.to_string(),
        ];
        let guidelines = generate_runtime_tool_guidelines_for_profile(
            &tools,
            false,
            ResolvedShellPromptProfile::PowerShell,
        );

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
        let guidelines = generate_runtime_tool_guidelines_for_profile(
            &tools,
            false,
            ResolvedShellPromptProfile::UnixLike,
        );

        assert!(guidelines.contains("`ls`, `rg`, `find`, `cat`, `sed`, and `awk`"));
        assert!(guidelines.contains("Advanced `code_search` takes `query`"));
        assert!(guidelines.contains("literal smart-case"));
        assert!(guidelines.contains("shell-only tasks"));
        assert!(!guidelines.contains("native PowerShell commands"));
        assert!(!guidelines.contains("`Get-ChildItem`"));
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
        assert!(unix_prompt.contains("`ls`, `rg`, `find`, `cat`, `sed`, and `awk`"));
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

        append_runtime_tool_prompt_sections(&mut prompt, &snapshot, true);

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

        append_runtime_tool_prompt_sections(&mut prompt, &first, true);
        append_runtime_tool_prompt_sections(&mut prompt, &second, true);

        assert_eq!(prompt.matches("## Active Tools").count(), 1);
        assert_eq!(prompt.matches("[Runtime Tool Catalog]").count(), 1);
        assert!(prompt.contains("version: 7"));
        assert!(!prompt.contains("version: 1"));
        assert!(prompt.contains("request_user_input_enabled: true"));
        assert!(!prompt.contains("request_user_input_enabled: false"));
    }
}
