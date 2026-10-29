use super::*;

#[test]
fn classifies_agent_spec_fields_for_primary_and_subagent_roles() {
    assert_eq!(classify_agent_spec_field("name"), Some(AgentSpecFieldClass::Shared));
    assert_eq!(classify_agent_spec_field("description"), Some(AgentSpecFieldClass::PrimaryMetadata));
    assert_eq!(classify_agent_spec_field("aliases"), Some(AgentSpecFieldClass::PrimaryMetadata));
    assert_eq!(classify_agent_spec_field("disallowedTools"), Some(AgentSpecFieldClass::PrimaryRuntime));
    assert_eq!(classify_agent_spec_field("permissions"), Some(AgentSpecFieldClass::PrimaryRuntime));
    assert_eq!(classify_agent_spec_field("mcpServers"), Some(AgentSpecFieldClass::PrimaryRuntime));
    assert_eq!(classify_agent_spec_field("maxTurns"), Some(AgentSpecFieldClass::SubagentOnly));
    assert_eq!(classify_agent_spec_field("initial_prompt"), Some(AgentSpecFieldClass::SubagentOnly));
    assert_eq!(classify_agent_spec_field("mode"), Some(AgentSpecFieldClass::Availability));
    assert_eq!(classify_agent_spec_field("unknown"), None);
}

#[test]
fn parses_agent_availability_modes() -> Result<()> {
    let temp = TempDir::new()?;
    for (name, mode, expected) in [
        ("primary", "primary", AgentMode::Primary),
        ("subagent", "subagent", AgentMode::Subagent),
        ("all", "all", AgentMode::All),
    ] {
        let path = temp.path().join(format!("{name}.md"));
        fs::write(
            &path,
            format!(
                r#"---
name: {name}
description: {name} agent
mode: {mode}
permissions:
  default: ask
---
Prompt."#
            ),
        )?;

        let spec = load_subagent_from_file(&path, SubagentSource::ProjectVtcode)?;
        assert_eq!(spec.mode, expected);
    }

    Ok(())
}

#[test]
fn defaults_missing_permissions_to_ask() -> Result<()> {
    let temp = TempDir::new()?;
    let path = temp.path().join("missing-permissions.md");
    fs::write(
        &path,
        r#"---
name: missing-permissions
description: Missing permissions
---
Prompt."#,
    )?;

    let spec = load_subagent_from_file(&path, SubagentSource::ProjectVtcode)?;
    assert_eq!(spec.permissions.default, PermissionDefault::Ask);
    assert!(spec.permissions.allow.is_empty());
    assert!(spec.permissions.ask.is_empty());
    assert!(spec.permissions.auto.is_empty());
    assert!(spec.permissions.deny.is_empty());
    Ok(())
}

#[test]
fn rejects_invalid_permissions_default() -> Result<()> {
    let temp = TempDir::new()?;
    let path = temp.path().join("invalid-permissions.md");
    fs::write(
        &path,
        r#"---
name: invalid-permissions
description: Invalid permissions
permissions:
  default: plan
---
Prompt."#,
    )?;

    let err = load_subagent_from_file(&path, SubagentSource::ProjectVtcode).unwrap_err();
    assert!(err.to_string().contains("failed to parse subagent permissions"));
    Ok(())
}

#[test]
fn rejects_legacy_top_level_permission_fields() -> Result<()> {
    let temp = TempDir::new()?;

    for legacy_field in ["permissionMode", "permission_mode"] {
        let markdown_path = temp.path().join(format!("{legacy_field}.md"));
        fs::write(
            &markdown_path,
            format!(
                r#"---
name: {legacy_field}
description: Legacy frontmatter permissions
permissions:
  default: ask
{legacy_field}: allow
---
Prompt."#
            ),
        )?;

        let markdown_err = load_subagent_from_file(&markdown_path, SubagentSource::ProjectVtcode).unwrap_err();
        assert!(markdown_err.to_string().contains(legacy_field));

        let toml_path = temp.path().join(format!("{legacy_field}.toml"));
        fs::write(
            &toml_path,
            format!(
                r#"name = "{legacy_field}"
description = "Legacy TOML permissions"
prompt = "Prompt."
permissions = {{ default = "ask" }}
{legacy_field} = "allow"
"#
            ),
        )?;

        let toml_err = load_subagent_from_file(&toml_path, SubagentSource::ProjectCodex).unwrap_err();
        assert!(toml_err.to_string().contains(legacy_field));

        let cli_payload = json!({
            legacy_field: {
                "description": "Legacy CLI permissions",
                "permissions": { "default": "ask" },
                legacy_field: "allow"
            }
        });

        let cli_err = load_cli_agents(&cli_payload).unwrap_err();
        assert!(cli_err.to_string().contains(legacy_field));
    }

    Ok(())
}

