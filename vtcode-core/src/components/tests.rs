use super::*;
use crate::cache::EvictionPolicy;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

// ================================================================
// Test executor provider
// ================================================================

struct EchoExecutor;

#[async_trait]
impl<Ctx: Send + Sync> ExecuteProvider<Ctx> for EchoExecutor {
    async fn execute(_ctx: &Ctx, args: Value) -> Result<Value> {
        Ok(serde_json::json!({
            "tool_name": "echo",
            "echoed": args,
        }))
    }
}

// ================================================================
// Test contexts with different component wiring
// ================================================================

struct TestAutoCtx;

struct EchoMetadata;

impl<Ctx> MetadataProvider<Ctx> for EchoMetadata {
    fn tool_name(_ctx: &Ctx) -> &'static str {
        "echo"
    }

    fn tool_description(_ctx: &Ctx) -> &'static str {
        "Echo tool"
    }
}

delegate_components!(TestAutoCtx {
    ApprovalComponent => AutoApproval,
    SandboxComponent  => NoSandbox,
    ExecuteComponent  => EchoExecutor,
    MetadataComponent => EchoMetadata,
    LoggingComponent  => NoLogging,
    CacheComponent    => NoCache,
    RetryComponent    => NoRetry,
});

struct TestDenyCtx;

struct ExecMetadata;

impl<Ctx> MetadataProvider<Ctx> for ExecMetadata {
    fn tool_name(_ctx: &Ctx) -> &'static str {
        "exec"
    }

    fn tool_description(_ctx: &Ctx) -> &'static str {
        "Exec tool"
    }
}

delegate_components!(TestDenyCtx {
    ApprovalComponent => DenyAllApproval,
    SandboxComponent  => NoSandbox,
    ExecuteComponent  => EchoExecutor,
    MetadataComponent => ExecMetadata,
    LoggingComponent  => NoLogging,
    CacheComponent    => NoCache,
    RetryComponent    => NoRetry,
});

struct TestTracingCtx;

struct TracedToolMetadata;

impl<Ctx> MetadataProvider<Ctx> for TracedToolMetadata {
    fn tool_name(_ctx: &Ctx) -> &'static str {
        "traced_tool"
    }

    fn tool_description(_ctx: &Ctx) -> &'static str {
        "A traced tool"
    }
}

delegate_components!(TestTracingCtx {
    ApprovalComponent => AutoApproval,
    SandboxComponent  => NoSandbox,
    ExecuteComponent  => EchoExecutor,
    MetadataComponent => TracedToolMetadata,
    LoggingComponent  => TracingLogging,
    CacheComponent    => NoCache,
    RetryComponent    => NoRetry,
});

struct NamedToolCtx;

struct NamedToolMetadata;

impl<Ctx> MetadataProvider<Ctx> for NamedToolMetadata {
    fn tool_name(_ctx: &Ctx) -> &'static str {
        "my_tool"
    }

    fn tool_description(_ctx: &Ctx) -> &'static str {
        "My description"
    }
}

delegate_components!(NamedToolCtx {
    ApprovalComponent => AutoApproval,
    SandboxComponent  => NoSandbox,
    ExecuteComponent  => EchoExecutor,
    MetadataComponent => NamedToolMetadata,
    LoggingComponent  => NoLogging,
    CacheComponent    => NoCache,
    RetryComponent    => NoRetry,
});

trait HasExecutionCount: Send + Sync {
    fn execution_count(&self) -> &AtomicUsize;
}

struct CountingExecutor;

#[async_trait]
impl<Ctx: HasExecutionCount + Send + Sync> ExecuteProvider<Ctx> for CountingExecutor {
    async fn execute(ctx: &Ctx, args: Value) -> Result<Value> {
        let count = ctx.execution_count().fetch_add(1, Ordering::SeqCst) + 1;
        Ok(serde_json::json!({
            "tool_name": "counting",
            "attempt": count,
            "args": args,
        }))
    }
}

struct TestCachingCtx {
    executions: Arc<AtomicUsize>,
    json_cache: UnifiedCache<ToolExecutionCacheKey, Value>,
    dual_cache: UnifiedCache<ToolExecutionCacheKey, SplitToolResult>,
}

