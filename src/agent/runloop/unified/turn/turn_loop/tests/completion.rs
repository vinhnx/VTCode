use super::*;

#[test]
fn accumulate_turn_usage_merges_prompt_completion_and_cached_tokens() {
    let mut total = HarnessUsage::default();

    accumulate_turn_usage(
        "openai",
        &mut total,
        &Some(uni::Usage {
            prompt_tokens: 100,
            completion_tokens: 20,
            total_tokens: 120,
            cached_prompt_tokens: Some(15),
            cache_creation_tokens: None,
            cache_read_tokens: Some(15),
            iterations: None,
        }),
    );
    accumulate_turn_usage(
        "openai",
        &mut total,
        &Some(uni::Usage {
            prompt_tokens: 40,
            completion_tokens: 10,
            total_tokens: 50,
            cached_prompt_tokens: None,
            cache_creation_tokens: None,
            cache_read_tokens: None,
            iterations: None,
        }),
    );

    assert_eq!(total.input_tokens, 140);
    assert_eq!(total.cached_input_tokens, 15);
    assert_eq!(total.output_tokens, 30);
    assert!(has_turn_usage(&total));
}

#[test]
fn accumulate_turn_usage_normalizes_anthropic_exclusive_input() {
    let mut total = HarnessUsage::default();

    accumulate_turn_usage(
        "anthropic",
        &mut total,
        &Some(uni::Usage {
            prompt_tokens: 100,
            completion_tokens: 20,
            total_tokens: 120,
            cached_prompt_tokens: None,
            cache_creation_tokens: Some(50),
            cache_read_tokens: Some(400),
            iterations: None,
        }),
    );

    assert_eq!(total.input_tokens, 550);
    assert_eq!(total.cached_input_tokens, 400);
    assert_eq!(total.cache_creation_tokens, 50);
    assert_eq!(total.output_tokens, 20);
}

#[tokio::test]
async fn explicit_build_and_auto_approval_selections_handoff_persisted_plan_without_implementation_writes() {
    for (selection, expected_agent, expected_skip_confirmations) in [
        (InlineListSelection::PlanApprovalSwitchBuild, builtin_primary_build_agent().name, false),
        (InlineListSelection::PlanApprovalSwitchAuto, builtin_primary_auto_agent().name, true),
    ] {
        let workspace = tempfile::TempDir::new().expect("create approval selection workspace");
        let mut tool_registry = vtcode_core::tools::ToolRegistry::new(workspace.path().to_path_buf()).await;
        tool_registry.enable_planning();
        let plan_state = tool_registry.planning_workflow_state();
        persist_plan_draft(&plan_state, STREAMED_VALID_PLAN)
            .await
            .expect("persist canonical plan before explicit approval selection");
        let plan = load_plan_text_for_approval(&tool_registry)
            .await
            .expect("load the canonical persisted plan for approval");
        let mut plan_session = PlanningWorkflowSessionState::default();
        plan_session.enter(vtcode_core::core::interfaces::session::PlanningEntrySource::UserRequest);

        assert!(
            fs::read_dir(workspace.path())
                .expect("read workspace before approval")
                .all(|entry| entry.expect("read workspace entry").file_name() == ".vtcode"),
            "persisting a plan must not create implementation files before explicit approval"
        );

        let (command_tx, _command_rx) = tokio::sync::mpsc::unbounded_channel();
        let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();
        let handle = InlineHandle::new_for_tests(command_tx);
        let mut session = InlineSession {
            handle: handle.clone(),
            events: event_rx,
            worker: None,
        };
        event_tx
            .send(InlineEvent::Transient(TransientEvent::Submitted(TransientSubmission::Selection(selection))))
            .expect("submit explicit plan approval selection");

        let approval_workspace_root = tool_registry.workspace_root().clone();
        let outcome = execute_plan_approval(
            &mut tool_registry,
            &mut plan_session,
            &handle,
            &mut session,
            &Arc::new(crate::agent::runloop::unified::state::CtrlCState::new()),
            &Arc::new(tokio::sync::Notify::new()),
            PlanApprovalRequestContext {
                plan: &plan,
                skip_confirmations: false,
                full_auto: false,
                context_usage_percent: 0,
                editor: vtcode_core::config::EditorToolConfig::default(),
                workspace_root: approval_workspace_root,
            },
            PlanApprovalTelemetryContext {
                emitter: None,
                thread_id: "thread-test",
                turn_id: "turn-test",
            },
        )
        .await
        .expect("explicit approval selection should hand off the persisted plan");

        assert!(matches!(
            outcome,
            TurnHandlerOutcome::SwitchPrimaryAgentWithPolicy {
                target,
            } if target.agent_name() == expected_agent
                && target.skip_confirmations == expected_skip_confirmations
                && target.execution_context == PlanExecutionContext::Current
        ));
        let entries: Vec<_> = fs::read_dir(workspace.path())
            .expect("read workspace after approval handoff")
            .map(|entry| entry.expect("read workspace entry").file_name())
            .collect();
        assert!(
            entries.iter().all(|name| name == ".vtcode" || name == "vtcode.toml"),
            "approval handoff must remain execution-pending and must not write implementation files: {entries:?}"
        );
        // Registry admission can initialize runtime configuration, which is
        // distinct from implementation file writes.
        vtcode_core::config::ConfigManager::load_from_file(workspace.path().join("vtcode.toml"))
            .expect("load runtime configuration");
        assert_eq!(
            tool_registry.get_tool_policy(tool_names::TASK_TRACKER).await,
            vtcode_core::tool_policy::ToolPolicy::Allow,
        );
        assert!(tool_registry.tracker_adopted_for_request());
    }
}

