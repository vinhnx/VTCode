use super::{
    PermissionRequest, PermissionRequestKind, PermissionRuleDecision, ResolvedPermissionDecision,
    build_advertised_permission_requests, build_permission_request, evaluate_agent_permissions,
    evaluate_effective_permissions, evaluate_permissions,
};
use crate::config::{PermissionsConfig, constants::tools};
use serde_json::json;
use tempfile::TempDir;
use vtcode_config::core::permissions::{AgentPermissionsConfig, PermissionDefault};

fn workspace_roots() -> (TempDir, std::path::PathBuf, std::path::PathBuf) {
    let temp = TempDir::new().expect("temp dir");
    let workspace = temp.path().join("workspace");
    let cwd = workspace.join("nested");
    std::fs::create_dir_all(&cwd).expect("create dirs");
    (temp, workspace, cwd)
}

fn agent_permissions(default: PermissionDefault) -> AgentPermissionsConfig {
    AgentPermissionsConfig::new(default)
}

fn exact_tool_request(tool_name: &str) -> PermissionRequest {
    PermissionRequest {
        exact_tool_name: tool_name.to_string(),
        kind: PermissionRequestKind::Other,
        builtin_file_mutation: false,
        protected_write_paths: Vec::new(),
    }
}

#[test]
fn deny_precedes_ask_and_allow() {
    let (_temp, workspace, cwd) = workspace_roots();
    let config = PermissionsConfig {
        allow: vec!["Read".to_string()],
        ask: vec!["Read(/docs/**)".to_string()],
        deny: vec!["Read(/docs/secret.txt)".to_string()],
        ..PermissionsConfig::default()
    };
    let request = PermissionRequest {
        exact_tool_name: "read_file".to_string(),
        kind: PermissionRequestKind::Read { paths: vec![workspace.join("docs/secret.txt")] },
        builtin_file_mutation: false,
        protected_write_paths: Vec::new(),
    };

    assert_eq!(evaluate_permissions(&config, &workspace, &cwd, &request).decision(), PermissionRuleDecision::Deny);
}

#[test]
fn bash_glob_matches_command_text() {
    let (_temp, workspace, cwd) = workspace_roots();
    let config = PermissionsConfig {
        allow: vec!["Bash(cargo test *)".to_string()],
        ..PermissionsConfig::default()
    };
    let request = PermissionRequest {
        exact_tool_name: "command_session".to_string(),
        kind: PermissionRequestKind::Bash { command: "cargo test -p vtcode".to_string() },
        builtin_file_mutation: false,
        protected_write_paths: Vec::new(),
    };

    assert_eq!(evaluate_permissions(&config, &workspace, &cwd, &request).decision(), PermissionRuleDecision::Allow);
}

#[test]
fn read_path_rules_use_workspace_relative_matching() {
    let (_temp, workspace, cwd) = workspace_roots();
    let config = PermissionsConfig {
        ask: vec!["Read(/src/**/*.rs)".to_string()],
        ..PermissionsConfig::default()
    };
    let request = PermissionRequest {
        exact_tool_name: "read_file".to_string(),
        kind: PermissionRequestKind::Read { paths: vec![workspace.join("src/lib.rs")] },
        builtin_file_mutation: false,
        protected_write_paths: Vec::new(),
    };

    assert_eq!(evaluate_permissions(&config, &workspace, &cwd, &request).decision(), PermissionRuleDecision::Ask);
}

#[test]
fn mcp_rules_match_canonical_requests() {
    let (_temp, workspace, cwd) = workspace_roots();
    let config = PermissionsConfig {
        allow: vec!["mcp__context7__*".to_string()],
        ..PermissionsConfig::default()
    };
    let request = PermissionRequest {
        exact_tool_name: "mcp::context7::search-docs".to_string(),
        kind: PermissionRequestKind::Mcp {
            server: "context7".to_string(),
            tool: "search-docs".to_string(),
        },
        builtin_file_mutation: false,
        protected_write_paths: Vec::new(),
    };

    assert_eq!(evaluate_permissions(&config, &workspace, &cwd, &request).decision(), PermissionRuleDecision::Allow);
}

