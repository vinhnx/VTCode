use super::*;

#[test]
fn builtin_aliases_cover_compat_names() {
    let builtins = builtin_subagents();
    let explorer = builtins.iter().find(|spec| spec.name == "explorer").expect("explorer builtin");
    let worker = builtins.iter().find(|spec| spec.name == "worker").expect("worker builtin");
    assert!(explorer.matches_name("explore"));
    assert!(worker.matches_name("general"));
    assert!(worker.matches_name("general-purpose"));
}

#[test]
fn builtin_primary_agents_are_available() {
    let builtins = builtin_subagents();
    let expected_readonly_tools = vec![
        tools::CODE_SEARCH.to_string(),
        tools::EXEC_COMMAND.to_string(),
        tools::GREP_FILE.to_string(),
        tools::READ_FILE.to_string(),
        tools::LIST_FILES.to_string(),
    ];
    let mut expected_primary_readonly_tools = expected_readonly_tools.clone();
    expected_primary_readonly_tools.push(tools::REQUEST_USER_INPUT.to_string());
    expected_primary_readonly_tools.push(tools::RECORD_DECISION.to_string());
    let default = builtins
        .iter()
        .find(|spec| spec.name == "default")
        .expect("missing default built-in");
    assert_eq!(default.permissions.default, PermissionDefault::Ask);
    let explorer = builtins
        .iter()
        .find(|spec| spec.name == "explorer")
        .expect("missing explorer built-in");
    assert_eq!(explorer.permissions.default, PermissionDefault::Deny);
    assert_eq!(explorer.permissions.allow, vec!["read".to_string()]);
    assert_eq!(explorer.tools.as_deref(), Some(expected_readonly_tools.as_slice()));
    assert!(
        !explorer
            .tools
            .as_deref()
            .unwrap_or_default()
            .contains(&tools::APPLY_PATCH.to_string())
    );
    assert!(explorer.disallowed_tools.is_empty());

    for name in ["build", "auto", "duck", "plan"] {
        let spec = builtins
            .iter()
            .find(|spec| spec.name == name && spec.is_primary())
            .unwrap_or_else(|| panic!("missing built-in primary agent {name}"));
        assert_eq!(spec.source, SubagentSource::Builtin);
        let expected_default = match name {
            "build" => PermissionDefault::Ask,
            "auto" => PermissionDefault::Auto,
            "duck" | "plan" => PermissionDefault::Deny,
            _ => unreachable!("unexpected built-in primary agent"),
        };
        assert_eq!(spec.permissions.default, expected_default);
        if matches!(name, "duck" | "plan") {
            assert_eq!(spec.tools.as_deref(), Some(expected_primary_readonly_tools.as_slice()));
            assert!(
                !spec
                    .tools
                    .as_deref()
                    .unwrap_or_default()
                    .contains(&tools::APPLY_PATCH.to_string())
            );
            assert!(spec.disallowed_tools.is_empty());
            // The allow list must keep every listed tool wire-visible:
            // `request_user_input` for the interview, and (plan only)
            // `bash` for read-only exec_command inspection gated by the
            // planning dispatch checks.
            let mut expected_allow = vec![
                "read".to_string(),
                "request_user_input".to_string(),
                tools::RECORD_DECISION.to_string(),
            ];
            if name == "plan" {
                expected_allow.push("bash".to_string());
            }
            assert_eq!(spec.permissions.allow, expected_allow);
        }
    }
    let plan = builtins
        .iter()
        .find(|spec| spec.name == "plan" && spec.mode == AgentMode::Primary)
        .expect("missing built-in primary-only plan agent");
    assert_eq!(plan.source, SubagentSource::Builtin);
    assert_eq!(plan.permissions.default, PermissionDefault::Deny);
    // `plan` is primary-only (like `duck`); projects that want a delegatable
    // plan subagent define their own `.vtcode/agents/plan.md` (mode: subagent).
    assert!(!plan.is_subagent());

    let auto = builtins
        .iter()
        .find(|spec| spec.name == "auto" && spec.mode == AgentMode::Primary)
        .expect("missing built-in auto primary agent");
    assert_eq!(auto.permissions.default, PermissionDefault::Auto);
    assert!(
        builtins.iter().all(|spec| spec.name != "review"),
        "review must not be a built-in primary or subagent"
    );
}

#[test]
fn build_and_auto_primary_agents_have_identical_tool_authority() {
    let builtins = builtin_subagents();
    let build = builtins.iter().find(|spec| spec.name == "build").expect("missing build");
    let auto = builtins.iter().find(|spec| spec.name == "auto").expect("missing auto");

    assert_eq!(build.tools, auto.tools);
    assert_eq!(build.disallowed_tools, auto.disallowed_tools);
    assert_eq!(build.tool_policy_overrides, auto.tool_policy_overrides);
    assert_eq!(build.mode, auto.mode);
    assert_eq!(build.mcp_servers, auto.mcp_servers);
    assert_eq!(build.skills, auto.skills);
    assert_eq!(build.permissions.allow, auto.permissions.allow);
    assert_eq!(build.permissions.ask, auto.permissions.ask);
    assert_eq!(build.permissions.auto, auto.permissions.auto);
    assert_eq!(build.permissions.deny, auto.permissions.deny);
    assert_eq!(build.permissions.default, PermissionDefault::Ask);
    assert_eq!(auto.permissions.default, PermissionDefault::Auto);
}

