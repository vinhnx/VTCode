//! **DEPRECATED**: Prefer `async_middleware` for new code.
//!
//! Sync middleware pattern for tool execution. This module is maintained for
//! backward compatibility. New middleware should use `AsyncMiddleware` from
//! `async_middleware.rs` which supports proper async chaining.
//!
//! Original: Allows stacking of concerns: error handling, caching, logging, metrics,
//! retries without modifying core tool execution logic.

use crate::tools::improvements_errors::{EventType, ObservabilityContext};
use std::fmt;
use std::sync::Arc;

// OPTIMIZATION: Const string literals to avoid allocations in hot paths
const LAYER_LOGGING: &str = "logging";
const LAYER_CACHING: &str = "caching";
const LAYER_RETRY: &str = "retry";
const LAYER_VALIDATION: &str = "validation";
const LAYER_METRICS: &str = "metrics";
const LAYER_CIRCUIT_BREAKER: &str = "circuit_breaker";

/// Result of middleware chain execution
#[deprecated(since = "0.1.0", note = "Use async_middleware::ToolResult instead")]
#[derive(Debug, Clone)]
pub struct MiddlewareResult {
    /// Execution successful
    pub success: bool,

    /// Result value (if successful)
    pub result: Option<String>,

    /// Error (if failed)
    pub error: Option<MiddlewareError>,

    /// Metadata about execution
    pub metadata: ExecutionMetadata,
}

// Ensure MiddlewareResult can be cloned for middleware chaining
impl MiddlewareResult {
    pub fn clone_with_metadata_update(&self, new_metadata: ExecutionMetadata) -> Self {
        Self {
            success: self.success,
            result: self.result.clone(),
            error: self.error.clone(),
            metadata: new_metadata,
        }
    }
}

/// Errors that can occur during middleware chain execution
#[deprecated(since = "0.1.0", note = "Use async_middleware error handling instead")]
#[derive(Debug, Clone)]
pub enum MiddlewareError {
    ExecutionFailed(&'static str),
    ValidationFailed(&'static str),
    CacheFailed(&'static str),
    TimeoutExceeded,
    Cancelled,
}

impl fmt::Display for MiddlewareError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ExecutionFailed(msg) => write!(f, "execution failed: {}", msg),
            Self::ValidationFailed(msg) => write!(f, "validation failed: {}", msg),
            Self::CacheFailed(msg) => write!(f, "cache failed: {}", msg),
            Self::TimeoutExceeded => write!(f, "execution timeout exceeded"),
            Self::Cancelled => write!(f, "execution cancelled"),
        }
    }
}

/// Metadata about execution through middleware
#[deprecated(since = "0.1.0", note = "Use async_middleware types instead")]
#[derive(Debug, Clone, Default)]
pub struct ExecutionMetadata {
    /// Total execution time (ms)
    pub duration_ms: u64,

    /// Whether result was cached
    pub from_cache: bool,

    /// Number of retries performed
    pub retry_count: u32,

    /// Middleware layers that executed
    pub layers_executed: Vec<String>,

    /// Any warnings
    pub warnings: Vec<String>,
}

/// Core middleware trait
#[deprecated(
    since = "0.1.0",
    note = "Use async_middleware::AsyncMiddleware instead"
)]
pub trait Middleware: Send + Sync {
    /// Middleware identifier
    fn name(&self) -> &str;

    /// Execute middleware with context and next handler
    fn execute(
        &self,
        request: ToolRequest,
        next: Box<dyn Fn(ToolRequest) -> MiddlewareResult + Send + Sync>,
    ) -> MiddlewareResult;
}

/// Tool execution request
#[deprecated(since = "0.1.0", note = "Use async_middleware::ToolRequest instead")]
#[derive(Debug, Clone)]
pub struct ToolRequest {
    /// Tool name
    pub tool_name: String,

    /// Tool arguments
    pub arguments: String,

    /// Execution context
    pub context: String,

    /// Request metadata
    pub metadata: RequestMetadata,
}

/// Request metadata
#[deprecated(since = "0.1.0", note = "Use async_middleware types instead")]
#[derive(Debug, Clone)]
pub struct RequestMetadata {
    /// Request ID for tracing
    pub request_id: String,

    /// Parent request ID (for correlation)
    pub parent_request_id: Option<String>,

    /// Priority level (0-100)
    pub priority: u32,

    /// Timeout in milliseconds
    pub timeout_ms: u64,