#[test]
fn protected_directory_exceptions_are_not_flagged() {
    let (_temp, workspace, cwd) = workspace_roots();
    let request = build_permission_request(
        &workspace,
        &cwd,
        tools::UNIFIED_FILE,
        Some(&json!({
            "action": "write",
            "path": "../.vtcode/skills/example.md"
        })),
    );
    assert!(!request.requires_protected_write_prompt());

    let request = build_permission_request(
        &workspace,
        &cwd,
        tools::UNIFIED_FILE,
        Some(&json!({
            "action": "write",
            "path": "../.vtcode/settings.toml"
        })),
    );
    assert!(request.requires_protected_write_prompt());
}

#[test]
fn apply_patch_paths_are_extracted_for_edit_rules() {
    let (_temp, workspace, cwd) = workspace_roots();
    let config = PermissionsConfig {
        ask: vec!["Edit(/src/**)".to_string()],
        ..PermissionsConfig::default()
    };
    let request = build_permission_request(
        &workspace,
        &cwd,
        "apply_patch",
        Some(&json!({
            "patch": "*** Begin Patch\n*** Update File: ../src/main.rs\n@@\n-test\n+test\n*** End Patch\n"
        })),
    );

    assert_eq!(evaluate_permissions(&config, &workspace, &cwd, &request).decision(), PermissionRuleDecision::Ask);
}

#[test]
fn relative_paths_resolve_from_current_directory() {
    let (_temp, workspace, cwd) = workspace_roots();
    let config = PermissionsConfig {
        ask: vec!["Read(./nested-file.rs)".to_string()],
        ..PermissionsConfig::default()
    };
    let request = build_permission_request(&workspace, &cwd, "read_file", Some(&json!({"path": "nested-file.rs"})));

    assert_eq!(evaluate_permissions(&config, &workspace, &cwd, &request).decision(), PermissionRuleDecision::Ask);
}

#[test]
fn exact_tool_rules_feed_rule_tiers() {
    let (_temp, workspace, cwd) = workspace_roots();
    // Use semantic rules which are the recommended approach
    let config = PermissionsConfig {
        allow: vec!["read".to_string()],
        deny: vec!["bash".to_string()],
        ..PermissionsConfig::default()
    };

    let read_request = PermissionRequest {
        exact_tool_name: "read_file".to_string(),
        kind: PermissionRequestKind::Read { paths: vec![] },
        builtin_file_mutation: false,
        protected_write_paths: Vec::new(),
    };
    let exec_request = PermissionRequest {
        exact_tool_name: "command_session".to_string(),
        kind: PermissionRequestKind::Bash { command: "test".to_string() },
        builtin_file_mutation: false,
        protected_write_paths: Vec::new(),
    };

    assert!(evaluate_permissions(&config, &workspace, &cwd, &read_request).allow);
    assert!(evaluate_permissions(&config, &workspace, &cwd, &exec_request).deny);
}

#[test]
fn agent_deny_wins_over_ask_auto_allow_and_default() {
    let (_temp, workspace, cwd) = workspace_roots();
    // Use a custom tool name that won't be normalized to a semantic rule
    let request = exact_tool_request("custom_tool");
    let mut permissions = agent_permissions(PermissionDefault::Allow);
    permissions.allow = vec!["custom_tool".to_string()];
    permissions.auto = vec!["custom_tool".to_string()];
    permissions.ask = vec!["custom_tool".to_string()];
    permissions.deny = vec!["custom_tool".to_string()];

    assert_eq!(
        evaluate_agent_permissions(&permissions, &workspace, &cwd, &request),
        ResolvedPermissionDecision::Deny
    );
}

#[test]
fn agent_ask_wins_over_auto_allow_and_default() {
    let (_temp, workspace, cwd) = workspace_roots();
    // Use a custom tool name that won't be normalized to a semantic rule
    let request = exact_tool_request("custom_tool");
    let mut permissions = agent_permissions(PermissionDefault::Deny);
    permissions.allow = vec!["custom_tool".to_string()];
    permissions.auto = vec!["custom_tool".to_string()];
    permissions.ask = vec!["custom_tool".to_string()];

    assert_eq!(
        evaluate_agent_permissions(&permissions, &workspace, &cwd, &request),
        ResolvedPermissionDecision::Ask
    );
}