#[test]
fn primary_agent_parser_accepts_supported_fields() -> Result<()> {
    let temp = TempDir::new()?;
    let path = temp.path().join("build.md");
    fs::write(
        &path,
        r#"---
name: build
description: Primary build agent
tools: [Read, Bash]
disallowedTools: [Write]
model: gpt-5.4
color: blue
reasoning_effort: high
permissions:
  default: ask
  allow: [code_search]
  ask: [exec_command]
  auto: [code_search]
  deny: [apply_patch]
skills: [rust, repo]
mcpServers:
  - filesystem
  - demo:
      command: demo-mcp
hooks:
  PreToolUse:
    - matcher: Bash
      hooks:
        - command: echo pre
memory: project
aliases: [builder, implementer]
mode: primary
---
Primary prompt."#,
    )?;

    let spec = load_subagent_from_file(&path, SubagentSource::ProjectVtcode)?;

    assert_eq!(spec.name, "build");
    assert_eq!(spec.description, "Primary build agent");
    assert_eq!(spec.prompt, "Primary prompt.");
    assert_eq!(spec.tools, Some(vec![tools::EXEC_COMMAND.to_string()]));
    assert_eq!(spec.disallowed_tools, vec![tools::APPLY_PATCH.to_string()]);
    assert_eq!(spec.permissions.default, PermissionDefault::Ask);
    assert_eq!(spec.permissions.allow, vec![tools::CODE_SEARCH.to_string()]);
    assert_eq!(spec.permissions.ask, vec![tools::EXEC_COMMAND.to_string()]);
    assert_eq!(spec.permissions.auto, vec![tools::CODE_SEARCH.to_string()]);
    assert_eq!(spec.permissions.deny, vec![tools::APPLY_PATCH.to_string()]);
    assert_eq!(spec.model.as_deref(), Some("gpt-5.4"));
    assert_eq!(spec.reasoning_effort, Some(ReasoningEffortLevel::High));
    assert_eq!(spec.skills, vec!["rust".to_string(), "repo".to_string()]);
    assert_eq!(spec.mcp_servers.len(), 2);
    assert!(matches!(spec.mcp_servers[0], SubagentMcpServer::Named(_)));
    assert!(matches!(spec.mcp_servers[1], SubagentMcpServer::Inline(_)));
    assert_eq!(spec.memory, Some(SubagentMemoryScope::Project));
    assert_eq!(spec.color.as_deref(), Some("blue"));
    assert_eq!(spec.aliases, vec!["builder".to_string(), "implementer".to_string()]);
    assert_eq!(spec.mode, AgentMode::Primary);
    assert!(spec.hooks.is_some());
    assert!(spec.warnings.is_empty());
    Ok(())
}

#[test]
fn primary_agent_specs_warn_for_subagent_only_fields() -> Result<()> {
    let temp = TempDir::new()?;
    let path = temp.path().join("primary.md");
    fs::write(
        &path,
        r#"---
name: primary
description: Primary with child-only fields
mode: primary
permissions:
  default: ask
background: true
maxTurns: 4
initialPrompt: Start here
nickname_candidates: [helper]
isolation: full
---
Prompt."#,
    )?;

    let spec = load_subagent_from_file(&path, SubagentSource::ProjectVtcode)?;

    assert!(spec.background);
    assert_eq!(spec.max_turns, Some(4));
    assert_eq!(spec.initial_prompt.as_deref(), Some("Start here"));
    assert_eq!(spec.nickname_candidates, vec!["helper".to_string()]);
    assert_eq!(spec.isolation, Some(IsolationMode::Full));
    assert_eq!(
        spec.warnings,
        vec![
            "field 'background' is for subagents only and is ignored by primary agents".to_string(),
            "field 'max_turns' is for subagents only and is ignored by primary agents".to_string(),
            "field 'initial_prompt' is for subagents only and is ignored by primary agents".to_string(),
            "field 'nickname_candidates' is for subagents only and is ignored by primary agents".to_string(),
            "field 'isolation' is for subagents only and is ignored by primary agents".to_string(),
        ]
    );
    Ok(())
}