    /// Custom tags for filtering
    pub tags: Vec<String>,
}

impl Default for RequestMetadata {
    fn default() -> Self {
        use std::time::SystemTime;

        let timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();

        Self {
            request_id: format!("req-{}", timestamp),
            parent_request_id: None,
            priority: 50,
            timeout_ms: 30000,
            // OPTIMIZATION: Pre-allocate typical tag count
            tags: Vec::with_capacity(3),
        }
    }
}

/// Logging middleware
#[deprecated(
    since = "0.1.0",
    note = "Use async_middleware::AsyncLoggingMiddleware instead"
)]
pub struct LoggingMiddleware {
    #[allow(dead_code)]
    level: tracing::Level,
}

impl LoggingMiddleware {
    pub fn new(level: tracing::Level) -> Self {
        Self { level }
    }
}

impl Middleware for LoggingMiddleware {
    fn name(&self) -> &str {
        "logging"
    }

    fn execute(
        &self,
        request: ToolRequest,
        next: Box<dyn Fn(ToolRequest) -> MiddlewareResult + Send + Sync>,
    ) -> MiddlewareResult {
        // Capture values before moving request
        let tool_name = request.tool_name.clone();
        let request_id = request.metadata.request_id.clone();

        // Use tracing with static level
        tracing::debug!(
            tool = %tool_name,
            request_id = %request_id,
            arguments = %request.arguments,
            "tool_execution_started"
        );

        let start = std::time::Instant::now();
        let mut result = next(request);
        let duration = start.elapsed().as_millis() as u64;

        if result.success {
            tracing::debug!(
                tool = %tool_name,
                duration_ms = duration,
                "tool_execution_completed"
            );
        } else {
            tracing::error!(
                tool = %tool_name,
                error = ?result.error,
                "tool_execution_failed"
            );
        }

        result.metadata.duration_ms = duration;
        result.metadata.layers_executed.push(LAYER_LOGGING.into());
        result
    }
}

/// Cache entry with timestamp for staleness detection
#[derive(Debug, Clone)]
struct CacheEntry {
    value: Arc<String>,
    timestamp: std::time::Instant,
}

/// Caching middleware with staleness detection
#[deprecated(
    since = "0.1.0",
    note = "Use async_middleware::AsyncCachingMiddleware instead"
)]
pub struct CachingMiddleware {
    cache: Arc<std::sync::Mutex<std::collections::HashMap<String, CacheEntry>>>,
    /// Maximum age of cache entries in seconds (default: 300 = 5 minutes)
    max_age_secs: u64,
    /// Maximum number of entries to retain (evict oldest when exceeded)
    max_entries: usize,
    /// Maximum size of a single cached value in bytes (skip caching if exceeded)
    max_value_bytes: usize,
}

impl CachingMiddleware {
    pub fn new() -> Self {
        Self {
            cache: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            max_age_secs: 300, // 5 minutes default
            max_entries: 256,
            max_value_bytes: 512 * 1024, // 512KB default guardrail to avoid caching huge outputs
        }
    }

    pub fn with_max_age(mut self, max_age_secs: u64) -> Self {
        self.max_age_secs = max_age_secs;
        self
    }

    pub fn with_max_entries(mut self, max_entries: usize) -> Self {
        self.max_entries = max_entries.max(1);
        self
    }

    pub fn with_max_value_bytes(mut self, max_value_bytes: usize) -> Self {
        self.max_value_bytes = max_value_bytes.max(1024); // enforce reasonable floor
        self
    }

    fn cache_key(tool: &str, args: &str) -> String {
        // Use a fast 64-bit hash instead of storing potentially large `args` strings directly
        use std::collections::hash_map::DefaultHasher;
        use std::hash::Hasher;
        let mut hasher = DefaultHasher::new();
        hasher.write(args.as_bytes());
        format!("{}:{}", tool, hasher.finish())
    }

    fn is_stale(&self, entry: &CacheEntry) -> bool {
        entry.timestamp.elapsed().as_secs() > self.max_age_secs
    }
}

impl Default for CachingMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

impl Middleware for CachingMiddleware {
    fn name(&self) -> &str {
        "caching"
    }