#[test]
fn agent_auto_wins_over_allow_and_default() {
    let (_temp, workspace, cwd) = workspace_roots();
    // Use a custom tool name that won't be normalized to a semantic rule
    let request = exact_tool_request("custom_tool");
    let mut permissions = agent_permissions(PermissionDefault::Deny);
    permissions.allow = vec!["custom_tool".to_string()];
    permissions.auto = vec!["custom_tool".to_string()];

    assert_eq!(
        evaluate_agent_permissions(&permissions, &workspace, &cwd, &request),
        ResolvedPermissionDecision::Auto
    );
}

#[test]
fn agent_allow_wins_over_default() {
    let (_temp, workspace, cwd) = workspace_roots();
    // Use a custom tool name that won't be normalized to a semantic rule
    let request = exact_tool_request("custom_tool");
    let mut permissions = agent_permissions(PermissionDefault::Deny);
    permissions.allow = vec!["custom_tool".to_string()];

    assert_eq!(
        evaluate_agent_permissions(&permissions, &workspace, &cwd, &request),
        ResolvedPermissionDecision::Allow
    );
}

#[test]
fn agent_read_permission_allows_file_operation_read_only() {
    let (_temp, workspace, cwd) = workspace_roots();
    let mut permissions = agent_permissions(PermissionDefault::Deny);
    permissions.allow = vec!["read".to_string()];

    let read_request = build_permission_request(
        &workspace,
        &cwd,
        tools::UNIFIED_FILE,
        Some(&json!({"action": "read", "path": "README.md"})),
    );
    let write_request = build_permission_request(
        &workspace,
        &cwd,
        tools::UNIFIED_FILE,
        Some(&json!({"action": "write", "path": "README.md", "content": "x"})),
    );

    assert_eq!(
        evaluate_agent_permissions(&permissions, &workspace, &cwd, &read_request),
        ResolvedPermissionDecision::Allow
    );
    assert_eq!(
        evaluate_agent_permissions(&permissions, &workspace, &cwd, &write_request),
        ResolvedPermissionDecision::Deny
    );
}

#[test]
fn agent_read_permission_allows_code_search_but_not_exec_command() {
    let (_temp, workspace, cwd) = workspace_roots();
    let mut permissions = agent_permissions(PermissionDefault::Deny);
    permissions.allow = vec!["Read".to_string()];

    let code_search =
        build_permission_request(&workspace, &cwd, tools::CODE_SEARCH, Some(&json!({"query": "PermissionRequest"})));
    let exec_command =
        build_permission_request(&workspace, &cwd, tools::EXEC_COMMAND, Some(&json!({"cmd": "rg --files"})));

    assert!(matches!(code_search.kind, PermissionRequestKind::Read { .. }));
    assert_eq!(exec_command.kind, PermissionRequestKind::Bash { command: "rg --files".to_string() });
    assert_eq!(
        evaluate_agent_permissions(&permissions, &workspace, &cwd, &code_search),
        ResolvedPermissionDecision::Allow
    );
    assert_eq!(
        evaluate_agent_permissions(&permissions, &workspace, &cwd, &exec_command),
        ResolvedPermissionDecision::Deny
    );
}

#[test]
fn dry_run_exec_command_remains_bash_under_read_only_permissions() {
    let (_temp, workspace, cwd) = workspace_roots();
    let mut permissions = agent_permissions(PermissionDefault::Deny);
    permissions.allow = vec!["Read".to_string()];
    let request = build_permission_request(
        &workspace,
        &cwd,
        tools::EXEC_COMMAND,
        Some(&json!({"cmd": "python mutate.py --dry-run"})),
    );

    assert_eq!(request.kind, PermissionRequestKind::Bash { command: "python mutate.py --dry-run".to_string() });
    assert_eq!(
        evaluate_agent_permissions(&permissions, &workspace, &cwd, &request),
        ResolvedPermissionDecision::Deny
    );
}

