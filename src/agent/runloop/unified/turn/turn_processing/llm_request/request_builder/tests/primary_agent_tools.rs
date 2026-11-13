use super::*;

#[tokio::test]
async fn active_primary_agent_deny_list_applies_after_allow_list() {
    let mut backing = TestTurnProcessingBacking::new().await;
    backing.add_tool_definition(named_tool("code_search")).await;
    backing.add_tool_definition(named_tool("apply_patch")).await;
    let mut spec = test_primary_agent_spec("planner", "Use deterministic tools.");
    spec.tools = Some(vec!["code_search".to_string(), "apply_patch".to_string()]);
    spec.disallowed_tools = vec!["code_search".to_string()];
    backing.select_primary_agent_from_specs(&[spec], "planner");

    let built = {
        let mut ctx = backing.turn_processing_context();
        ctx.working_history.push(uni::Message::user("hello".to_string()));
        let snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
        build_turn_request(&mut ctx, 1, "noop-model", &snapshot, Some(320), None, false)
            .await
            .expect("request should build")
    };

    assert_eq!(request_tool_names(&built.request), vec!["apply_patch"]);
    assert!(system_prompt_text(&built.request).contains("- Effective request tools: apply_patch"));
}
#[tokio::test]
async fn unconstrained_primary_agent_falls_back_to_baseline_tools() {
    let mut backing = TestTurnProcessingBacking::new().await;
    backing.select_primary_agent_from_specs(&[vtcode_config::builtin_primary_build_agent()], "build");
    backing.add_tool_definition(named_tool("code_search")).await;
    backing.add_tool_definition(named_tool("apply_patch")).await;

    let built = {
        let mut ctx = backing.turn_processing_context();
        ctx.working_history.push(uni::Message::user("hello".to_string()));
        let snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
        build_turn_request(&mut ctx, 1, "noop-model", &snapshot, Some(320), None, false)
            .await
            .expect("request should build")
    };

    assert_eq!(request_tool_names(&built.request), vec!["apply_patch", "code_search"]);
    assert_eq!(built.continuation_messages.as_slice(), [uni::Message::user("hello".to_string())]);
}
#[tokio::test]
async fn active_primary_agent_runtime_state_ignores_openai_response_chain() {
    let mut backing = TestTurnProcessingBacking::new().await;
    let spec = test_primary_agent_spec("planner", "Use the active primary agent.");
    backing.select_primary_agent_from_specs(&[spec], "planner");

    let prior_messages = vec![uni::Message::user("hello".to_string())];
    let built = {
        let mut ctx = backing.turn_processing_context();
        ctx.working_history.extend(prior_messages.clone());
        ctx.working_history.push(uni::Message::user("continue".to_string()));
        ctx.session_stats
            .set_previous_response_chain("openai", "noop-model", Some("resp_123"), &prior_messages);
        let snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
        build_turn_request(&mut ctx, 1, "noop-model", &snapshot, Some(320), None, false)
            .await
            .expect("request should build")
    };

    assert_eq!(built.request.previous_response_id, None);
    assert_eq!(
        non_runtime_request_messages(&built.request),
        vec![
            uni::Message::user("hello".to_string()),
            uni::Message::user("continue".to_string())
        ]
    );
    assert!(system_prompt_text(&built.request).contains("## Active Primary Agent Runtime State"));
    assert_eq!(
        built.continuation_messages.as_slice(),
        [
            uni::Message::user("hello".to_string()),
            uni::Message::user("continue".to_string())
        ]
    );
}
#[tokio::test]
async fn active_primary_agent_runtime_state_keeps_stable_prompt_cache_friendly() {
    let mut backing = TestTurnProcessingBacking::new().await;
    let mut cfg = VTCodeConfig::default();
    cfg.agent.include_temporal_context = true;
    cfg.prompt_cache.cache_friendly_prompt_shaping = true;
    let cfg = Box::leak(Box::new(cfg));
    let first = test_primary_agent_spec("planner", "Planner instructions.");
    let second = test_primary_agent_spec("reviewer", "Reviewer instructions.");

    backing.select_primary_agent_from_specs(std::slice::from_ref(&first), "planner");
    let first_built = {
        let mut ctx = backing.turn_processing_context();
        ctx.vt_cfg = Some(cfg);
        ctx.working_history.push(uni::Message::user("hello".to_string()));
        let snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
        build_turn_request(&mut ctx, 1, "noop-model", &snapshot, Some(320), None, false)
            .await
            .expect("first request should build")
    };

    backing.select_primary_agent_from_specs(std::slice::from_ref(&second), "reviewer");
    let second_built = {
        let mut ctx = backing.turn_processing_context();
        ctx.vt_cfg = Some(cfg);
        ctx.working_history.clear();
        ctx.working_history.push(uni::Message::user("hello".to_string()));
        let snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
        build_turn_request(&mut ctx, 2, "noop-model", &snapshot, Some(320), None, false)
            .await
            .expect("second request should build")
    };

    let first_system = first_built.request.system_prompt.as_ref().expect("system");
    let second_system = second_built.request.system_prompt.as_ref().expect("system");
    assert_ne!(first_system, second_system);
    assert_eq!(stable_system_prefix_hash(first_system), stable_system_prefix_hash(second_system));
    assert!(first_system.contains("Planner instructions."));
    assert!(second_system.contains("Reviewer instructions."));
    assert!(
        !first_system.contains("Current date and time") && !second_system.contains("Current date and time"),
        "temporal context must not be regenerated in the per-turn primary-agent appendix"
    );
}
#[tokio::test]
async fn active_primary_agent_skills_are_request_scoped_on_switch() {
    let mut backing = TestTurnProcessingBacking::new().await;
    let mut first = test_primary_agent_spec("planner", "Planner instructions.");
    first.skills = vec!["alpha".to_string()];
    let mut second = test_primary_agent_spec("reviewer", "Reviewer instructions.");
    second.skills = vec!["beta".to_string()];

    backing.select_primary_agent_from_specs(std::slice::from_ref(&first), "planner");
    let first_built = {
        let mut ctx = backing.turn_processing_context();
        ctx.working_history.push(uni::Message::user("hello".to_string()));
        let snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
        build_turn_request(&mut ctx, 1, "noop-model", &snapshot, Some(320), None, false)
            .await
            .expect("first request should build")
    };

    backing.select_primary_agent_from_specs(std::slice::from_ref(&second), "reviewer");
    let second_built = {
        let mut ctx = backing.turn_processing_context();
        ctx.working_history.clear();
        ctx.working_history.push(uni::Message::user("hello".to_string()));
        let snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
        build_turn_request(&mut ctx, 2, "noop-model", &snapshot, Some(320), None, false)
            .await
            .expect("second request should build")
    };

    let first_system = first_built.request.system_prompt.as_ref().expect("system");
    let second_system = second_built.request.system_prompt.as_ref().expect("system");
    assert!(first_system.contains("## Active Primary Agent Skills"));
    assert!(first_system.contains("- alpha"));
    assert!(!first_system.contains("- beta"));
    assert!(second_system.contains("## Active Primary Agent Skills"));
    assert!(second_system.contains("- beta"));
    assert!(!second_system.contains("- alpha"));

    assert!(first_system.contains("- Active primary skills: alpha"));
    assert!(second_system.contains("- Active primary skills: beta"));
}
#[tokio::test]
async fn primary_agent_model_and_reasoning_feed_request_metadata() {
    let mut backing = TestTurnProcessingBacking::new().await;
    let mut spec = test_primary_agent_spec("planner", "Use agent metadata.");
    spec.model = Some("overlay-model".to_string());
    spec.reasoning_effort = Some(ReasoningEffortLevel::High);
    backing.select_primary_agent_from_specs(&[spec], "planner");

    let mut cfg = VTCodeConfig::default();
    cfg.agent.reasoning_effort = ReasoningEffortLevel::Medium;
    let cfg = Box::leak(Box::new(cfg));
    let built = {
        let mut ctx = backing.turn_processing_context();
        ctx.vt_cfg = Some(cfg);
        ctx.working_history.push(uni::Message::user("hello".to_string()));
        let mut snapshot = capture_turn_request_snapshot(&mut ctx, "base-model", false);
        assert_eq!(snapshot.active_model, "overlay-model");
        snapshot.capabilities.reasoning_effort = true;
        build_turn_request(&mut ctx, 1, "base-model", &snapshot, Some(320), None, false)
            .await
            .expect("request should build")
    };

    assert_eq!(built.request.model, "overlay-model");
    assert_eq!(built.request.reasoning_effort, Some(ReasoningEffortLevel::High));
    let runtime_context = system_prompt_text(&built.request);
    assert!(runtime_context.contains("- Request model: overlay-model"));
    assert!(runtime_context.contains("- Request reasoning effort: high"));
}
