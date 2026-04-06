//! Optimized LLM client with connection pooling and request batching

use anyhow::Result;
use hashbrown::HashMap;
use parking_lot::Mutex;
use serde_json::Value;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::{Notify, RwLock, Semaphore, mpsc};
use tracing::debug;

use crate::llm::types::LLMError;

/// Simplified request structure for optimization
#[derive(Debug, Clone)]
pub struct OptimizedRequest {
    pub model: String,
    pub messages: Vec<Value>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
}

/// Simplified response structure
#[derive(Debug, Clone)]
pub struct OptimizedResponse {
    pub content: String,
    pub usage: Option<Value>,
}

/// Connection pool for HTTP clients
pub struct ConnectionPool {
    /// Pool of reusable HTTP clients
    clients: Arc<RwLock<Vec<reqwest::Client>>>,

    /// Maximum pool size
    max_size: usize,

    /// Current pool utilization
    active_connections: Arc<Semaphore>,
}

/// Request batching manager for similar requests
pub struct RequestBatcher {
    /// Pending requests waiting to be batched
    pending_requests: Arc<RwLock<HashMap<String, Vec<BatchedRequest>>>>,

    /// Wakes the processing loop when new partial batches arrive.
    work_notify: Arc<Notify>,

    /// Batch processing interval
    batch_interval: Duration,

    /// Maximum batch size
    max_batch_size: usize,

    /// Guards against spawning duplicate processing loops.
    processing_started: AtomicBool,

    /// Shutdown signal sender for the background processing loop.
    shutdown_tx: Mutex<Option<mpsc::Sender<()>>>,

    /// Handle for the background processing loop task.
    processing_task: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

/// A request that can be batched with similar requests
#[derive(Debug)]
pub struct BatchedRequest {
    pub id: String,
    pub request: OptimizedRequest,
    pub response_tx: tokio::sync::oneshot::Sender<Result<OptimizedResponse, LLMError>>,
    pub submitted_at: Instant,
}

/// Optimized LLM client with advanced performance features
pub struct OptimizedLLMClient {
    /// Connection pool for HTTP requests
    connection_pool: Arc<ConnectionPool>,

    /// Request batcher for similar requests
    request_batcher: Arc<RequestBatcher>,

    /// Response cache for identical requests
    response_cache: Arc<RwLock<lru::LruCache<String, CachedResponse>>>,

    /// Rate limiter for API calls
    rate_limiter: Arc<RateLimiter>,

    /// Performance metrics
    metrics: Arc<RwLock<ClientMetrics>>,
}

/// Cached response with TTL
#[derive(Debug, Clone)]
pub struct CachedResponse {
    pub response: OptimizedResponse,
    pub cached_at: Instant,
    pub ttl: Duration,
}

/// Rate limiter for API requests
pub struct RateLimiter {
    /// Token bucket for burst handling
    token_bucket: Arc<RwLock<TokenBucket>>,
}

/// Token bucket for rate limiting
#[derive(Debug)]
pub struct TokenBucket {
    pub tokens: f64,
    pub capacity: f64,
    pub refill_rate: f64,
    pub last_refill: Instant,
}

/// Client performance metrics
#[derive(Debug, Default, Clone)]
pub struct ClientMetrics {
    pub total_requests: u64,
    pub cache_hits: u64,
    pub batched_requests: u64,
    pub avg_response_time_ms: f64,
    pub connection_pool_utilization: f64,
    pub rate_limit_hits: u64,
}

impl ConnectionPool {
    pub fn new(max_size: usize) -> Self {
        let clients = Vec::with_capacity(max_size);

        Self {
            clients: Arc::new(RwLock::new(clients)),
            max_size,
            active_connections: Arc::new(Semaphore::new(max_size)),
        }
    }

    /// Get a client from the pool or create a new one
    pub async fn get_client(&self) -> Result<reqwest::Client> {
        // Try to get from pool first
        {
            let mut clients = self.clients.write().await;
            if let Some(client) = clients.pop() {
                return Ok(client);
            }
        }

        // Create new client with optimized settings
        let client = reqwest::Client::builder()
            .pool_max_idle_per_host(10)
            .pool_idle_timeout(Duration::from_secs(30))
            .timeout(Duration::from_secs(60))
            .tcp_keepalive(Duration::from_secs(60))
            .http2_prior_knowledge()
            .build()?;

        Ok(client)
    }

