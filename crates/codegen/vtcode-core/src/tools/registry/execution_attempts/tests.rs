use super::*;
use crate::config::constants::tools;
use crate::config::types::CapabilityLevel;
use crate::tools::registry::{ExecutionPolicySnapshot, ToolRegistration};
use crate::tools::traits::Tool;
use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tempfile::TempDir;

const PROBE_NAME: &str = "request_attempt_probe";

struct AttemptProbe {
    calls: Arc<AtomicUsize>,
    failures_before_success: usize,
}

#[async_trait]
impl Tool for AttemptProbe {
    async fn execute(&self, args: Value) -> Result<Value> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        if call <= self.failures_before_success {
            return Ok(json!({"error": {
                "tool_name": PROBE_NAME,
                "error_type": "NetworkError",
                "category": "Network",
                "message": "connection reset by peer",
                "retryable": true,
                "is_recoverable": true,
                "retry_after_ms": 1
            }}));
        }
        Ok(json!({"call": call, "input": args["input"]}))
    }

    fn name(&self) -> &str {
        PROBE_NAME
    }

    fn description(&self) -> &str {
        "Counts request attempts and returns controlled transient failures"
    }
}

async fn register_probe(registry: &ToolRegistry, failures_before_success: usize) -> Result<Arc<AtomicUsize>> {
    let calls = Arc::new(AtomicUsize::new(0));
    registry
        .register_tool(ToolRegistration::from_tool_instance(
            PROBE_NAME,
            CapabilityLevel::CodeSearch,
            AttemptProbe { calls: Arc::clone(&calls), failures_before_success },
        ))
        .await?;
    registry.allow_all_tools().await?;
    Ok(calls)
}

#[tokio::test]
async fn octen_web_search_and_aliases_never_use_generic_automatic_retries() -> Result<()> {
    let workspace = TempDir::new()?;
    let registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
    let calls = Arc::new(AtomicUsize::new(0));
    registry
        .register_tool(
            ToolRegistration::from_tool_instance(
                tools::WEB_SEARCH,
                CapabilityLevel::Basic,
                AttemptProbe {
                    calls: Arc::clone(&calls),
                    failures_before_success: usize::MAX,
                },
            )
            .with_aliases(["search_web", "websearch"]),
        )
        .await?;
    registry.allow_all_tools().await?;
    for (index, name) in [tools::WEB_SEARCH, "search_web", "websearch"].iter().enumerate() {
        let outcome = registry
            .execute_public_tool_request(
                ToolExecutionRequest::new(*name, json!({"query":"rust"})).with_policy(retry_policy(5)),
            )
            .await;
        assert!(!outcome.is_success());
        assert_eq!(outcome.attempts, 1);
        assert_eq!(calls.load(Ordering::SeqCst), index + 1);
    }
    Ok(())
}

fn retry_policy(max_retries: usize) -> ExecutionPolicySnapshot {
    ExecutionPolicySnapshot {
        max_retries,
        retry_base_delay: Duration::from_millis(1),
        retry_max_delay: Duration::from_millis(1),
        retry_multiplier: 1.0,
        ..Default::default()
    }
}

#[tokio::test]
async fn successful_first_attempt_preserves_output_and_metadata() -> Result<()> {
    let workspace = TempDir::new()?;
    let registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
    let calls = register_probe(&registry, 0).await?;
    let outcome = registry
        .execute_public_tool_request(ToolExecutionRequest::new(PROBE_NAME, json!({"input": "first"})))
        .await;
    assert!(outcome.is_success());
    assert_eq!(outcome.attempts, 1);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(outcome.output.expect("output")["input"], "first");
    assert_eq!(outcome.last_error_category, None);
    assert_eq!(registry.execution_history.get_recent_records(10).len(), 1);
    Ok(())
}

#[tokio::test]
async fn structured_failure_recovers_on_second_attempt() -> Result<()> {
    let workspace = TempDir::new()?;
    let registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
    let calls = register_probe(&registry, 1).await?;
    let outcome = registry
        .execute_public_tool_request(
            ToolExecutionRequest::new(PROBE_NAME, json!({"input": "recover"})).with_policy(retry_policy(1)),
        )
        .await;
    assert!(outcome.is_success(), "{:?}", outcome.error);
    assert_eq!(outcome.attempts, 2);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let output = outcome.output.expect("recovered output");
    assert_eq!(output["call"], 2);
    assert_eq!(output["input"], "recover");
    assert_eq!(outcome.last_error_category, Some(ErrorCategory::Network));
    let records = registry.execution_history.get_recent_records(10);
    assert_eq!(records.len(), 2);
    assert_eq!(records.iter().filter(|record| record.success).count(), 1);
    Ok(())
}

