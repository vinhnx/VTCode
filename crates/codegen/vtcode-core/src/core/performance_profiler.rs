//! Performance benchmarking and profiling tools for VT Code optimizations

use crate::utils::file_utils::write_file_with_context;
use anyhow::Result;
use hashbrown::HashMap;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// Performance benchmark results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkResults {
    /// Name of the benchmark test.
    pub test_name: String,
    /// Total number of iterations executed.
    pub iterations: u64,
    /// Wall-clock time for the entire benchmark run.
    pub total_duration: Duration,
    /// Average duration per iteration in nanoseconds.
    pub avg_duration_ns: u64,
    /// Minimum duration of any single iteration in nanoseconds.
    pub min_duration_ns: u64,
    /// Maximum duration of any single iteration in nanoseconds.
    pub max_duration_ns: u64,
    /// 95th percentile duration in nanoseconds.
    pub percentile_95_ns: u64,
    /// 99th percentile duration in nanoseconds.
    pub percentile_99_ns: u64,
    /// Operations per second throughput.
    pub throughput_ops_per_sec: f64,
    /// Average memory usage in megabytes, if sampled.
    pub memory_usage_mb: Option<f64>,
    /// Average CPU usage percentage, if sampled.
    pub cpu_usage_percent: Option<f64>,
}

/// System resource usage metrics
#[derive(Debug, Clone, Default)]
pub struct ResourceMetrics {
    /// Memory usage in megabytes.
    pub memory_used_mb: f64,
    /// CPU usage as a percentage (0.0 to 100.0).
    pub cpu_percent: f64,
    /// Total bytes sent over the network.
    pub network_bytes_sent: u64,
    /// Total bytes received over the network.
    pub network_bytes_received: u64,
    /// Number of disk read operations.
    pub disk_reads: u64,
    /// Number of disk write operations.
    pub disk_writes: u64,
}

/// Performance profiler for tracking execution metrics
pub struct PerformanceProfiler {
    /// Active benchmark sessions
    sessions: Arc<RwLock<HashMap<String, BenchmarkSession>>>,

    /// System resource monitor
    resource_monitor: Arc<ResourceMonitor>,

    /// Historical benchmark results
    history: Arc<RwLock<Vec<BenchmarkResults>>>,
}

/// Individual benchmark session
#[derive(Debug)]
pub struct BenchmarkSession {
    /// Name of the benchmark session.
    pub name: String,
    /// Wall-clock time when the session started.
    pub start_time: Instant,
    /// Number of iterations recorded so far.
    pub iterations: u64,
    /// Duration of each recorded iteration.
    pub durations: Vec<Duration>,
    /// Resource usage snapshots captured periodically during the session.
    pub resource_snapshots: Vec<ResourceMetrics>,
}

/// System resource monitoring
pub struct ResourceMonitor {
    /// Current resource usage
    current_metrics: Arc<RwLock<ResourceMetrics>>,

    /// Monitoring interval
    monitor_interval: Duration,

    /// Whether monitoring is active
    is_monitoring: Arc<RwLock<bool>>,
}

impl PerformanceProfiler {
    /// Create a new performance profiler with default settings.
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            resource_monitor: Arc::new(ResourceMonitor::new(Duration::from_millis(100))),
            history: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Start a new benchmark session
    pub async fn start_benchmark(&self, name: &str) -> Result<()> {
        let session = BenchmarkSession {
            name: name.to_string(),
            start_time: Instant::now(),
            iterations: 0,
            durations: Vec::new(),
            resource_snapshots: Vec::new(),
        };

        self.sessions
            .write()
            .await
            .insert(name.to_string(), session);
        self.resource_monitor.start_monitoring().await?;

        Ok(())
    }

    /// Record a single operation timing
    pub async fn record_operation(&self, session_name: &str, duration: Duration) -> Result<()> {
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.get_mut(session_name) {
            session.iterations += 1;
            session.durations.push(duration);

            // Capture resource snapshot every 100 operations
            if session.iterations % 100 == 0 {
                let metrics = self.resource_monitor.get_current_metrics().await;
                session.resource_snapshots.push(metrics);
            }
        }

        Ok(())
    }

