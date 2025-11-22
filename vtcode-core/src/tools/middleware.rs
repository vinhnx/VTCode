//! Middleware pattern for tool execution with observability and extensibility
//!
//! Allows stacking of concerns: error handling, caching, logging, metrics,
//! retries without modifying core tool execution logic.

use crate::tools::improvements_errors::{ObservabilityContext, EventType};
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
    ExecutionFailed(String),
    ValidationFailed(String),
    CacheFailed(String),
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
#[derive(Debug, Clone)]
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

impl Default for ExecutionMetadata {
    fn default() -> Self {
        Self {
            duration_ms: 0,
            from_cache: false,
            retry_count: 0,
            layers_executed: Vec::new(),
            warnings: Vec::new(),
        }
    }
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
        // Use tracing with static level
        tracing::debug!(
            tool = %request.tool_name,
            request_id = %request.metadata.request_id,
            arguments = %request.arguments,
            "tool_execution_started"
        );

        let start = std::time::Instant::now();
        let mut result = next(request.clone());
        let duration = start.elapsed().as_millis() as u64;

        if result.success {
            tracing::debug!(
                tool = %request.tool_name,
                duration_ms = duration,
                "tool_execution_completed"
            );
        } else {
            tracing::error!(
                tool = %request.tool_name,
                error = ?result.error,
                "tool_execution_failed"
            );
        }

        result.metadata.duration_ms = duration;
        result.metadata.layers_executed.push(self.name().to_string());
        result
    }
}

/// Caching middleware
pub struct CachingMiddleware {
    // In production, this would be a proper cache implementation
    cache: Arc<std::sync::RwLock<std::collections::HashMap<String, String>>>,
}

impl CachingMiddleware {
    pub fn new() -> Self {
        Self {
            cache: Arc::new(std::sync::RwLock::new(std::collections::HashMap::new())),
        }
    }

    fn cache_key(tool: &str, args: &str) -> String {
        format!("{}:{}", tool, args)
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
                let mut result = MiddlewareResult {
                    success: true,
                    result: Some(cached.clone()),
                    error: None,
                    metadata: ExecutionMetadata::default(),
                };
                result.metadata.from_cache = true;
                result.metadata.layers_executed.push(self.name().to_string());
                return result;
            }
        }

        // Execute and cache result
        let mut result = next(request);
        
        if result.success {
            if let Some(ref output) = result.result {
                if let Ok(mut cache) = self.cache.write() {
                    cache.insert(key, output.clone());
                }
            }
        }

        result.metadata.layers_executed.push(self.name().to_string());
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
        let mut result = MiddlewareResult {
            success: false,
            result: None,
            error: Some(MiddlewareError::ExecutionFailed("not executed".to_string())),
            metadata: ExecutionMetadata::default(),
        };

        for attempt in 0..self.max_attempts {
            if attempt > 0 {
                let backoff = self.backoff_duration(attempt - 1);
                std::thread::sleep(std::time::Duration::from_millis(backoff));
                result.metadata.retry_count = attempt;
            }

            result = next(request.clone());
            
            if result.success {
                break;
            }
        }

        result.metadata.layers_executed.push(self.name().to_string());
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
                "tool_name is empty".to_string(),
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
        result.metadata.layers_executed.push(self.name().to_string());
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
        if self.middlewares.is_empty() {
            return executor(request);
        }

        // Apply middlewares in reverse order (first in list = outermost wrapper)
        let mut result = executor(request.clone());
        
        for middleware in self.middlewares.iter().rev() {
            // Re-execute through this middleware
            let current_mw = middleware.clone();
            let executor_fn = Box::new(move |_req: ToolRequest| result.clone());
            result = current_mw.execute(request.clone(), executor_fn);
        }
        
        result
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
            tool_name: "grep_file".to_string(),
            arguments: "pattern:test".to_string(),
            context: "src/".to_string(),
            metadata: RequestMetadata::default(),
        };

        let executor = Box::new(|_req: ToolRequest| MiddlewareResult {
            success: true,
            result: Some("found test".to_string()),
            error: None,
            metadata: ExecutionMetadata::default(),
        });

        let result = middleware.execute(request, executor);
        assert!(result.success);
        assert!(result.metadata.layers_executed.contains(&"logging".to_string()));
    }

    #[test]
    fn test_caching_middleware() {
        let middleware = CachingMiddleware::new();
        let request = ToolRequest {
            tool_name: "test_tool".to_string(),
            arguments: "arg1".to_string(),
            context: "ctx".to_string(),
            metadata: RequestMetadata::default(),
        };

        // First execution (cache miss)
        let executor = Box::new(|_req: ToolRequest| MiddlewareResult {
            success: true,
            result: Some("result".to_string()),
            error: None,
            metadata: ExecutionMetadata::default(),
        });

        let result1 = middleware.execute(request.clone(), executor);
        assert!(!result1.metadata.from_cache);

        // Second execution (cache hit)
        let executor = Box::new(|_req: ToolRequest| MiddlewareResult {
            success: true,
            result: Some("new result".to_string()),
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
            tool_name: "".to_string(),
            arguments: "arg".to_string(),
            context: "ctx".to_string(),
            metadata: RequestMetadata::default(),
        };

        let executor = Box::new(|_req: ToolRequest| MiddlewareResult {
            success: true,
            result: Some("result".to_string()),
            error: None,
            metadata: ExecutionMetadata::default(),
        });

        let result = middleware.execute(invalid_request, executor);
        assert!(!result.success);
    }
}