#[test]
fn ask_default_mutating_builtins_are_not_read_only() {
    let builtins = builtin_subagents();

    for name in ["default", "worker", "build"] {
        let spec = builtins
            .iter()
            .find(|spec| spec.name == name)
            .unwrap_or_else(|| panic!("missing built-in mutating agent {name}"));
        assert_eq!(spec.permissions.default, PermissionDefault::Ask);
        assert!(!spec.is_read_only());
    }

    let auto = builtins
        .iter()
        .find(|spec| spec.name == "auto")
        .expect("missing built-in auto agent");
    assert_eq!(auto.permissions.default, PermissionDefault::Auto);
    assert!(!auto.is_read_only());

    for name in ["duck", "explorer"] {
        let spec = builtins
            .iter()
            .find(|spec| spec.name == name)
            .unwrap_or_else(|| panic!("missing built-in read-only agent {name}"));
        assert_eq!(spec.permissions.default, PermissionDefault::Deny);
        assert!(spec.is_read_only());
    }

    // `plan` permits read-only `bash` so exec_command stays wire-visible
    // during planning, so the static `is_read_only()` heuristic
    // classifies it as mutation-capable. Its mutations are instead
    // blocked dynamically by the planning-workflow dispatch gate, and
    // `resolve_approved_plan_execution_agent` excludes it by name from
    // executing approved plans.
    let plan = builtins
        .iter()
        .find(|spec| spec.name == "plan")
        .expect("missing built-in plan agent");
    assert_eq!(plan.permissions.default, PermissionDefault::Deny);
    assert!(!plan.is_read_only());
    assert!(
        !plan.permissions.allow.iter().any(|rule| rule == "edit" || rule == "write"),
        "plan must not gain direct file-mutation permissions"
    );
}

#[test]
fn background_subagent_runtime_defaults_match_documented_shortcuts() {
    let config = BackgroundSubagentConfig::default();
    assert!(!config.enabled);
    assert_eq!(config.default_agent, None);
    assert_eq!(config.refresh_interval_ms, 2_000);
    assert!(!config.auto_restore);
    assert_eq!(config.toggle_shortcut, "ctrl+b");
}

#[test]
fn subagent_runtime_limits_embed_background_defaults() {
    let limits = SubagentRuntimeLimits::default();
    assert_eq!(limits.max_concurrent, 3);
    assert_eq!(limits.background.default_agent, None);
    assert_eq!(limits.background.toggle_shortcut, "ctrl+b");
}

#[test]
fn background_subagent_runtime_deserializes_explicit_default_agent() {
    let config: BackgroundSubagentConfig = toml::from_str(
        r#"
enabled = true
default_agent = "rust-engineer"
refresh_interval_ms = 1500
auto_restore = true
toggle_shortcut = "ctrl+b"
"#,
    )
    .expect("background config");

    assert!(config.enabled);
    assert_eq!(config.default_agent.as_deref(), Some("rust-engineer"));
    assert_eq!(config.refresh_interval_ms, 1_500);
    assert!(config.auto_restore);
    assert_eq!(config.toggle_shortcut, "ctrl+b");
}

#[test]
fn emits_warning_for_legacy_tool_name_permissions() -> Result<()> {
    let temp = TempDir::new()?;
    let path = temp.path().join("legacy.md");
    fs::write(
        &path,
        r#"---
name: legacy
description: Agent with legacy permission rules
permissions:
  default: deny
  allow: [read_file, "write_file(/docs/**)"]
  deny: [run_pty_cmd]
---
Legacy prompt."#,
    )?;

    let spec = load_subagent_from_file(&path, SubagentSource::ProjectVtcode)?;
    assert!(spec.warnings.iter().any(|w| w.contains("read_file")));
    assert!(spec.warnings.iter().any(|w| w.contains("write_file")));
    assert!(spec.warnings.iter().any(|w| w.contains("run_pty_cmd")));
    Ok(())
}

#[test]
fn builtin_plan_agent_exposes_request_user_input() {
    let spec = builtin_plan_agent();
    let tools = spec.tools.as_ref().expect("plan agent should have tool list");
    assert!(
        tools.iter().any(|t| t == tools::REQUEST_USER_INPUT),
        "plan agent should expose request_user_input for clarifying questions"
    );
    // The wire catalog filters advertised tools through these permission
    // rules; a static tool-list entry is not enough (turn_912 regression:
    // the planning wire catalog collapsed to only `code_search` because
    // `request_user_input`/`exec_command` hit the default-deny fallback).
    assert!(
        spec.permissions.allow.iter().any(|rule| rule == "request_user_input"),
        "plan agent permissions must allow request_user_input so it survives wire shaping"
    );
    assert!(
        spec.permissions.allow.iter().any(|rule| rule == "bash"),
        "plan agent permissions must allow bash so exec_command survives wire shaping; \
         the planning-workflow dispatch gate keeps execution read-only"
    );
}