impl TestCachingCtx {
    fn new(executions: Arc<AtomicUsize>) -> Self {
        Self {
            executions,
            json_cache: UnifiedCache::new(8, Duration::from_secs(60), EvictionPolicy::Lru),
            dual_cache: UnifiedCache::new(8, Duration::from_secs(60), EvictionPolicy::Lru),
        }
    }
}

impl HasExecutionCount for TestCachingCtx {
    fn execution_count(&self) -> &AtomicUsize {
        self.executions.as_ref()
    }
}

impl HasExecutionCaches for TestCachingCtx {
    fn json_cache(&self) -> &UnifiedCache<ToolExecutionCacheKey, Value> {
        &self.json_cache
    }

    fn dual_cache(&self) -> &UnifiedCache<ToolExecutionCacheKey, SplitToolResult> {
        &self.dual_cache
    }
}

struct CachedToolMetadata;

impl<Ctx> MetadataProvider<Ctx> for CachedToolMetadata {
    fn tool_name(_ctx: &Ctx) -> &'static str {
        "cached_tool"
    }

    fn tool_description(_ctx: &Ctx) -> &'static str {
        "A cached tool"
    }
}

delegate_components!(TestCachingCtx {
    ApprovalComponent => AutoApproval,
    SandboxComponent  => NoSandbox,
    ExecuteComponent  => CountingExecutor,
    MetadataComponent => CachedToolMetadata,
    LoggingComponent  => NoLogging,
    CacheComponent    => CachedResults,
    RetryComponent    => NoRetry,
});

struct FlakyExecutor;

#[async_trait]
impl<Ctx: HasExecutionCount + Send + Sync> ExecuteProvider<Ctx> for FlakyExecutor {
    async fn execute(ctx: &Ctx, args: Value) -> Result<Value> {
        let attempt = ctx.execution_count().fetch_add(1, Ordering::SeqCst) + 1;
        if attempt == 1 {
            anyhow::bail!("transient failure")
        }

        Ok(serde_json::json!({
            "tool_name": "flaky",
            "attempt": attempt,
            "args": args,
        }))
    }
}

struct TestRetryCtx {
    executions: Arc<AtomicUsize>,
    retry_policy: RetryPolicy,
}

impl TestRetryCtx {
    fn new(executions: Arc<AtomicUsize>, retry_policy: RetryPolicy) -> Self {
        Self {
            executions,
            retry_policy,
        }
    }
}

impl HasExecutionCount for TestRetryCtx {
    fn execution_count(&self) -> &AtomicUsize {
        self.executions.as_ref()
    }
}

impl HasRetryPolicy for TestRetryCtx {
    fn retry_policy(&self) -> RetryPolicy {
        self.retry_policy
    }
}

struct FlakyToolMetadata;

impl<Ctx> MetadataProvider<Ctx> for FlakyToolMetadata {
    fn tool_name(_ctx: &Ctx) -> &'static str {
        "flaky_tool"
    }

    fn tool_description(_ctx: &Ctx) -> &'static str {
        "A flaky tool"
    }
}

delegate_components!(TestRetryCtx {
    ApprovalComponent => AutoApproval,
    SandboxComponent  => NoSandbox,
    ExecuteComponent  => FlakyExecutor,
    MetadataComponent => FlakyToolMetadata,
    LoggingComponent  => NoLogging,
    CacheComponent    => NoCache,
    RetryComponent    => ExponentialBackoffRetry,
});

// ================================================================
// Phase 1 tests: approval + sandbox
// ================================================================

#[tokio::test]
async fn auto_ctx_approves() {
    let ctx = TestAutoCtx;
    let result = ComposableRuntime::run(&ctx, "grep", "search files").await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn consumer_trait_executes_directly_on_context() {
    let ctx = TestAutoCtx;
    let result = ctx
        .execute_tool_json("echo", serde_json::json!({"via": "consumer"}))
        .await
        .expect("context capability should execute");

    assert_eq!(
        result
            .get("echoed")
            .and_then(|value| value.get("via"))
            .and_then(|value| value.as_str()),
        Some("consumer")
    );
}

#[tokio::test]
async fn deny_ctx_rejects() {
    let ctx = TestDenyCtx;
    let result = ComposableRuntime::run(&ctx, "exec", "run command").await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("operation denied"));
}

