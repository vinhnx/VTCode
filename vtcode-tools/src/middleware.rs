//! Composable async middleware for tool execution.
//!
//! Provides a chain-of-responsibility pattern for pre/post-processing
//! tool calls with observability and error recovery.

use async_trait::async_trait;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use vtcode_core::tools::{ToolCallRequest, ToolCallResponse};

/// Request context flowing through middleware.
pub type ToolRequest = ToolCallRequest;

/// Response from tool execution.
pub type ToolResponse = ToolCallResponse;

/// Middleware trait for intercepting tool execution.
#[async_trait]
pub trait Middleware: Send + Sync {
    /// Called before tool execution.
    async fn before_execute(&self, _req: &ToolRequest) -> anyhow::Result<()> {
        Ok(())
    }

    /// Called after successful execution.
    async fn after_execute(&self, _req: &ToolRequest, _res: &ToolResponse) -> anyhow::Result<()> {
        Ok(())
    }

    /// Called on execution error.
    async fn on_error(&self, _req: &ToolRequest, _err: &anyhow::Error) -> anyhow::Result<()> {
        Ok(())
    }
}

/// Noop middleware (default).
pub struct NoopMiddleware;

#[async_trait]
impl Middleware for NoopMiddleware {}

/// Middleware composition chain.
pub struct MiddlewareChain {
    middlewares: Vec<Arc<dyn Middleware>>,
}

impl MiddlewareChain {
    /// Create new chain.
    pub fn new() -> Self {
        Self {
            middlewares: Vec::new(),
        }
    }

    /// Add middleware to chain.
    pub fn add(mut self, mw: Arc<dyn Middleware>) -> Self {
        self.middlewares.push(mw);
        self
    }

    /// Execute before hooks.
    pub async fn before_execute(&self, req: &ToolRequest) -> anyhow::Result<()> {
        for mw in &self.middlewares {
            mw.before_execute(req).await?;
        }
        Ok(())
    }

    /// Execute after hooks.
    pub async fn after_execute(&self, req: &ToolRequest, res: &ToolResponse) -> anyhow::Result<()> {
        // Run in reverse order.
        for mw in self.middlewares.iter().rev() {
            mw.after_execute(req, res).await?;
        }
        Ok(())
    }

    /// Execute error hooks.
    pub async fn on_error(&self, req: &ToolRequest, err: &anyhow::Error) -> anyhow::Result<()> {
        for mw in self.middlewares.iter().rev() {
            let _ = mw.on_error(req, err).await;
        }
        Ok(())
    }
}

impl Default for MiddlewareChain {
    fn default() -> Self {
        Self::new()
    }
}

/// Logging middleware.
pub struct LoggingMiddleware {
    name: String,
}

impl LoggingMiddleware {
    pub fn new(name: impl Into<String>) -> Arc<Self> {
        Arc::new(Self { name: name.into() })
    }
}

#[async_trait]
impl Middleware for LoggingMiddleware {
    async fn before_execute(&self, req: &ToolRequest) -> anyhow::Result<()> {
        eprintln!("[{}] Executing: {}", self.name, req.tool_name);
        Ok(())
    }

    async fn after_execute(&self, req: &ToolRequest, res: &ToolResponse) -> anyhow::Result<()> {
        let duration_ms = res.duration_ms.unwrap_or(0);
        let cache_hit = res.cache_hit.unwrap_or(false);
        eprintln!(
            "[{}] Completed: {} ({}ms, cache_hit={})",
            self.name, req.tool_name, duration_ms, cache_hit
        );
        Ok(())
    }

    async fn on_error(&self, req: &ToolRequest, err: &anyhow::Error) -> anyhow::Result<()> {
        eprintln!("[{}] Error in {}: {}", self.name, req.tool_name, err);
        Ok(())
    }
}

/// Metrics middleware.
#[derive(Clone, Copy, Debug)]
pub struct MetricsSnapshot {
    pub total_calls: u64,
    pub successful_calls: u64,
    pub failed_calls: u64,
    pub total_duration_ms: u64,
    pub cache_hits: u64,
}