    fn execute(
        &self,
        request: ToolRequest,
        next: Box<dyn Fn(ToolRequest) -> MiddlewareResult + Send + Sync>,
    ) -> MiddlewareResult {
        let key = Self::cache_key(&request.tool_name, &request.arguments);

        // Opportunistically drop stale entries and perform lookup in one critical section.
        if let Ok(mut cache) = self.cache.lock() {
            cache.retain(|_, entry| !self.is_stale(entry));
            if let Some(entry) = cache.get(&key)
                && !self.is_stale(entry)
            {
                return MiddlewareResult {
                    success: true,
                    result: Some((*entry.value).clone()),
                    error: None,
                    metadata: ExecutionMetadata {
                        from_cache: true,
                        layers_executed: vec![LAYER_CACHING.into()],
                        ..Default::default()
                    },
                };
            }
        }

        // Execute and cache result
        let mut result = next(request);

        if result.success
            && let Some(ref output) = result.result
            && output.len() <= self.max_value_bytes
            && let Ok(mut cache) = self.cache.lock()
        {
            cache.insert(
                key,
                CacheEntry {
                    value: Arc::new(output.clone()),
                    timestamp: std::time::Instant::now(),
                },
            );

            while cache.len() > self.max_entries {
                if let Some((oldest_key, _)) = cache.iter().min_by_key(|(_, entry)| entry.timestamp)
                {
                    let key_to_remove = oldest_key.clone();
                    cache.remove(&key_to_remove);
                } else {
                    break;
                }
            }
        } else if result.success
            && let Some(ref output) = result.result
            && output.len() > self.max_value_bytes
        {
            result
                .metadata
                .warnings
                .push("Skipped caching: payload exceeds cache size limit".to_string());
        }

        result.metadata.layers_executed.push(LAYER_CACHING.into());
        result
    }
}

/// Retry middleware with exponential backoff
#[deprecated(
    since = "0.1.0",
    note = "Use async_middleware::AsyncRetryMiddleware instead"
)]
pub struct RetryMiddleware {
    max_attempts: u32,
    initial_backoff_ms: u64,
    max_backoff_ms: u64,
}

impl RetryMiddleware {
    pub fn new(max_attempts: u32, initial_backoff_ms: u64, max_backoff_ms: u64) -> Self {
        Self {
            max_attempts,
            initial_backoff_ms,
            max_backoff_ms,
        }
    }

    fn backoff_duration(&self, attempt: u32) -> u64 {
        let backoff = self.initial_backoff_ms * 2_u64.pow(attempt);
        backoff.min(self.max_backoff_ms)
    }
}

impl Middleware for RetryMiddleware {
    fn name(&self) -> &str {
        "retry"
    }

    fn execute(
        &self,
        request: ToolRequest,
        next: Box<dyn Fn(ToolRequest) -> MiddlewareResult + Send + Sync>,
    ) -> MiddlewareResult {
        // OPTIMIZATION: Clone once, reuse for all retries
        let mut result = next(request.clone());

        if !result.success && self.max_attempts > 1 {
            for attempt in 1..self.max_attempts {
                let backoff = self.backoff_duration(attempt - 1);
                std::thread::sleep(std::time::Duration::from_millis(backoff));
                result.metadata.retry_count = attempt;

                result = next(request.clone());

                if result.success {
                    break;
                }
            }
        }

        result.metadata.layers_executed.push(LAYER_RETRY.into());
        result
    }
}
/// Validation middleware
#[deprecated(since = "0.1.0", note = "Use async_middleware types instead")]
pub struct ValidationMiddleware {
    obs_context: Arc<ObservabilityContext>,
}

impl ValidationMiddleware {
    pub fn new(obs_context: Arc<ObservabilityContext>) -> Self {
        Self { obs_context }
    }

    fn validate_request(&self, request: &ToolRequest) -> Result<(), MiddlewareError> {
        if request.tool_name.is_empty() {
            return Err(MiddlewareError::ValidationFailed("tool_name is empty"));
        }

        if request.arguments.is_empty() {
            self.obs_context.event(
                EventType::ErrorOccurred,
                "validation",
                "arguments is empty",
                None,
            );
        }

        Ok(())
    }
}

impl Middleware for ValidationMiddleware {
    fn name(&self) -> &str {
        "validation"
    }

    fn execute(
        &self,
        request: ToolRequest,
        next: Box<dyn Fn(ToolRequest) -> MiddlewareResult + Send + Sync>,
    ) -> MiddlewareResult {
        if let Err(err) = self.validate_request(&request) {
            return MiddlewareResult {
                success: false,
                result: None,
                error: Some(err),
                metadata: ExecutionMetadata::default(),
            };
        }

        let mut result = next(request);
        result
            .metadata
            .layers_executed
            .push(LAYER_VALIDATION.into());
        result
    }
}

