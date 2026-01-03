//! High-performance async tool execution pipeline with batching and streaming

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};
use anyhow::{Result, anyhow};
use serde_json::Value;
use tokio::sync::{mpsc, Semaphore, RwLock};
use tokio::time::timeout;
use tracing::{debug, error};

use crate::core::memory_pool::global_pool;

/// Tool execution request with priority and context
#[derive(Debug, Clone)]
pub struct ToolRequest {
    pub id: String,
    pub tool_name: String,
    pub args: Value,
    pub priority: ExecutionPriority,
    pub timeout: Duration,
    pub context: ExecutionContext,
}

/// Execution priority levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ExecutionPriority {
    Low = 0,
    Normal = 1,
    High = 2,
    Critical = 3,
}

/// Execution context for tracking and optimization
#[derive(Debug, Clone)]
pub struct ExecutionContext {
    pub session_id: String,
    pub user_id: Option<String>,
    pub workspace_path: String,
    pub parent_request_id: Option<String>,
}

/// Tool execution result with performance metrics
#[derive(Debug)]
pub struct ToolResult {
    pub request_id: String,
    pub result: Result<Value>,
    pub execution_time: Duration,
    pub memory_used: Option<usize>,
    pub cache_hit: bool,
}

/// Batch of tool requests for efficient processing
#[derive(Debug)]
pub struct ToolBatch {
    pub requests: Vec<ToolRequest>,
    pub batch_id: String,
    pub created_at: Instant,
}

/// High-performance async tool execution pipeline
pub struct AsyncToolPipeline {
    /// Request queue with priority ordering
    request_queue: Arc<RwLock<VecDeque<ToolRequest>>>,
    
    /// Batch processor for grouping similar requests
    batch_processor: Arc<BatchProcessor>,
    
    /// Execution semaphore for concurrency control
    execution_semaphore: Arc<Semaphore>,
    
    /// Result cache for avoiding duplicate work
    result_cache: Arc<RwLock<lru::LruCache<String, ToolResult>>>,
    
    /// Performance metrics collector
    metrics: Arc<RwLock<PipelineMetrics>>,
    
    /// Shutdown signal
    shutdown_tx: Option<mpsc::Sender<()>>,
}

/// Batch processor for grouping similar tool requests
pub struct BatchProcessor {
    /// Current batch being assembled
    current_batch: Arc<RwLock<Option<ToolBatch>>>,
    
    /// Batch size threshold
    batch_size: usize,
    
    /// Batch timeout threshold
    batch_timeout: Duration,
}

/// Pipeline performance metrics
#[derive(Debug, Default)]
pub struct PipelineMetrics {
    pub total_requests: u64,
    pub successful_executions: u64,
    pub failed_executions: u64,
    pub cache_hits: u64,
    pub avg_execution_time_ms: f64,
    pub batch_efficiency: f64,
}

impl AsyncToolPipeline {
    pub fn new(
        max_concurrent_tools: usize,
        cache_size: usize,
        batch_size: usize,
        batch_timeout: Duration,
    ) -> Self {
        Self {
            request_queue: Arc::new(RwLock::new(VecDeque::with_capacity(256))),
            batch_processor: Arc::new(BatchProcessor::new(batch_size, batch_timeout)),
            execution_semaphore: Arc::new(Semaphore::new(max_concurrent_tools)),
            result_cache: Arc::new(RwLock::new(lru::LruCache::new(
                std::num::NonZeroUsize::new(cache_size).unwrap()
            ))),
            metrics: Arc::new(RwLock::new(PipelineMetrics::default())),
            shutdown_tx: None,
        }
    }

    /// Start the pipeline processing loop
    pub async fn start(&mut self) -> Result<()> {
        let (shutdown_tx, mut shutdown_rx) = mpsc::channel(1);
        self.shutdown_tx = Some(shutdown_tx);

        let request_queue = Arc::clone(&self.request_queue);
        let batch_processor = Arc::clone(&self.batch_processor);
        let execution_semaphore = Arc::clone(&self.execution_semaphore);
        let result_cache = Arc::clone(&self.result_cache);
        let metrics = Arc::clone(&self.metrics);

        // Main processing loop
        tokio::spawn(async move {
            let mut batch_timer = tokio::time::interval(Duration::from_millis(50));
            
            loop {
                tokio::select! {
                    _ = batch_timer.tick() => {
                        Self::process_batch(
                            &request_queue,
                            &batch_processor,
                            &execution_semaphore,
                            &result_cache,
                            &metrics,
                        ).await;
                    }
                    _ = shutdown_rx.recv() => {
                        debug!("Pipeline shutdown requested");
                        break;
                    }
                }
            }
        });

        Ok(())
    }

    /// Submit a tool request for execution
    pub async fn submit_request(&self, request: ToolRequest) -> Result<String> {
        // Check cache first
        let cache_key = self.generate_cache_key(&request);
        if let Some(_cached_result) = self.result_cache.read().await.peek(&cache_key) {
            self.metrics.write().await.cache_hits += 1;
            return Ok(request.id.clone());
        }

        // Add to priority queue
        let mut queue = self.request_queue.write().await;
        
        // Insert based on priority (higher priority first)
        let insert_pos = queue
            .iter()
            .position(|r| r.priority < request.priority)
            .unwrap_or(queue.len());
        
        queue.insert(insert_pos, request.clone());
        
        self.metrics.write().await.total_requests += 1;
        
        Ok(request.id)
    }

