//! Timeout detection and intelligent retry system for long-running operations
//!
//! This module provides comprehensive timeout detection capabilities with intelligent
//! retry mechanisms to ensure the agent can continue operations without manual intervention.

use std::collections::HashMap;
use std::sync::Arc;

use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tokio::time;

/// Represents different types of operations that can timeout
#[derive(Debug, Clone, Hash, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum OperationType {
    /// API calls to external services
    ApiCall,
    /// File system operations (read/write)
    FileOperation,
    /// Code analysis operations
    CodeAnalysis,
    /// Tool execution
    ToolExecution,
    /// Network requests
    NetworkRequest,
    /// Long-running processing tasks
    Processing,
    /// Custom operation types
    Custom(String),
}

/// Configuration for timeout detection and retry behavior
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TimeoutConfig {
    /// Maximum time allowed for the operation
    pub timeout_duration: Duration,
    /// Maximum number of retry attempts
    pub max_retries: u32,
    /// Initial delay between retries
    pub initial_retry_delay: Duration,
    /// Maximum delay between retries
    pub max_retry_delay: Duration,
    /// Backoff multiplier for exponential backoff
    pub backoff_multiplier: f64,
    /// Whether to use jitter in retry delays
    pub use_jitter: bool,
    /// Whether to retry on timeout
    pub retry_on_timeout: bool,
    /// Whether to retry on specific error types
    pub retry_on_errors: Vec<String>,
}

impl Default for TimeoutConfig {
    fn default() -> Self {
        Self {
            timeout_duration: Duration::from_secs(30),
            max_retries: 3,
            initial_retry_delay: Duration::from_millis(100),
            max_retry_delay: Duration::from_secs(30),
            backoff_multiplier: 2.0,
            use_jitter: true,
            retry_on_timeout: true,
            retry_on_errors: vec![
                "timeout".to_string(),
                "connection".to_string(),
                "network".to_string(),
                "server_error".to_string(),
            ],
        }
    }
}

impl TimeoutConfig {
    /// Configuration optimized for API calls
    pub fn api_call() -> Self {
        Self {
            timeout_duration: Duration::from_secs(60),
            max_retries: 5,
            initial_retry_delay: Duration::from_millis(200),
            max_retry_delay: Duration::from_secs(10),
            backoff_multiplier: 1.5,
            ..Default::default()
        }
    }

    /// Configuration optimized for file operations
    pub fn file_operation() -> Self {
        Self {
            timeout_duration: Duration::from_secs(10),
            max_retries: 2,
            initial_retry_delay: Duration::from_millis(50),
            max_retry_delay: Duration::from_secs(2),
            backoff_multiplier: 2.0,
            retry_on_timeout: false, // File ops usually don't benefit from retries
            ..Default::default()
        }
    }

    /// Configuration optimized for long-running analysis
    pub fn analysis() -> Self {
        Self {
            timeout_duration: Duration::from_secs(120),
            max_retries: 1,
            initial_retry_delay: Duration::from_secs(5),
            max_retry_delay: Duration::from_secs(10),
            backoff_multiplier: 2.0,
            ..Default::default()
        }
    }
}

/// Information about a timeout event
#[derive(Debug, Clone)]
pub struct TimeoutEvent {
    pub operation_id: String,
    pub operation_type: OperationType,
    pub start_time: Instant,
    pub timeout_duration: Duration,
    pub retry_count: u32,
    pub error_message: Option<String>,
}

/// Statistics for timeout detection and retries
#[derive(Debug, Clone, Default)]
pub struct TimeoutStats {
    pub total_operations: usize,
    pub timed_out_operations: usize,
    pub successful_retries: usize,
    pub failed_retries: usize,
    pub average_timeout_duration: Duration,
    pub total_retry_attempts: usize,
}

/// Main timeout detector and retry manager
pub struct TimeoutDetector {
    configs: Arc<RwLock<HashMap<OperationType, TimeoutConfig>>>,
    active_operations: Arc<RwLock<HashMap<String, TimeoutEvent>>>,
    stats: Arc<RwLock<TimeoutStats>>,
}

impl Default for TimeoutDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl TimeoutDetector {
    pub fn new() -> Self {
        let mut configs = HashMap::new();

        // Set default configurations for different operation types
        configs.insert(OperationType::ApiCall, TimeoutConfig::api_call());
        configs.insert(
            OperationType::FileOperation,
            TimeoutConfig::file_operation(),
        );
        configs.insert(OperationType::CodeAnalysis, TimeoutConfig::analysis());
        configs.insert(OperationType::ToolExecution, TimeoutConfig::default());
        configs.insert(OperationType::NetworkRequest, TimeoutConfig::api_call());
        configs.insert(OperationType::Processing, TimeoutConfig::analysis());

        Self {
            configs: Arc::new(RwLock::new(configs)),
            active_operations: Arc::new(RwLock::new(HashMap::new())),
            stats: Arc::new(RwLock::new(TimeoutStats::default())),
        }
    }

    /// Set configuration for a specific operation type
    pub async fn set_config(&self, operation_type: OperationType, config: TimeoutConfig) {
        let mut configs = self.configs.write().await;
        configs.insert(operation_type, config);
    }