    /// End benchmark session and calculate results
    pub async fn end_benchmark(&self, session_name: &str) -> Result<BenchmarkResults> {
        let session = {
            let mut sessions = self.sessions.write().await;
            sessions
                .remove(session_name)
                .ok_or_else(|| anyhow::anyhow!("Benchmark session '{session_name}' not found"))?
        };

        self.resource_monitor.stop_monitoring().await?;

        let results = self.calculate_results(session).await;

        // Store in history
        self.history.write().await.push(results.clone());

        Ok(results)
    }

    /// Calculate benchmark results from session data
    async fn calculate_results(&self, session: BenchmarkSession) -> BenchmarkResults {
        let total_duration = session.start_time.elapsed();
        let mut durations_ns: Vec<u64> = session
            .durations
            .iter()
            .map(|d| d.as_nanos() as u64)
            .collect();

        durations_ns.sort_unstable();

        let avg_duration_ns = if !durations_ns.is_empty() {
            durations_ns.iter().sum::<u64>() / durations_ns.len() as u64
        } else {
            0
        };

        let min_duration_ns = durations_ns.first().copied().unwrap_or(0);
        let max_duration_ns = durations_ns.last().copied().unwrap_or(0);

        let percentile_95_ns = if !durations_ns.is_empty() {
            #[allow(clippy::cast_sign_loss)]
            let index = (durations_ns.len() as f64 * 0.95) as usize;
            durations_ns
                .get(index.min(durations_ns.len() - 1))
                .copied()
                .unwrap_or(0)
        } else {
            0
        };

        let percentile_99_ns = if !durations_ns.is_empty() {
            #[allow(clippy::cast_sign_loss)]
            let index = (durations_ns.len() as f64 * 0.99) as usize;
            durations_ns
                .get(index.min(durations_ns.len() - 1))
                .copied()
                .unwrap_or(0)
        } else {
            0
        };

        let throughput_ops_per_sec = if total_duration.as_secs_f64() > 0.0 {
            session.iterations as f64 / total_duration.as_secs_f64()
        } else {
            0.0
        };

        // Calculate average resource usage
        let avg_memory_mb = if !session.resource_snapshots.is_empty() {
            Some(
                session
                    .resource_snapshots
                    .iter()
                    .map(|m| m.memory_used_mb)
                    .sum::<f64>()
                    / session.resource_snapshots.len() as f64,
            )
        } else {
            None
        };

        let avg_cpu_percent = if !session.resource_snapshots.is_empty() {
            Some(
                session
                    .resource_snapshots
                    .iter()
                    .map(|m| m.cpu_percent)
                    .sum::<f64>()
                    / session.resource_snapshots.len() as f64,
            )
        } else {
            None
        };

        BenchmarkResults {
            test_name: session.name,
            iterations: session.iterations,
            total_duration,
            avg_duration_ns,
            min_duration_ns,
            max_duration_ns,
            percentile_95_ns,
            percentile_99_ns,
            throughput_ops_per_sec,
            memory_usage_mb: avg_memory_mb,
            cpu_usage_percent: avg_cpu_percent,
        }
    }

    /// Get all historical benchmark results
    pub async fn get_history(&self) -> Vec<BenchmarkResults> {
        self.history.read().await.clone()
    }

    /// Compare two benchmark results
    pub fn compare_results(
        &self,
        baseline: &BenchmarkResults,
        current: &BenchmarkResults,
    ) -> ComparisonReport {
        let throughput_change = if baseline.throughput_ops_per_sec > 0.0 {
            ((current.throughput_ops_per_sec - baseline.throughput_ops_per_sec)
                / baseline.throughput_ops_per_sec)
                * 100.0
        } else {
            0.0
        };

        let avg_latency_change = if baseline.avg_duration_ns > 0 {
            ((current.avg_duration_ns as f64 - baseline.avg_duration_ns as f64)
                / baseline.avg_duration_ns as f64)
                * 100.0
        } else {
            0.0
        };

        let memory_change = match (baseline.memory_usage_mb, current.memory_usage_mb) {
            (Some(baseline_mem), Some(current_mem)) => {
                Some(((current_mem - baseline_mem) / baseline_mem) * 100.0)
            }
            _ => None,
        };

        ComparisonReport {
            baseline_name: baseline.test_name.clone(),
            current_name: current.test_name.clone(),
            throughput_change_percent: throughput_change,
            avg_latency_change_percent: avg_latency_change,
            memory_change_percent: memory_change,
            is_improvement: throughput_change > 0.0 && avg_latency_change < 0.0,
        }
    }