#[tokio::test]
async fn exhausted_retry_budget_preserves_last_error_context() -> Result<()> {
    let workspace = TempDir::new()?;
    let registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
    let calls = register_probe(&registry, 2).await?;
    let outcome = registry
        .execute_public_tool_request(
            ToolExecutionRequest::new(PROBE_NAME, json!({"input": "exhaust"})).with_policy(retry_policy(1)),
        )
        .await;
    assert!(!outcome.is_success());
    assert_eq!(outcome.attempts, 2);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert!(outcome.output.is_none());
    assert_eq!(outcome.last_error_category, Some(ErrorCategory::Network));
    let error = outcome.error.expect("terminal error");
    assert_eq!(error.tool_name, PROBE_NAME);
    assert_eq!(error.message, "connection reset by peer");
    assert_eq!(error.attempts_made(), Some(2));
    assert_eq!(error.debug_context.expect("context").surface.as_deref(), Some("tool_registry"));
    Ok(())
}

#[tokio::test]
async fn safety_limit_rejects_before_dispatch_and_without_retry() -> Result<()> {
    let workspace = TempDir::new()?;
    let registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
    let calls = register_probe(&registry, 0).await?;
    registry.safety_gateway().set_limits(0, 100);
    let outcome = registry
        .execute_public_tool_request(
            ToolExecutionRequest::new(PROBE_NAME, json!({"input": "denied"})).with_policy(retry_policy(3)),
        )
        .await;
    assert!(!outcome.is_success());
    assert_eq!(outcome.attempts, 1);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(registry.execution_history.get_recent_records(10).is_empty());
    let error = outcome.error.expect("safety error");
    assert_eq!(error.category, ErrorCategory::ResourceExhausted);
    assert!(!error.retryable);
    assert_eq!(error.attempts_made(), Some(1));
    assert!(error.message.contains("Per-turn tool limit reached"));
    Ok(())
}

#[tokio::test]
async fn dispatch_failure_remains_terminal_despite_retry_budget() -> Result<()> {
    let workspace = TempDir::new()?;
    let registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
    let outcome = registry
        .execute_public_tool_request(
            ToolExecutionRequest::new("unknown_attempt_tool", json!({})).with_policy(retry_policy(3)),
        )
        .await;
    assert!(!outcome.is_success());
    assert_eq!(outcome.attempts, 1);
    assert!(outcome.output.is_none());
    let error = outcome.error.expect("dispatch error");
    assert_eq!(error.tool_name, "unknown_attempt_tool");
    assert!(!error.retryable);
    assert_eq!(error.debug_context.expect("context").surface.as_deref(), Some("tool_registry"));
    assert!(registry.exec_sessions.list_sessions().await.is_empty());
    Ok(())
}

#[tokio::test]
async fn safety_denial_categories_keep_recovery_and_retry_metadata() -> Result<()> {
    let workspace = TempDir::new()?;
    let registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
    struct DenialCase {
        violation: GatewaySafetyError,
        category: ErrorCategory,
        retryable: bool,
        recoverable: bool,
    }
    let cases = [
        DenialCase {
            violation: GatewaySafetyError::RateLimitExceeded { current: 2, max: 1, window: "1s" },
            category: ErrorCategory::RateLimit,
            retryable: true,
            recoverable: true,
        },
        DenialCase {
            violation: GatewaySafetyError::TurnLimitReached { max: 1 },
            category: ErrorCategory::ResourceExhausted,
            retryable: false,
            recoverable: false,
        },
        DenialCase {
            violation: GatewaySafetyError::SessionLimitReached { max: 2 },
            category: ErrorCategory::ResourceExhausted,
            retryable: false,
            recoverable: false,
        },
        DenialCase {
            violation: GatewaySafetyError::PlanningPolicyViolation("write blocked".to_string()),
            category: ErrorCategory::PlanningPolicyViolation,
            retryable: false,
            recoverable: true,
        },
    ];
    for case in cases {
        let error = registry.safety_denial_error(
            PROBE_NAME,
            "bounded denial",
            Some(case.violation),
            Some(Duration::from_millis(25)),
        );
        assert_eq!(error.category, case.category);
        assert_eq!(error.retryable, case.retryable);
        assert_eq!(error.is_recoverable, case.recoverable);
        assert_eq!(error.retry_after_ms, Some(25));
        assert_eq!(error.message, "Safety gateway denied execution: bounded denial");
        assert_eq!(error.to_json_value()["error"]["retry_after_ms"], 25);
    }
    Ok(())
}

