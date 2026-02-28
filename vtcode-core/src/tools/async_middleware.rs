//! Async middleware for LLM-compatible tool execution
//!
//! Proper composition pattern with async/await support.
//! Suitable for tokio-based systems handling LLM operations.

use crate::tools::improvements_errors::ObservabilityContext;
use serde_json::{Map, Value};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;

/// Type alias for the async continuation function
type AsyncContinuation<'a> =
    Box<dyn Fn(ToolRequest) -> Pin<Box<dyn Future<Output = ToolResult> + Send>> + Send + Sync + 'a>;

/// Type alias for the owned async continuation function
type AsyncContinuationOwned =
    Box<dyn Fn(ToolRequest) -> Pin<Box<dyn Future<Output = ToolResult> + Send>> + Send + Sync>;

/// Async middleware trait
#[async_trait::async_trait]
pub trait AsyncMiddleware: Send + Sync {
    /// Middleware name
    fn name(&self) -> &str;

    /// Execute middleware
    async fn execute<'a>(&'a self, request: ToolRequest, next: AsyncContinuation<'a>)
    -> ToolResult;
}

/// Tool request
#[derive(Clone, Debug)]
pub struct ToolRequest {
    pub tool_name: String,
    pub arguments: String,
    pub context: String,
}

/// Tool result
#[derive(Clone, Debug)]
pub struct ToolResult {
    pub success: bool,
    pub output: Option<String>,
    pub error: Option<String>,
    pub duration_ms: u64,
    pub from_cache: bool,
}

/// Async middleware chain executor
pub struct AsyncMiddlewareChain {
    middlewares: Vec<Arc<dyn AsyncMiddleware>>,
}

impl AsyncMiddlewareChain {
    pub fn new() -> Self {
        Self {
            middlewares: Vec::new(),
        }
    }

    pub fn with_middleware(mut self, middleware: Arc<dyn AsyncMiddleware>) -> Self {
        self.middlewares.push(middleware);
        self
    }

    /// Execute request through chain (simplified)
    pub async fn execute_simple<F>(&self, request: ToolRequest, executor: F) -> ToolResult
    where
        F: Fn(ToolRequest) -> ToolResult + Send + Sync + 'static,
    {
        if self.middlewares.is_empty() {
            return executor(request);
        }

        let executor = Arc::new(executor);
        let middlewares = self.middlewares.clone();

        fn build_chain(
            middlewares: &[Arc<dyn AsyncMiddleware>],
            executor: Arc<dyn Fn(ToolRequest) -> ToolResult + Send + Sync>,
        ) -> AsyncContinuationOwned {
            if middlewares.is_empty() {
                Box::new(move |req: ToolRequest| {
                    let result = executor(req);
                    Box::pin(async move { result })
                })
            } else {
                let current = middlewares[0].clone();
                let rest = build_chain(&middlewares[1..], executor);
                let rest = Arc::new(rest);
                Box::new(move |req: ToolRequest| {
                    let current = current.clone();
                    let rest = rest.clone();
                    Box::pin(async move {
                        let next: AsyncContinuationOwned = Box::new(move |r: ToolRequest| {
                            let rest = rest.clone();
                            Box::pin(async move { rest(r).await })
                        });
                        current.execute(req, next).await
                    })
                })
            }
        }

        let chain = build_chain(&middlewares, executor);
        chain(request).await
    }
}

impl Default for AsyncMiddlewareChain {
    fn default() -> Self {
        Self::new()
    }
}