    /// Process a batch of requests efficiently
    async fn process_batch(
        request_queue: &Arc<RwLock<VecDeque<ToolRequest>>>,
        batch_processor: &Arc<BatchProcessor>,
        execution_semaphore: &Arc<Semaphore>,
        result_cache: &Arc<RwLock<lru::LruCache<String, ToolResult>>>,
        metrics: &Arc<RwLock<PipelineMetrics>>,
    ) {
        // Extract batch from queue
        let batch = {
            let mut queue = request_queue.write().await;
            if queue.is_empty() {
                return;
            }

            let batch_size = std::cmp::min(queue.len(), batch_processor.batch_size);
            let requests: Vec<_> = queue.drain(..batch_size).collect();
            
            ToolBatch {
                requests,
                batch_id: uuid::Uuid::new_v4().to_string(),
                created_at: Instant::now(),
            }
        };

        if batch.requests.is_empty() {
            return;
        }

        debug!("Processing batch {} with {} requests", batch.batch_id, batch.requests.len());

        // Process batch concurrently
        let mut handles = Vec::with_capacity(batch.requests.len());
        let batch_size = batch.requests.len(); // Store size before moving
        
        for request in batch.requests {
            let semaphore = Arc::clone(execution_semaphore);
            let cache = Arc::clone(result_cache);
            let metrics_ref = Arc::clone(metrics);
            
            let handle = tokio::spawn(async move {
                let _permit = semaphore.acquire().await.unwrap();
                Self::execute_single_request(request, cache, metrics_ref).await
            });
            
            handles.push(handle);
        }

        // Wait for all requests in batch to complete
        for handle in handles {
            if let Err(e) = handle.await {
                error!("Tool execution failed: {}", e);
            }
        }

        // Update batch efficiency metrics
        let batch_time = batch.created_at.elapsed();
        let mut metrics_guard = metrics.write().await;
        metrics_guard.batch_efficiency = 
            batch_size as f64 / batch_time.as_millis() as f64;
    }

    /// Execute a single tool request with caching and metrics
    async fn execute_single_request(
        request: ToolRequest,
        result_cache: Arc<RwLock<lru::LruCache<String, ToolResult>>>,
        metrics: Arc<RwLock<PipelineMetrics>>,
    ) -> Result<()> {
        let start_time = Instant::now();
        let cache_key = format!("{}:{}", request.tool_name, 
            serde_json::to_string(&request.args).unwrap_or_default());

        // Check cache again (double-checked locking pattern)
        {
            let cache_guard = result_cache.read().await;
            if cache_guard.peek(&cache_key).is_some() {
                metrics.write().await.cache_hits += 1;
                return Ok(());
            }
        }

        // Execute tool with timeout
        let execution_result = timeout(
            request.timeout,
            Self::execute_tool_impl(&request.tool_name, &request.args)
        ).await;

        let execution_time = start_time.elapsed();
        let result = match execution_result {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(anyhow!("Tool execution timed out after {:?}", request.timeout)),
        };

        // Create result with metrics
        let result_for_cache = result.is_ok();
        let tool_result = ToolResult {
            request_id: request.id.clone(),
            result: result.map_err(|e| anyhow::anyhow!(e.to_string())),
            execution_time,
            memory_used: None, // Could be implemented with memory tracking
            cache_hit: false,
        };

        // Cache successful results
        if result_for_cache {
            result_cache.write().await.put(cache_key, tool_result);
        }

        // Update metrics
        let mut metrics_guard = metrics.write().await;
        if result_for_cache {
            metrics_guard.successful_executions += 1;
        } else {
            metrics_guard.failed_executions += 1;
        }
        
        // Update average execution time using exponential moving average
        let alpha = 0.1; // Smoothing factor
        metrics_guard.avg_execution_time_ms = 
            alpha * execution_time.as_millis() as f64 + 
            (1.0 - alpha) * metrics_guard.avg_execution_time_ms;

        Ok(())
    }

    /// Generate cache key for request deduplication
    fn generate_cache_key(&self, request: &ToolRequest) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        request.tool_name.hash(&mut hasher);
        request.args.to_string().hash(&mut hasher);
        
        format!("{}:{:x}", request.tool_name, hasher.finish())
    }

    /// Placeholder for actual tool execution
    async fn execute_tool_impl(_tool_name: &str, _args: &Value) -> Result<Value> {
        // Simulate work with memory pool usage
        let pool = global_pool();
        let mut work_string = pool.get_string();
        
        // Simulate some processing
        work_string.push_str("Executed tool with args");
        tokio::time::sleep(Duration::from_millis(10)).await;
        
        pool.return_string(work_string);
        
        Ok(Value::String("Tool execution result".to_string()))
    }

    /// Get current pipeline metrics
    pub async fn get_metrics(&self) -> PipelineMetrics {
        self.metrics.read().await.clone()
    }

    /// Shutdown the pipeline gracefully
    pub async fn shutdown(&mut self) -> Result<()> {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(()).await;
        }
        Ok(())
    }
}

impl BatchProcessor {
    pub fn new(batch_size: usize, batch_timeout: Duration) -> Self {
        Self {
            current_batch: Arc::new(RwLock::new(None)),
            batch_size,
            batch_timeout,
        }
    }
}

impl Clone for PipelineMetrics {
    fn clone(&self) -> Self {
        Self {
            total_requests: self.total_requests,
            successful_executions: self.successful_executions,
            failed_executions: self.failed_executions,
            cache_hits: self.cache_hits,
            avg_execution_time_ms: self.avg_execution_time_ms,
            batch_efficiency: self.batch_efficiency,
        }
    }
}
