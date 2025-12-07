//! Middleware pattern for tool execution with observability and extensibility
//!
//! Allows stacking of concerns: error handling, caching, logging, metrics,
//! retries without modifying core tool execution logic.

use crate::tools::improvements_errors::{EventType, ObservabilityContext};
use std::fmt;
use std::sync::Arc;

/// Result of middleware chain execution
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
            tags: Vec::new(),
        }
    }
}

/// Logging middleware
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
        result
            .metadata
            .layers_executed
            .push("logging".into());
        result
    }
}

/// Caching middleware
pub struct CachingMiddleware {
    // In production, this would be a proper cache implementation
    cache: Arc<std::sync::RwLock<std::collections::HashMap<String, Arc<String>>>>,
}

impl CachingMiddleware {
    pub fn new() -> Self {
        Self {
            cache: Arc::new(std::sync::RwLock::new(std::collections::HashMap::new())),
        }
    }

    fn cache_key(tool: &str, args: &str) -> String {
        // Use a fast 64-bit hash instead of storing potentially large `args` strings directly
        use std::collections::hash_map::DefaultHasher;
        use std::hash::Hasher;
        let mut hasher = DefaultHasher::new();
        hasher.write(args.as_bytes());
        format!("{}:{}", tool, hasher.finish())
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

        // Check cache
        if let Ok(cache) = self.cache.read() {
            if let Some(cached) = cache.get(&key) {
                return MiddlewareResult {
                    success: true,
                    result: Some((**cached).clone()),
                    error: None,
                    metadata: ExecutionMetadata {
                        from_cache: true,
                        layers_executed: vec!["caching".into()],
                        ..Default::default()
                    },
                };
            }
        }

        // Execute and cache result
        let mut result = next(request);

        if result.success {
            if let Some(ref output) = result.result {
                if let Ok(mut cache) = self.cache.write() {
                    cache.insert(key, Arc::new(output.clone()));
                }
            }
        }

        result
            .metadata
            .layers_executed
            .push("caching".into());
        result
    }
}

/// Retry middleware with exponential backoff
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
        // Try once, then retry with clones
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

        result.metadata.layers_executed.push("retry".into());
        result
    }
}

/// Validation middleware
pub struct ValidationMiddleware {
    obs_context: Arc<ObservabilityContext>,
}

impl ValidationMiddleware {
    pub fn new(obs_context: Arc<ObservabilityContext>) -> Self {
        Self { obs_context }
    }

    fn validate_request(&self, request: &ToolRequest) -> Result<(), MiddlewareError> {
        if request.tool_name.is_empty() {
            return Err(MiddlewareError::ValidationFailed(
                "tool_name is empty",
            ));
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
            .push("validation".into());
        result
    }
}

/// Middleware chain executor
pub struct MiddlewareChain {
    middlewares: Vec<Arc<dyn Middleware>>,
}

impl MiddlewareChain {
    pub fn new() -> Self {
        Self {
            middlewares: Vec::new(),
        }
    }

    pub fn add(mut self, middleware: Arc<dyn Middleware>) -> Self {
        self.middlewares.push(middleware);
        self
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
            tool_name: "grep_file".into(),
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
}