#[tokio::test]
async fn sandbox_check_returns_policy() {
    let ctx = TestAutoCtx;
    let sandboxed = ComposableRuntime::run_with_sandbox(&ctx, "file_write", "write file")
        .await
        .expect("should succeed");
    assert!(!sandboxed);
}

struct StrictSandbox;

impl<Ctx: Send + Sync> SandboxProvider<Ctx> for StrictSandbox {
    fn sandbox_enabled(_ctx: &Ctx) -> bool {
        true
    }

    fn workspace_root(_ctx: &Ctx) -> Option<&PathBuf> {
        None
    }
}

struct StrictCtx;

delegate_components!(StrictCtx {
    ApprovalComponent => AutoApproval,
    SandboxComponent  => StrictSandbox,
});

#[tokio::test]
async fn strict_ctx_enables_sandbox() {
    let ctx = StrictCtx;
    let sandboxed = ComposableRuntime::run_with_sandbox(&ctx, "exec", "run cmd")
        .await
        .expect("should succeed");
    assert!(sandboxed);
}

// ================================================================
// Phase 2 tests: ToolFacade — same context projected as Tool
// ================================================================

#[tokio::test]
async fn tool_facade_executes_via_cgp() {
    let facade = ToolFacade::new(TestAutoCtx);
    let result = facade
        .execute(serde_json::json!({"msg": "hello"}))
        .await
        .expect("should succeed");

    assert_eq!(
        result
            .get("echoed")
            .and_then(|v| v.get("msg"))
            .and_then(|v| v.as_str()),
        Some("hello")
    );
}

#[tokio::test]
async fn tool_facade_denied_by_ctx() {
    let facade = ToolFacade::new(TestDenyCtx);
    let result = facade.execute(serde_json::json!({})).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("operation denied"));
}

#[tokio::test]
async fn tool_facade_name_and_description() {
    let facade = ToolFacade::new(NamedToolCtx);
    assert_eq!(facade.name(), "my_tool");
    assert_eq!(facade.description(), "My description");
}

#[tokio::test]
async fn tool_facade_dual_output() {
    let facade = ToolFacade::new(TestAutoCtx);
    let result = facade
        .execute_dual(serde_json::json!({"key": "value"}))
        .await
        .expect("should succeed");

    assert!(result.success);
}

// ================================================================
// Phase 2 tests: HandlerFacade — same context projected as ToolHandler
// ================================================================

#[tokio::test]
async fn handler_facade_executes_via_cgp() {
    let facade = HandlerFacade::new(TestAutoCtx);
    let session: Arc<dyn crate::tools::handlers::tool_handler::ToolSession> = Arc::new(
        crate::tools::handlers::adapter::DefaultToolSession::new(PathBuf::from("/tmp")),
    );
    let turn = Arc::new(crate::tools::handlers::tool_handler::TurnContext {
        cwd: PathBuf::from("/tmp"),
        turn_id: "test".to_string(),
        sub_id: None,
        shell_environment_policy:
            crate::tools::handlers::tool_handler::ShellEnvironmentPolicy::default(),
        approval_policy: crate::tools::handlers::tool_handler::Constrained::allow_any(
            crate::tools::handlers::tool_handler::ApprovalPolicy::default(),
        ),
        codex_linux_sandbox_exe: None,
        sandbox_policy: crate::tools::handlers::tool_handler::Constrained::allow_any(
            Default::default(),
        ),
    });
    let invocation = ToolInvocation {
        session,
        turn,
        tracker: None,
        call_id: "test-call".to_string(),
        tool_name: "echo".to_string(),
        payload: ToolPayload::Function {
            arguments: r#"{"msg":"handler"}"#.to_string(),
        },
    };

    let output = facade.handle(invocation).await.expect("should succeed");
    assert!(output.is_success());
    let content = output.content().expect("should have content");
    assert!(content.contains("handler"));
}

