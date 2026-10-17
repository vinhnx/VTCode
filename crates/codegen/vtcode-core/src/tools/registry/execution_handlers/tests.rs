use super::*;
use crate::config::types::CapabilityLevel;
use crate::tool_policy::ToolPolicy;
use async_trait::async_trait;
use futures::future::BoxFuture;
use serde_json::json;
use tempfile::TempDir;

const TOOL_NAME: &str = "handler_probe";
const TOOL_ALIAS: &str = "handler_probe_alias";

struct TaggedTool(&'static str);

#[async_trait]
impl Tool for TaggedTool {
    async fn execute(&self, args: Value) -> Result<Value> {
        Ok(json!({"handler": self.0, "received": args}))
    }

    fn name(&self) -> &str {
        TOOL_NAME
    }

    fn description(&self) -> &str {
        "Identifies the selected handler"
    }
}

fn function_handler<'a>(_registry: &'a ToolRegistry, args: Value) -> BoxFuture<'a, Result<Value>> {
    Box::pin(async move { Ok(json!({"handler": "function", "received": args})) })
}

fn failing_handler<'a>(_registry: &'a ToolRegistry, _args: Value) -> BoxFuture<'a, Result<Value>> {
    Box::pin(async { Err(anyhow::anyhow!("handler root cause").context("handler context")) })
}

#[tokio::test]
async fn function_handler_preserves_payload_and_pool_toggle_without_using_trait_cache() -> Result<()> {
    let workspace = TempDir::new()?;
    let mut registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
    let registration = ToolRegistration::new(TOOL_NAME, CapabilityLevel::CodeSearch, false, function_handler);
    let cached: Arc<dyn Tool> = Arc::new(TaggedTool("cached"));
    let args = json!({"ordered": [3, 1], "label": "function input"});
    for pooling in [false, true] {
        registry.optimization_config.memory_pool.enabled = pooling;
        let before = registry.memory_pool.get_stats();
        let result = registry
            .execute_registered_handler(TOOL_NAME, &registration, args.clone(), Some(&cached))
            .await?;
        let after = registry.memory_pool.get_stats();
        assert_eq!(result, json!({"handler": "function", "received": args}));
        let expected_retrievals = usize::from(pooling);
        assert_eq!(
            after.value_hits + after.value_misses - before.value_hits - before.value_misses,
            expected_retrievals
        );
        assert_eq!(
            after.string_hits + after.string_misses - before.string_hits - before.string_misses,
            expected_retrievals
        );
        assert_eq!(after.vec_hits + after.vec_misses - before.vec_hits - before.vec_misses, expected_retrievals);
        assert!(registry.hot_tool_cache.read().peek(TOOL_NAME).is_none());
    }
    Ok(())
}

#[tokio::test]
async fn trait_handler_uses_supplied_cache_only_when_optimization_is_enabled() -> Result<()> {
    let workspace = TempDir::new()?;
    let mut registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
    let registration =
        ToolRegistration::from_tool_instance(TOOL_NAME, CapabilityLevel::CodeSearch, TaggedTool("registered"));
    let cached: Arc<dyn Tool> = Arc::new(TaggedTool("cached"));
    for optimized in [false, true] {
        registry.optimization_config.tool_registry.use_optimized_registry = optimized;
        let result = registry
            .execute_registered_handler(TOOL_NAME, &registration, json!({"probe": 17}), Some(&cached))
            .await?;
        assert_eq!(result["handler"], if optimized { "cached" } else { "registered" });
        assert_eq!(result["received"], json!({"probe": 17}));
        assert!(registry.hot_tool_cache.read().peek(TOOL_NAME).is_none());
    }
    Ok(())
}

#[tokio::test]
async fn uncached_trait_handler_populates_only_canonical_key_when_enabled() -> Result<()> {
    let workspace = TempDir::new()?;
    let mut registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
    let registration =
        ToolRegistration::from_tool_instance(TOOL_NAME, CapabilityLevel::CodeSearch, TaggedTool("registered"))
            .with_aliases([TOOL_ALIAS]);
    for optimized in [false, true] {
        registry.optimization_config.tool_registry.use_optimized_registry = optimized;
        let result = registry
            .execute_registered_handler(TOOL_NAME, &registration, json!({"probe": 23}), None)
            .await?;
        assert_eq!(result, json!({"handler": "registered", "received": {"probe": 23}}));
        let cache = registry.hot_tool_cache.read();
        assert_eq!(cache.peek(TOOL_NAME).is_some(), optimized);
        assert!(cache.peek(TOOL_ALIAS).is_none());
    }
    Ok(())
}

