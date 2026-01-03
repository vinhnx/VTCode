//! Optimized LLM client with connection pooling and request batching

use anyhow::Result;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{RwLock, Semaphore};
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

    /// Batch processing interval
    batch_interval: Duration,

    /// Maximum batch size
    max_batch_size: usize,
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
    /// Semaphore for request rate limiting
    permits: Arc<Semaphore>,

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
#[derive(Debug, Default)]
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
            batch_interval,
            max_batch_size,
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
        }

        Ok(())
    }

    /// Generate batch key for grouping similar requests
    fn generate_batch_key(&self, request: &OptimizedRequest) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();

        // Hash model for batching
        request.model.hash(&mut hasher);

        format!("{:x}", hasher.finish())
    }

    /// Process a batch of similar requests
    async fn process_batch(requests: Vec<BatchedRequest>) {
        debug!("Processing batch of {} requests", requests.len());

        // For now, process individually (could be optimized for providers that support batching)
        for request in requests {
            let result = Self::execute_single_request(request.request).await;
            let _ = request.response_tx.send(result);
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
        let pending_requests = Arc::clone(&self.pending_requests);
        let batch_interval = self.batch_interval;

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(batch_interval);

            loop {
                interval.tick().await;

                let batches_to_process = {
                    let mut pending = pending_requests.write().await;
                    let mut batches = Vec::new();

                    for (key, requests) in pending.iter_mut() {
                        if !requests.is_empty() {
                            batches.push((key.clone(), std::mem::take(requests)));
                        }
                    }

                    // Clean up empty entries
                    pending.retain(|_, v| !v.is_empty());

                    batches
                };

                // Process all batches concurrently
                for (_, batch) in batches_to_process {
                    tokio::spawn(async move {
                        Self::process_batch(batch).await;
                    });
                }
            }
        });
    }
}

impl RateLimiter {
    pub fn new(requests_per_second: f64, burst_capacity: usize) -> Self {
        Self {
            permits: Arc::new(Semaphore::new(burst_capacity)),
            token_bucket: Arc::new(RwLock::new(TokenBucket {
                tokens: burst_capacity as f64,
                capacity: burst_capacity as f64,
                refill_rate: requests_per_second,
                last_refill: Instant::now(),
            })),
        }
    }

    /// Acquire a permit for making a request
    pub async fn acquire(&self) -> Result<()> {
        // Refill token bucket
        self.refill_tokens().await;

        // Try to acquire a token
        let mut bucket = self.token_bucket.write().await;
        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            Ok(())
        } else {
            drop(bucket);

            // Wait for permit from semaphore
            let _permit = self.permits.acquire().await?;
            Ok(())
        }
    }

    /// Refill token bucket based on elapsed time
    async fn refill_tokens(&self) {
        let mut bucket = self.token_bucket.write().await;
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
                std::num::NonZeroUsize::new(cache_size).unwrap(),
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
        {
            let cache = self.response_cache.read().await;
            if let Some(cached) = cache.peek(&cache_key) {
                if cached.cached_at.elapsed() < cached.ttl {
                    self.metrics.write().await.cache_hits += 1;
                    return Ok(cached.response.clone());
                }
            }
        }

        // Acquire rate limit permit
        self.rate_limiter
            .acquire()
            .await
            .map_err(|_e| LLMError::RateLimit { metadata: None })?;

        // Create batched request
        let (response_tx, response_rx) = tokio::sync::oneshot::channel();
        let batched_request = BatchedRequest {
            id: uuid::Uuid::new_v4().to_string(),
            request: request.clone(),
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

    /// Get current client metrics
    pub async fn get_metrics(&self) -> ClientMetrics {
        let mut metrics = self.metrics.read().await.clone();
        metrics.connection_pool_utilization = self.connection_pool.utilization().await;
        metrics
    }
}

impl Clone for ClientMetrics {
    fn clone(&self) -> Self {
        Self {
            total_requests: self.total_requests,
            cache_hits: self.cache_hits,
            batched_requests: self.batched_requests,
            avg_response_time_ms: self.avg_response_time_ms,
            connection_pool_utilization: self.connection_pool_utilization,
            rate_limit_hits: self.rate_limit_hits,
        }
    }
}