#[test]
fn aliases_do_not_replace_canonical_agent_names() -> Result<()> {
    let temp = TempDir::new()?;
    let path = temp.path().join("canonical.md");
    fs::write(
        &path,
        r#"---
name: canonical
description: Canonical primary
mode: primary
permissions:
  default: ask
aliases: [alias]
---
Prompt."#,
    )?;

    let spec = load_subagent_from_file(&path, SubagentSource::ProjectVtcode)?;

    assert_eq!(spec.name, "canonical");
    assert!(spec.matches_name("alias"));
    assert!(spec.matches_name("canonical"));
    Ok(())
}

#[test]
fn primary_agent_parser_preserves_baseline_runtime_fields() -> Result<()> {
    let temp = TempDir::new()?;
    let path = temp.path().join("baseline.toml");
    fs::write(
        &path,
        r#"name = "baseline"
description = "Baseline primary"
prompt = "Baseline prompt"
mode = "primary"
tools = ["code_search", "exec_command"]
disallowed_tools = ["exec_command"]
permissions = { default = "allow" }
model = "gpt-5.6-sol"
reasoning_effort = "medium"
"#,
    )?;

    let spec = load_subagent_from_file(&path, SubagentSource::ProjectCodex)?;

    assert_eq!(spec.tools, Some(vec![tools::CODE_SEARCH.to_string(), tools::EXEC_COMMAND.to_string(),]));
    assert_eq!(spec.disallowed_tools, vec![tools::EXEC_COMMAND.to_string()]);
    assert_eq!(spec.permissions.default, PermissionDefault::Allow);
    assert!(spec.permissions.allow.is_empty());
    assert!(spec.permissions.ask.is_empty());
    assert!(spec.permissions.auto.is_empty());
    assert!(spec.permissions.deny.is_empty());
    assert_eq!(spec.model.as_deref(), Some("gpt-5.6-sol"));
    assert_eq!(spec.reasoning_effort, Some(ReasoningEffortLevel::Medium));
    assert_eq!(spec.prompt, "Baseline prompt");
    assert!(spec.warnings.is_empty());
    Ok(())
}

#[test]
fn parses_claude_markdown_frontmatter() -> Result<()> {
    let temp = TempDir::new()?;
    let path = temp.path().join("reviewer.md");
    fs::write(
        &path,
        r#"---
name: reviewer
description: Review code
tools: [Read, Grep, Glob]
disallowedTools: [Write]
model: sonnet
color: blue
permissions:
  default: deny
  allow: [code_search]
skills: [rust]
memory: project
background: true
mode: primary
maxTurns: 7
nickname_candidates: [rev]
---

Review the target changes."#,
    )?;

    let spec = load_subagent_from_file(&path, SubagentSource::ProjectClaude)?;
    assert_eq!(spec.name, "reviewer");
    assert_eq!(spec.description, "Review code");
    assert_eq!(spec.model.as_deref(), Some("sonnet"));
    assert_eq!(spec.color.as_deref(), Some("blue"));
    assert_eq!(spec.tools, Some(vec![tools::EXEC_COMMAND.to_string()]));
    assert_eq!(spec.disallowed_tools, vec![tools::APPLY_PATCH.to_string()]);
    assert!(spec.background);
    assert_eq!(spec.mode, AgentMode::Primary);
    assert_eq!(spec.max_turns, Some(7));
    assert_eq!(spec.prompt, "Review the target changes.");
    Ok(())
}

#[test]
fn normalizes_claude_tool_aliases_to_vtcode_tools() -> Result<()> {
    let temp = TempDir::new()?;
    let path = temp.path().join("debugger.md");
    fs::write(
        &path,
        r#"---
name: debugger
description: Debug agent
permissions:
  default: allow
tools: [Read, Bash, Edit, Write, Glob, Grep]
disallowedTools: [Task]
---
Debug the issue."#,
    )?;

    let spec = load_subagent_from_file(&path, SubagentSource::ProjectClaude)?;
    assert_eq!(spec.tools, Some(vec![tools::EXEC_COMMAND.to_string(), tools::APPLY_PATCH.to_string(),]));
    assert_eq!(spec.disallowed_tools, vec![tools::SPAWN_AGENT.to_string()]);
    assert!(!spec.is_read_only());
    Ok(())
}