#[tokio::test]
async fn approval_input_without_plan_synthesizes_before_approval() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Clone)]
    struct PlanProvider {
        calls: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl uni::LLMProvider for PlanProvider {
        fn name(&self) -> &str {
            "openai"
        }

        fn supports_streaming(&self) -> bool {
            false
        }

        async fn generate(&self, request: uni::LLMRequest) -> Result<uni::LLMResponse, uni::LLMError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(uni::LLMResponse {
                content: Some(
                    "<proposed_plan>\nSummary: optimize one startup subsystem.\n1. Measure -> src/main.rs -> verify: cargo check --locked\nValidation: run the focused startup test.\nAssumptions: preserve public APIs.\n</proposed_plan>"
                        .to_string(),
                ),
                model: request.model,
                tool_calls: None,
                usage: None,
                finish_reason: uni::FinishReason::Stop,
                reasoning: None,
                reasoning_details: None,
                organization_id: None,
                request_id: None,
                tool_references: Vec::new(),
                compaction: None,
            })
        }

        fn supported_models(&self) -> Vec<String> {
            vec!["noop-model".to_string()]
        }

        fn validate_request(&self, _request: &uni::LLMRequest) -> Result<(), uni::LLMError> {
            Ok(())
        }
    }

    let calls = Arc::new(AtomicUsize::new(0));
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.activate_planning_for_test();
    backing.set_provider(Box::new(PlanProvider { calls: calls.clone() }));

    let mut history = vec![uni::Message::user("yes".to_string())];
    run_turn_loop(&mut history, backing.turn_loop_context())
        .await
        .expect("missing-plan approval should continue to synthesis");

    assert_eq!(calls.load(Ordering::SeqCst), 1, "the missing-plan path must synthesize exactly once");
    assert!(
        history
            .iter()
            .any(|message| { message.content.as_text().contains("no completed plan draft exists yet") })
    );
}

