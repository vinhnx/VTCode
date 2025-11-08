use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionMetrics {
    pub total_executions: u64,
    pub successful_executions: u64,
    pub failed_executions: u64,
    pub timeouts: u64,
    pub total_duration_ms: u64,
    pub memory_peak_mb: u64,
    pub memory_total_mb: u64,
    pub language_distribution: HashMap<String, u64>,
    pub recent_executions: VecDeque<ExecutionRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionRecord {
    pub language: String,
    pub duration_ms: u64,
    pub success: bool,
    pub memory_used_mb: u64,
    pub timestamp: DateTime<Utc>,
}

impl ExecutionMetrics {
    pub fn new() -> Self {
        Self {
            total_executions: 0,
            successful_executions: 0,
            failed_executions: 0,
            timeouts: 0,
            total_duration_ms: 0,
            memory_peak_mb: 0,
            memory_total_mb: 0,
            language_distribution: HashMap::new(),
            recent_executions: VecDeque::with_capacity(100),
        }
    }

    pub fn record_start(&mut self, _language: String) {
        // Placeholder for tracking execution start if needed
    }

    pub fn record_complete(
        &mut self,
        language: String,
        duration_ms: u64,
        memory_mb: u64,
        success: bool,
    ) {
        self.total_executions += 1;
        if success {
            self.successful_executions += 1;
        } else {
            self.failed_executions += 1;
        }

        self.total_duration_ms += duration_ms;
        self.memory_total_mb += memory_mb;
        if memory_mb > self.memory_peak_mb {
            self.memory_peak_mb = memory_mb;
        }

        *self
            .language_distribution
            .entry(language.clone())
            .or_insert(0) += 1;

        let record = ExecutionRecord {
            language,
            duration_ms,
            success,
            memory_used_mb: memory_mb,
            timestamp: Utc::now(),
        };

        if self.recent_executions.len() >= 100 {
            self.recent_executions.pop_front();
        }
        self.recent_executions.push_back(record);
    }

    pub fn record_failure(&mut self, language: String, duration_ms: u64) {
        self.record_complete(language, duration_ms, 0, false);
    }

    pub fn record_timeout(&mut self, language: String, duration_ms: u64) {
        self.timeouts += 1;
        self.record_failure(language, duration_ms);
    }

    pub fn record_result_size(&mut self, _size_bytes: usize) {
        // Can be used to track result sizes for token estimation
    }

    pub fn avg_duration_ms(&self) -> u64 {
        if self.total_executions > 0 {
            self.total_duration_ms / self.total_executions
        } else {
            0
        }
    }

    pub fn avg_memory_mb(&self) -> u64 {
        if self.total_executions > 0 {
            self.memory_total_mb / self.total_executions
        } else {
            0
        }
    }

    pub fn success_rate(&self) -> f64 {
        if self.total_executions > 0 {
            self.successful_executions as f64 / self.total_executions as f64
        } else {
            0.0
        }
    }

    pub fn timeout_rate(&self) -> f64 {
        if self.total_executions > 0 {
            self.timeouts as f64 / self.total_executions as f64
        } else {
            0.0
        }
    }
}

impl Default for ExecutionMetrics {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_complete() {
        let mut metrics = ExecutionMetrics::new();
        metrics.record_complete("python3".to_string(), 1000, 50, true);

        assert_eq!(metrics.total_executions, 1);
        assert_eq!(metrics.successful_executions, 1);
        assert_eq!(metrics.avg_duration_ms(), 1000);
        assert_eq!(metrics.memory_peak_mb, 50);
    }

    #[test]
    fn test_record_failure() {
        let mut metrics = ExecutionMetrics::new();
        metrics.record_failure("javascript".to_string(), 500);

        assert_eq!(metrics.total_executions, 1);
        assert_eq!(metrics.failed_executions, 1);
        assert_eq!(metrics.success_rate(), 0.0);
    }

    #[test]
    fn test_record_timeout() {
        let mut metrics = ExecutionMetrics::new();
        metrics.record_timeout("python3".to_string(), 5000);

        assert_eq!(metrics.timeouts, 1);
        assert_eq!(metrics.timeout_rate(), 1.0);
    }

    #[test]
    fn test_success_rate() {
        let mut metrics = ExecutionMetrics::new();
        metrics.record_complete("python3".to_string(), 100, 40, true);
        metrics.record_complete("python3".to_string(), 100, 42, true);
        metrics.record_failure("python3".to_string(), 100);

        assert_eq!(metrics.total_executions, 3);
        assert_eq!(metrics.success_rate(), 2.0 / 3.0);
    }

    #[test]
    fn test_language_distribution() {
        let mut metrics = ExecutionMetrics::new();
        metrics.record_complete("python3".to_string(), 100, 40, true);
        metrics.record_complete("javascript".to_string(), 150, 45, true);
        metrics.record_complete("python3".to_string(), 120, 42, true);

        assert_eq!(metrics.language_distribution.get("python3"), Some(&2));
        assert_eq!(metrics.language_distribution.get("javascript"), Some(&1));
    }

    #[test]
    fn test_memory_peak() {
        let mut metrics = ExecutionMetrics::new();
        metrics.record_complete("python3".to_string(), 100, 40, true);
        metrics.record_complete("python3".to_string(), 100, 60, true);
        metrics.record_complete("python3".to_string(), 100, 30, true);

        assert_eq!(metrics.memory_peak_mb, 60);
        assert_eq!(metrics.avg_memory_mb(), (40 + 60 + 30) / 3);
    }

    #[test]
    fn test_recent_executions_limit() {
        let mut metrics = ExecutionMetrics::new();
        for _ in 0..150 {
            metrics.record_complete("python3".to_string(), 100, 40, true);
        }

        assert_eq!(metrics.total_executions, 150);
        assert_eq!(metrics.recent_executions.len(), 100);
    }
}