#[tokio::test]
async fn handler_facade_denied_by_ctx() {
    let facade = HandlerFacade::new(TestDenyCtx);
    let session: Arc<dyn crate::tools::handlers::tool_handler::ToolSession> = Arc::new(
        crate::tools::handlers::adapter::DefaultToolSession::new(PathBuf::from("/tmp")),
    );
    let turn = Arc::new(crate::tools::handlers::tool_handler::TurnContext {
        cwd: PathBuf::from("/tmp"),
        turn_id: "test".to_string(),
        sub_id: None,
        shell_environment_policy:
            crate::tools::handlers::tool_handler::ShellEnvironmentPolicy::default(),
        approval_policy: crate::tools::handlers::tool_handler::Constrained::allow_any(
            crate::tools::handlers::tool_handler::ApprovalPolicy::default(),
        ),
        codex_linux_sandbox_exe: None,
        sandbox_policy: crate::tools::handlers::tool_handler::Constrained::allow_any(
            Default::default(),
        ),
    });
    let invocation = ToolInvocation {
        session,
        turn,
        tracker: None,
        call_id: "test-call".to_string(),
        tool_name: "exec".to_string(),
        payload: ToolPayload::Function {
            arguments: "{}".to_string(),
        },
    };

    let result = facade.handle(invocation).await;
    assert!(result.is_err());
}

// ================================================================
// Phase 2 tests: same context, two facades — unification proof
// ================================================================

#[tokio::test]
async fn same_context_both_facades() {
    let tool = ToolFacade::new(TestAutoCtx);
    let tool_result = tool
        .execute(serde_json::json!({"via": "tool"}))
        .await
        .expect("tool facade should succeed");
    assert!(tool_result.get("echoed").is_some());

    let handler = HandlerFacade::new(TestAutoCtx);
    let session: Arc<dyn crate::tools::handlers::tool_handler::ToolSession> = Arc::new(
        crate::tools::handlers::adapter::DefaultToolSession::new(PathBuf::from("/tmp")),
    );
    let turn = Arc::new(crate::tools::handlers::tool_handler::TurnContext {
        cwd: PathBuf::from("/tmp"),
        turn_id: "test".to_string(),
        sub_id: None,
        shell_environment_policy:
            crate::tools::handlers::tool_handler::ShellEnvironmentPolicy::default(),
        approval_policy: crate::tools::handlers::tool_handler::Constrained::allow_any(
            crate::tools::handlers::tool_handler::ApprovalPolicy::default(),
        ),
        codex_linux_sandbox_exe: None,
        sandbox_policy: crate::tools::handlers::tool_handler::Constrained::allow_any(
            Default::default(),
        ),
    });
    let invocation = ToolInvocation {
        session,
        turn,
        tracker: None,
        call_id: "test-call".to_string(),
        tool_name: "echo".to_string(),
        payload: ToolPayload::Function {
            arguments: r#"{"via":"handler"}"#.to_string(),
        },
    };

    let handler_output = handler
        .handle(invocation)
        .await
        .expect("handler facade should succeed");
    assert!(handler_output.is_success());
    let content = handler_output.content().expect("should have content");
    assert!(content.contains("handler"));
}

// ================================================================
// Phase 6 tests: static logging, cache, and retry providers
// ================================================================

#[tokio::test]
async fn tracing_logging_executes() {
    let facade = ToolFacade::new(TestTracingCtx);
    let result = facade
        .execute(serde_json::json!({"test": true}))
        .await
        .expect("should succeed with tracing logging");

    assert!(result.get("echoed").is_some());
}

#[tokio::test]
async fn cached_results_short_circuit_second_execute() {
    let executions = Arc::new(AtomicUsize::new(0));
    let facade = ToolFacade::new(TestCachingCtx::new(executions.clone()));

    let first = facade
        .execute(serde_json::json!({"query": "same"}))
        .await
        .expect("first execution should succeed");
    let second = facade
        .execute(serde_json::json!({"query": "same"}))
        .await
        .expect("second execution should succeed");

    assert_eq!(executions.load(Ordering::SeqCst), 1);
    assert_eq!(first, second);
}

