//! Performance Metrics and Telemetry
//!
//! This module tracks command execution performance, cache efficiency,
//! and resource usage for optimization and monitoring.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Single metric data point
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct MetricPoint {
    /// Metric name (e.g., "command.execute", "cache.hit")
    pub name: String,
    /// Measured value (duration in ms, count, bytes, etc.)
    pub value: f64,
    /// Unit of measurement
    pub unit: String,
    /// When this metric was recorded
    pub timestamp: Instant,
}

/// Aggregated statistics for a metric
#[derive(Debug, Clone)]
pub struct MetricStats {
    pub name: String,
    pub count: usize,
    pub total: f64,
    pub min: f64,
    pub max: f64,
    pub avg: f64,
}

impl MetricStats {
    fn calculate(name: String, values: &[f64]) -> Self {
        if values.is_empty() {
            return Self {
                name,
                count: 0,
                total: 0.0,
                min: 0.0,
                max: 0.0,
                avg: 0.0,
            };
        }

        let total: f64 = values.iter().sum();
        let min = values.iter().cloned().fold(f64::INFINITY, f64::min);
        let max = values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let avg = total / values.len() as f64;

        Self {
            name,
            count: values.len(),
            total,
            min,
            max,
            avg,
        }
    }

    pub fn format(&self) -> String {
        format!(
            "{}: count={}, avg={:.2}ms, min={:.2}ms, max={:.2}ms, total={:.2}ms",
            self.name, self.count, self.avg, self.min, self.max, self.total
        )
    }
}

/// Command execution timer
pub struct CommandTimer {
    name: String,
    start: Instant,
    metrics: Arc<Mutex<MetricsCollector>>,
}

impl CommandTimer {
    pub fn new(name: impl Into<String>, metrics: Arc<Mutex<MetricsCollector>>) -> Self {
        Self {
            name: name.into(),
            start: Instant::now(),
            metrics,
        }
    }

    pub fn elapsed_ms(&self) -> f64 {
        self.start.elapsed().as_secs_f64() * 1000.0
    }
}

impl Drop for CommandTimer {
    fn drop(&mut self) {
        let elapsed_ms = self.elapsed_ms();
        if let Ok(mut metrics) = self.metrics.lock() {
            metrics.record_duration(&self.name, Duration::from_secs_f64(elapsed_ms / 1000.0));
        }
    }
}

/// Central metrics collection system
#[derive(Debug)]
pub struct MetricsCollector {
    /// Recorded metrics organized by name
    metrics: HashMap<String, Vec<f64>>,
    /// Counter metrics
    counters: HashMap<String, usize>,
    /// Memory tracking (bytes)
    memory_usage: HashMap<String, usize>,
    /// Max capacity before pruning old entries
    max_entries_per_metric: usize,
}

impl MetricsCollector {
    pub fn new() -> Self {
        Self {
            metrics: HashMap::new(),
            counters: HashMap::new(),
            memory_usage: HashMap::new(),
            max_entries_per_metric: 1000,
        }
    }

    /// Record a duration measurement
    pub fn record_duration(&mut self, name: &str, duration: Duration) {
        let ms = duration.as_secs_f64() * 1000.0;
        self.metrics.entry(name.to_string()).or_default().push(ms);

        // Prune old entries if too many
        if let Some(values) = self.metrics.get_mut(name) {
            if values.len() > self.max_entries_per_metric {
                values.drain(0..values.len() / 2);
            }
        }
    }

    /// Record a counter increment
    pub fn increment_counter(&mut self, name: &str, amount: usize) {
        *self.counters.entry(name.to_string()).or_insert(0) += amount;
    }

    /// Record memory usage
    pub fn record_memory(&mut self, name: &str, bytes: usize) {
        self.memory_usage.insert(name.to_string(), bytes);
    }

    /// Get statistics for a metric
    pub fn get_stats(&self, name: &str) -> Option<MetricStats> {
        self.metrics
            .get(name)
            .map(|values| MetricStats::calculate(name.to_string(), values))
    }

    /// Get all statistics
    pub fn all_stats(&self) -> Vec<MetricStats> {
        self.metrics
            .iter()
            .map(|(name, values)| MetricStats::calculate(name.clone(), values))
            .collect()
    }

    /// Get counter value
    pub fn get_counter(&self, name: &str) -> usize {
        *self.counters.get(name).unwrap_or(&0)
    }

    /// Get all counters
    pub fn all_counters(&self) -> &HashMap<String, usize> {
        &self.counters
    }