    /// Return a client to the pool
    pub async fn return_client(&self, client: reqwest::Client) {
        let mut clients = self.clients.write().await;
        if clients.len() < self.max_size {
            clients.push(client);
        }
    }

    /// Get current pool utilization
    pub async fn utilization(&self) -> f64 {
        let available = self.active_connections.available_permits();
        let total = self.max_size;
        (total - available) as f64 / total as f64
    }
}

impl RequestBatcher {
    pub fn new(batch_interval: Duration, max_batch_size: usize) -> Self {
        Self {
            pending_requests: Arc::new(RwLock::new(HashMap::new())),
            work_notify: Arc::new(Notify::new()),
            batch_interval,
            max_batch_size,
            processing_started: AtomicBool::new(false),
            shutdown_tx: Mutex::new(None),
            processing_task: Mutex::new(None),
        }
    }

    /// Add request to batch queue
    pub async fn add_request(&self, request: BatchedRequest) -> Result<()> {
        let batch_key = self.generate_batch_key(&request.request);

        let mut pending = self.pending_requests.write().await;
        let batch = pending.entry(batch_key).or_insert_with(Vec::new);

        batch.push(request);

        // Trigger immediate processing if batch is full
        if batch.len() >= self.max_batch_size {
            // Process batch immediately
            let batch_requests = std::mem::take(batch);
            drop(pending);

            tokio::spawn(async move {
                Self::process_batch(batch_requests).await;
            });
        } else {
            drop(pending);
            self.work_notify.notify_one();
        }

        Ok(())
    }

    /// Generate batch key for grouping similar requests
    fn generate_batch_key(&self, request: &OptimizedRequest) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();

        request.model.hash(&mut hasher);
        request.temperature.map(f32::to_bits).hash(&mut hasher);
        request.max_tokens.hash(&mut hasher);

        format!("{:x}", hasher.finish())
    }

    /// Process a batch of similar requests
    async fn process_batch(requests: Vec<BatchedRequest>) {
        debug!("Processing batch of {} requests", requests.len());

        let mut tasks = tokio::task::JoinSet::new();
        for request in requests {
            tasks.spawn(async move {
                let result = Self::execute_single_request(request.request).await;
                let _ = request.response_tx.send(result);
            });
        }

        while let Some(result) = tasks.join_next().await {
            if let Err(error) = result {
                debug!(?error, "batched request task failed");
            }
        }
    }

    /// Execute a single request (placeholder)
    async fn execute_single_request(
        _request: OptimizedRequest,
    ) -> Result<OptimizedResponse, LLMError> {
        // Placeholder implementation
        tokio::time::sleep(Duration::from_millis(100)).await;

        Ok(OptimizedResponse {
            content: "Batched response".to_string(),
            usage: None,
        })
    }

    /// Start batch processing loop
    pub async fn start_processing(&self) {
        if self.processing_started.swap(true, Ordering::SeqCst) {
            return;
        }

        let (shutdown_tx, mut shutdown_rx) = mpsc::channel(1);
        *self.shutdown_tx.lock() = Some(shutdown_tx);

        let pending_requests = Arc::clone(&self.pending_requests);
        let work_notify = Arc::clone(&self.work_notify);
        let batch_interval = self.batch_interval;

        let processing_task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = shutdown_rx.recv() => {
                        debug!("LLM request batch processing shutdown requested");
                        break;
                    }
                    _ = work_notify.notified() => {}
                }

                let flush_deadline = tokio::time::Instant::now() + batch_interval;
                let sleep_until_flush = tokio::time::sleep_until(flush_deadline);
                tokio::pin!(sleep_until_flush);

                loop {
                    tokio::select! {
                        _ = shutdown_rx.recv() => {
                            debug!("LLM request batch processing shutdown requested");
                            return;
                        }
                        _ = &mut sleep_until_flush => {
                            let batches_to_process = Self::take_pending_batches(&pending_requests).await;
                            for batch in batches_to_process {
                                tokio::spawn(async move {
                                    Self::process_batch(batch).await;
                                });
                            }
                            break;
                        }
                        _ = work_notify.notified() => {}
                    }
                }
            }
        });
        *self.processing_task.lock() = Some(processing_task);
    }

    async fn take_pending_batches(
        pending_requests: &Arc<RwLock<HashMap<String, Vec<BatchedRequest>>>>,
    ) -> Vec<Vec<BatchedRequest>> {
        let mut pending = pending_requests.write().await;
        let mut batches = Vec::new();

        for requests in pending.values_mut() {
            if !requests.is_empty() {
                batches.push(std::mem::take(requests));
            }
        }

        pending.retain(|_, requests| !requests.is_empty());
        batches
    }

    pub async fn shutdown_processing(&self) {
        let shutdown_tx = { self.shutdown_tx.lock().take() };
        if let Some(tx) = shutdown_tx {
            let _ = tx.send(()).await;
        }

        let handle = { self.processing_task.lock().take() };
        if let Some(handle) = handle {
            let _ = handle.await;
        }

        self.processing_started.store(false, Ordering::SeqCst);
    }
}