#[test]
fn shell_only_agents_are_not_read_only() -> Result<()> {
    let temp = TempDir::new()?;
    let path = temp.path().join("shell.md");
    fs::write(
        &path,
        r#"---
name: shell
description: Shell-capable agent
permissions:
  default: allow
tools: [Bash]
---
Run shell commands."#,
    )?;

    let spec = load_subagent_from_file(&path, SubagentSource::ProjectClaude)?;
    assert_eq!(spec.tools, Some(vec![tools::EXEC_COMMAND.to_string()]));
    assert!(!spec.is_read_only());
    Ok(())
}

#[test]
fn normalizes_claude_read_aliases_to_one_exec_command() {
    let normalized = normalize_subagent_tools(
        ["Read", "Grep", "Glob", "list_files", "LISTFILES", "Read(*)"]
            .into_iter()
            .map(ToString::to_string)
            .collect(),
    );

    assert_eq!(normalized, vec![tools::EXEC_COMMAND.to_string()]);
}

#[test]
fn keeps_explicit_code_search_distinct_from_claude_read_aliases() {
    let normalized = normalize_subagent_tools(
        ["code_search", "Read", "Code_Search", "Glob(*)"]
            .into_iter()
            .map(ToString::to_string)
            .collect(),
    );

    assert_eq!(normalized, vec![tools::CODE_SEARCH.to_string(), tools::EXEC_COMMAND.to_string(),]);
}

#[test]
fn parses_codex_toml_definition() -> Result<()> {
    let temp = TempDir::new()?;
    let path = temp.path().join("worker.toml");
    fs::write(
        &path,
        r##"name = "worker"
description = "Write-capable implementation agent"
developer_instructions = "Implement the assigned change."
model = "gpt-5.6-sol"
color = "#4f8fd8"
model_reasoning_effort = "high"
nickname_candidates = ["builder"]
permissions = { default = "ask" }
"##,
    )?;

    let spec = load_subagent_from_file(&path, SubagentSource::ProjectCodex)?;
    assert_eq!(spec.name, "worker");
    assert_eq!(spec.description, "Write-capable implementation agent");
    assert_eq!(spec.prompt, "Implement the assigned change.");
    assert_eq!(spec.model.as_deref(), Some("gpt-5.6-sol"));
    assert_eq!(spec.color.as_deref(), Some("#4f8fd8"));
    assert_eq!(spec.reasoning_effort, Some(ReasoningEffortLevel::High));
    assert_eq!(spec.nickname_candidates, vec!["builder".to_string()]);
    Ok(())
}

#[test]
fn precedence_prefers_project_vtcode_then_claude_then_codex_then_user() -> Result<()> {
    let temp = TempDir::new()?;
    fs::create_dir_all(temp.path().join(".codex/agents"))?;
    fs::create_dir_all(temp.path().join(".claude/agents"))?;
    fs::create_dir_all(temp.path().join(".vtcode/agents"))?;

    fs::write(
        temp.path().join(".codex/agents/example.toml"),
        r#"name = "example"
description = "codex"
developer_instructions = "codex"
permissions = { default = "ask" }
"#,
    )?;
    fs::write(
        temp.path().join(".claude/agents/example.md"),
        r#"---
name: example
description: claude
permissions:
  default: ask
---
claude"#,
    )?;
    fs::write(
        temp.path().join(".vtcode/agents/example.md"),
        r#"---
name: example
description: vtcode
permissions:
  default: ask
---
vtcode"#,
    )?;

    let mut input = SubagentDiscoveryInput::new(temp.path().to_path_buf());
    input.include_user_agents = false;
    let discovered = discover_subagents(&input)?;
    let effective = discovered
        .effective
        .into_iter()
        .find(|spec| spec.name == "example")
        .expect("example effective");
    assert_eq!(effective.description, "vtcode");
    assert_eq!(effective.source, SubagentSource::ProjectVtcode);
    Ok(())
}

