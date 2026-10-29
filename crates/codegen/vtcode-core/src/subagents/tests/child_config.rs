use super::*;

#[test]
fn build_child_config_does_not_inherit_coordinator_role_or_remove_discovery_permissions() {
    let mut parent = VTCodeConfig {
        default_primary_agent: "coordinator".into(),
        ..Default::default()
    };
    parent.permissions.allow = vec![tools::READ_FILE.into(), tools::CODE_SEARCH.into()];
    parent.permissions.deny = vec![tools::APPLY_PATCH.into()];
    let spec = vtcode_config::builtin_subagents()
        .into_iter()
        .find(|spec| spec.matches_name("explore"))
        .unwrap();

    let child = build_child_config(&parent, &spec, models::openai::GPT_5_6_SOL, None, false);

    assert_eq!(child.default_primary_agent, "build");
    assert_eq!(child.permissions.allow, parent.permissions.allow);
    assert!(child.permissions.deny.contains(&tools::APPLY_PATCH.into()));
    assert!(child.permissions.deny.contains(&tools::SPAWN_AGENT.into()));
    assert_eq!(child.runtime_agent_permissions.as_ref(), Some(&spec.permissions));
    assert_eq!(parent.default_primary_agent, "coordinator");
}
#[tokio::test]
async fn subagent_restart_at_cap_waits_for_aborted_launch_before_restarting() {
    for interrupt in [false, true] {
        let root = TempDir::new().unwrap();
        let mut cfg = VTCodeConfig::default();
        cfg.subagents.max_concurrent = 1;
        let controller = SubagentController::new(test_controller_config(root.path().to_path_buf(), cfg))
            .await
            .unwrap();
        let spec = vtcode_config::builtin_subagents()
            .into_iter()
            .find(|spec| spec.name == "default")
            .unwrap();
        let mut child =
            test_child_record("existing", "child-session", "parent-session", &spec, SubagentStatus::Running, 1, None);
        let permit = controller.admission.clone().try_acquire_owned().unwrap();
        let (started, receiver) = tokio::sync::oneshot::channel();
        child.handle = Some(tokio::spawn(async move {
            let _permit = permit;
            started.send(()).unwrap();
            std::future::pending::<()>().await;
        }));
        controller.state.write().await.children.insert("existing".into(), child);
        receiver.await.unwrap();
        assert_eq!(controller.admission.available_permits(), 0);

        for message in ["Inspect source.rs", "Also inspect the second input"] {
            let waiting = controller
                .send_input(SendInputRequest {
                    target: "existing".into(),
                    message: Some(message.into()),
                    items: vec![],
                    interrupt: false,
                })
                .await
                .unwrap();
            assert_eq!(waiting.status, SubagentStatus::Waiting);
            assert_eq!(controller.admission.available_permits(), 0);
        }

        let resumed = tokio::time::timeout(Duration::from_millis(100), controller.resume("existing"))
            .await
            .expect("resuming a waiting child must not wait for its active launch")
            .unwrap();
        assert_eq!(resumed.status, SubagentStatus::Waiting);
        assert_eq!(controller.state.read().await.children["existing"].queued_prompts.len(), 2);
        assert_eq!(controller.admission.available_permits(), 0);

        let restarted = if interrupt {
            controller
                .send_input(SendInputRequest {
                    target: "existing".into(),
                    message: Some("Inspect source.rs".into()),
                    items: vec![],
                    interrupt: true,
                })
                .await
        } else {
            controller.close("existing").await.unwrap();
            assert_eq!(controller.admission.available_permits(), 1, "close must finish releasing the owned launch");
            controller.resume("existing").await
        };
        // Stop the replacement before it can execute provider work.
        controller.close("existing").await.unwrap();
        assert!(
            restarted.is_ok(),
            "a stopped child must be able to reuse its launch slot: interrupt={interrupt}, {restarted:?}"
        );
        tokio::task::yield_now().await;
        assert_eq!(controller.admission.available_permits(), 1);
        controller.signal_shutdown().await;
    }
}
#[tokio::test]
async fn subagent_restart_at_cap_failure_keeps_one_prompt_and_allows_later_resume() {
    let root = TempDir::new().unwrap();
    let mut cfg = VTCodeConfig::default();
    cfg.subagents.max_concurrent = 1;
    let controller = SubagentController::new(test_controller_config(root.path().to_path_buf(), cfg))
        .await
        .unwrap();
    let spec = vtcode_config::builtin_subagents()
        .into_iter()
        .find(|spec| spec.name == "default")
        .unwrap();
    let mut child =
        test_child_record("existing", "child-session", "parent-session", &spec, SubagentStatus::Completed, 1, None);
    child.last_prompt = Some("Inspect source.rs".into());
    controller.state.write().await.children.insert("existing".into(), child);
    let occupied = controller.admission.clone().try_acquire_owned().unwrap();

    assert!(
        controller
            .resume("existing")
            .await
            .unwrap_err()
            .to_string()
            .contains("concurrency limit")
    );
    {
        let state = controller.state.read().await;
        let child = &state.children["existing"];
        assert_eq!(child.status, SubagentStatus::Failed, "failed admission must not leave a phantom queued launch");
        assert_eq!(child.queued_prompts.len(), 1);
        assert!(child.handle.is_none());
        assert!(child.error.as_deref().unwrap().contains("concurrency limit"));
    }
    drop(occupied);

    controller.resume("existing").await.unwrap();
    assert_eq!(controller.admission.available_permits(), 0);
    assert_eq!(controller.state.read().await.children["existing"].queued_prompts.len(), 1);
    controller.close("existing").await.unwrap();
    assert_eq!(controller.admission.available_permits(), 1);
    controller.signal_shutdown().await;
}
#[tokio::test]
async fn matrix_active_admission_rejects_child_resume_and_input_without_mutation() {
    let root = TempDir::new().unwrap();
    let controller =
        SubagentController::new(test_controller_config(root.path().to_path_buf(), VTCodeConfig::default()))
            .await
            .unwrap();
    let spec = vtcode_config::builtin_subagents()
        .into_iter()
        .find(|spec| spec.name == "default")
        .unwrap();
    controller.state.write().await.children.insert(
        "existing".into(),
        test_child_record("existing", "child-session", "parent-session", &spec, SubagentStatus::Completed, 1, None),
    );
    controller.matrix.executing.store(true, Ordering::Release);
    for result in [
        controller.resume("existing").await,
        controller
            .send_input(SendInputRequest {
                target: "existing".into(),
                message: Some("Inspect source.rs".into()),
                items: vec![],
                interrupt: false,
            })
            .await,
    ] {
        assert!(result.unwrap_err().to_string().contains("scheduler-owned workers"));
    }
    assert!(
        controller
            .launch_child("existing")
            .await
            .unwrap_err()
            .to_string()
            .contains("scheduler-owned workers")
    );
    let state = controller.state.read().await;
    let child = &state.children["existing"];
    assert_eq!(child.status, SubagentStatus::Completed);
    assert!(child.queued_prompts.is_empty());
    assert!(child.handle.is_none());
    assert_eq!(controller.admission.available_permits(), 3);
}
#[test]
fn build_child_config_intersects_allowed_tools_and_preserves_global_denies() {
    let mut parent = VTCodeConfig::default();
    parent.permissions.allow = vec![tools::READ_FILE.to_string(), tools::CODE_SEARCH.to_string()];
    parent.permissions.deny = vec![tools::UNIFIED_EXEC.to_string()];

    let mut spec = vtcode_config::builtin_subagents()
        .into_iter()
        .find(|spec| spec.name == "worker")
        .expect("worker");
    spec.permissions = AgentPermissionsConfig {
        auto: vec!["Bash(*)".to_string()],
        ..AgentPermissionsConfig::new(PermissionDefault::Auto)
    };
    spec.tools = Some(vec![
        tools::SPAWN_AGENT.to_string(),
        tools::CODE_SEARCH.to_string(),
        tools::READ_FILE.to_string(),
    ]);

    let child = build_child_config(&parent, &spec, models::openai::GPT_5_6_SOL, None, false);
    assert_eq!(child.runtime_agent_permissions.as_ref(), Some(&spec.permissions));
    assert_eq!(child.permissions.allow, vec![tools::READ_FILE.to_string(), tools::CODE_SEARCH.to_string()]);
    assert!(child.permissions.deny.contains(&tools::UNIFIED_EXEC.to_string()));
    assert!(child.permissions.deny.contains(&tools::SPAWN_AGENT.to_string()));
}
#[test]
fn build_child_config_allows_nested_delegation_keeps_agent_tools_out_of_deny() {
    let mut parent = VTCodeConfig::default();
    parent.permissions.allow = vec![
        tools::SPAWN_AGENT.to_string(),
        tools::WAIT_AGENT.to_string(),
        tools::CODE_SEARCH.to_string(),
    ];

    let mut spec = vtcode_config::builtin_subagents()
        .into_iter()
        .find(|spec| spec.name == "worker")
        .expect("worker");
    spec.tools = Some(parent.permissions.allow.clone());

    let child = build_child_config(&parent, &spec, models::openai::GPT_5_6_SOL, None, true);

    assert_eq!(
        child.permissions.allow,
        vec![
            tools::SPAWN_AGENT.to_string(),
            tools::WAIT_AGENT.to_string(),
            tools::CODE_SEARCH.to_string()
        ],
        "nested delegation keeps delegation tools in the allow-list"
    );
    assert!(
        !child.permissions.deny.contains(&tools::SPAWN_AGENT.to_string()),
        "spawn_agent must not be denied when nested delegation is allowed"
    );
    assert!(
        !child.permissions.deny.contains(&tools::AGENT.to_string()),
        "agent must not be denied when nested delegation is allowed"
    );
    assert!(
        child.permissions.deny.contains(&tools::SPAWN_BACKGROUND_SUBPROCESS.to_string()),
        "background subprocess alias stays blocked for children"
    );
}
#[test]
fn build_child_config_default_denies_all_subagent_tools() {
    let parent = VTCodeConfig::default();
    let spec = vtcode_config::builtin_subagents()
        .into_iter()
        .find(|spec| spec.name == "worker")
        .expect("worker");

    let child = build_child_config(&parent, &spec, models::openai::GPT_5_6_SOL, None, false);

    assert!(child.permissions.deny.contains(&tools::SPAWN_AGENT.to_string()));
    assert!(child.permissions.deny.contains(&tools::AGENT.to_string()));
    assert!(child.permissions.deny.contains(&tools::SEND_INPUT.to_string()));
    assert!(child.permissions.deny.contains(&tools::WAIT_AGENT.to_string()));
    assert!(child.permissions.deny.contains(&tools::RESUME_AGENT.to_string()));
    assert!(child.permissions.deny.contains(&tools::CLOSE_AGENT.to_string()));
    assert!(child.permissions.deny.contains(&tools::SPAWN_BACKGROUND_SUBPROCESS.to_string()));
}
#[test]
fn build_child_config_preserves_subagent_lifecycle_stripping_and_hook_merging() {
    let mut parent = VTCodeConfig::default();
    parent.permissions.allow = vec![
        tools::SPAWN_AGENT.to_string(),
        tools::CODE_SEARCH.to_string(),
        tools::UNIFIED_EXEC.to_string(),
    ];

    let mut spec = vtcode_config::builtin_subagents()
        .into_iter()
        .find(|spec| spec.name == "worker")
        .expect("worker");
    spec.tools = Some(parent.permissions.allow.clone());
    let mut hooks = HooksConfig::default();
    hooks.lifecycle.pre_tool_use.push(HookGroupConfig {
        matcher: Some("*".to_string()),
        hooks: vec![HookCommandConfig {
            command: "echo child".to_string(),
            ..HookCommandConfig::default()
        }],
    });
    spec.hooks = Some(hooks);

    let child = build_child_config(&parent, &spec, models::openai::GPT_5_6_SOL, None, false);

    assert_eq!(child.permissions.allow, vec![tools::CODE_SEARCH.to_string(), tools::UNIFIED_EXEC.to_string()]);
    assert!(child.permissions.deny.contains(&tools::SPAWN_AGENT.to_string()));
    assert_eq!(child.hooks.lifecycle.pre_tool_use.len(), 1);
    assert_eq!(child.hooks.lifecycle.pre_tool_use[0].hooks[0].command, "echo child");
}
#[test]
fn prepare_child_runtime_config_uses_shared_view_for_model_and_reasoning() {
    let parent = VTCodeConfig::default();
    let mut spec = vtcode_config::builtin_subagents()
        .into_iter()
        .find(|spec| spec.name == "worker")
        .expect("worker");
    spec.model = Some(models::openai::GPT_5_6_LUNA.to_string());
    spec.reasoning_effort = Some(ReasoningEffortLevel::High);

    let (resolved_model, child_reasoning_effort, child_cfg) = prepare_child_runtime_config(
        &parent,
        &spec,
        models::openai::GPT_5_6_SOL,
        "openai",
        ReasoningEffortLevel::Low,
        None,
        None,
        None,
        false,
        |_, parent_model, parent_provider, model_override, spec_model, agent_name| {
            assert_eq!(parent_model, models::openai::GPT_5_6_SOL);
            assert_eq!(parent_provider, "openai");
            assert_eq!(model_override, None);
            assert_eq!(spec_model, Some(models::openai::GPT_5_6_LUNA));
            assert_eq!(agent_name, "worker");
            Ok(models::openai::GPT_5_6_LUNA.parse::<ModelId>().expect("valid model"))
        },
    )
    .expect("prepared child runtime config");

    assert_eq!(resolved_model.as_str(), models::openai::GPT_5_6_LUNA);
    assert_eq!(child_cfg.agent.default_model, models::openai::GPT_5_6_LUNA);
    assert_eq!(child_reasoning_effort, ReasoningEffortLevel::High);
    assert_eq!(child_cfg.agent.reasoning_effort, ReasoningEffortLevel::High);
}
#[test]
fn subagent_instruction_composition_uses_shared_runtime_prompt_and_skill_appendix() {
    let mut spec = vtcode_config::builtin_subagents()
        .into_iter()
        .find(|spec| spec.name == "worker")
        .expect("worker");
    spec.prompt = "Worker instructions".to_string();
    spec.skills = vec!["rust".to_string(), "repo".to_string()];

    let instructions = compose_subagent_instructions(&spec, Some("Memory appendix".to_string()));

    assert!(instructions.contains("Worker instructions"));
    assert!(instructions.contains("Preloaded skill names: rust, repo."));
    assert!(instructions.contains("Memory appendix"));
    assert!(instructions.contains("Return your final response using this exact Markdown contract"));
    // Writable children state the loop detector's real subagent limits.
    assert!(instructions.contains(&format!(
        "{} read-only calls in total",
        crate::core::loop_detector::SUBAGENT_MAX_TOTAL_READONLY_CALLS
    )));
    assert!(instructions.contains(&format!(
        "{} consecutive reads/searches",
        crate::core::loop_detector::SUBAGENT_NAVIGATION_HARD_STOP_STREAK
    )));
    assert!(!instructions.contains("CRITICAL"));
}
#[test]
fn final_response_contract_yields_to_agent_defined_format() {
    // The verifier's own format ends with a `Decision:` line the harness
    // parses; the generic contract must not compete with it.
    let mut spec = vtcode_config::builtin_subagents()
        .into_iter()
        .find(|spec| spec.name == "explorer")
        .expect("explorer");
    spec.name = "verifier".to_string();
    spec.prompt = "End with exactly one line, `Decision: APPROVED` or `Decision: REJECTED`.".to_string();

    let instructions = compose_subagent_instructions(&spec, None);

    assert!(instructions.contains(
        "If your agent instructions or the task define their own response format, follow that format instead."
    ));
}
#[test]
fn build_child_config_preserves_matching_rule_and_exact_tool_ids() {
    let mut parent = VTCodeConfig::default();
    parent.permissions.allow = vec![
        "Read(/docs/**)".to_string(),
        "mcp::context7::search".to_string(),
        tools::READ_FILE.to_string(),
    ];

    let mut spec = vtcode_config::builtin_subagents()
        .into_iter()
        .find(|spec| spec.name == "worker")
        .expect("worker");
    spec.tools = Some(vec![
        "mcp::context7::search".to_string(),
        tools::UNIFIED_EXEC.to_string(),
        tools::READ_FILE.to_string(),
    ]);

    let child = build_child_config(&parent, &spec, models::openai::GPT_5_6_SOL, None, false);

    assert_eq!(
        child.permissions.allow,
        vec![
            "Read(/docs/**)".to_string(),
            "mcp::context7::search".to_string(),
            tools::READ_FILE.to_string()
        ]
    );
}
#[test]
fn build_child_config_preserves_parent_rule_shaped_allowlist() {
    let mut parent = VTCodeConfig::default();
    parent.permissions.allow = vec!["Read".to_string()];

    let mut spec = vtcode_config::builtin_subagents()
        .into_iter()
        .find(|spec| spec.name == "worker")
        .expect("worker");
    spec.tools = Some(vec![
        tools::READ_FILE.to_string(),
        tools::CODE_SEARCH.to_string(),
        tools::UNIFIED_EXEC.to_string(),
    ]);

    let child = build_child_config(&parent, &spec, models::openai::GPT_5_6_SOL, None, false);

    assert_eq!(child.permissions.allow, vec!["Read".to_string()]);
}
#[test]
fn build_child_config_promotes_single_turn_budget_to_recovery_budget() {
    let parent = VTCodeConfig::default();
    let spec = vtcode_config::builtin_subagents()
        .into_iter()
        .find(|spec| spec.name == "worker")
        .expect("worker");

    let child = build_child_config(&parent, &spec, models::openai::GPT_5_6_SOL, Some(1), false);

    assert_eq!(child.automation.full_auto.max_turns, SUBAGENT_MIN_MAX_TURNS);
}
#[test]
fn background_children_get_a_higher_turn_floor() {
    assert_eq!(normalize_background_child_max_turns(Some(2), true), Some(4));
    assert_eq!(normalize_background_child_max_turns(Some(3), true), Some(4));
    assert_eq!(normalize_background_child_max_turns(Some(4), true), Some(4));
}
#[test]
fn foreground_children_keep_the_existing_turn_floor() {
    assert_eq!(normalize_background_child_max_turns(Some(1), false), Some(SUBAGENT_MIN_MAX_TURNS));
    assert_eq!(normalize_background_child_max_turns(Some(2), false), Some(2));
    assert_eq!(normalize_background_child_max_turns(None, true), None);
}
#[test]
fn build_child_config_merges_inline_mcp_provider() {
    let parent = VTCodeConfig::default();
    let mut spec = vtcode_config::builtin_subagents()
        .into_iter()
        .find(|spec| spec.name == "default")
        .expect("default");
    spec.mcp_servers = vec![SubagentMcpServer::Inline(BTreeMap::from([(
        "playwright".to_string(),
        serde_json::json!({
            "type": "stdio",
            "command": "npx",
            "args": ["-y", "@playwright/mcp@latest"],
        }),
    )]))];

    let child = build_child_config(&parent, &spec, models::openai::GPT_5_6_SOL, None, false);
    let provider = child
        .mcp
        .providers
        .iter()
        .find(|provider| provider.name == "playwright")
        .expect("playwright provider");
    assert_eq!(provider.name, "playwright");
}
#[test]
fn explicit_delegation_request_detects_mentions_and_keywords() {
    let direct_mentions = extract_explicit_agent_mentions("@agent-worker fix the issue", &[]);
    assert!(contains_explicit_delegation_request("@agent-worker fix the issue", direct_mentions.as_slice()));
    let no_mentions = extract_explicit_agent_mentions("delegate this in parallel", &[]);
    assert!(contains_explicit_delegation_request("delegate this in parallel", no_mentions.as_slice()));
    let empty_mentions = extract_explicit_agent_mentions("review the repository", &[]);
    assert!(!contains_explicit_delegation_request("review the repository", empty_mentions.as_slice()));
}
#[test]
fn explicit_agent_mentions_detect_natural_language_selection() {
    let rust_engineer = read_only_test_spec("rust-engineer");
    assert_eq!(
        extract_explicit_agent_mentions("use rust-engineer agent to review current code", &[rust_engineer]),
        vec!["rust-engineer".to_string()]
    );
}
#[test]
fn explicit_agent_mentions_detect_looser_subagent_selection() {
    let background_demo = read_only_test_spec("background-demo");
    assert_eq!(
        extract_explicit_agent_mentions("use background-demo and run the subagent", &[background_demo]),
        vec!["background-demo".to_string()]
    );
}
#[test]
fn explicit_agent_mentions_detect_run_subagent_selection() {
    let rust_engineer = read_only_test_spec("rust-engineer");
    assert_eq!(
        extract_explicit_agent_mentions("run rust-engineer subagent and review changes", &[rust_engineer]),
        vec!["rust-engineer".to_string()]
    );
}
#[test]
fn explicit_agent_mentions_ignore_primary_only_agents() {
    let mut duck = read_only_test_spec("duck");
    duck.mode = vtcode_config::AgentMode::Primary;

    assert_eq!(
        extract_explicit_agent_mentions("@agent-duck discuss the task", &[duck.clone()]),
        Vec::<String>::new()
    );
    assert_eq!(
        extract_explicit_agent_mentions("run duck agent and discuss the task", &[duck]),
        Vec::<String>::new()
    );
}
#[test]
fn explicit_model_request_detects_aliases_and_full_ids() {
    assert!(contains_explicit_model_request("delegate this using gpt-5.6-luna", "gpt-5.6-luna"));
    assert!(contains_explicit_model_request("use the worker subagent with haiku", "haiku"));
    assert!(contains_explicit_model_request("run this with the small model", "small"));
    assert!(!contains_explicit_model_request("delegate this small cleanup task", "small"));
    assert!(!contains_explicit_model_request("delegate this task", "gpt-5.6-luna"));
}
#[test]
fn normalize_requested_model_override_drops_default_like_values() {
    assert_eq!(normalize_requested_model_override(Some("default".to_string()), "delegate this task"), None);
    assert_eq!(normalize_requested_model_override(Some(" inherit ".to_string()), "delegate this task"), None);
    assert_eq!(
        normalize_requested_model_override(Some(" inherit ".to_string()), "delegate this task using inherit"),
        Some("inherit".to_string())
    );
}
#[test]
fn sanitize_subagent_input_items_drops_empty_fields() {
    let mut items = vec![
        SubagentInputItem {
            item_type: Some("text".to_string()),
            text: Some("  Workspace: /tmp/repo  ".to_string()),
            path: Some(String::new()),
            name: Some(" ".to_string()),
            image_url: None,
        },
        SubagentInputItem {
            item_type: Some("text".to_string()),
            text: Some("   ".to_string()),
            path: Some(String::new()),
            name: None,
            image_url: None,
        },
    ];

    sanitize_subagent_input_items(&mut items);

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].text.as_deref(), Some("Workspace: /tmp/repo"));
    assert!(items[0].path.is_none());
    assert!(items[0].name.is_none());
}
#[test]
fn subagent_input_item_reads_schema_type_field() {
    let item: SubagentInputItem =
        serde_json::from_value(serde_json::json!({"type": "path", "path": "src/lib.rs"})).expect("item");
    assert_eq!(item.item_type.as_deref(), Some("path"));
    assert_eq!(serde_json::to_value(&item).expect("serialize")["type"], serde_json::json!("path"));

    let legacy: SubagentInputItem =
        serde_json::from_value(serde_json::json!({"item_type": "text", "text": "x"})).expect("legacy item");
    assert_eq!(legacy.item_type.as_deref(), Some("text"));
}
#[test]
fn agent_schema_reasoning_effort_enum_matches_parser() {
    let schema = vtcode_utility_tool_specs::agent_parameters();
    let listed = schema["properties"]["reasoning_effort"]["enum"]
        .as_array()
        .expect("enum")
        .iter()
        .map(|value| value.as_str().expect("string enum value").to_string())
        .collect::<Vec<_>>();
    // Schema -> parser: every advertised value parses back to itself.
    for value in &listed {
        let parsed = ReasoningEffortLevel::parse(value).unwrap_or_else(|| panic!("schema value {value} must parse"));
        assert_eq!(parsed.as_str(), value);
    }
    // Parser -> schema: every named level is advertised. The exhaustive match
    // makes a new variant fail to compile here until it is listed.
    let named = [
        ReasoningEffortLevel::None,
        ReasoningEffortLevel::Minimal,
        ReasoningEffortLevel::Low,
        ReasoningEffortLevel::Medium,
        ReasoningEffortLevel::High,
        ReasoningEffortLevel::XHigh,
        ReasoningEffortLevel::Max,
    ];
    for level in named {
        match level {
            ReasoningEffortLevel::None
            | ReasoningEffortLevel::Minimal
            | ReasoningEffortLevel::Low
            | ReasoningEffortLevel::Medium
            | ReasoningEffortLevel::High
            | ReasoningEffortLevel::XHigh
            | ReasoningEffortLevel::Max
            | ReasoningEffortLevel::Unknown => {}
        }
        assert!(listed.iter().any(|value| value == level.as_str()), "{level} missing from schema enum");
    }
    assert_eq!(listed.len(), named.len());
}