    /// Get configuration for a specific operation type
    pub async fn get_config(&self, operation_type: &OperationType) -> TimeoutConfig {
        let configs = self.configs.read().await;
        configs.get(operation_type).cloned().unwrap_or_default()
    }

    /// Start monitoring an operation
    pub async fn start_operation(
        &self,
        operation_id: String,
        operation_type: OperationType,
    ) -> TimeoutHandle {
        let config = self.get_config(&operation_type).await;

        let event = TimeoutEvent {
            operation_id: operation_id.clone(),
            operation_type,
            start_time: Instant::now(),
            timeout_duration: config.timeout_duration,
            retry_count: 0,
            error_message: None,
        };

        let mut active_ops = self.active_operations.write().await;
        active_ops.insert(operation_id.clone(), event);

        let mut stats = self.stats.write().await;
        stats.total_operations += 1;

        TimeoutHandle {
            operation_id,
            detector: Arc::new(self.clone()),
        }
    }

    /// Check if an operation has timed out
    pub async fn check_timeout(&self, operation_id: &str) -> Option<TimeoutEvent> {
        let active_ops = self.active_operations.read().await;
        if let Some(event) = active_ops.get(operation_id) {
            if event.start_time.elapsed() >= event.timeout_duration {
                return Some(event.clone());
            }
        }
        None
    }

    /// Record a timeout event
    pub async fn record_timeout(&self, operation_id: &str, error_message: Option<String>) {
        let mut active_ops = self.active_operations.write().await;
        if let Some(event) = active_ops.get_mut(operation_id) {
            event.error_message = error_message;
        }

        let mut stats = self.stats.write().await;
        stats.timed_out_operations += 1;
    }

    /// Record a successful retry
    pub async fn record_successful_retry(&self, _operation_id: &str) {
        let mut stats = self.stats.write().await;
        stats.successful_retries += 1;
        stats.total_retry_attempts += 1;
    }

    /// Record a failed retry
    pub async fn record_failed_retry(&self, _operation_id: &str) {
        let mut stats = self.stats.write().await;
        stats.failed_retries += 1;
        stats.total_retry_attempts += 1;
    }

    /// End monitoring an operation
    pub async fn end_operation(&self, operation_id: &str) {
        let mut active_ops = self.active_operations.write().await;
        if let Some(event) = active_ops.remove(operation_id) {
            let duration = event.start_time.elapsed();
            let mut stats = self.stats.write().await;
            // Update average timeout duration
            if stats.total_operations > 0 {
                let total_duration =
                    stats.average_timeout_duration * (stats.total_operations - 1) as u32;
                stats.average_timeout_duration =
                    (total_duration + duration) / stats.total_operations as u32;
            }
        }
    }

    /// Get current timeout statistics
    pub async fn get_stats(&self) -> TimeoutStats {
        self.stats.read().await.clone()
    }

    /// Calculate retry delay with exponential backoff and optional jitter
    pub async fn calculate_retry_delay(
        &self,
        operation_type: &OperationType,
        attempt: u32,
    ) -> Duration {
        let config = self.get_config(operation_type).await;

        let base_delay = config.initial_retry_delay.as_millis() as f64;
        let multiplier = config.backoff_multiplier.powi(attempt as i32);
        let delay_ms = (base_delay * multiplier) as u64;

        let mut delay =
            Duration::from_millis(delay_ms.min(config.max_retry_delay.as_millis() as u64));

        // Add jitter if enabled
        if config.use_jitter {
            use std::time::SystemTime;
            let seed = SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos() as u64;
            let jitter_factor = (seed % 100) as f64 / 100.0; // 0.0 to 1.0
            let jitter_ms = (delay.as_millis() as f64 * 0.1 * jitter_factor) as u64; // 10% jitter
            delay += Duration::from_millis(jitter_ms);
        }

        delay
    }

    /// Determine if an error should trigger a retry
    pub async fn should_retry(
        &self,
        operation_type: &OperationType,
        error: &anyhow::Error,
        attempt: u32,
    ) -> bool {
        let config = self.get_config(operation_type).await;

        if attempt >= config.max_retries {
            return false;
        }

        let error_str = error.to_string().to_lowercase();

        // Check if error matches retryable patterns
        for retry_error in &config.retry_on_errors {
            if error_str.contains(retry_error) {
                return true;
            }
        }

        // Check for timeout-specific retry
        if config.retry_on_timeout
            && (error_str.contains("timeout") || error_str.contains("timed out"))
        {
            return true;
        }

        false
    }