    /// Get memory usage for a component
    pub fn get_memory(&self, name: &str) -> usize {
        *self.memory_usage.get(name).unwrap_or(&0)
    }

    /// Total memory usage across all components
    pub fn total_memory(&self) -> usize {
        self.memory_usage.values().sum()
    }

    /// Generate a metrics report
    pub fn report(&self) -> String {
        let mut report = String::from("=== Performance Metrics ===\n\n");

        // Durations
        report.push_str("Execution Times:\n");
        for stats in self.all_stats() {
            report.push_str(&format!("  {}\n", stats.format()));
        }

        // Counters
        if !self.counters.is_empty() {
            report.push_str("\nCounters:\n");
            for (name, count) in &self.counters {
                report.push_str(&format!("  {}: {}\n", name, count));
            }
        }

        // Memory
        if !self.memory_usage.is_empty() {
            report.push_str("\nMemory Usage:\n");
            for (name, bytes) in &self.memory_usage {
                let mb = *bytes as f64 / 1024.0 / 1024.0;
                report.push_str(&format!("  {}: {:.2}MB\n", name, mb));
            }
            let total_mb = self.total_memory() as f64 / 1024.0 / 1024.0;
            report.push_str(&format!("  Total: {:.2}MB\n", total_mb));
        }

        report
    }

    pub fn reset(&mut self) {
        self.metrics.clear();
        self.counters.clear();
        self.memory_usage.clear();
    }
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_metrics_collector_creation() {
        let collector = MetricsCollector::new();
        assert_eq!(collector.get_counter("test"), 0);
    }

    #[test]
    fn test_record_duration() {
        let mut collector = MetricsCollector::new();
        collector.record_duration("test_command", Duration::from_millis(100));

        let stats = collector.get_stats("test_command").unwrap();
        assert_eq!(stats.count, 1);
        assert!(stats.avg >= 100.0 && stats.avg < 150.0);
    }

    #[test]
    fn test_multiple_durations() {
        let mut collector = MetricsCollector::new();
        collector.record_duration("test", Duration::from_millis(50));
        collector.record_duration("test", Duration::from_millis(100));
        collector.record_duration("test", Duration::from_millis(150));

        let stats = collector.get_stats("test").unwrap();
        assert_eq!(stats.count, 3);
        assert_eq!(stats.min, 50.0);
        assert_eq!(stats.max, 150.0);
    }

    #[test]
    fn test_increment_counter() {
        let mut collector = MetricsCollector::new();
        collector.increment_counter("cache_hits", 5);
        collector.increment_counter("cache_hits", 3);
        assert_eq!(collector.get_counter("cache_hits"), 8);
    }

    #[test]
    fn test_record_memory() {
        let mut collector = MetricsCollector::new();
        collector.record_memory("workspace_cache", 1024 * 1024);
        assert_eq!(collector.get_memory("workspace_cache"), 1024 * 1024);
    }

    #[test]
    fn test_command_timer() {
        let collector = Arc::new(Mutex::new(MetricsCollector::new()));
        {
            let _timer = CommandTimer::new("test_cmd", Arc::clone(&collector));
            thread::sleep(Duration::from_millis(10));
        }

        let collector = collector.lock().unwrap();
        let stats = collector.get_stats("test_cmd").unwrap();
        assert!(stats.avg >= 10.0);
    }

    #[test]
    fn test_metrics_report() {
        let mut collector = MetricsCollector::new();
        collector.record_duration("cmd1", Duration::from_millis(100));
        collector.increment_counter("test_count", 42);
        collector.record_memory("test_mem", 2048);

        let report = collector.report();
        assert!(report.contains("Execution Times"));
        assert!(report.contains("cmd1"));
        assert!(report.contains("Counters"));
        assert!(report.contains("test_count: 42"));
        assert!(report.contains("Memory Usage"));
    }

    #[test]
    fn test_metrics_reset() {
        let mut collector = MetricsCollector::new();
        collector.record_duration("test", Duration::from_millis(100));
        collector.increment_counter("count", 5);

        collector.reset();
        assert_eq!(collector.get_counter("count"), 0);
        assert!(collector.get_stats("test").is_none());
    }

    #[test]
    fn test_metric_stats_format() {
        let stats = MetricStats {
            name: "test_metric".to_string(),
            count: 10,
            total: 1000.0,
            min: 50.0,
            max: 200.0,
            avg: 100.0,
        };

        let formatted = stats.format();
        assert!(formatted.contains("test_metric"));
        assert!(formatted.contains("count=10"));
        assert!(formatted.contains("avg=100.00ms"));
    }
}
