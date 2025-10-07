use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// Performance metrics for the vtcode system
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PerformanceMetrics {
    pub response_times: Vec<Duration>,
    pub cache_hit_rate: f64,
    pub memory_usage: usize,
    pub error_rate: f64,
    pub throughput: usize, // requests per second
    pub context_accuracy: f64,
}

/// Performance monitor for tracking system metrics
pub struct PerformanceMonitor {
    metrics: Arc<RwLock<PerformanceMetrics>>,
    operation_start_times: Arc<RwLock<HashMap<String, Instant>>>,
    total_requests: Arc<RwLock<usize>>,
    successful_requests: Arc<RwLock<usize>>,
    context_predictions: Arc<RwLock<usize>>,
    context_correct: Arc<RwLock<usize>>,
}

impl PerformanceMonitor {
    pub fn new() -> Self {
        Self {
            metrics: Arc::new(RwLock::new(PerformanceMetrics {
                response_times: Vec::new(),
                cache_hit_rate: 0.0,
                memory_usage: 0,
                error_rate: 0.0,
                throughput: 0,
                context_accuracy: 0.0,
            })),
            operation_start_times: Arc::new(RwLock::new(HashMap::new())),
            total_requests: Arc::new(RwLock::new(0)),
            successful_requests: Arc::new(RwLock::new(0)),
            context_predictions: Arc::new(RwLock::new(0)),
            context_correct: Arc::new(RwLock::new(0)),
        }
    }

    /// Start tracking an operation
    pub async fn start_operation(&self, operation_id: String) {
        let mut start_times = self.operation_start_times.write().await;
        start_times.insert(operation_id, Instant::now());

        let mut total = self.total_requests.write().await;
        *total += 1;
    }

    /// End tracking an operation and record response time
    pub async fn end_operation(&self, operation_id: String, success: bool) {
        let start_time = {
            let mut start_times = self.operation_start_times.write().await;
            start_times.remove(&operation_id)
        };

        if let Some(start) = start_time {
            let duration = start.elapsed();

            let mut metrics = self.metrics.write().await;
            metrics.response_times.push(duration);

            // Keep only last 1000 measurements
            if metrics.response_times.len() > 1000 {
                metrics.response_times.remove(0);
            }

            // Update success rate
            if success {
                let mut successful = self.successful_requests.write().await;
                *successful += 1;
            }

            // Update error rate
            let total = *self.total_requests.read().await;
            let successful = *self.successful_requests.read().await;
            metrics.error_rate = if total > 0 {
                (total - successful) as f64 / total as f64
            } else {
                0.0
            };
        }
    }

    /// Record context prediction result
    pub async fn record_context_prediction(&self, correct: bool) {
        let mut predictions = self.context_predictions.write().await;
        *predictions += 1;

        if correct {
            let mut correct_predictions = self.context_correct.write().await;
            *correct_predictions += 1;
        }

        // Update context accuracy
        let predictions = *self.context_predictions.read().await;
        let correct = *self.context_correct.read().await;

        let mut metrics = self.metrics.write().await;
        metrics.context_accuracy = if predictions > 0 {
            correct as f64 / predictions as f64
        } else {
            0.0
        };
    }

    /// Update cache hit rate
    pub async fn update_cache_hit_rate(&self, hit_rate: f64) {
        let mut metrics = self.metrics.write().await;
        metrics.cache_hit_rate = hit_rate;
    }

    /// Update memory usage
    pub async fn update_memory_usage(&self, memory_mb: usize) {
        let mut metrics = self.metrics.write().await;
        metrics.memory_usage = memory_mb;
    }

    /// Get current performance metrics
    pub async fn get_metrics(&self) -> PerformanceMetrics {
        let metrics = self.metrics.read().await;
        metrics.clone()
    }

    /// Calculate average response time
    pub async fn average_response_time(&self) -> Duration {
        let metrics = self.metrics.read().await;
        if metrics.response_times.is_empty() {
            Duration::from_millis(0)
        } else {
            let total: Duration = metrics.response_times.iter().sum();
            total / metrics.response_times.len() as u32
        }
    }

    /// Calculate 95th percentile response time
    pub async fn percentile_95_response_time(&self) -> Duration {
        let metrics = self.metrics.read().await;
        if metrics.response_times.is_empty() {
            Duration::from_millis(0)
        } else {
            let mut times = metrics.response_times.clone();
            times.sort();
            let index = (times.len() as f64 * 0.95) as usize;
            let safe_index = index.min(times.len() - 1);
            times[safe_index]
        }
    }

    /// Check if Phase 1 performance targets are met
    pub async fn check_phase1_targets(&self) -> Phase1Status {
        let avg_response = self.average_response_time().await;
        let p95_response = self.percentile_95_response_time().await;
        let metrics = self.get_metrics().await;

        Phase1Status {
            response_time_target: avg_response < Duration::from_millis(500),
            p95_response_time: p95_response,
            memory_target: metrics.memory_usage < 100,
            cache_target: metrics.cache_hit_rate >= 0.6,
            error_recovery_target: metrics.error_rate <= 0.1,
            context_target: metrics.context_accuracy >= 0.8,
        }
    }

    /// Generate performance report
    pub async fn generate_report(&self) -> String {
        let avg_response = self.average_response_time().await;
        let p95_response = self.percentile_95_response_time().await;
        let metrics = self.get_metrics().await;

        format!(
            "Performance Report - Phase 1 Targets\n\n\
             Response Times:\n\
             • Average: {:.2}ms (Target: <500ms)\n\
             • 95th percentile: {:.2}ms\n\n\
             💾 Resource Usage:\n\
             • Memory: {}MB (Target: <100MB)\n\
             • Cache Hit Rate: {:.1}% (Target: ≥60%)\n\n\
              System Health:\n\
             • Error Rate: {:.1}% (Target: ≤10%)\n\
             • Context Accuracy: {:.1}% (Target: ≥80%)\n\n\
             Throughput: {} req/sec",
            avg_response.as_millis(),
            p95_response.as_millis(),
            metrics.memory_usage,
            metrics.cache_hit_rate * 100.0,
            metrics.error_rate * 100.0,
            metrics.context_accuracy * 100.0,
            metrics.throughput
        )
    }
}

/// Phase 1 target status
#[derive(Debug, Clone)]
pub struct Phase1Status {
    pub response_time_target: bool,
    pub p95_response_time: Duration,
    pub memory_target: bool,
    pub cache_target: bool,
    pub error_recovery_target: bool,
    pub context_target: bool,
}

impl Phase1Status {
    pub fn all_targets_met(&self) -> bool {
        self.response_time_target
            && self.memory_target
            && self.cache_target
            && self.error_recovery_target
            && self.context_target
    }

    pub fn completion_percentage(&self) -> f64 {
        let targets = [
            self.response_time_target,
            self.memory_target,
            self.cache_target,
            self.error_recovery_target,
            self.context_target,
        ];

        let met = targets.iter().filter(|&&t| t).count();
        met as f64 / targets.len() as f64
    }
}

/// Global performance monitor instance
pub static PERFORMANCE_MONITOR: Lazy<PerformanceMonitor> = Lazy::new(PerformanceMonitor::new);