impl Drop for RequestBatcher {
    fn drop(&mut self) {
        if let Some(handle) = self.processing_task.lock().take() {
            handle.abort();
        }
        self.shutdown_tx.lock().take();
    }
}

impl RateLimiter {
    pub fn new(requests_per_second: f64, burst_capacity: usize) -> Self {
        let refill_rate = if requests_per_second.is_finite() && requests_per_second > 0.0 {
            requests_per_second
        } else {
            1.0
        };
        let burst_capacity = burst_capacity.max(1);

        Self {
            token_bucket: Arc::new(RwLock::new(TokenBucket {
                tokens: burst_capacity as f64,
                capacity: burst_capacity as f64,
                refill_rate,
                last_refill: Instant::now(),
            })),
        }
    }

    /// Acquire a permit for making a request
    pub async fn acquire(&self) -> Result<()> {
        loop {
            let wait_time = {
                let mut bucket = self.token_bucket.write().await;
                Self::refill_tokens(&mut bucket);

                if bucket.tokens >= 1.0 {
                    bucket.tokens -= 1.0;
                    return Ok(());
                }

                let wait_secs = (1.0 - bucket.tokens) / bucket.refill_rate;
                Duration::try_from_secs_f64(wait_secs).unwrap_or(Duration::from_secs(60))
            };

            tokio::time::sleep(wait_time).await;
        }
    }

    /// Refill token bucket based on elapsed time
    fn refill_tokens(bucket: &mut TokenBucket) {
        let now = Instant::now();
        let elapsed = now.duration_since(bucket.last_refill).as_secs_f64();

        let tokens_to_add = elapsed * bucket.refill_rate;
        bucket.tokens = (bucket.tokens + tokens_to_add).min(bucket.capacity);
        bucket.last_refill = now;
    }
}

impl OptimizedLLMClient {
    pub fn new(
        pool_size: usize,
        cache_size: usize,
        requests_per_second: f64,
        burst_capacity: usize,
    ) -> Self {
        Self {
            connection_pool: Arc::new(ConnectionPool::new(pool_size)),
            request_batcher: Arc::new(RequestBatcher::new(Duration::from_millis(100), 10)),
            response_cache: Arc::new(RwLock::new(lru::LruCache::new(
                std::num::NonZeroUsize::new(cache_size).unwrap_or(std::num::NonZeroUsize::MIN),
            ))),
            rate_limiter: Arc::new(RateLimiter::new(requests_per_second, burst_capacity)),
            metrics: Arc::new(RwLock::new(ClientMetrics::default())),
        }
    }

    /// Make an optimized LLM request with caching and batching
    pub async fn chat_optimized(
        &self,
        request: OptimizedRequest,
    ) -> Result<OptimizedResponse, LLMError> {
        let start_time = Instant::now();

        // Generate cache key
        let cache_key = self.generate_cache_key(&request);

        // Check cache first
        let cached_response = {
            let cache = self.response_cache.read().await;
            cache
                .peek(&cache_key)
                .filter(|cached| cached.cached_at.elapsed() < cached.ttl)
                .map(|cached| cached.response.clone())
        };
        if let Some(response) = cached_response {
            self.metrics.write().await.cache_hits += 1;
            return Ok(response);
        }

        self.request_batcher.start_processing().await;

        // Acquire rate limit permit
        self.rate_limiter
            .acquire()
            .await
            .map_err(|_e| LLMError::RateLimit { metadata: None })?;

        // Create batched request
        let (response_tx, response_rx) = tokio::sync::oneshot::channel();
        let batched_request = BatchedRequest {
            id: uuid::Uuid::new_v4().to_string(),
            request,
            response_tx,
            submitted_at: start_time,
        };

        // Add to batch queue
        self.request_batcher
            .add_request(batched_request)
            .await
            .map_err(|e| LLMError::InvalidRequest {
                message: e.to_string(),
                metadata: None,
            })?;

        // Wait for response
        let response = response_rx.await.map_err(|e| LLMError::InvalidRequest {
            message: e.to_string(),
            metadata: None,
        })??;

        // Cache successful response
        let cached_response = CachedResponse {
            response: response.clone(),
            cached_at: Instant::now(),
            ttl: Duration::from_secs(300), // 5 minutes
        };

        self.response_cache
            .write()
            .await
            .put(cache_key, cached_response);

        // Update metrics
        let execution_time = start_time.elapsed();
        let mut metrics = self.metrics.write().await;
        metrics.total_requests += 1;

        // Update average response time using exponential moving average
        let alpha = 0.1;
        metrics.avg_response_time_ms = alpha * execution_time.as_millis() as f64
            + (1.0 - alpha) * metrics.avg_response_time_ms;

        Ok(response)
    }

