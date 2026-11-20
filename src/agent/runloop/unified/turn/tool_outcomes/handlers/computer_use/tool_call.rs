//! Model-initiated computer turns through the `computer_use` tool.
//!
//! The turn loop intercepts validated `computer_use` calls here instead of
//! registry execution: the dispatcher, the permission-flow approval, and the
//! isolated target all live on the binary side of the core/binary boundary,
//! where registry executors cannot reach. The registered executor stays a
//! fail-closed stub for defense in depth.
//!
//! Every outcome maps onto the generic [`ToolPipelineOutcome`] contract so
//! batch tracking, circuit diagnostics, harness events, and transcript
//! rendering behave exactly like any other tool: completed turns report
//! redacted lines, denials and cancellations are `Cancelled`, and every
//! fail-closed refusal is a `Failure` carrying its guidance text.

use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use serde_json::Value;
use vtcode_core::llm::provider::{LLMRequest, Message, MessageContent, MessageRole};

use super::execution::{
    ComputerTurnOutcome, ComputerTurnOutcomeKind, ComputerTurnProposal, ComputerTurnRuntime, execute_proposed_turn,
    propose_computer_turn,
};
use super::report::turn_report_lines;
use super::{NATIVE_SANDBOX_BRIEFING, TOOL_NAME, request_target_approval};
use crate::agent::runloop::unified::tool_pipeline::status::{ToolExecutionStatus, ToolPipelineOutcome};
use crate::agent::runloop::unified::turn::context::TurnProcessingContext;

/// Common public suffixes plus RFC-reserved names, for bare-domain
/// detection. Deliberately small: unknown suffixes escape (documented)
/// rather than risk false refuses on filenames and versions.
const EXTERNAL_ADDRESS_SUFFIXES: &[&str] = &[
    "com",
    "net",
    "org",
    "io",
    "dev",
    "app",
    "ai",
    "co",
    "edu",
    "gov",
    "info",
    "biz",
    "us",
    "uk",
    "de",
    "fr",
    "jp",
    "local",
    "localhost",
    "invalid",
    "test",
    "example",
];

/// Whether a host part names an external address: IP literal, localhost,
/// or a dotted name under a known suffix. Strict on shape (valid hostname
/// characters, no leading/trailing dots or dashes) so versions, filenames,
/// and times fail open instead of refusing legitimate preview tasks.
fn host_is_external_address(hostport: &str) -> bool {
    if let Some(rest) = hostport.strip_prefix('[') {
        return rest
            .split_once(']')
            .is_some_and(|(inner, _)| inner.parse::<std::net::IpAddr>().is_ok());
    }
    if hostport.parse::<std::net::IpAddr>().is_ok() {
        return true;
    }
    let host = match hostport.rsplit_once(':') {
        Some((head, port)) if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => head,
        _ => hostport,
    };
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    match host.rsplit_once('.') {
        Some((head, tld)) => {
            !head.is_empty()
                && head.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
                && !head.starts_with(['.', '-'])
                && !head.ends_with(['.', '-'])
                && EXTERNAL_ADDRESS_SUFFIXES.contains(&tld.to_ascii_lowercase().as_str())
        }
        None => false,
    }
}

/// Whether the task names an address outside the offline preview: URL
/// schemes, IP literals, localhost, or bare domains. Guards the
/// fail-closed refusal above. Known limits, all fail-open: unknown
/// suffixes, scheme-less brand names ("screenshot google"), and single
/// labels escape to the system briefing instead; preview tasks that
/// merely mention a domain ("type example.com into the field") over-match.
fn task_requests_external_address(task: &str) -> bool {
    task.split_whitespace().any(|raw| {
        let token = raw.trim_matches(['\'', '"', '(', ')', ',', ';', '!', '?', '.']);
        let lower = token.to_ascii_lowercase();
        if lower.starts_with("http://") || lower.starts_with("https://") {
            return true;
        }
        let hostport = token.split(['/', '?', '#']).next().unwrap_or_default();
        host_is_external_address(hostport)
    })
}