    /// Export results to JSON
    pub async fn export_results(&self, file_path: &str) -> Result<()> {
        let history = self.get_history().await;
        let json = serde_json::to_string_pretty(&history)?;
        write_file_with_context(Path::new(file_path), &json, "benchmark results").await?;
        Ok(())
    }
}

/// Benchmark comparison report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComparisonReport {
    /// Name of the baseline benchmark.
    pub baseline_name: String,
    /// Name of the current benchmark being compared.
    pub current_name: String,
    /// Percentage change in throughput (positive means improvement).
    pub throughput_change_percent: f64,
    /// Percentage change in average latency (negative means improvement).
    pub avg_latency_change_percent: f64,
    /// Percentage change in memory usage, if both benchmarks reported it.
    pub memory_change_percent: Option<f64>,
    /// Whether the current benchmark shows an overall improvement over the baseline.
    pub is_improvement: bool,
}

impl ResourceMonitor {
    /// Create a new resource monitor with the specified polling interval.
    pub fn new(monitor_interval: Duration) -> Self {
        Self {
            current_metrics: Arc::new(RwLock::new(ResourceMetrics::default())),
            monitor_interval,
            is_monitoring: Arc::new(RwLock::new(false)),
        }
    }

    /// Start resource monitoring
    pub async fn start_monitoring(&self) -> Result<()> {
        let mut is_monitoring = self.is_monitoring.write().await;
        if *is_monitoring {
            return Ok(()); // Already monitoring
        }
        *is_monitoring = true;
        drop(is_monitoring);

        let current_metrics = Arc::clone(&self.current_metrics);
        let is_monitoring_flag = Arc::clone(&self.is_monitoring);
        let interval = self.monitor_interval;

        tokio::spawn(async move {
            let mut interval_timer = tokio::time::interval(interval);

            while *is_monitoring_flag.read().await {
                interval_timer.tick().await;

                let metrics = Self::collect_system_metrics().await;
                *current_metrics.write().await = metrics;
            }
        });

        Ok(())
    }

    /// Stop resource monitoring
    pub async fn stop_monitoring(&self) -> Result<()> {
        *self.is_monitoring.write().await = false;
        Ok(())
    }

    /// Get current resource metrics
    pub async fn get_current_metrics(&self) -> ResourceMetrics {
        self.current_metrics.read().await.clone()
    }

    /// Collect system resource metrics
    async fn collect_system_metrics() -> ResourceMetrics {
        // This is a simplified implementation
        // In a real system, you'd use system APIs or libraries like `sysinfo`

        ResourceMetrics {
            memory_used_mb: Self::get_memory_usage_mb(),
            cpu_percent: Self::get_cpu_usage_percent(),
            network_bytes_sent: 0,
            network_bytes_received: 0,
            disk_reads: 0,
            disk_writes: 0,
        }
    }

    /// Get current memory usage in MB
    fn get_memory_usage_mb() -> f64 {
        // Simplified implementation - would use actual system APIs
        #[cfg(target_os = "linux")]
        {
            if let Ok(contents) = std::fs::read_to_string("/proc/self/status") {
                for line in contents.lines() {
                    if line.starts_with("VmRSS:")
                        && let Some(kb_str) = line.split_whitespace().nth(1)
                        && let Ok(kb) = kb_str.parse::<f64>()
                    {
                        return kb / 1024.0; // Convert KB to MB
                    }
                }
            }
        }

        // Fallback estimation
        100.0
    }

    /// Get current CPU usage percentage
    fn get_cpu_usage_percent() -> f64 {
        // Simplified implementation - would use actual system APIs
        // This would require tracking CPU time over intervals
        0.0
    }
}