#[tokio::test]
async fn single_tool_free_text_answer_completes_without_fallback() {
    #[derive(Clone)]
    struct LongAnswerProvider {
        requests: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl uni::LLMProvider for LongAnswerProvider {
        fn name(&self) -> &str {
            "openai"
        }

        fn supports_streaming(&self) -> bool {
            false
        }

        async fn generate(&self, request: uni::LLMRequest) -> Result<uni::LLMResponse, uni::LLMError> {
            self.requests.fetch_add(1, Ordering::SeqCst);
            let body = "VT Code stands out through its focus on being a secure, extensible, terminal-native coding agent. \
                It splits functionality into focused crates for providers, configuration, UI, indexing, memory, safety, MCP, ACP, plugins, and evaluation. \
                Command execution is treated as an adversarial boundary with workspace checks and fail-closed behavior. \
                The runtime uses a structured ThreadEvent contract for replay and observability. \
                The terminal remains the core experience while supporting Zed, MCP, browser bridges, skills, and plugins. \
                All set. Summary complete.";
            Ok(uni::LLMResponse {
                content: Some(body.to_string()),
                model: request.model,
                tool_calls: None,
                usage: None,
                finish_reason: uni::FinishReason::Stop,
                reasoning: None,
                reasoning_details: None,
                organization_id: None,
                request_id: None,
                tool_references: Vec::new(),
                compaction: None,
            })
        }

        fn supported_models(&self) -> Vec<String> {
            vec!["noop-model".to_string()]
        }

        fn validate_request(&self, _request: &uni::LLMRequest) -> Result<(), uni::LLMError> {
            Ok(())
        }
    }

    let requests = Arc::new(AtomicUsize::new(0));
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.set_provider(Box::new(LongAnswerProvider { requests: requests.clone() }));

    let mut history = vec![uni::Message::user("tell me about this project".to_string())];
    let outcome = run_turn_loop(&mut history, backing.turn_loop_context())
        .await
        .expect("single text answer should complete");

    assert!(
        matches!(outcome.result, TurnLoopResult::Completed { plan_approved_execution_pending: false }),
        "simple Q&A must stay Completed, got {:?}",
        outcome.result
    );
    assert!(!outcome.final_response_was_fallback, "valid model answer must not be labeled fallback");
    assert_eq!(requests.load(Ordering::SeqCst), 1, "simple answer needs exactly one request");
    assert!(
        !history
            .iter()
            .any(|message| { message.content.as_text().contains(COMPLETED_TURN_FALLBACK_RESPONSE) }),
        "must not inject the generic fallback for a valid answer"
    );
    assert!(
        history.iter().any(|message| {
            message.role == uni::MessageRole::Assistant
                && message.phase == Some(uni::AssistantPhase::FinalAnswer)
                && message.content.as_text().contains("VT Code stands out")
        }),
        "final answer must be retained as FinalAnswer"
    );
}

#[tokio::test]
async fn ensure_completed_reuses_found_final_without_requiring_prior_render() {
    // Regression for simple requests ending Blocked with
    // COMPLETED_TURN_FALLBACK_REASON: a valid FinalAnswer already in history
    // (e.g. streamed output that missed the render flag) must publish and
    // succeed, not synthesize the generic fallback.
    let mut backing = TestTurnProcessingBacking::new(4).await;
    let mut history = vec![
        uni::Message::user("tell me about this project".to_string()),
        uni::Message::assistant("VT Code stands out as a secure terminal agent.".to_string())
            .with_phase(Some(uni::AssistantPhase::FinalAnswer)),
    ];
    let history_len_before = history.len();
    let mut ctx = backing.turn_loop_context();
    assert!(!ctx.harness_state.final_response_rendered());
    assert!(!ctx.harness_state.final_response_was_fallback());

    let was_fallback = ensure_completed_turn_response(&mut ctx, &mut history, 0).expect("publish must succeed");

    assert!(!was_fallback, "found model answer must not be labeled fallback");
    assert_eq!(history.len(), history_len_before, "must not inject a second fallback answer");
    assert!(!ctx.harness_state.final_response_was_fallback());
    assert!(ctx.harness_state.final_response_rendered());
    assert!(ctx.harness_state.final_response_event_emitted());
}

#[tokio::test]
async fn ensure_completed_synthesizes_fallback_when_no_final_exists() {
    // Asymmetric counterpart: with no FinalAnswer in the turn slice, the
    // helper must synthesize COMPLETED_TURN_FALLBACK_RESPONSE and report fallback.
    let mut backing = TestTurnProcessingBacking::new(4).await;
    let mut history = vec![uni::Message::user("do work".to_string())];
    let mut ctx = backing.turn_loop_context();

    let was_fallback = ensure_completed_turn_response(&mut ctx, &mut history, 0).expect("fallback must succeed");

    assert!(was_fallback, "missing final must be reported as fallback");
    assert!(ctx.harness_state.final_response_was_fallback());
    assert!(
        history.iter().any(|message| {
            message.role == uni::MessageRole::Assistant
                && message.phase == Some(uni::AssistantPhase::FinalAnswer)
                && message.content.as_text().contains(COMPLETED_TURN_FALLBACK_RESPONSE)
        }),
        "must push the generic fallback as FinalAnswer"
    );
}

#[tokio::test]
async fn ensure_completed_synthesizes_planning_fallback_when_planning_active() {
    // Planning counterpart: with no FinalAnswer and planning active, the
    // helper must publish the planning resumable handoff (keep planning +
    // `<proposed_plan>` contract), not the generic retry text.
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.activate_planning_for_test();
    let mut history = vec![uni::Message::user("plan the fix".to_string())];
    let mut ctx = backing.turn_loop_context();

    let was_fallback = ensure_completed_turn_response(&mut ctx, &mut history, 0).expect("fallback must succeed");

    assert!(was_fallback, "missing final must be reported as fallback");
    assert!(ctx.harness_state.final_response_was_fallback());
    assert!(
        history.iter().any(|message| {
            message.role == uni::MessageRole::Assistant
                && message.phase == Some(uni::AssistantPhase::FinalAnswer)
                && message.content.as_text().contains(PLANNING_COMPLETED_FALLBACK_RESPONSE)
                && message.content.as_text().contains("keep planning")
                && message.content.as_text().contains("<proposed_plan>")
        }),
        "must push the planning fallback as FinalAnswer"
    );
    assert!(
        history.iter().all(|message| {
            message.role != uni::MessageRole::Assistant
                || !message.content.as_text().contains(COMPLETED_TURN_FALLBACK_RESPONSE)
        }),
        "planning fallback must not contain the generic retry text"
    );
}

#[tokio::test]
async fn ensure_completed_preserves_explicit_recovery_flag() {
    // Recovery fallbacks mark the flag at creation; a found final with the
    // flag set must stay fallback so the outer loop still blocks resumably.
    let mut backing = TestTurnProcessingBacking::new(4).await;
    let mut history = vec![
        uni::Message::user("summarize".to_string()),
        uni::Message::assistant("recovery synthesis output".to_string())
            .with_phase(Some(uni::AssistantPhase::FinalAnswer)),
    ];
    let mut ctx = backing.turn_loop_context();
    ctx.harness_state.mark_final_response_fallback();

    let was_fallback = ensure_completed_turn_response(&mut ctx, &mut history, 0).expect("publish must succeed");

    assert!(was_fallback, "explicit recovery flag must survive even with a found final");
}

#[tokio::test]
async fn plan_mode_auto_continue_directive_reaches_provider_without_early_exit() {
    // Regression for checkpoint turn_857: the harness-generated plan-mode
    // auto-continue directive contains the phrase "do not implement", which
    // normalizes to a `STAY_PHRASES` entry. When the planning exit trigger
    // consumed that directive as a genuine user stay message, the turn broke
    // before any provider request, published the completed-turn fallback, and
    // the outer loop re-queued the same directive forever.
    #[derive(Clone)]
    struct PlanAnsweringProvider {
        requests: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl uni::LLMProvider for PlanAnsweringProvider {
        fn name(&self) -> &str {
            "openai"
        }

        fn supports_streaming(&self) -> bool {
            false
        }

        async fn generate(&self, request: uni::LLMRequest) -> Result<uni::LLMResponse, uni::LLMError> {
            self.requests.fetch_add(1, Ordering::SeqCst);
            Ok(uni::LLMResponse {
                content: Some(format!("<proposed_plan>\n{STREAMED_VALID_PLAN}\n</proposed_plan>")),
                model: request.model,
                tool_calls: None,
                usage: None,
                finish_reason: uni::FinishReason::Stop,
                reasoning: None,
                reasoning_details: None,
                organization_id: None,
                request_id: None,
                tool_references: Vec::new(),
                compaction: None,
            })
        }

        fn supported_models(&self) -> Vec<String> {
            vec!["noop-model".to_string()]
        }

        fn validate_request(&self, _request: &uni::LLMRequest) -> Result<(), uni::LLMError> {
            Ok(())
        }
    }

    let requests = Arc::new(AtomicUsize::new(0));
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.activate_planning_for_test();
    backing.set_provider(Box::new(PlanAnsweringProvider { requests: requests.clone() }));
    let directive = crate::agent::runloop::unified::turn::tool_outcomes::helpers::plan_mode_continue_follow_up();
    backing.queue_follow_up_input_for_test(&directive);

    // The seeded message must be neutral: a bare `continue` is an exit alias
    // and would end the turn before the directive is ever consumed.
    let mut history = vec![uni::Message::user("please research the current work".to_string())];
    let outcome = run_turn_loop(&mut history, backing.turn_loop_context())
        .await
        .expect("an auto-continue directive must not break the turn before a provider request");

    assert!(
        matches!(outcome.result, TurnLoopResult::Completed { .. }),
        "the directive must run a normal plan turn, got {:?}",
        outcome.result
    );
    assert!(
        history.last().is_some_and(|message| {
            message.role == uni::MessageRole::User
                && crate::agent::runloop::unified::planning_workflow::exit_trigger::is_plan_mode_auto_continue_directive(
                    &message.content.as_text(),
                )
        }),
        "the directive must be steered into history as the last user message"
    );
    assert!(
        requests.load(Ordering::SeqCst) >= 1,
        "the directive must reach the provider instead of breaking the turn early"
    );
    assert!(
        !history.iter().any(|message| {
            message.role == uni::MessageRole::Assistant
                && message.content.as_text().contains(PLANNING_COMPLETED_FALLBACK_RESPONSE)
        }),
        "an auto-continue directive must not publish the completed-turn fallback"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn session_shaped_external_limit_local_check_and_awaited_timer_finish_once() {
    use crate::agent::runloop::unified::turn::background_completion::{
        PendingBackgroundCompletions, drain_exec_session_completions,
    };
    use vtcode_core::config::types::CapabilityLevel;
    use vtcode_core::tools::registry::ToolRegistration;
    use vtcode_core::tools::traits::Tool;

    struct LimitedFetch(Arc<AtomicUsize>);
    #[async_trait::async_trait]
    impl Tool for LimitedFetch {
        async fn execute(&self, _: serde_json::Value) -> anyhow::Result<serde_json::Value> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(json!({"error":"HTTP request failed with status: 429", "http_status":429,
                "retry_after_ms":6000, "next_action":"Report unavailable image evidence."}))
        }
        fn name(&self) -> &str {
            "web_fetch"
        }
        fn description(&self) -> &str {
            "Offline external limitation fixture"
        }
    }

    struct SessionScript {
        requests: Arc<AtomicUsize>,
    }
    #[async_trait::async_trait]
    impl uni::LLMProvider for SessionScript {
        fn name(&self) -> &str {
            "openai"
        }
        fn supports_streaming(&self) -> bool {
            false
        }
        fn supported_models(&self) -> Vec<String> {
            vec!["noop-model".into()]
        }
        fn validate_request(&self, _: &uni::LLMRequest) -> Result<(), uni::LLMError> {
            Ok(())
        }
        async fn generate(&self, request: uni::LLMRequest) -> Result<uni::LLMResponse, uni::LLMError> {
            let index = self.requests.fetch_add(1, Ordering::SeqCst);
            let call = match index {
                0 => Some(("web_fetch", json!({"url":"https://example.com/badge.svg"}))),
                1 => Some(("read_file", json!({"path":"README.md"}))),
                2 => Some((
                    "apply_patch",
                    json!({"input":"*** Begin Patch\n*** Update File: README.md\n@@\n-# Before\n+# After\n*** End Patch"}),
                )),
                3 => Some(("exec_command", json!({"cmd":"./verify.sh", "yield_time_ms":1000}))),
                4 => Some(("exec_command", json!({"cmd":"sleep 0.1", "background":true, "yield_time_ms":1}))),
                5 => {
                    let output = request
                        .messages
                        .iter()
                        .rev()
                        .find_map(|message| {
                            if message.role != uni::MessageRole::Tool {
                                return None;
                            }
                            serde_json::from_str::<serde_json::Value>(message.content.as_text().as_ref()).ok()
                        })
                        .expect("timer launch result");
                    Some(("write_stdin", json!({"session_id":output["session_id"],"chars":"","yield_time_ms":1000})))
                }
                6 => None,
                _ => panic!(
                    "finished work reopened after {} model requests: {:?}",
                    index,
                    request
                        .messages
                        .iter()
                        .rev()
                        .take(6)
                        .map(|message| (message.role, message.content.as_text().chars().take(1400).collect::<String>()))
                        .collect::<Vec<_>>()
                ),
            };
            let finish_reason = if call.is_some() {
                uni::FinishReason::ToolCalls
            } else {
                uni::FinishReason::Stop
            };
            Ok(uni::LLMResponse {
                content: call.is_none().then(||"Updated README; local check passed. Remote image rendering remains unverified because the host rate-limited requests.".into()),
                tool_calls:call.map(|(name,args)|vec![uni::ToolCall::function(format!("script-{index}"),name.into(),args.to_string())]),
                model:request.model,usage:None,finish_reason,
                reasoning:None,reasoning_details:None,organization_id:None,request_id:None,
                tool_references:vec![],compaction:None,
            })
        }
    }

    let requests = Arc::new(AtomicUsize::new(0));
    let fetches = Arc::new(AtomicUsize::new(0));
    let mut backing = TestTurnProcessingBacking::new(16).await;
    let registry = backing.turn_processing_context().tool_registry.clone();
    fs::write(registry.workspace_root().join("README.md"), "# Before\n").unwrap();
    let checker = registry.workspace_root().join("verify.sh");
    fs::write(&checker, "#!/bin/sh\nprintf 'checked\\n' >> checks.log\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&checker, fs::Permissions::from_mode(0o755)).unwrap();
    registry
        .register_tool(ToolRegistration::from_tool_instance(
            "web_fetch",
            CapabilityLevel::CodeSearch,
            LimitedFetch(fetches.clone()),
        ))
        .await
        .unwrap();
    registry.allow_all_tools().await.unwrap();
    let mut commands = registry.commands_config();
    commands.allow_list.push("./verify.sh".into());
    registry.apply_commands_config(&commands);
    registry
        .execute_tool(
            "task_tracker",
            json!({"action":"create","items":[{
                "description":"Verify README", "status":"completed", "verify":["./verify.sh"]
            }]}),
        )
        .await
        .unwrap();
    let mut completions = registry.exec_session_manager().subscribe_completion();
    backing.set_provider(Box::new(SessionScript { requests: requests.clone() }));
    let mut history = vec![uni::Message::user(
        "Update README heading and run ./verify.sh. Report unavailable remote evidence and finish.".into(),
    )];
    let outcome = run_turn_loop(&mut history, backing.turn_loop_context()).await.unwrap();
    assert!(matches!(outcome.result, TurnLoopResult::Completed { .. }), "{:?}", outcome.result);
    assert_eq!(requests.load(Ordering::SeqCst), 7);
    assert_eq!(fetches.load(Ordering::SeqCst), 1);
    assert_eq!(fs::read_to_string(registry.workspace_root().join("README.md")).unwrap(), "# After\n");
    assert_eq!(fs::read_to_string(registry.workspace_root().join("checks.log")).unwrap(), "checked\n");
    assert_eq!(
        history
            .iter()
            .filter(|message| message.role == uni::MessageRole::Assistant
                && message.content.as_text().starts_with("Updated README; local check passed."))
            .count(),
        1
    );
    let mut pending = PendingBackgroundCompletions::default();
    pending.observe_terminal_exec_results(&registry);
    let completion_notify = registry.exec_session_manager().completion_notify();
    tokio::time::timeout(std::time::Duration::from_secs(2), completion_notify.notified())
        .await
        .unwrap();
    let drained = drain_exec_session_completions(Some(&mut completions), &mut pending);
    assert_eq!(drained.added, 1, "awaited background timer must retain its canonical notification");
    assert!(!pending.should_schedule_continuation(false, false));
    assert_eq!(requests.load(Ordering::SeqCst), 7);
}