/// Run one model-requested computer turn for `task`, approving through the
/// permission flow. Borrows never overlap: propose, approve, and execute
/// each take the turn context in turn.
pub(crate) async fn handle_computer_use_tool_call(
    ctx: &mut TurnProcessingContext<'_>,
    args_val: &Value,
) -> Result<ToolPipelineOutcome> {
    let Some(task) = args_val
        .get("task")
        .and_then(Value::as_str)
        .filter(|task| !task.trim().is_empty())
    else {
        return Ok(failure_outcome("computer_use needs a non-empty task describing one screen-sized goal."));
    };
    // Fail closed on tasks the offline sandbox cannot serve: without this,
    // an external-URL task would boot the placeholder preview, screenshot
    // it, and report success — a silent wrong answer the redacted outer
    // transcript cannot catch. Refuse before config, provider, proposal,
    // and approval; the check needs only the task text.
    if task_requests_external_address(task) {
        let outcome = ComputerTurnOutcome {
            kind: ComputerTurnOutcomeKind::Disabled {
                reason: "external address outside the offline preview",
            },
            model: ctx.config.model.clone(),
            usage: None,
        };
        return Ok(map_computer_outcome(&outcome));
    }
    let computer_use = match ctx.vt_cfg.as_ref() {
        Some(config) => config.computer_use.clone(),
        None => crate::main_helpers::load_workspace_config(&ctx.config.workspace)
            .context("Failed to load VT Code configuration")?
            .config()
            .computer_use
            .clone(),
    };
    let model = ctx.config.model.clone();
    let provider_name = ctx.provider_client.name().to_string();
    let request = LLMRequest {
        model,
        messages: Arc::new(vec![
            Message {
                role: MessageRole::System,
                content: MessageContent::Text(NATIVE_SANDBOX_BRIEFING.to_string()),
                ..Default::default()
            },
            Message {
                role: MessageRole::User,
                content: MessageContent::Text(task.to_string()),
                ..Default::default()
            },
        ]),
        ..Default::default()
    };
    let proposal = propose_computer_turn(
        &**ctx.provider_client,
        request,
        &computer_use,
        &mut ComputerTurnRuntime {
            stop: ctx.ctrl_c_state,
            notify: ctx.ctrl_c_notify,
            stats: ctx.session_stats,
            emitter: ctx.harness_emitter,
        },
    )
    .await?;
    let outcome = match proposal {
        ComputerTurnProposal::Done(outcome) => outcome,
        ComputerTurnProposal::Proceed(proposed) => {
            // Approval infrastructure failures are tool data, not turn
            // aborts (matching sibling tools); usage still records.
            let granted = match request_target_approval(ctx, &proposed.approval).await {
                Ok(granted) => granted,
                Err(err) => {
                    ctx.session_stats.record_usage(&provider_name, proposed.usage.as_ref());
                    return Ok(failure_outcome(&format!("Computer turn approval failed: {err:#}")));
                }
            };
            execute_proposed_turn(
                proposed,
                &computer_use,
                &mut ComputerTurnRuntime {
                    stop: ctx.ctrl_c_state,
                    notify: ctx.ctrl_c_notify,
                    stats: ctx.session_stats,
                    emitter: ctx.harness_emitter,
                },
                granted,
            )
            .await?
        }
    };
    ctx.session_stats.record_usage(&provider_name, outcome.usage.as_ref());
    Ok(map_computer_outcome(&outcome))
}

fn failure_outcome(message: &str) -> ToolPipelineOutcome {
    ToolPipelineOutcome::from_status(ToolExecutionStatus::Failure {
        error: vtcode_core::tools::registry::ToolExecutionError::from_anyhow(
            TOOL_NAME,
            &anyhow!(message.to_string()),
            0,
            false,
            false,
            Some("unified_runloop"),
        ),
    })
}

/// Map a dispatcher outcome onto the pipeline contract. Denials and user
/// cancellations are `Cancelled` (not failures); refusals carry their
/// guidance as the failure message so the model can relay it.
fn map_computer_outcome(outcome: &ComputerTurnOutcome) -> ToolPipelineOutcome {
    match &outcome.kind {
        ComputerTurnOutcomeKind::ApprovalDenied | ComputerTurnOutcomeKind::Cancelled => {
            ToolPipelineOutcome::from_status(ToolExecutionStatus::Cancelled)
        }
        ComputerTurnOutcomeKind::Completed { .. } => ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: Value::String(turn_report_lines(outcome).join("\n")),
            stdout: None,
            modified_files: Vec::new(),
            command_success: true,
        }),
        _ => failure_outcome(&turn_report_lines(outcome).join("\n")),
    }
}