#[test]
fn exec_command_advertisement_uses_bash_permission() {
    let (_temp, workspace, cwd) = workspace_roots();
    let requests = build_advertised_permission_requests(&workspace, &cwd, tools::EXEC_COMMAND);

    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].kind, PermissionRequestKind::Bash { command: "rg --files".to_string() });
}

#[test]
fn public_read_requests_preserve_global_deny_and_ask_precedence() {
    let (_temp, workspace, cwd) = workspace_roots();
    let request =
        build_permission_request(&workspace, &cwd, tools::CODE_SEARCH, Some(&json!({"query": "PermissionRequest"})));
    let mut permissions = agent_permissions(PermissionDefault::Deny);
    permissions.allow = vec!["Read".to_string()];

    let deny = PermissionsConfig {
        deny: vec!["Read".to_string()],
        ..PermissionsConfig::default()
    };
    assert_eq!(
        evaluate_effective_permissions(&deny, &permissions, &workspace, &cwd, &request),
        ResolvedPermissionDecision::Deny
    );

    let ask = PermissionsConfig {
        ask: vec!["Read".to_string()],
        ..PermissionsConfig::default()
    };
    assert_eq!(
        evaluate_effective_permissions(&ask, &permissions, &workspace, &cwd, &request),
        ResolvedPermissionDecision::Ask
    );
}

#[test]
fn explicit_bash_rules_continue_to_govern_mutating_exec_command() {
    let (_temp, workspace, cwd) = workspace_roots();
    let request = build_permission_request(
        &workspace,
        &cwd,
        tools::EXEC_COMMAND,
        Some(&json!({"cmd": "printf changed > file.txt"})),
    );
    let mut permissions = agent_permissions(PermissionDefault::Deny);
    permissions.allow = vec!["Bash(printf changed*)".to_string()];

    assert_eq!(
        evaluate_agent_permissions(&permissions, &workspace, &cwd, &request),
        ResolvedPermissionDecision::Allow
    );

    let global = PermissionsConfig {
        deny: vec!["Bash(printf changed*)".to_string()],
        ..PermissionsConfig::default()
    };
    assert_eq!(
        evaluate_effective_permissions(&global, &permissions, &workspace, &cwd, &request),
        ResolvedPermissionDecision::Deny
    );
}

#[test]
fn unmatched_agent_calls_use_permissions_default() {
    let (_temp, workspace, cwd) = workspace_roots();
    let request = exact_tool_request("read_file");
    let permissions = agent_permissions(PermissionDefault::Auto);

    assert_eq!(
        evaluate_agent_permissions(&permissions, &workspace, &cwd, &request),
        ResolvedPermissionDecision::Auto
    );
}