pub struct MetricsMiddleware {
    total_calls: Arc<AtomicU64>,
    successful_calls: Arc<AtomicU64>,
    failed_calls: Arc<AtomicU64>,
    total_duration_ms: Arc<AtomicU64>,
    cache_hits: Arc<AtomicU64>,
}

impl MetricsMiddleware {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            total_calls: Arc::new(AtomicU64::new(0)),
            successful_calls: Arc::new(AtomicU64::new(0)),
            failed_calls: Arc::new(AtomicU64::new(0)),
            total_duration_ms: Arc::new(AtomicU64::new(0)),
            cache_hits: Arc::new(AtomicU64::new(0)),
        })
    }

    pub async fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            total_calls: self.total_calls.load(Ordering::Relaxed),
            successful_calls: self.successful_calls.load(Ordering::Relaxed),
            failed_calls: self.failed_calls.load(Ordering::Relaxed),
            total_duration_ms: self.total_duration_ms.load(Ordering::Relaxed),
            cache_hits: self.cache_hits.load(Ordering::Relaxed),
        }
    }
}

#[async_trait]
impl Middleware for MetricsMiddleware {
    async fn before_execute(&self, _: &ToolRequest) -> anyhow::Result<()> {
        self.total_calls.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    async fn after_execute(&self, _: &ToolRequest, res: &ToolResponse) -> anyhow::Result<()> {
        self.successful_calls.fetch_add(1, Ordering::Relaxed);
        self.total_duration_ms
            .fetch_add(res.duration_ms.unwrap_or(0), Ordering::Relaxed);

        if res.cache_hit.unwrap_or(false) {
            self.cache_hits.fetch_add(1, Ordering::Relaxed);
        }

        Ok(())
    }

    async fn on_error(&self, _: &ToolRequest, _: &anyhow::Error) -> anyhow::Result<()> {
        self.failed_calls.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}

impl Default for MetricsMiddleware {
    fn default() -> Self {
        Self {
            total_calls: Arc::new(AtomicU64::new(0)),
            successful_calls: Arc::new(AtomicU64::new(0)),
            failed_calls: Arc::new(AtomicU64::new(0)),
            total_duration_ms: Arc::new(AtomicU64::new(0)),
            cache_hits: Arc::new(AtomicU64::new(0)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[tokio::test]
    async fn test_chain_execution() {
        let chain = MiddlewareChain::new()
            .add(LoggingMiddleware::new("test"))
            .add(MetricsMiddleware::new());

        let req = ToolRequest {
            id: "req-1".to_string(),
            tool_name: "test_tool".to_string(),
            args: Value::Null,
            metadata: Some(Default::default()),
        };

        chain.before_execute(&req).await.unwrap();

        let res = ToolResponse {
            id: "req-1".to_string(),
            success: true,
            result: Some(Value::Null),
            error: None,
            duration_ms: Some(100),
            cache_hit: Some(false),
        };

        chain.after_execute(&req, &res).await.unwrap();
    }

    #[tokio::test]
    async fn test_metrics_tracking() {
        let metrics = MetricsMiddleware::new();
        let chain = MiddlewareChain::new().add(metrics.clone());

        let req = ToolRequest {
            id: "req-2".to_string(),
            tool_name: "test".to_string(),
            args: Value::Null,
            metadata: Some(Default::default()),
        };

        for i in 0..5 {
            chain.before_execute(&req).await.unwrap();
            let res = ToolResponse {
                id: format!("req-2-{}", i),
                success: true,
                result: Some(Value::Null),
                error: None,
                duration_ms: Some(10),
                cache_hit: Some(true),
            };
            chain.after_execute(&req, &res).await.unwrap();
        }

        let snapshot = metrics.snapshot().await;
        assert_eq!(snapshot.total_calls, 5);
        assert_eq!(snapshot.successful_calls, 5);
        assert_eq!(snapshot.cache_hits, 5);
    }
}
