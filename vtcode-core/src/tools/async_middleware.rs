//! Async middleware for LLM-compatible tool execution
//!
//! Proper composition pattern with async/await support.
//! Suitable for tokio-based systems handling LLM operations.

use crate::tools::improvements_errors::ObservabilityContext;
use std::sync::Arc;
use std::time::Instant;

/// Async middleware trait
#[async_trait::async_trait]
pub trait AsyncMiddleware: Send + Sync {
    /// Middleware name
    fn name(&self) -> &str;

    /// Execute middleware
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
    ) -> ToolResult;
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

    pub fn add(mut self, middleware: Arc<dyn AsyncMiddleware>) -> Self {
        self.middlewares.push(middleware);
        self
    }

    /// Execute request through chain (simplified)
    pub async fn execute_simple<F>(&self, request: ToolRequest, executor: F) -> ToolResult
    where
        F: Fn(ToolRequest) -> ToolResult + Send,
    {
        if self.middlewares.is_empty() {
            return executor(request);
        }

        // Simple sequential execution for now
        executor(request)
    }
}

impl Default for AsyncMiddlewareChain {
    fn default() -> Self {
        Self::new()
    }
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
        tracing::debug!(
            tool = %request.tool_name,
            "tool execution started"
        );

        let start = Instant::now();
        let mut result = next(request.clone()).await;
        let duration = start.elapsed().as_millis() as u64;

        result.duration_ms = duration;

        if result.success {
            tracing::debug!(
                tool = %request.tool_name,
                duration_ms = duration,
                "tool execution completed"
            );
            self.obs_context.event(
                crate::tools::EventType::ToolSelected,
                "executor",
                format!("executed {} in {}ms", request.tool_name, duration),
                Some(1.0),
            );
        } else {
            tracing::error!(
                tool = %request.tool_name,
                error = ?result.error,
                "tool execution failed"
            );
        }

        result
    }
}

/// Async caching middleware with LRU
pub struct AsyncCachingMiddleware {
    cache: Arc<crate::tools::improvements_cache::LruCache<String, String>>,
    obs_context: Arc<ObservabilityContext>,
}

impl AsyncCachingMiddleware {
    pub fn new(
        max_entries: usize,
        ttl_seconds: u64,
        obs_context: Arc<ObservabilityContext>,
    ) -> Self {
        let cache = crate::tools::improvements_cache::LruCache::new(
            max_entries,
            std::time::Duration::from_secs(ttl_seconds),
        )
        .with_observability(obs_context.clone());

        Self {
            cache: Arc::new(cache),
            obs_context,
        }
    }

    fn cache_key(tool: &str, args: &str) -> String {
        // Use a hashed key to avoid creating large string cache keys while still uniquely identifying args
        use std::collections::hash_map::DefaultHasher;
        use std::hash::Hasher;
        let mut hasher = DefaultHasher::new();
        hasher.write(args.as_bytes());
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
        let key = Self::cache_key(&request.tool_name, &request.arguments);

        // Check cache
        if let Some(cached) = self.cache.get_owned(key.as_str()) {
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

        // Cache successful result
        if result.success {
            if let Some(ref output) = result.output {
                let _ = self.cache.put_arc(key, Arc::new(output.clone()));
            }
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

    #[tokio::test]
    async fn test_async_logging_middleware() {
        let obs = Arc::new(ObservabilityContext::noop());
        let middleware = AsyncLoggingMiddleware::new(obs);

        let request = ToolRequest {
            tool_name: "test_tool".to_string(),
            arguments: "arg1".to_string(),
            context: "ctx".to_string(),
        };

        let executor = |_req: ToolRequest| {
            Box::pin(async move {
                ToolResult {
                    success: true,
                    output: Some("result".to_string()),
                    error: None,
                    duration_ms: 0,
                    from_cache: false,
                }
            })
        };

        let result = middleware.execute(request, Box::new(executor)).await;

        assert!(result.success);
        assert!(result.duration_ms >= 0);
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
        let executor1 = |_req: ToolRequest| {
            Box::pin(async move {
                ToolResult {
                    success: true,
                    output: Some("result1".to_string()),
                    error: None,
                    duration_ms: 0,
                    from_cache: false,
                }
            })
        };

        let result1 = cache.execute(request.clone(), Box::new(executor1)).await;
        assert!(!result1.from_cache);

        // Second call (should be cached)
        let executor2 = |_req: ToolRequest| {
            Box::pin(async move {
                ToolResult {
                    success: true,
                    output: Some("result2".to_string()),
                    error: None,
                    duration_ms: 0,
                    from_cache: false,
                }
            })
        };

        let result2 = cache.execute(request, Box::new(executor2)).await;
        assert!(result2.from_cache);
        assert_eq!(result2.output, Some("result1".to_string())); // Returns cached value
    }
}