#[test]
fn builtin_plan_agent_and_duck_share_discussion_first_guidance() {
    let plan_prompt = builtin_plan_agent().prompt;
    let duck_prompt = builtin_primary_duck_agent().prompt;

    for prompt in [&plan_prompt, &duck_prompt] {
        assert!(prompt.contains("discussion-first"));
        assert!(prompt.contains("Clarify scope, constraints, contradictions, and options"));
        assert!(prompt.contains("Resolve ordinary ambiguity from repository evidence when possible"));
        assert!(prompt.contains("ask the user directly only when material ambiguity is critical"));
        assert!(prompt.contains("Stop researching when existing evidence supports a decision"));
    }
    assert!(duck_prompt.contains("rubber-ducking only"));
    assert!(duck_prompt.contains("pressing Tab to switch to the Build agent"));
    assert!(!duck_prompt.contains("<proposed_plan>"));
    assert!(!duck_prompt.contains("plan file"));
}

#[test]
fn builtin_plan_agent_prompt_requires_grounded_discovery_and_approval() {
    let prompt = builtin_plan_agent().prompt;

    assert!(prompt.contains("repository-grounded, read-only discovery"));
    assert!(prompt.contains("exactly one final <proposed_plan> block"));
    assert!(prompt.contains("Never write the plan file with shell or file-editing tools"));
    assert!(prompt.contains("wait for explicit user approval before implementation"));
    assert!(prompt.contains("instead of suggesting an immediate edit"));
    assert_eq!(prompt.matches("wait for").count(), 1, "approval rule should be stated once");
}

#[test]
fn builtin_agent_prompts_ground_claims_without_repeated_absolutes() {
    let mut prompts: Vec<(String, String)> =
        builtin_subagents().into_iter().map(|spec| (spec.name, spec.prompt)).collect();
    for spec in [
        builtin_primary_build_agent(),
        builtin_primary_auto_agent(),
        builtin_plan_agent(),
        builtin_primary_duck_agent(),
    ] {
        prompts.push((spec.name, spec.prompt));
    }

    for (name, prompt) in &prompts {
        assert!(!prompt.contains("Never speculate"), "{name} prompt repeats a bare absolute");
        assert!(!prompt.contains("in parallel for efficiency"), "{name} prompt coaches read strategy");
    }
    for (name, prompt) in prompts.iter().filter(|(name, _)| name != "duck" && name != "auto") {
        assert!(
            prompt.to_lowercase().contains("read") && prompt.contains("before"),
            "{name} prompt should still ask to read files before claiming or editing"
        );
    }
}

#[test]
fn builtin_coordinator_restricts_tools_without_denying_worker_permissions() {
    let coordinator = super::super::builtin_primary_coordinator_agent();
    assert_eq!(coordinator.mode, AgentMode::Primary);
    assert_eq!(coordinator.permissions, builtin_primary_build_agent().permissions);
    let tool_ids = coordinator.tools.as_ref().unwrap();
    assert_eq!(
        tool_ids,
        &[
            "matrix",
            "request_user_input",
            "agent",
            "record_decision",
            "task_tracker"
        ]
    );
    assert!(coordinator.tool_policy_overrides.is_empty());
    assert!(
        coordinator
            .prompt
            .contains("Delegate all shell execution, file changes, and verification")
    );
    assert!(coordinator.prompt.contains("while the matrix is idle"));
    assert!(builtin_subagents().iter().any(|spec| spec.name == "coordinator"));
    assert_eq!(builtin_primary_build_agent().tools, None);
}

#[test]
fn readonly_permissions_allow_request_user_input_but_not_bash() {
    let readonly = readonly_agent_permissions();
    assert_eq!(readonly.default, PermissionDefault::Deny);
    assert_eq!(readonly.allow, vec!["read".to_string()]);

    let interview = readonly_interview_agent_permissions();
    assert_eq!(interview.default, PermissionDefault::Deny);
    assert!(interview.allow.iter().any(|rule| rule == "read"));
    assert!(interview.allow.iter().any(|rule| rule == "request_user_input"));
    assert!(!interview.allow.iter().any(|rule| rule == "bash"), "duck must keep exec_command denied");
}

#[test]
fn builtin_duck_agent_exposes_request_user_input() {
    let spec = builtin_primary_duck_agent();
    let tools = spec.tools.as_ref().expect("duck agent should have tool list");
    assert!(
        tools.iter().any(|t| t == tools::REQUEST_USER_INPUT),
        "duck agent should expose request_user_input for clarifying questions"
    );
}