#[test]
fn agent_definitions_with_same_name_shadow_by_precedence() -> Result<()> {
    let temp = TempDir::new()?;
    let project_vtcode_agents = temp.path().join(".vtcode/agents");
    let project_claude_agents = temp.path().join(".claude/agents");
    fs::create_dir_all(&project_vtcode_agents)?;
    fs::create_dir_all(&project_claude_agents)?;
    fs::write(
        project_claude_agents.join("plan.md"),
        r#"---
name: plan
description: Project delegated plan child
permissions:
  default: ask
---
Project child plan."#,
    )?;
    fs::write(
        project_vtcode_agents.join("plan.md"),
        r#"---
name: plan
description: Project primary plan
mode: primary
permissions:
  default: ask
---
Project primary plan."#,
    )?;

    let mut input = SubagentDiscoveryInput::new(temp.path().to_path_buf());
    input.include_user_agents = false;
    let discovered = discover_subagents(&input)?;
    let project_plan_specs = discovered
        .effective
        .iter()
        .filter(|spec| spec.name == "plan")
        .collect::<Vec<_>>();

    assert_eq!(project_plan_specs.len(), 1);
    assert_eq!(project_plan_specs[0].description, "Project primary plan");
    assert_eq!(project_plan_specs[0].mode, AgentMode::Primary);
    assert_eq!(project_plan_specs[0].source, SubagentSource::ProjectVtcode);
    Ok(())
}

#[test]
fn plugin_restrictions_strip_unsafe_overrides() -> Result<()> {
    let temp = TempDir::new()?;
    let path = temp.path().join("plugin-agent.md");
    fs::write(
        &path,
        r#"---
name: plugin-agent
description: Plugin agent
permissions:
  default: ask
mcpServers:
  - github
hooks:
  PreToolUse:
    - matcher: Bash
      hooks:
        - type: command
          command: ./check.sh
---
Plugin prompt"#,
    )?;

    let spec = load_subagent_from_file(&path, SubagentSource::Plugin { plugin: "demo".to_string() })?;
    assert!(spec.mcp_servers.is_empty());
    assert!(spec.hooks.is_none());
    assert_eq!(spec.warnings.len(), 2);
    Ok(())
}

#[test]
fn plugin_restrictions_normalize_permission_overrides() -> Result<()> {
    let temp = TempDir::new()?;
    let path = temp.path().join("plugin-agent.md");
    fs::write(
        &path,
        r#"---
name: plugin-agent
description: Plugin agent
permissions:
  default: auto
  allow: [code_search, "Read(*)", Bash, "Bash(*)", apply_patch, "Edit(/src/**)"]
  ask: [exec_command]
  auto: [code_search, "Glob(**/*.rs)", Write, "Write(*)", apply_patch, "apply_patch(*)"]
---
Plugin prompt"#,
    )?;

    let spec = load_subagent_from_file(&path, SubagentSource::Plugin { plugin: "demo".to_string() })?;

    assert_eq!(spec.permissions.default, PermissionDefault::Ask);
    assert_eq!(spec.permissions.allow, vec![tools::CODE_SEARCH.to_string(), "Read(*)".to_string()]);
    assert_eq!(spec.permissions.ask, vec![tools::EXEC_COMMAND.to_string()]);
    assert_eq!(spec.permissions.auto, vec![tools::CODE_SEARCH.to_string(), "Glob(**/*.rs)".to_string(),]);
    assert!(
        spec.warnings
            .iter()
            .any(|warning| { warning == "plugin subagent permission overrides are restricted for safety" })
    );
    Ok(())
}

#[test]
fn parses_subagent_lifecycle_hooks_from_frontmatter() -> Result<()> {
    let temp = TempDir::new()?;
    let path = temp.path().join("hooks.md");
    fs::write(
        &path,
        r#"---
name: hook-agent
description: Hooked agent
permissions:
  default: ask
hooks:
  SubagentStart:
    - matcher: worker
      hooks:
        - type: command
          command: echo start
  SubagentStop:
    - hooks:
        - type: command
          command: echo stop
---
Hook prompt"#,
    )?;

    let spec = load_subagent_from_file(&path, SubagentSource::ProjectClaude)?;
    let hooks = spec.hooks.expect("hooks");
    assert_eq!(hooks.lifecycle.subagent_start.len(), 1);
    assert_eq!(hooks.lifecycle.subagent_stop.len(), 1);
    assert_eq!(hooks.lifecycle.subagent_start[0].matcher.as_deref(), Some("worker"));
    Ok(())
}