#[tokio::test]
async fn cached_results_short_circuit_dual_output() {
    let executions = Arc::new(AtomicUsize::new(0));
    let facade = ToolFacade::new(TestCachingCtx::new(executions.clone()));

    let first = facade
        .execute_dual(serde_json::json!({"query": "same"}))
        .await
        .expect("first dual execution should succeed");
    let second = facade
        .execute_dual(serde_json::json!({"query": "same"}))
        .await
        .expect("second dual execution should succeed");

    assert_eq!(executions.load(Ordering::SeqCst), 1);
    assert_eq!(first.ui_content, second.ui_content);
    assert_eq!(first.llm_content, second.llm_content);
}

#[tokio::test]
async fn retry_provider_retries_failed_execute() {
    let executions = Arc::new(AtomicUsize::new(0));
    let retry_policy = RetryPolicy {
        max_attempts: 2,
        initial_backoff: Duration::ZERO,
        max_backoff: Duration::ZERO,
    };
    let facade = ToolFacade::new(TestRetryCtx::new(executions.clone(), retry_policy));

    let result = facade
        .execute(serde_json::json!({"retry": true}))
        .await
        .expect("retry should recover the transient failure");

    assert_eq!(executions.load(Ordering::SeqCst), 2);
    assert_eq!(
        result.get("attempt").and_then(|value| value.as_u64()),
        Some(2)
    );
}

// ================================================================
// Phase 3 tests: concrete runtime contexts
// ================================================================

#[tokio::test]
async fn interactive_ctx_enables_sandbox() {
    let ctx = InteractiveCtx::new(PathBuf::from("/workspace"));
    let sandboxed = ComposableRuntime::run_with_sandbox(&ctx, "exec", "run cmd")
        .await
        .expect("should succeed");
    assert!(sandboxed);
}

#[tokio::test]
async fn ci_ctx_auto_approves_with_sandbox() {
    let ctx = CiCtx::new(PathBuf::from("/ci/workspace"));
    let sandboxed = ComposableRuntime::run_with_sandbox(&ctx, "exec", "run cmd")
        .await
        .expect("should succeed");
    assert!(sandboxed);
}

#[tokio::test]
async fn bench_ctx_no_sandbox() {
    let sandboxed = ComposableRuntime::run_with_sandbox(&BenchCtx, "exec", "run cmd")
        .await
        .expect("should succeed");
    assert!(!sandboxed);
}

// ================================================================
// Phase 3 tests: ToolBridgeCtx + PassthroughExecutor
// ================================================================

struct SimpleTool;

#[async_trait]
impl Tool for SimpleTool {
    async fn execute(&self, args: Value) -> Result<Value> {
        Ok(serde_json::json!({
            "tool_name": "simple",
            "input": args,
            "result": "ok"
        }))
    }

    fn name(&self) -> &'static str {
        "simple"
    }

    fn description(&self) -> &'static str {
        "A simple test tool"
    }

    fn parameter_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": {
                "query": { "type": "string" }
            }
        }))
    }

    fn default_permission(&self) -> ToolPolicy {
        ToolPolicy::Allow
    }

    fn is_mutating(&self) -> bool {
        false
    }

    fn kind(&self) -> &'static str {
        "test"
    }
}

#[tokio::test]
async fn bridge_interactive_passthrough() {
    let tool: Arc<dyn Tool> = Arc::new(SimpleTool);
    let facade = wrap_tool_interactive(tool, PathBuf::from("/workspace"));

    assert_eq!(facade.name(), "simple");
    assert_eq!(facade.description(), "A simple test tool");

    let result = facade
        .execute(serde_json::json!({"query": "test"}))
        .await
        .expect("should succeed");

    assert_eq!(result.get("result").and_then(|v| v.as_str()), Some("ok"));
    assert_eq!(
        result
            .get("input")
            .and_then(|v| v.get("query"))
            .and_then(|v| v.as_str()),
        Some("test")
    );
}

#[tokio::test]
async fn bridge_ci_passthrough() {
    let tool: Arc<dyn Tool> = Arc::new(SimpleTool);
    let facade = wrap_tool_ci(tool, PathBuf::from("/ci"));

    let result = facade
        .execute(serde_json::json!({"key": "value"}))
        .await
        .expect("should succeed");

    assert_eq!(result.get("result").and_then(|v| v.as_str()), Some("ok"));
}