fn normalize_context(context: &str) -> String {
    let mut normalized = Map::new();
    let parsed: Value = serde_json::from_str(context).unwrap_or_else(|_| Value::Object(Map::new()));

    if let Some(session) = parsed.get("session_id").and_then(Value::as_str)
        && !session.is_empty()
    {
        normalized.insert("session_id".into(), Value::String(session.to_string()));
    }

    if let Some(task) = parsed.get("task_id").and_then(Value::as_str)
        && !task.is_empty()
    {
        normalized.insert("task_id".into(), Value::String(task.to_string()));
    }

    if let Some(version) = parsed.get("plan_version").and_then(Value::as_u64) {
        normalized.insert("plan_version".into(), Value::Number(version.into()));
    }

    if let Some(plan) = parsed.get("plan_summary").and_then(Value::as_object) {
        let mut summary = Map::new();
        if let Some(status) = plan.get("status").and_then(Value::as_str) {
            summary.insert("status".into(), Value::String(status.to_string()));
        }
        if let Some(total) = plan.get("total_steps").and_then(Value::as_u64) {
            summary.insert("total_steps".into(), Value::Number(total.into()));
        }
        if let Some(completed) = plan.get("completed_steps").and_then(Value::as_u64) {
            summary.insert("completed_steps".into(), Value::Number(completed.into()));
        }
        if !summary.is_empty() {
            normalized.insert("plan_summary".into(), Value::Object(summary));
        }
    }

    if let Some(phase) = parsed
        .get("plan_phase")
        .and_then(|v| v.as_str())
        .filter(|p| !p.is_empty())
    {
        normalized.insert("plan_phase".into(), Value::String(phase.to_string()));
    }

    serde_json::to_string(&Value::Object(normalized)).unwrap_or_else(|_| "{}".to_string())
}

/// Async logging middleware
pub struct AsyncLoggingMiddleware {
    obs_context: Arc<ObservabilityContext>,
}

impl AsyncLoggingMiddleware {
    pub fn new(obs_context: Arc<ObservabilityContext>) -> Self {
        Self { obs_context }
    }
}

#[async_trait::async_trait]
impl AsyncMiddleware for AsyncLoggingMiddleware {
    fn name(&self) -> &str {
        "async_logging"
    }

    async fn execute<'a>(
        &'a self,
        request: ToolRequest,
        next: Box<
            dyn Fn(
                    ToolRequest,
                )
                    -> std::pin::Pin<Box<dyn std::future::Future<Output = ToolResult> + Send>>
                + Send
                + Sync
                + 'a,
        >,
    ) -> ToolResult {
        let tool_name = request.tool_name.clone();
        let normalized_context = normalize_context(&request.context);
        let context_json: Option<Value> = serde_json::from_str(&normalized_context).ok();
        let session_id = context_json
            .as_ref()
            .and_then(|v| v.get("session_id").and_then(|s| s.as_str()))
            .unwrap_or("");
        let task_id = context_json
            .as_ref()
            .and_then(|v| v.get("task_id").and_then(|s| s.as_str()))
            .unwrap_or("");
        let plan_summary = context_json.as_ref().and_then(|v| v.get("plan_summary"));
        let plan_status = plan_summary
            .and_then(|v| v.get("status").and_then(|s| s.as_str()))
            .unwrap_or("");
        let plan_phase = context_json
            .as_ref()
            .and_then(|v| v.get("plan_phase").and_then(|p| p.as_str()))
            .unwrap_or("");
        let plan_total_steps = plan_summary
            .and_then(|v| v.get("total_steps").and_then(|n| n.as_u64()))
            .unwrap_or(0);
        let plan_completed_steps = plan_summary
            .and_then(|v| v.get("completed_steps").and_then(|n| n.as_u64()))
            .unwrap_or(0);
        let plan_version = context_json
            .as_ref()
            .and_then(|v| v.get("plan_version").and_then(|n| n.as_u64()))
            .unwrap_or(0);

        tracing::debug!(
            tool = %tool_name,
            session_id = %session_id,
            task_id = %task_id,
            plan_version,
            plan_status = %plan_status,
            plan_phase = %plan_phase,
            plan_total_steps,
            plan_completed_steps,
            "tool execution started"
        );
        tracing::trace!(
            tool = %tool_name,
            context = %normalized_context,
            "tool execution context payload"
        );

        let start = Instant::now();
        let mut result = next(request).await;
        let duration = start.elapsed().as_millis() as u64;

        result.duration_ms = duration;

        if result.success {
            tracing::debug!(
                tool = %tool_name,
                duration_ms = duration,
                session_id = %session_id,
                task_id = %task_id,
                plan_version,
                plan_status = %plan_status,
                plan_phase = %plan_phase,
                plan_total_steps,
                plan_completed_steps,
                from_cache = result.from_cache,
                "tool execution completed"
            );
            self.obs_context.event(
                crate::tools::EventType::ToolSelected,
                "executor",
                format!("executed {} in {}ms", tool_name, duration),
                Some(1.0),
            );
        } else {
            tracing::error!(
                tool = %tool_name,
                error = ?result.error,
                session_id = %context_json
                    .as_ref()
                    .and_then(|v| v.get("session_id").and_then(|s| s.as_str()))
                    .unwrap_or(""),
                task_id = %context_json
                    .as_ref()
                    .and_then(|v| v.get("task_id").and_then(|s| s.as_str()))
                    .unwrap_or(""),
                "tool execution failed"
            );
        }

        result
    }
}

