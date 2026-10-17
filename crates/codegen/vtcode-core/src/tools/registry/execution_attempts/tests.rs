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