#[tokio::test]
async fn bridge_passthrough_metadata_is_preserved() {
    let tool: Arc<dyn Tool> = Arc::new(SimpleTool);
    let facade = wrap_tool_interactive(tool, PathBuf::from("/workspace"));

    assert!(facade.parameter_schema().is_some());
    assert_eq!(facade.default_permission(), ToolPolicy::Allow);
    assert!(!facade.is_mutating());
    assert_eq!(facade.kind(), "test");
}

#[tokio::test]
async fn native_typed_tool_preserves_metadata_and_execution() {
    let facade = wrap_native_tool_interactive(SimpleTool, PathBuf::from("/workspace"));

    assert_eq!(facade.name(), "simple");
    assert_eq!(facade.description(), "A simple test tool");
    assert!(facade.parameter_schema().is_some());
    assert_eq!(facade.default_permission(), ToolPolicy::Allow);
    assert!(!facade.is_mutating());
    assert_eq!(facade.kind(), "test");

    let result = facade
        .execute(serde_json::json!({"query": "native"}))
        .await
        .expect("should succeed");
    assert_eq!(
        result
            .get("input")
            .and_then(|v| v.get("query"))
            .and_then(|v| v.as_str()),
        Some("native")
    );
}

#[tokio::test]
async fn bridge_dual_output() {
    let tool: Arc<dyn Tool> = Arc::new(SimpleTool);
    let facade = wrap_tool_interactive(tool, PathBuf::from("/workspace"));

    let result = facade
        .execute_dual(serde_json::json!({"x": 1}))
        .await
        .expect("should succeed");

    assert!(result.success);
    assert_eq!(result.tool_name, "simple");
}

#[tokio::test]
async fn bridge_handler_facade() {
    let tool: Arc<dyn Tool> = Arc::new(SimpleTool);
    let bridge_ctx = ToolBridgeCtx {
        inner: tool,
        runtime: InteractiveCtx::new(PathBuf::from("/workspace")),
    };
    let handler = HandlerFacade::new(bridge_ctx);

    let session: Arc<dyn crate::tools::handlers::tool_handler::ToolSession> = Arc::new(
        crate::tools::handlers::adapter::DefaultToolSession::new(PathBuf::from("/tmp")),
    );
    let turn = Arc::new(crate::tools::handlers::tool_handler::TurnContext {
        cwd: PathBuf::from("/tmp"),
        turn_id: "test".to_string(),
        sub_id: None,
        shell_environment_policy:
            crate::tools::handlers::tool_handler::ShellEnvironmentPolicy::default(),
        approval_policy: crate::tools::handlers::tool_handler::Constrained::allow_any(
            crate::tools::handlers::tool_handler::ApprovalPolicy::default(),
        ),
        codex_linux_sandbox_exe: None,
        sandbox_policy: crate::tools::handlers::tool_handler::Constrained::allow_any(
            Default::default(),
        ),
    });
    let invocation = ToolInvocation {
        session,
        turn,
        tracker: None,
        call_id: "bridge-test".to_string(),
        tool_name: "simple".to_string(),
        payload: ToolPayload::Function {
            arguments: r#"{"via":"bridge"}"#.to_string(),
        },
    };

    let output = handler.handle(invocation).await.expect("should succeed");
    assert!(output.is_success());
    let content = output.content().expect("should have content");
    assert!(content.contains("bridge"));
}

// ================================================================
// Phase 3 tests: HasComponent delegation through ToolBridgeCtx
// ================================================================

#[tokio::test]
async fn bridge_ctx_delegates_components() {
    let tool: Arc<dyn Tool> = Arc::new(SimpleTool);
    let bridge = ToolBridgeCtx {
        inner: tool,
        runtime: InteractiveCtx::new(PathBuf::from("/workspace")),
    };

    let sandboxed = ComposableRuntime::run_with_sandbox(&bridge, "exec", "test")
        .await
        .expect("should succeed");
    assert!(sandboxed);
}