/// Macro for easy benchmarking
#[macro_export]
macro_rules! benchmark {
    ($profiler:expr, $name:expr, $code:block) => {{
        let start = std::time::Instant::now();
        let result = $code;
        let duration = start.elapsed();
        $profiler.record_operation($name, duration).await?;
        result
    }};
}

/// Utility functions for common benchmarking scenarios
pub struct BenchmarkUtils;

impl BenchmarkUtils {
    /// Benchmark a function with multiple iterations
    pub async fn benchmark_function<F, R>(
        profiler: &PerformanceProfiler,
        name: &str,
        iterations: u64,
        mut func: F,
    ) -> Result<BenchmarkResults>
    where
        F: FnMut() -> R,
    {
        profiler.start_benchmark(name).await?;

        for _ in 0..iterations {
            let start = Instant::now();
            let _ = func();
            let duration = start.elapsed();
            profiler.record_operation(name, duration).await?;
        }

        profiler.end_benchmark(name).await
    }

    /// Benchmark an async function with multiple iterations
    pub async fn benchmark_async_function<F, Fut, R>(
        profiler: &PerformanceProfiler,
        name: &str,
        iterations: u64,
        mut func: F,
    ) -> Result<BenchmarkResults>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = R>,
    {
        profiler.start_benchmark(name).await?;

        for _ in 0..iterations {
            let start = Instant::now();
            let _ = func().await;
            let duration = start.elapsed();
            profiler.record_operation(name, duration).await?;
        }

        profiler.end_benchmark(name).await
    }

    /// Run a performance regression test
    pub async fn regression_test(
        profiler: &PerformanceProfiler,
        baseline_name: &str,
        current_name: &str,
        max_regression_percent: f64,
    ) -> Result<bool> {
        let history = profiler.get_history().await;

        let baseline = history
            .iter()
            .find(|r| r.test_name == baseline_name)
            .ok_or_else(|| anyhow::anyhow!("Baseline '{baseline_name}' not found"))?;

        let current = history
            .iter()
            .find(|r| r.test_name == current_name)
            .ok_or_else(|| anyhow::anyhow!("Current '{current_name}' not found"))?;

        let comparison = profiler.compare_results(baseline, current);

        // Check if performance regressed beyond threshold
        let regression = comparison.avg_latency_change_percent > max_regression_percent
            || comparison.throughput_change_percent < -max_regression_percent;

        if regression {
            tracing::warn!(
                latency_change_percent = comparison.avg_latency_change_percent,
                throughput_change_percent = comparison.throughput_change_percent,
                "Performance regression detected"
            );
        }

        Ok(!regression)
    }
}

impl Default for PerformanceProfiler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::sleep;

    #[tokio::test]
    async fn test_benchmark_session() -> Result<()> {
        let profiler = PerformanceProfiler::new();

        profiler.start_benchmark("test_session").await?;

        // Simulate some operations
        for i in 0..10 {
            let duration = Duration::from_millis(10 + i);
            profiler.record_operation("test_session", duration).await?;
        }

        let results = profiler.end_benchmark("test_session").await?;

        assert_eq!(results.test_name, "test_session");
        assert_eq!(results.iterations, 10);
        assert!(results.avg_duration_ns > 0);

        Ok(())
    }

    #[tokio::test]
    async fn test_benchmark_utils() -> Result<()> {
        let profiler = PerformanceProfiler::new();

        let results = BenchmarkUtils::benchmark_function(&profiler, "test_function", 100, || {
            // Simulate work
            std::thread::sleep(Duration::from_micros(100));
            42
        })
        .await?;

        assert_eq!(results.iterations, 100);
        assert!(results.throughput_ops_per_sec > 0.0);

        Ok(())
    }

    #[tokio::test]
    async fn test_async_benchmark() -> Result<()> {
        let profiler = PerformanceProfiler::new();

        let results = BenchmarkUtils::benchmark_async_function(
            &profiler,
            "test_async_function",
            50,
            || async {
                sleep(Duration::from_micros(200)).await;
                "result"
            },
        )
        .await?;

        assert_eq!(results.iterations, 50);
        assert!(results.avg_duration_ns > 0);

        Ok(())
    }
}