    /// Generate cache key for request
    fn generate_cache_key(&self, request: &OptimizedRequest) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        request.model.hash(&mut hasher);
        request.temperature.map(f32::to_bits).hash(&mut hasher);
        request.max_tokens.hash(&mut hasher);

        for message in &request.messages {
            message.to_string().hash(&mut hasher);
        }

        format!("{:x}", hasher.finish())
    }

    /// Start the client's background processing
    pub async fn start(&self) -> Result<()> {
        self.request_batcher.start_processing().await;
        Ok(())
    }

    pub async fn shutdown(&self) -> Result<()> {
        self.request_batcher.shutdown_processing().await;
        Ok(())
    }

    /// Get current client metrics
    pub async fn get_metrics(&self) -> ClientMetrics {
        let mut metrics = self.metrics.read().await.clone();
        metrics.connection_pool_utilization = self.connection_pool.utilization().await;
        metrics
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_key_includes_generation_settings() {
        let client = OptimizedLLMClient::new(1, 16, 10.0, 1);
        let base_request = OptimizedRequest {
            model: "model-a".to_string(),
            messages: vec![serde_json::json!({"role": "user", "content": "hello"})],
            temperature: Some(0.2),
            max_tokens: Some(128),
        };
        let different_temperature = OptimizedRequest {
            temperature: Some(0.8),
            ..base_request.clone()
        };
        let different_max_tokens = OptimizedRequest {
            max_tokens: Some(256),
            ..base_request.clone()
        };

        assert_ne!(
            client.generate_cache_key(&base_request),
            client.generate_cache_key(&different_temperature)
        );
        assert_ne!(
            client.generate_cache_key(&base_request),
            client.generate_cache_key(&different_max_tokens)
        );
    }

    #[test]
    fn test_batch_key_includes_generation_settings() {
        let batcher = RequestBatcher::new(Duration::from_millis(100), 10);
        let base_request = OptimizedRequest {
            model: "model-a".to_string(),
            messages: vec![serde_json::json!({"role": "user", "content": "hello"})],
            temperature: Some(0.2),
            max_tokens: Some(128),
        };
        let different_request = OptimizedRequest {
            temperature: Some(0.8),
            ..base_request.clone()
        };

        assert_ne!(
            batcher.generate_batch_key(&base_request),
            batcher.generate_batch_key(&different_request)
        );
    }

    #[tokio::test]
    async fn test_chat_optimized_starts_batch_processing_automatically() {
        let client = OptimizedLLMClient::new(1, 16, 10.0, 1);
        let response = tokio::time::timeout(
            Duration::from_secs(1),
            client.chat_optimized(OptimizedRequest {
                model: "model-a".to_string(),
                messages: vec![serde_json::json!({"role": "user", "content": "hello"})],
                temperature: Some(0.2),
                max_tokens: Some(128),
            }),
        )
        .await
        .expect("request should complete without explicit start")
        .expect("request should succeed");

        assert_eq!(response.content, "Batched response");
    }

    #[tokio::test]
    async fn rate_limiter_zero_burst_capacity_still_allows_progress() {
        let limiter = RateLimiter::new(10.0, 0);

        tokio::time::timeout(Duration::from_millis(100), limiter.acquire())
            .await
            .expect("rate limiter should not stall with zero configured burst")
            .expect("rate limiter acquire should succeed");

        assert_eq!(limiter.token_bucket.read().await.capacity, 1.0);
    }
}