#[tokio::test]
async fn unsandboxed_request_requires_approval_even_when_prevalidated() -> Result<()> {
    let workspace = TempDir::new()?;
    let registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
    registry.allow_all_tools().await?;
    for permission in ["require_escalated", "BYPASS_SANDBOX"] {
        let outcome = registry
            .execute_public_tool_request(
                ToolExecutionRequest::new(
                    tools::EXEC_COMMAND,
                    json!({"cmd": "touch must_not_exist", "sandbox_permissions": permission}),
                )
                .with_policy(retry_policy(3).with_prevalidated(true).with_safety_prevalidated(true)),
            )
            .await;
        assert!(!outcome.is_success());
        assert_eq!(outcome.attempts, 1);
        let error = outcome.error.expect("approval error");
        assert_eq!(error.error_type, ToolErrorType::PolicyViolation);
        assert!(error.message.contains("enforced operator approval decision"));
    }
    assert!(!workspace.path().join("must_not_exist").exists());
    assert!(registry.exec_sessions.list_sessions().await.is_empty());
    assert!(registry.execution_history.get_recent_records(10).is_empty());
    Ok(())
}

#[tokio::test]
async fn invocation_approval_survives_retry_but_not_a_separate_call() -> Result<()> {
    use crate::tool_policy::ToolPolicy;
    let workspace = TempDir::new()?;
    let registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
    let calls = register_probe(&registry, 1).await?;
    registry.set_tool_policy(PROBE_NAME, ToolPolicy::Prompt).await?;
    registry.mark_tool_preapproved(PROBE_NAME).await;
    let request = || ToolExecutionRequest::new(PROBE_NAME, json!({"input":"approved"})).with_policy(retry_policy(2));
    let outcome = registry.execute_public_tool_request(request()).await;
    assert!(outcome.is_success(), "{:?}", outcome.error);
    assert_eq!(outcome.attempts, 2);
    for args in [json!({"input":"approved"}), json!({"input":"changed"})] {
        let denied = registry
            .execute_public_tool_request(ToolExecutionRequest::new(PROBE_NAME, args).with_policy(retry_policy(2)))
            .await;
        assert!(!denied.is_success());
        assert_eq!(denied.attempts, 1);
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    Ok(())
}

struct HttpProbe {
    calls: Arc<AtomicUsize>,
    status: u16,
    retry_after_ms: u64,
    failures: usize,
    policy_change: Option<Arc<super::super::policy::ToolPolicyGateway>>,
}

#[async_trait]
impl Tool for HttpProbe {
    async fn execute(&self, args: Value) -> Result<Value> {
        let url = url::Url::parse(args["url"].as_str().unwrap())?;
        let budget = crate::tools::web_fetch::request_budget::RequestBudget::current();
        let _transport = match &budget {
            Some(budget) => Some(budget.lock_transport().await?),
            None => None,
        };
        if let Some(budget) = &budget
            && let Err(error) = budget.admit(&url)
        {
            return Ok(error.to_json_value());
        }
        let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        if let Some(registry) = &self.policy_change {
            registry
                .set_tool_policy(tools::WEB_FETCH, crate::tool_policy::ToolPolicy::Deny)
                .await?;
        }
        if call <= self.failures {
            let output = json!({"error":format!("HTTP request failed with status: {}", self.status),
                "error_type":"http_error", "http_status":self.status,
                "retry_after_ms":self.retry_after_ms, "next_action":"Disclose unavailable external evidence."});
            match budget.as_ref() {
                Some(budget) => Ok(budget
                    .observe(&url, ToolExecutionError::from_tool_output(&output).unwrap())
                    .to_json_value()),
                None => Ok(output),
            }
        } else {
            Ok(json!({"preview":"remote text", "call":call}))
        }
    }
    fn name(&self) -> &str {
        tools::WEB_FETCH
    }
    fn description(&self) -> &str {
        "Controlled HTTP fetch outcome without external network access"
    }
}

async fn http_probe(
    registry: &ToolRegistry,
    status: u16,
    delay: u64,
    failures: usize,
    change_policy: bool,
) -> Result<Arc<AtomicUsize>> {
    let calls = Arc::new(AtomicUsize::new(0));
    registry
        .register_tool(ToolRegistration::from_tool_instance(
            tools::WEB_FETCH,
            CapabilityLevel::CodeSearch,
            HttpProbe {
                calls: calls.clone(),
                status,
                retry_after_ms: delay,
                failures,
                policy_change: change_policy.then(|| registry.policy_gateway.clone()),
            },
        ))
        .await?;
    registry.allow_all_tools().await?;
    Ok(calls)
}

fn fetch_request(url: &str) -> ToolExecutionRequest {
    ToolExecutionRequest::new(tools::WEB_FETCH, json!({"url":url})).with_policy(retry_policy(5))
}

#[tokio::test]
async fn approved_fetch_transient_failure_recovers_and_policy_changes_revoke_retry() -> Result<()> {
    for change_policy in [false, true] {
        let workspace = TempDir::new()?;
        let registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
        let calls = http_probe(&registry, 503, 1, 1, change_policy).await?;
        registry
            .set_tool_policy(tools::WEB_FETCH, crate::tool_policy::ToolPolicy::Prompt)
            .await?;
        registry.mark_tool_preapproved(tools::WEB_FETCH).await;
        let outcome = registry
            .execute_public_tool_request(fetch_request("https://example.com/page"))
            .await;
        assert_eq!(outcome.is_success(), !change_policy, "{:?}", outcome.error);
        assert_eq!(calls.load(Ordering::SeqCst), if change_policy { 1 } else { 2 });
        if change_policy {
            assert_eq!(outcome.error.unwrap().error_type, ToolErrorType::PolicyViolation);
        }
    }
    Ok(())
}

#[tokio::test]
async fn rate_limited_origin_stays_bounded_across_paths_and_internal_continuations() -> Result<()> {
    let workspace = TempDir::new()?;
    let registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
    let calls = http_probe(&registry, 429, 1, usize::MAX, false).await?;
    let outcome = registry
        .execute_public_tool_request(fetch_request("https://example.com/one"))
        .await;
    assert_eq!(outcome.attempts, 2);
    let error = outcome.error.expect("HTTP limitation");
    let wire = error.to_json_value();
    assert_eq!(wire["error"]["http_status"], 429);
    assert_eq!(wire["error"]["retry_after_ms"], 1);
    assert_eq!(wire["error"]["recovery_suggestions"][0], "Disclose unavailable external evidence.");
    // No genuine-request reset, just another invocation as an internal turn would issue.
    for path in ["two", "three"] {
        let outcome = registry
            .execute_public_tool_request(fetch_request(&format!("https://example.com/{path}")))
            .await;
        assert!(!outcome.is_success());
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    registry
        .execute_public_tool_request(fetch_request("https://other.example.org/one"))
        .await;
    // Other existing safety/loop limits may stop the second attempt; the
    // origin budget must admit this different origin without reopening the first.
    assert!((3..=4).contains(&calls.load(Ordering::SeqCst)));
    registry.begin_tracker_request(false);
    assert!(registry.harness_context.fetch_origin_attempts.is_empty());
    Ok(())
}

#[tokio::test]
async fn oversized_delays_and_exhausted_deadlines_do_not_retry() -> Result<()> {
    for (delay, timeout) in [(5001, 30), (1001, 1), (u64::MAX, 30)] {
        let workspace = TempDir::new()?;
        let registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
        let calls = http_probe(&registry, 429, delay, usize::MAX, false).await?;
        let mut request = fetch_request("https://example.com/slow");
        request.args["timeout_secs"] = json!(timeout);
        let outcome =
            tokio::time::timeout(Duration::from_secs(1), registry.execute_public_tool_request(request)).await?;
        assert_eq!(outcome.attempts, 1);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let error = outcome.error.unwrap();
        assert!(!error.retryable);
        assert_eq!(error.retry_after_ms, Some(delay));
        let next = registry
            .execute_public_tool_request(fetch_request("https://example.com/another-path"))
            .await;
        assert!(!next.is_success());
        assert_eq!(next.error.unwrap().retry_after_ms, Some(delay));
        assert_eq!(calls.load(Ordering::SeqCst), 1, "terminal retry rejection must survive another invocation");
    }
    Ok(())
}

#[tokio::test]
async fn denied_fetch_does_not_consume_the_origins_network_attempts() -> Result<()> {
    let workspace = TempDir::new()?;
    let registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
    let calls = http_probe(&registry, 429, 1, 1, false).await?;
    registry
        .set_tool_policy(tools::WEB_FETCH, crate::tool_policy::ToolPolicy::Deny)
        .await?;
    let denied = registry
        .execute_public_tool_request(fetch_request("https://example.com/denied"))
        .await;
    assert_eq!(denied.error.unwrap().error_type, ToolErrorType::PolicyViolation);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    registry
        .set_tool_policy(tools::WEB_FETCH, crate::tool_policy::ToolPolicy::Allow)
        .await?;
    let approved = registry
        .execute_public_tool_request(fetch_request("https://example.com/approved"))
        .await;
    assert!(approved.is_success(), "{:?}", approved.error);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    Ok(())
}

#[tokio::test]
async fn concurrent_rate_limited_calls_share_the_origin_budget() -> Result<()> {
    let workspace = TempDir::new()?;
    let registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
    let calls = http_probe(&registry, 429, 1, usize::MAX, false).await?;
    let (first, second) = tokio::join!(
        registry.execute_public_tool_request(fetch_request("https://example.com/a")),
        registry.execute_public_tool_request(fetch_request("https://example.com/b")),
    );
    assert!(!first.is_success());
    assert!(!second.is_success());
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    Ok(())
}

#[tokio::test]
async fn cancellation_drops_retry_wait_without_another_network_attempt() -> Result<()> {
    let workspace = TempDir::new()?;
    let registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
    let calls = http_probe(&registry, 429, 5000, usize::MAX, false).await?;
    let result = tokio::time::timeout(
        Duration::from_millis(100),
        registry.execute_public_tool_request(fetch_request("https://example.com/wait")),
    )
    .await;
    assert!(result.is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    // Cancellation releases the transport permit, but cannot erase Retry-After.
    let resumed = registry
        .execute_public_tool_request_with_deadline(
            fetch_request("https://example.com/after-cancel"),
            Instant::now() + Duration::from_millis(200),
        )
        .await;
    assert!(!resumed.is_success());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    registry.begin_tracker_request(false);
    let mut fresh = fetch_request("https://example.com/fresh-request");
    fresh.policy.max_retries = 0;
    registry.execute_public_tool_request(fresh).await;
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    Ok(())
}

#[tokio::test]
async fn retry_wait_respects_the_callers_execution_deadline() -> Result<()> {
    let workspace = TempDir::new()?;
    let registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
    let calls = http_probe(&registry, 429, 1000, usize::MAX, false).await?;
    let outcome = registry
        .execute_public_tool_request_with_deadline(
            fetch_request("https://example.com/deadline"),
            Instant::now() + Duration::from_millis(200),
        )
        .await;
    assert_eq!(outcome.attempts, 1);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(outcome.error.unwrap().retry_after_ms, Some(1000));
    Ok(())
}

#[tokio::test]
async fn concurrent_fetch_invocations_cannot_share_one_shot_approval() -> Result<()> {
    let workspace = TempDir::new()?;
    let registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
    let calls = http_probe(&registry, 503, 1, 1, false).await?;
    registry
        .set_tool_policy(tools::WEB_FETCH, crate::tool_policy::ToolPolicy::Prompt)
        .await?;
    registry.mark_tool_preapproved(tools::WEB_FETCH).await;
    let (first, second) = tokio::join!(
        registry.execute_public_tool_request(fetch_request("https://example.com/a")),
        registry.execute_public_tool_request(fetch_request("https://example.com/b")),
    );
    assert!(first.is_success());
    assert!(!second.is_success());
    assert_eq!(second.attempts, 1);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    Ok(())
}

#[tokio::test]
async fn retry_approval_cannot_cross_workspace_registry_scope() -> Result<()> {
    let first_workspace = TempDir::new()?;
    let second_workspace = TempDir::new()?;
    let first = ToolRegistry::new(first_workspace.path().to_path_buf()).await;
    let second = ToolRegistry::new(second_workspace.path().to_path_buf()).await;
    let first_calls = register_probe(&first, 0).await?;
    let second_calls = register_probe(&second, 0).await?;
    for registry in [&first, &second] {
        registry
            .set_tool_policy(PROBE_NAME, crate::tool_policy::ToolPolicy::Prompt)
            .await?;
    }
    assert_eq!(first.policy_gateway.revision(), second.policy_gateway.revision());
    let args = json!({"input":"same arguments"});
    let approval = RetryApproval {
        tool_name: PROBE_NAME.into(),
        args: args.clone(),
        policy_revision: first.policy_gateway.revision(),
        policy_gateway: first.policy_gateway.clone(),
        policy: crate::tool_policy::ToolPolicy::Prompt,
    };
    RETRY_APPROVAL
        .scope(std::cell::RefCell::new(Some(approval)), async {
            let denied = second.execute_tool(PROBE_NAME, args.clone()).await;
            assert!(denied.is_err(), "a different workspace must retain admission checks");
            assert_eq!(second_calls.load(Ordering::SeqCst), 0);
            let admitted = first.execute_tool(PROBE_NAME, args).await?;
            assert_eq!(admitted["call"], 1);
            assert_eq!(first_calls.load(Ordering::SeqCst), 1);
            Ok::<(), anyhow::Error>(())
        })
        .await?;
    Ok(())
}