#[test]
fn missing_permissions_default_is_invalid_before_evaluation() {
    let err = toml::from_str::<AgentPermissionsConfig>(r#"allow = ["read_file"]"#).unwrap_err();

    assert!(err.to_string().contains("missing field `default`"));
}

#[test]
fn global_deny_is_hard_ceiling() {
    let (_temp, workspace, cwd) = workspace_roots();
    // Use a custom tool name that won't be normalized to a semantic rule
    let request = exact_tool_request("custom_tool");
    let global = PermissionsConfig {
        deny: vec!["custom_tool".to_string()],
        ..PermissionsConfig::default()
    };
    let permissions = agent_permissions(PermissionDefault::Allow);

    assert_eq!(
        evaluate_effective_permissions(&global, &permissions, &workspace, &cwd, &request),
        ResolvedPermissionDecision::Deny
    );
}

#[test]
fn global_ask_forces_prompt_over_agent_allow_or_auto() {
    let (_temp, workspace, cwd) = workspace_roots();
    // Use a custom tool name that won't be normalized to a semantic rule
    let request = exact_tool_request("custom_tool");
    let global = PermissionsConfig {
        ask: vec!["custom_tool".to_string()],
        ..PermissionsConfig::default()
    };

    assert_eq!(
        evaluate_effective_permissions(
            &global,
            &agent_permissions(PermissionDefault::Allow),
            &workspace,
            &cwd,
            &request,
        ),
        ResolvedPermissionDecision::Ask
    );
    assert_eq!(
        evaluate_effective_permissions(
            &global,
            &agent_permissions(PermissionDefault::Auto),
            &workspace,
            &cwd,
            &request,
        ),
        ResolvedPermissionDecision::Ask
    );
}

#[test]
fn global_allow_cannot_override_agent_deny_or_auto() {
    let (_temp, workspace, cwd) = workspace_roots();
    let request = exact_tool_request("command_session");
    let global = PermissionsConfig {
        allow: vec!["command_session".to_string()],
        ..PermissionsConfig::default()
    };

    assert_eq!(
        evaluate_effective_permissions(
            &global,
            &agent_permissions(PermissionDefault::Deny),
            &workspace,
            &cwd,
            &request,
        ),
        ResolvedPermissionDecision::Deny
    );
    assert_eq!(
        evaluate_effective_permissions(
            &global,
            &agent_permissions(PermissionDefault::Auto),
            &workspace,
            &cwd,
            &request,
        ),
        ResolvedPermissionDecision::Auto
    );
}

#[test]
fn agent_specific_deny_wins_within_agent_scope() {
    let (_temp, workspace, cwd) = workspace_roots();
    // Use a custom tool name that won't be normalized to a semantic rule
    let request = exact_tool_request("custom_tool");
    let global = PermissionsConfig {
        allow: vec!["custom_tool".to_string()],
        ..PermissionsConfig::default()
    };
    let mut permissions = agent_permissions(PermissionDefault::Allow);
    permissions.deny = vec!["custom_tool".to_string()];

    assert_eq!(
        evaluate_effective_permissions(&global, &permissions, &workspace, &cwd, &request),
        ResolvedPermissionDecision::Deny
    );
}

#[test]
fn auto_bucket_resolves_to_classifier_backed_decision() {
    let (_temp, workspace, cwd) = workspace_roots();
    // Use a custom tool name that won't be normalized to a semantic rule
    let request = exact_tool_request("custom_tool");
    let mut permissions = agent_permissions(PermissionDefault::Ask);
    permissions.auto = vec!["custom_tool".to_string()];

    assert_eq!(
        evaluate_agent_permissions(&permissions, &workspace, &cwd, &request),
        ResolvedPermissionDecision::Auto
    );
}

/// Turn_912/913 regression: the built-in plan agent's permission rules
/// must keep every tool in the planning wire catalog visible — the wire
/// shaper hides a tool only when ALL advertised permission requests are
/// denied, and the old read-only allow list denied `exec_command` (Bash)
/// and `request_user_input` (Other), collapsing the planning catalog to
/// bare `code_search`. Mutation tools must stay fully denied.
#[test]
fn builtin_plan_agent_keeps_planning_catalog_wire_visible() {
    let (_temp, workspace, cwd) = workspace_roots();
    let global = PermissionsConfig::default();
    let plan = vtcode_config::builtin_plan_agent();

    for tool in [
        tools::EXEC_COMMAND,
        tools::CODE_SEARCH,
        tools::GREP_FILE,
        tools::READ_FILE,
        tools::LIST_FILES,
        tools::REQUEST_USER_INPUT,
        tools::RECORD_DECISION,
    ] {
        let requests = build_advertised_permission_requests(&workspace, &cwd, tool);
        assert!(!requests.is_empty(), "{tool} must advertise at least one permission request");
        let any_allowed = requests.iter().any(|request| {
            evaluate_effective_permissions(&global, &plan.permissions, &workspace, &cwd, request)
                != ResolvedPermissionDecision::Deny
        });
        assert!(any_allowed, "{tool} must survive wire shaping for the plan agent");
    }

    for tool in [tools::APPLY_PATCH, tools::WRITE_FILE, tools::EDIT_FILE] {
        let requests = build_advertised_permission_requests(&workspace, &cwd, tool);
        let all_denied = requests.iter().all(|request| {
            evaluate_effective_permissions(&global, &plan.permissions, &workspace, &cwd, request)
                == ResolvedPermissionDecision::Deny
        });
        assert!(all_denied, "{tool} must stay hidden from the plan agent");
    }
}