    /// Execute an operation with automatic timeout detection and retries
    pub async fn execute_with_timeout_retry<F, Fut, T>(
        &self,
        operation_id: String,
        operation_type: OperationType,
        mut operation: F,
    ) -> Result<T, anyhow::Error>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<T, anyhow::Error>>,
    {
        let config = self.get_config(&operation_type).await;
        let mut attempt = 0;
        let _last_error: Option<anyhow::Error> = None;

        loop {
            let handle = self
                .start_operation(
                    format!("{}_{}", operation_id, attempt),
                    operation_type.clone(),
                )
                .await;

            let result = match time::timeout(config.timeout_duration, operation()).await {
                Ok(result) => result,
                Err(_) => {
                    self.record_timeout(
                        &handle.operation_id,
                        Some("Operation timed out".to_string()),
                    )
                    .await;
                    Err(anyhow::anyhow!(
                        "Operation '{}' timed out after {:?}",
                        operation_id,
                        config.timeout_duration
                    ))
                }
            };

            handle.end().await;

            match result {
                Ok(value) => {
                    if attempt > 0 {
                        self.record_successful_retry(&format!("{}_{}", operation_id, attempt))
                            .await;
                    }
                    return Ok(value);
                }
                Err(error) => {
                    let should_retry_op = self.should_retry(&operation_type, &error, attempt).await;

                    if !should_retry_op {
                        if attempt > 0 {
                            self.record_failed_retry(&format!("{}_{}", operation_id, attempt))
                                .await;
                        }
                        return Err(error);
                    }

                    attempt += 1;
                    self.record_failed_retry(&format!("{}_{}", operation_id, attempt))
                        .await;

                    let delay = self.calculate_retry_delay(&operation_type, attempt).await;
                    eprintln!(
                        "Operation '{}' failed (attempt {}/{}), retrying in {:?}",
                        operation_id, attempt, config.max_retries, delay
                    );
                    time::sleep(delay).await;
                }
            }
        }
    }
}

impl Clone for TimeoutDetector {
    fn clone(&self) -> Self {
        Self {
            configs: Arc::clone(&self.configs),
            active_operations: Arc::clone(&self.active_operations),
            stats: Arc::clone(&self.stats),
        }
    }
}

/// Handle for tracking an operation's lifecycle
pub struct TimeoutHandle {
    operation_id: String,
    detector: Arc<TimeoutDetector>,
}

impl TimeoutHandle {
    /// End monitoring for this operation
    pub async fn end(self) {
        self.detector.end_operation(&self.operation_id).await;
    }

    /// Get the operation ID
    pub fn operation_id(&self) -> &str {
        &self.operation_id
    }
}

impl Drop for TimeoutHandle {
    fn drop(&mut self) {
        // Note: We can't make this async in Drop, so we spawn a task
        let operation_id = self.operation_id.clone();
        let detector = Arc::clone(&self.detector);

        tokio::spawn(async move {
            detector.end_operation(&operation_id).await;
        });
    }
}

/// Global timeout detector instance
use once_cell::sync::Lazy;
pub static TIMEOUT_DETECTOR: Lazy<TimeoutDetector> = Lazy::new(TimeoutDetector::new);

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;
    use tokio::time::sleep;

    #[tokio::test]
    async fn test_timeout_detection() {
        let detector = TimeoutDetector::new();

        // Test with a short timeout
        let config = TimeoutConfig {
            timeout_duration: Duration::from_millis(10),
            max_retries: 0,
            ..Default::default()
        };

        detector.set_config(OperationType::ApiCall, config).await;

        let result = detector
            .execute_with_timeout_retry(
                "test_operation".to_string(),
                OperationType::ApiCall,
                || async {
                    sleep(Duration::from_millis(20)).await;
                    Ok("success")
                },
            )
            .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("timed out"));
    }

    #[tokio::test]
    async fn test_successful_retry() {
        let detector = TimeoutDetector::new();

        let config = TimeoutConfig {
            timeout_duration: Duration::from_millis(50),
            max_retries: 2,
            initial_retry_delay: Duration::from_millis(5),
            retry_on_timeout: true,
            ..Default::default()
        };

        detector.set_config(OperationType::ApiCall, config).await;

        let call_count = Arc::new(AtomicUsize::new(0));
        let call_count_clone = call_count.clone();
        let result = detector
            .execute_with_timeout_retry(
                "test_retry".to_string(),
                OperationType::ApiCall,
                move || {
                    let call_count = call_count_clone.clone();
                    async move {
                        let count = call_count.fetch_add(1, Ordering::SeqCst) + 1;
                        if count == 1 {
                            // First call fails with timeout
                            sleep(Duration::from_millis(60)).await;
                            Ok("should not reach here")
                        } else {
                            // Second call succeeds
                            sleep(Duration::from_millis(10)).await;
                            Ok("success")
                        }
                    }
                },
            )
            .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "success");
        assert_eq!(call_count.load(Ordering::SeqCst), 2);

        let stats = detector.get_stats().await;
        assert_eq!(stats.successful_retries, 1);
        assert_eq!(stats.total_retry_attempts, 2);
    }

    #[tokio::test]
    async fn test_calculate_retry_delay() {
        let detector = TimeoutDetector::new();

        let delay = detector
            .calculate_retry_delay(&OperationType::ApiCall, 0)
            .await;
        assert!(delay >= Duration::from_millis(200)); // Initial delay for API calls

        let delay2 = detector
            .calculate_retry_delay(&OperationType::ApiCall, 1)
            .await;
        assert!(delay2 > delay); // Should increase with backoff
    }
}