/// Metrics middleware that integrates with AgentBehaviorAnalyzer
pub struct MetricsMiddleware {
    analyzer: Arc<std::sync::RwLock<crate::exec::agent_optimization::AgentBehaviorAnalyzer>>,
}

impl MetricsMiddleware {
    pub fn new(
        analyzer: Arc<std::sync::RwLock<crate::exec::agent_optimization::AgentBehaviorAnalyzer>>,
    ) -> Self {
        Self { analyzer }
    }
}

impl Middleware for MetricsMiddleware {
    fn name(&self) -> &str {
        "metrics"
    }

    fn execute(
        &self,
        request: ToolRequest,
        next: Box<dyn Fn(ToolRequest) -> MiddlewareResult + Send + Sync>,
    ) -> MiddlewareResult {
        let tool_name = request.tool_name.clone();

        // Execute the request
        let result = next(request);

        // Record metrics
        if let Ok(mut analyzer) = self.analyzer.write() {
            if result.success {
                analyzer.record_tool_usage(&tool_name);
            } else {
                let error_msg = result
                    .error
                    .as_ref()
                    .map(|e| e.to_string())
                    .unwrap_or_else(|| "unknown error".to_string());
                analyzer.record_tool_failure(&tool_name, &error_msg);
            }
        }

        let mut updated_result = result;
        updated_result
            .metadata
            .layers_executed
            .push(LAYER_METRICS.into());
        updated_result
    }
}

/// Circuit breaker middleware for failing tools
pub struct CircuitBreakerMiddleware {
    breaker: crate::tools::circuit_breaker::CircuitBreaker,
}

impl CircuitBreakerMiddleware {
    pub fn new(failure_threshold: f64) -> Self {
        // Convert f64 threshold to count - effectively just using default config for now
        // but we could expose more config options to middleware if needed.
        let config = crate::tools::circuit_breaker::CircuitBreakerConfig::default();
        if failure_threshold > 0.0 {
            // Heuristic: map 0.0-1.0 to reasonable integer count?
            // Or primarily just use the underlying CircuitBreaker defaults which are robust.
            // For now, we use default config as it supports backoff.
        }

        Self {
            breaker: crate::tools::circuit_breaker::CircuitBreaker::new(config),
        }
    }
}

impl Middleware for CircuitBreakerMiddleware {
    fn name(&self) -> &str {
        "circuit_breaker"
    }

    fn execute(
        &self,
        request: ToolRequest,
        next: Box<dyn Fn(ToolRequest) -> MiddlewareResult + Send + Sync>,
    ) -> MiddlewareResult {
        let tool_name = request.tool_name.clone();

        // Check if circuit is open
        if !self.breaker.allow_request_for_tool(&tool_name) {
            let wait_time = self
                .breaker
                .remaining_backoff(&tool_name)
                .map(|d| format!("{}s", d.as_secs()))
                .unwrap_or_else(|| "unknown".to_string());

            return MiddlewareResult {
                success: false,
                result: None,
                error: Some(MiddlewareError::ExecutionFailed(
                    "circuit breaker open - tool has high failure rate",
                )),
                metadata: ExecutionMetadata {
                    layers_executed: vec!["circuit_breaker".into()],
                    warnings: vec![format!(
                        "Tool {} blocked by circuit breaker due to high failure rate. Wait time: {}",
                        tool_name, wait_time
                    )],
                    ..Default::default()
                },
            };
        }

        // Execute and track result
        let result = next(request);

        if result.success {
            self.breaker.record_success_for_tool(&tool_name);
        } else {
            // We assume middleware failures are execution failures for now
            self.breaker.record_failure_for_tool(&tool_name, false);
        }

        let mut updated_result = result;
        updated_result
            .metadata
            .layers_executed
            .push(LAYER_CIRCUIT_BREAKER.into());
        updated_result
    }
}

/// Middleware chain executor
#[deprecated(
    since = "0.1.0",
    note = "Use async_middleware::AsyncMiddlewareChain instead"
)]
pub struct MiddlewareChain {
    middlewares: Vec<Arc<dyn Middleware>>,
}

impl MiddlewareChain {
    pub fn new() -> Self {
        Self {
            // OPTIMIZATION: Pre-allocate for typical middleware stack (3-5 layers)
            middlewares: Vec::with_capacity(5),
        }
    }