#[tokio::test]
async fn handler_error_chain_remains_raw_until_facade_records_it() -> Result<()> {
    let workspace = TempDir::new()?;
    let mut registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
    let registration = ToolRegistration::new(TOOL_NAME, CapabilityLevel::CodeSearch, false, failing_handler);
    for pooling in [false, true] {
        registry.optimization_config.memory_pool.enabled = pooling;
        let err = registry
            .execute_registered_handler(TOOL_NAME, &registration, json!({}), None)
            .await
            .unwrap_err();
        assert_eq!(err.to_string(), "handler context");
        assert_eq!(err.root_cause().to_string(), "handler root cause");
        assert!(registry.execution_history.get_recent_records(10).is_empty());
    }
    Ok(())
}

#[tokio::test]
async fn public_function_alias_retains_canonical_history_and_handler_args() -> Result<()> {
    let workspace = TempDir::new()?;
    let registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
    registry
        .register_tool(
            ToolRegistration::new(TOOL_NAME, CapabilityLevel::CodeSearch, false, function_handler)
                .with_aliases([TOOL_ALIAS])
                .with_permission(ToolPolicy::Allow),
        )
        .await?;
    registry.set_tool_policy(TOOL_NAME, ToolPolicy::Allow).await?;
    let args = json!({"ordered": [8, 2], "max_output_tokens": 333});
    let result = registry.execute_tool_ref(TOOL_ALIAS, &args).await?;
    assert_eq!(result, json!({"success": true, "handler": "function", "received": {"ordered": [8, 2]}}));
    let records = registry.execution_history.get_recent_records(10);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].tool_name, TOOL_NAME);
    assert_eq!(records[0].requested_name, TOOL_ALIAS);
    assert_eq!(records[0].args, json!({"ordered": [8, 2]}));
    assert!(records[0].success);
    assert_eq!(args["max_output_tokens"], 333);
    Ok(())
}

struct PausingTool {
    entered: Arc<tokio::sync::Notify>,
    drops: Arc<std::sync::atomic::AtomicUsize>,
}

struct ExecutionDrop(Arc<std::sync::atomic::AtomicUsize>);

impl Drop for ExecutionDrop {
    fn drop(&mut self) {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
}

#[async_trait]
impl Tool for PausingTool {
    async fn execute(&self, args: Value) -> Result<Value> {
        if args["pause"] == true {
            let _drop = ExecutionDrop(Arc::clone(&self.drops));
            self.entered.notify_one();
            std::future::pending::<()>().await;
        }
        Ok(json!({"resumed": true}))
    }

    fn name(&self) -> &str {
        TOOL_NAME
    }

    fn description(&self) -> &str {
        "Pauses until the caller cancels execution"
    }
}

#[tokio::test]
async fn cancellation_drops_handler_and_pty_permit_before_cached_retry() -> Result<()> {
    let workspace = TempDir::new()?;
    let mut registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
    registry.optimization_config.tool_registry.use_optimized_registry = true;
    let entered = Arc::new(tokio::sync::Notify::new());
    let drops = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    registry
        .register_tool(
            ToolRegistration::from_tool_instance(
                TOOL_NAME,
                CapabilityLevel::CodeSearch,
                PausingTool {
                    entered: Arc::clone(&entered),
                    drops: Arc::clone(&drops),
                },
            )
            .with_pty(true)
            .with_permission(ToolPolicy::Allow),
        )
        .await?;
    registry.set_tool_policy(TOOL_NAME, ToolPolicy::Allow).await?;
    let registry = Arc::new(registry);
    let task_registry = Arc::clone(&registry);
    let task = tokio::spawn(async move { task_registry.execute_tool_ref(TOOL_NAME, &json!({"pause": true})).await });
    let reached = tokio::time::timeout(std::time::Duration::from_secs(5), entered.notified()).await;
    if reached.is_err() {
        task.abort();
        let _ = task.await;
        anyhow::bail!("handler did not reach its cancellation boundary");
    }
    assert_eq!(registry.active_pty_sessions(), 1);
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert_eq!(drops.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(registry.active_pty_sessions(), 0);
    assert!(registry.execution_history.get_recent_records(10).is_empty());
    assert!(registry.hot_tool_cache.read().peek(TOOL_NAME).is_some());
    let result = registry.execute_tool_ref(TOOL_NAME, &json!({"pause": false})).await?;
    assert_eq!(result, json!({"success": true, "resumed": true}));
    assert_eq!(registry.active_pty_sessions(), 0);
    let records = registry.execution_history.get_recent_records(10);
    assert_eq!(records.len(), 1);
    assert!(records[0].success);
    Ok(())
}