#[cfg(test)]
mod tests {
    use super::super::execution::{NativeTurnResults, RedactedTurnSummary, TeardownOutcome};
    use super::*;
    use vtcode_core::llm::provider::{
        FinishReason, LLMError, LLMProvider, LLMRequest, LLMResponse, NativeCall, NativeCallBody, NativeProvider,
        NativeResponse,
    };

    struct ScriptedNativeProvider;

    #[async_trait::async_trait]
    impl LLMProvider for ScriptedNativeProvider {
        fn name(&self) -> &str {
            "scripted-native"
        }

        fn supports_streaming(&self) -> bool {
            false
        }

        async fn generate(&self, _request: LLMRequest) -> Result<LLMResponse, LLMError> {
            Err(LLMError::Provider {
                message: "computer turns generate natives, never chat completions".to_string(),
                metadata: None,
            })
        }

        fn supported_models(&self) -> Vec<String> {
            vec!["scripted-native-model".to_string()]
        }

        fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
            Ok(())
        }

        fn supports_computer_use(&self) -> bool {
            true
        }

        async fn generate_native(&self, _request: LLMRequest) -> Result<NativeResponse, LLMError> {
            Ok(NativeResponse {
                content: Some("acting now".to_string()),
                reasoning: None,
                model: "scripted-native-model".to_string(),
                usage: None,
                finish_reason: FinishReason::ToolCalls,
                tool_calls: Vec::new(),
                native_calls: vec![NativeCall {
                    id: "shot".to_string(),
                    provider: NativeProvider::Anthropic,
                    index: 1,
                    body: NativeCallBody::Anthropic {
                        name: "screenshot".to_string(),
                        action: Ok(vtcode_computer_use::Action::Screenshot),
                    },
                }],
                assistant_blocks: Vec::new(),
                request_id: None,
                organization_id: None,
            })
        }
    }

    fn outcome(kind: ComputerTurnOutcomeKind) -> ComputerTurnOutcome {
        ComputerTurnOutcome { kind, model: "test-model".to_string(), usage: None }
    }

    #[test]
    fn computer_outcome_mapping_keeps_denials_out_of_failures() {
        for kind in [
            ComputerTurnOutcomeKind::ApprovalDenied,
            ComputerTurnOutcomeKind::Cancelled,
        ] {
            let mapped = map_computer_outcome(&outcome(kind));
            assert!(matches!(mapped.status, ToolExecutionStatus::Cancelled), "denials must not trip failure tracking");
            assert!(!mapped.status.is_failure_like());
        }
    }

    #[test]
    fn computer_outcome_mapping_reports_completion_and_refusals() {
        let summary = RedactedTurnSummary {
            target_kind: "browser",
            target: "fixture-app".to_string(),
            recipient: "test-model (test)".to_string(),
            action_kinds: vec!["screenshot".to_string()],
            action_count: 1,
            executed_units: 1,
            review: None,
            teardown_trigger: "finished",
            teardown: TeardownOutcome::Removed,
        };
        let completed = map_computer_outcome(&outcome(ComputerTurnOutcomeKind::Completed {
            results: NativeTurnResults::OpenAi(vec![serde_json::json!({"call_id": "call_1"})]),
            summary,
        }));
        match &completed.status {
            ToolExecutionStatus::Success { output, command_success, .. } => {
                assert!(command_success);
                assert!(
                    output
                        .as_str()
                        .is_some_and(|text| text.contains("completed (test-model): 1 units"))
                );
                assert!(output.as_str().is_some_and(|text| text.contains("call call_1: ok")));
            }
            status => panic!("completed turn must succeed, got {status:?}"),
        }
        assert!(!completed.status.is_failure_like());

        for (kind, needle) in [
            (ComputerTurnOutcomeKind::Disabled { reason: "disabled" }, "did not run: disabled."),
            (ComputerTurnOutcomeKind::BudgetExceeded { proposed: 60, max: 50 }, "exceed the budget of 50"),
            (ComputerTurnOutcomeKind::ReviewHalted, "SUSPECT"),
        ] {
            let mapped = map_computer_outcome(&outcome(kind));
            match &mapped.status {
                ToolExecutionStatus::Failure { error } => assert!(error.message.contains(needle), "{needle}"),
                status => panic!("refusals must fail loudly, got {status:?}"),
            }
            assert!(mapped.status.is_failure_like());
        }
    }

    #[tokio::test]
    async fn tool_call_without_a_task_fails_before_any_model_call() {
        let mut backing = super::super::super::tests::TestContextBacking::new().await;
        let mut ctx = backing.turn_processing_context();
        for args in [serde_json::json!({}), serde_json::json!({"task": "  "})] {
            let outcome = handle_computer_use_tool_call(&mut ctx, &args).await.expect("tool result");
            match &outcome.status {
                ToolExecutionStatus::Failure { error } => {
                    assert!(error.message.contains("non-empty task"), "{}", error.message)
                }
                status => panic!("missing task must fail loudly, got {status:?}"),
            }
        }
    }

    // The headless harness auto-allows before reaching the interactive
    // prompt, so the scope-prompt skip itself is not headless-observable
    // (verified by mutation: proceed-without-session-skip passes with and
    // without the skip). The headless-pinned properties are that validation
    // proceeds by default (usability pin, not a skip proof) and that denies
    // still apply, since the skip sits after every deny check.
    #[tokio::test]
    async fn computer_use_validation_proceeds_by_default() {
        let mut backing = super::super::super::tests::TestContextBacking::new().await;
        let mut ctx = backing.turn_processing_context();
        ctx.session.set_skip_confirmations(false);
        let result = super::super::super::validation::validate_tool_call(
            &mut ctx,
            "proceed-1",
            "computer_use",
            &serde_json::json!({"task": "take a screenshot"}),
        )
        .await
        .expect("validate");
        assert!(
            !matches!(result, super::super::super::types::ValidationResult::Blocked),
            "default validation must proceed for computer_use"
        );
    }

    #[tokio::test]
    async fn computer_use_flagged_scope_review_proceeds_to_batch() {
        // The gateway always flags computer_use (destructive
        // classification); validation must still proceed so the review
        // happens once, at the batch approval. Two-part proof: the
        // premise (gateway flags here) and the reroute itself — without
        // it the headless prompt resolves to Exit (the test session has
        // no event sender), so asserting Proceed fails on a reverted
        // reroute. Single-prompt behavior itself is verified
        // interactively.
        use crate::agent::runloop::unified::tool_call_safety::invocation_id_from_call_id;
        use crate::agent::runloop::unified::tool_pipeline::validation::{
            SafetyValidationFailure, validate_tool_call_safety,
        };

        let mut backing = super::super::super::tests::TestContextBacking::new().await;
        let mut ctx = backing.turn_processing_context();
        ctx.session.set_skip_confirmations(false);
        let args = serde_json::json!({"task": "take a screenshot"});
        let safety = validate_tool_call_safety(
            ctx.safety_validator,
            "computer_use",
            &args,
            invocation_id_from_call_id("reroute-premise"),
        )
        .await;
        assert!(
            matches!(safety, Err(SafetyValidationFailure::NeedsApproval(_))),
            "gateway must flag computer_use (destructive classification)"
        );
        let result = super::super::super::validation::validate_tool_call(&mut ctx, "reroute-1", "computer_use", &args)
            .await
            .expect("validate");
        assert!(
            matches!(result, super::super::super::types::ValidationResult::Proceed(_)),
            "flagged computer_use must proceed past validation so the review reroutes to the batch approval"
        );
    }

    #[tokio::test]
    async fn computer_use_cached_denial_still_blocks() {
        let mut backing = super::super::super::tests::TestContextBacking::new().await;
        let mut ctx = backing.turn_processing_context();
        ctx.tool_permission_cache
            .write()
            .await
            .cache_grant_tool("computer_use", vtcode_core::acp::PermissionGrant::Denied);
        let result = super::super::super::validation::validate_tool_call(
            &mut ctx,
            "deny-1",
            "computer_use",
            &serde_json::json!({"task": "take a screenshot"}),
        )
        .await
        .expect("validate");
        assert!(
            matches!(result, super::super::super::types::ValidationResult::Blocked),
            "cached denial must still block computer_use"
        );
    }

    #[tokio::test]
    async fn computer_use_full_auto_allowlist_denies_at_validation() {
        // The tool stays visible under full-auto (allow-list visibility
        // bypass) so interactive steering keeps working — but execution
        // must still deny when the allow-list omits it. Fail-closed.
        let mut backing = super::super::super::tests::TestContextBacking::new().await;
        let mut ctx = backing.turn_processing_context();
        ctx.tool_registry
            .enable_full_auto_permission(&["exec_command".to_string()])
            .await;
        let result = super::super::super::validation::validate_tool_call(
            &mut ctx,
            "deny-2",
            "computer_use",
            &serde_json::json!({"task": "take a screenshot"}),
        )
        .await
        .expect("validate");
        assert!(
            matches!(result, super::super::super::types::ValidationResult::Blocked),
            "full-auto allow-list must deny computer_use at validation"
        );
    }

    #[tokio::test]
    async fn computer_use_prompts_exactly_once_across_validation_and_batch() {
        // Single-prompt proof: validation must skip the scope prompt
        // (reroute + skip) while the batch approval still prompts. The
        // scripted session approves the first prompt; the sender is then
        // dropped so any second prompt exits fast instead of hanging.
        // Recorder counts approvals, and counts stay under the learned
        // auto-approve threshold, so the count equals prompts shown: 0
        // means the batch review was skipped (unsafe), 2 means the scope
        // prompt leaked back. The post-approval outcome is intentionally
        // unasserted beyond not-cancelled (docker presence varies).
        use vtcode_ui::tui::app::{
            InlineEvent, InlineHandle, InlineListSelection, InlineSession, TransientEvent, TransientSubmission,
        };

        let mut backing = super::super::super::tests::TestContextBacking::new().await;
        let (command_tx, _command_rx) = tokio::sync::mpsc::unbounded_channel();
        let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();
        event_tx
            .send(InlineEvent::Transient(TransientEvent::Submitted(TransientSubmission::Selection(
                InlineListSelection::ToolApproval(true),
            ))))
            .expect("queue approval");
        drop(event_tx);
        let mut approving = InlineSession {
            handle: InlineHandle::new_for_tests(command_tx),
            events: event_rx,
            worker: None,
        };
        let mut ctx = backing.turn_processing_context();
        ctx.session = &mut approving;
        ctx.session.set_skip_confirmations(false);
        let target = tempfile::tempdir().expect("target dir");
        std::fs::write(target.path().join("index.html"), "<h1>preview</h1>").expect("seed target");
        let mut vt_cfg = vtcode_core::config::loader::VTCodeConfig::default();
        vt_cfg.computer_use.enabled = true;
        vt_cfg.computer_use.browser_build = Some(target.path().to_path_buf());
        ctx.vt_cfg = Some(&vt_cfg);
        *ctx.provider_client = Box::new(ScriptedNativeProvider);
        let args = serde_json::json!({"task": "take a screenshot of the preview"});
        let validation =
            super::super::super::validation::validate_tool_call(&mut ctx, "prompt-count-1", "computer_use", &args)
                .await
                .expect("validate");
        assert!(
            matches!(validation, super::super::super::types::ValidationResult::Proceed(_)),
            "validation must proceed past the skipped scope prompt"
        );
        let outcome = handle_computer_use_tool_call(&mut ctx, &args).await.expect("tool result");
        assert!(
            !matches!(outcome.status, ToolExecutionStatus::Cancelled),
            "consumed approval must proceed past the batch, got {:?}",
            outcome.status
        );
        let prompts = ctx.approval_recorder.get_approval_count("computer_use").await;
        assert_eq!(prompts, 1, "exactly one prompt must show per computer turn");
    }

    #[tokio::test]
    async fn tool_call_headless_denial_cancels_before_launch() {
        let mut backing = super::super::super::tests::TestContextBacking::new().await;
        let mut ctx = backing.turn_processing_context();
        let mut vt_cfg = vtcode_core::config::loader::VTCodeConfig::default();
        vt_cfg.computer_use.enabled = true;
        vt_cfg.computer_use.browser_build = Some(std::path::PathBuf::from("/tmp/fixture-build"));
        ctx.vt_cfg = Some(&vt_cfg);
        *ctx.provider_client = Box::new(ScriptedNativeProvider);
        let outcome = handle_computer_use_tool_call(&mut ctx, &serde_json::json!({"task": "take a screenshot"}))
            .await
            .expect("tool result");
        // Headless permission flow denies: the turn must end cancelled with
        // nothing launched (no docker needed to prove it).
        assert!(
            matches!(outcome.status, ToolExecutionStatus::Cancelled),
            "headless denial must cancel, got {:?}",
            outcome.status
        );
        assert!(!outcome.status.is_failure_like());
    }

    #[test]
    fn external_address_matrix_covers_schemes_ips_domains_and_near_misses() {
        let cases: &[(&str, bool)] = &[
            ("screenshot https://apple.com", true),
            ("open HTTP://X.COM/a", true),
            ("go to https://example.org/a/b?q=1#x", true),
            ("screenshot apple.com", true),
            ("'apple.com.'", true),
            ("(see example.io!)", true),
            ("www.example.com/x", true),
            ("ping 192.168.1.5", true),
            ("open http://[::1]:3000/", true),
            ("screenshot localhost:3000", true),
            ("check LOCALHOST", true),
            ("take a screenshot", false),
            ("check index.html renders", false),
            ("runner.mjs threw", false),
            ("version 1.64.1 broke", false),
            ("open report.pdf", false),
            ("check the intranet", false),
            ("screenshot google", false),
            // Known over-match, documented on the detector: a preview
            // task that merely mentions a domain still refuses.
            ("type example.com into the field", true),
        ];
        for (task, expected) in cases {
            assert_eq!(task_requests_external_address(task), *expected, "task={task:?}");
        }
    }

    struct NeverCalledProvider;

    #[async_trait::async_trait]
    impl LLMProvider for NeverCalledProvider {
        fn name(&self) -> &str {
            "never-called"
        }

        fn supports_streaming(&self) -> bool {
            false
        }

        async fn generate(&self, _request: LLMRequest) -> Result<LLMResponse, LLMError> {
            panic!("refused turn must never reach the provider");
        }

        fn supported_models(&self) -> Vec<String> {
            vec![]
        }

        fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
            panic!("refused turn must never reach the provider");
        }
    }

    #[tokio::test]
    async fn external_address_task_refuses_before_provider_config_and_approval() {
        let mut backing = super::super::super::tests::TestContextBacking::new().await;
        let mut ctx = backing.turn_processing_context();
        *ctx.provider_client = Box::new(NeverCalledProvider);
        let outcome =
            handle_computer_use_tool_call(&mut ctx, &serde_json::json!({"task": "screenshot https://apple.com"}))
                .await
                .expect("tool result");
        assert!(
            matches!(outcome.status, ToolExecutionStatus::Failure { .. }),
            "refusal must fail closed, got {:?}",
            outcome.status
        );
    }

    #[tokio::test]
    async fn tool_call_full_auto_denial_cancels_before_launch() {
        // Full-auto never auto-approves computer turns: with an active
        // allow-list that omits the tool, the batch approval denies and
        // the turn cancels without launching anything.
        let mut backing = super::super::super::tests::TestContextBacking::new().await;
        let mut ctx = backing.turn_processing_context();
        let mut vt_cfg = vtcode_core::config::loader::VTCodeConfig::default();
        vt_cfg.computer_use.enabled = true;
        vt_cfg.computer_use.browser_build = Some(std::path::PathBuf::from("/tmp/fixture-build"));
        ctx.vt_cfg = Some(&vt_cfg);
        *ctx.provider_client = Box::new(ScriptedNativeProvider);
        ctx.tool_registry
            .enable_full_auto_permission(&["exec_command".to_string()])
            .await;
        let outcome = handle_computer_use_tool_call(&mut ctx, &serde_json::json!({"task": "take a screenshot"}))
            .await
            .expect("tool result");
        assert!(
            matches!(outcome.status, ToolExecutionStatus::Cancelled),
            "full-auto denial must cancel, got {:?}",
            outcome.status
        );
        assert!(!outcome.status.is_failure_like());
    }
}