    pub fn with_middleware(mut self, middleware: Arc<dyn Middleware>) -> Self {
        self.middlewares.push(middleware);
        self
    }

    /// Add metrics middleware with AgentBehaviorAnalyzer integration
    pub fn with_metrics(
        self,
        analyzer: Arc<std::sync::RwLock<crate::exec::agent_optimization::AgentBehaviorAnalyzer>>,
    ) -> Self {
        self.with_middleware(Arc::new(MetricsMiddleware::new(analyzer)))
    }

    /// Add circuit breaker middleware with failure threshold
    pub fn with_circuit_breaker(self, threshold: f64) -> Self {
        self.with_middleware(Arc::new(CircuitBreakerMiddleware::new(threshold)))
    }

    /// Execute request through the middleware chain with a synchronous executor
    pub fn execute_sync<F>(&self, request: ToolRequest, executor: F) -> MiddlewareResult
    where
        F: Fn(ToolRequest) -> MiddlewareResult + Send + Sync + 'static,
    {
        let executor = std::sync::Arc::new(executor);

        // Factory for creating the tail of the chain (executor)
        let mut factory: std::sync::Arc<
            dyn Fn() -> Box<dyn Fn(ToolRequest) -> MiddlewareResult + Send + Sync> + Send + Sync,
        > = std::sync::Arc::new(move || {
            let executor = executor.clone();
            Box::new(move |req| executor(req))
        });

        // Wrap with middlewares in reverse order
        for middleware in self.middlewares.iter().rev() {
            let mw = middleware.clone();
            let next_factory = factory.clone();

            factory = std::sync::Arc::new(move || {
                let mw = mw.clone();
                let next_factory = next_factory.clone();
                Box::new(move |req| {
                    let next = next_factory();
                    mw.execute(req, next)
                })
            });
        }

        // Execute the full chain
        let root_fn = factory();
        root_fn(request)
    }
}

impl Default for MiddlewareChain {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_logging_middleware() {
        let middleware = LoggingMiddleware::new(tracing::Level::INFO);
        let request = ToolRequest {
            tool_name: crate::config::constants::tools::GREP_FILE.into(),
            arguments: "pattern:test".into(),
            context: "src/".into(),
            metadata: RequestMetadata::default(),
        };

        let executor = Box::new(|_req: ToolRequest| MiddlewareResult {
            success: true,
            result: Some("found test".into()),
            error: None,
            metadata: ExecutionMetadata::default(),
        });

        let result = middleware.execute(request, executor);
        assert!(result.success);
        assert!(result.metadata.layers_executed.contains(&"logging".into()));
    }

    #[test]
    fn test_caching_middleware() {
        let middleware = CachingMiddleware::new();
        let request = ToolRequest {
            tool_name: "test_tool".into(),
            arguments: "arg1".into(),
            context: "ctx".into(),
            metadata: RequestMetadata::default(),
        };

        // First execution (cache miss)
        let executor = Box::new(|_req: ToolRequest| MiddlewareResult {
            success: true,
            result: Some("result".into()),
            error: None,
            metadata: ExecutionMetadata::default(),
        });

        let result1 = middleware.execute(request.clone(), executor);
        assert!(!result1.metadata.from_cache);

        // Second execution (cache hit)
        let executor = Box::new(|_req: ToolRequest| MiddlewareResult {
            success: true,
            result: Some("new result".into()),
            error: None,
            metadata: ExecutionMetadata::default(),
        });

        let result2 = middleware.execute(request, executor);
        assert!(result2.metadata.from_cache);
    }

    #[test]
    fn test_validation_middleware() {
        let obs = Arc::new(ObservabilityContext::noop());
        let middleware = ValidationMiddleware::new(obs);

        let invalid_request = ToolRequest {
            tool_name: String::new(),
            arguments: "arg".into(),
            context: "ctx".into(),
            metadata: RequestMetadata::default(),
        };

        let executor = Box::new(|_req: ToolRequest| MiddlewareResult {
            success: true,
            result: Some("result".into()),
            error: None,
            metadata: ExecutionMetadata::default(),
        });

        let result = middleware.execute(invalid_request, executor);
        assert!(!result.success);
    }