/// Async caching middleware with UnifiedCache (migrated from LruCache)
pub struct AsyncCachingMiddleware {
    cache: Arc<parking_lot::Mutex<crate::cache::UnifiedCache<AsyncCacheKey, String>>>,
    obs_context: Arc<ObservabilityContext>,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct AsyncCacheKey(String);

impl crate::cache::CacheKey for AsyncCacheKey {
    fn to_cache_key(&self) -> String {
        self.0.clone()
    }
}

impl AsyncCachingMiddleware {
    pub fn new(
        max_entries: usize,
        ttl_seconds: u64,
        obs_context: Arc<ObservabilityContext>,
    ) -> Self {
        let cache = crate::cache::UnifiedCache::new(
            max_entries,
            std::time::Duration::from_secs(ttl_seconds),
            crate::cache::EvictionPolicy::Lru,
        );

        Self {
            cache: Arc::new(parking_lot::Mutex::new(cache)),
            obs_context,
        }
    }

    fn cache_key(tool: &str, args: &str, context: &str) -> String {
        // Use a hashed key to avoid creating large string cache keys while still uniquely identifying args
        use std::collections::hash_map::DefaultHasher;
        use std::hash::Hasher;
        let mut hasher = DefaultHasher::new();
        hasher.write(args.as_bytes());
        let normalized = normalize_context(context);
        if !normalized.is_empty() {
            hasher.write(normalized.as_bytes());
        }
        format!("{}::{}", tool, hasher.finish())
    }
}

#[async_trait::async_trait]
impl AsyncMiddleware for AsyncCachingMiddleware {
    fn name(&self) -> &str {
        "async_caching"
    }

    async fn execute<'a>(
        &'a self,
        request: ToolRequest,
        next: Box<
            dyn Fn(
                    ToolRequest,
                )
                    -> std::pin::Pin<Box<dyn std::future::Future<Output = ToolResult> + Send>>
                + Send
                + Sync
                + 'a,
        >,
    ) -> ToolResult {
        let key = AsyncCacheKey(Self::cache_key(
            &request.tool_name,
            &request.arguments,
            &request.context,
        ));

        // Check cache (migrated to UnifiedCache)
        if let Some(cached) = self.cache.lock().get_owned(&key) {
            self.obs_context.event(
                crate::tools::EventType::CacheHit,
                "cache",
                "returning cached result",
                Some(1.0),
            );

            return ToolResult {
                success: true,
                output: Some(cached),
                error: None,
                duration_ms: 0,
                from_cache: true,
            };
        }

        // Execute
        let result = next(request).await;

        // Cache successful result (migrated to UnifiedCache)
        if result.success
            && let Some(ref output) = result.output
        {
            let size = output.len() as u64;
            self.cache.lock().insert(key, output.clone(), size);
        }

        result
    }
}

/// Async retry middleware with exponential backoff
pub struct AsyncRetryMiddleware {
    max_attempts: u32,
    initial_backoff_ms: u64,
    max_backoff_ms: u64,
    obs_context: Arc<ObservabilityContext>,
}

impl AsyncRetryMiddleware {
    pub fn new(
        max_attempts: u32,
        initial_backoff_ms: u64,
        max_backoff_ms: u64,
        obs_context: Arc<ObservabilityContext>,
    ) -> Self {
        Self {
            max_attempts,
            initial_backoff_ms,
            max_backoff_ms,
            obs_context,
        }
    }

    fn backoff_duration(&self, attempt: u32) -> std::time::Duration {
        let backoff = self.initial_backoff_ms * 2_u64.pow(attempt);
        std::time::Duration::from_millis(backoff.min(self.max_backoff_ms))
    }
}

#[async_trait::async_trait]
impl AsyncMiddleware for AsyncRetryMiddleware {
    fn name(&self) -> &str {
        "async_retry"
    }

    async fn execute<'a>(
        &'a self,
        request: ToolRequest,
        next: Box<
            dyn Fn(
                    ToolRequest,
                )
                    -> std::pin::Pin<Box<dyn std::future::Future<Output = ToolResult> + Send>>
                + Send
                + Sync
                + 'a,
        >,
    ) -> ToolResult {
        for attempt in 0..self.max_attempts {
            if attempt > 0 {
                let backoff = self.backoff_duration(attempt - 1);
                tracing::debug!(
                    attempt = attempt,
                    backoff_ms = backoff.as_millis(),
                    "retrying after backoff"
                );
                tokio::time::sleep(backoff).await;
            }

            let result = next(request.clone()).await;

            if result.success {
                if attempt > 0 {
                    self.obs_context.event(
                        crate::tools::EventType::FallbackSuccess,
                        "retry",
                        format!("succeeded on attempt {}", attempt + 1),
                        Some(1.0),
                    );
                }
                return result;
            }

            self.obs_context.event(
                crate::tools::EventType::FallbackAttempt,
                "retry",
                format!("attempt {} failed", attempt + 1),
                None,
            );
        }

        ToolResult {
            success: false,
            output: None,
            error: Some(format!("all {} attempts failed", self.max_attempts)),
            duration_ms: 0,
            from_cache: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::Future;
    use std::pin::Pin;

    fn make_executor(
        output: &'static str,
    ) -> Box<dyn Fn(ToolRequest) -> Pin<Box<dyn Future<Output = ToolResult> + Send>> + Send + Sync>
    {
        Box::new(move |_req: ToolRequest| {
            Box::pin(async move {
                ToolResult {
                    success: true,
                    output: Some(output.to_string()),
                    error: None,
                    duration_ms: 0,
                    from_cache: false,
                }
            })
        })
    }

    #[tokio::test]
    async fn test_async_logging_middleware() {
        let obs = Arc::new(ObservabilityContext::noop());
        let middleware = AsyncLoggingMiddleware::new(obs);

        let request = ToolRequest {
            tool_name: "test_tool".to_string(),
            arguments: "arg1".to_string(),
            context: "ctx".to_string(),
        };

        let executor = make_executor("result");

        let result = middleware.execute(request, executor).await;

        assert!(result.success);
    }

    #[tokio::test]
    async fn test_async_caching_middleware() {
        let obs = Arc::new(ObservabilityContext::noop());
        let cache = AsyncCachingMiddleware::new(10, 60, obs);

        let request = ToolRequest {
            tool_name: "cached_tool".to_string(),
            arguments: "arg1".to_string(),
            context: "ctx".to_string(),
        };

        // First call
        let executor1 = make_executor("result1");

        let result1 = cache.execute(request.clone(), executor1).await;
        assert!(!result1.from_cache);

        // Second call (should be cached)
        let executor2 = make_executor("result2");

        let result2 = cache.execute(request, executor2).await;
        assert!(result2.from_cache);
        assert_eq!(result2.output, Some("result1".to_string())); // Returns cached value
    }
}