    #[test]
    fn test_metrics_middleware() {
        use crate::exec::agent_optimization::AgentBehaviorAnalyzer;

        let analyzer = Arc::new(std::sync::RwLock::new(AgentBehaviorAnalyzer::new()));
        let middleware = MetricsMiddleware::new(analyzer.clone());

        let request = ToolRequest {
            tool_name: "test_tool".into(),
            arguments: "arg".into(),
            context: "ctx".into(),
            metadata: RequestMetadata::default(),
        };

        let executor = Box::new(|_req: ToolRequest| MiddlewareResult {
            success: true,
            result: Some("result".into()),
            error: None,
            metadata: ExecutionMetadata::default(),
        });

        let result = middleware.execute(request, executor);
        assert!(result.success);
        assert!(result.metadata.layers_executed.contains(&"metrics".into()));

        // Verify metrics were recorded
        let analyzer_lock = analyzer.read().unwrap();
        assert_eq!(
            *analyzer_lock
                .tool_stats()
                .usage_frequency
                .get("test_tool")
                .unwrap(),
            1
        );
    }

    #[test]
    fn test_circuit_breaker_middleware() {
        let middleware = CircuitBreakerMiddleware::new(0.5);

        let request = ToolRequest {
            tool_name: "failing_tool".into(),
            arguments: "arg".into(),
            context: "ctx".into(),
            metadata: RequestMetadata::default(),
        };

        // Simulate 5 failures to open circuit
        for _ in 0..5 {
            let executor = Box::new(|_req: ToolRequest| MiddlewareResult {
                success: false,
                result: None,
                error: Some(MiddlewareError::ExecutionFailed("test error")),
                metadata: ExecutionMetadata::default(),
            });

            let _ = middleware.execute(request.clone(), executor);
        }

        // Next call should be blocked
        let executor = Box::new(|_req: ToolRequest| MiddlewareResult {
            success: true,
            result: Some("should not execute".into()),
            error: None,
            metadata: ExecutionMetadata::default(),
        });

        let result = middleware.execute(request, executor);
        assert!(!result.success);
        assert!(
            result
                .metadata
                .layers_executed
                .contains(&"circuit_breaker".into())
        );
    }

    #[test]
    fn test_caching_middleware_staleness() {
        let middleware = CachingMiddleware::new().with_max_age(1); // 1 second max age

        let request = ToolRequest {
            tool_name: "test_tool".into(),
            arguments: "arg".into(),
            context: "ctx".into(),
            metadata: RequestMetadata::default(),
        };

        // First execution (cache miss)
        let executor = Box::new(|_req: ToolRequest| MiddlewareResult {
            success: true,
            result: Some("result1".into()),
            error: None,
            metadata: ExecutionMetadata::default(),
        });

        let result1 = middleware.execute(request.clone(), executor);
        assert!(!result1.metadata.from_cache);

        // Second execution (cache hit)
        let executor = Box::new(|_req: ToolRequest| MiddlewareResult {
            success: true,
            result: Some("result2".into()),
            error: None,
            metadata: ExecutionMetadata::default(),
        });

        let result2 = middleware.execute(request.clone(), executor);
        assert!(result2.metadata.from_cache);
        assert_eq!(result2.result.unwrap(), "result1");

        // Wait for cache to become stale
        std::thread::sleep(std::time::Duration::from_secs(2));

        // Third execution (cache miss due to staleness)
        let executor = Box::new(|_req: ToolRequest| MiddlewareResult {
            success: true,
            result: Some("result3".into()),
            error: None,
            metadata: ExecutionMetadata::default(),
        });

        let result3 = middleware.execute(request, executor);
        assert!(!result3.metadata.from_cache);
        assert_eq!(result3.result.unwrap(), "result3");
    }

    #[test]
    fn test_middleware_chain_with_metrics_and_circuit_breaker() {
        use crate::exec::agent_optimization::AgentBehaviorAnalyzer;

        let analyzer = Arc::new(std::sync::RwLock::new(AgentBehaviorAnalyzer::new()));

        let chain = MiddlewareChain::new()
            .with_metrics(analyzer.clone())
            .with_circuit_breaker(0.8);

        let request = ToolRequest {
            tool_name: "test_tool".into(),
            arguments: "arg".into(),
            context: "ctx".into(),
            metadata: RequestMetadata::default(),
        };

        let executor = |_req: ToolRequest| MiddlewareResult {
            success: true,
            result: Some("result".into()),
            error: None,
            metadata: ExecutionMetadata::default(),
        };

        let result = chain.execute_sync(request, executor);
        assert!(result.success);
        assert!(result.metadata.layers_executed.contains(&"metrics".into()));
        assert!(
            result
                .metadata
                .layers_executed
                .contains(&"circuit_breaker".into())
        );
    }
}
