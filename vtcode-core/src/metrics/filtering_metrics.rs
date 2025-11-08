use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilteringMetrics {
    pub total_operations: u64,
    pub total_input_bytes: u64,
    pub total_output_bytes: u64,
    pub operation_distribution: HashMap<String, u64>,
    pub recent_operations: VecDeque<FilteringRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilteringRecord {
    pub operation: String,
    pub input_size_bytes: u64,
    pub output_size_bytes: u64,
    pub duration_ms: u64,
    pub timestamp: DateTime<Utc>,
}

impl FilteringMetrics {
    pub fn new() -> Self {
        Self {
            total_operations: 0,
            total_input_bytes: 0,
            total_output_bytes: 0,
            operation_distribution: HashMap::new(),
            recent_operations: VecDeque::with_capacity(100),
        }
    }

    pub fn record_operation(
        &mut self,
        operation: String,
        input_size: u64,
        output_size: u64,
        duration_ms: u64,
    ) {
        self.total_operations += 1;
        self.total_input_bytes += input_size;
        self.total_output_bytes += output_size;

        *self.operation_distribution.entry(operation.clone()).or_insert(0) += 1;

        let record = FilteringRecord {
            operation,
            input_size_bytes: input_size,
            output_size_bytes: output_size,
            duration_ms,
            timestamp: Utc::now(),
        };

        if self.recent_operations.len() >= 100 {
            self.recent_operations.pop_front();
        }
        self.recent_operations.push_back(record);
    }

    pub fn avg_reduction_ratio(&self) -> f64 {
        if self.total_input_bytes > 0 {
            1.0 - (self.total_output_bytes as f64 / self.total_input_bytes as f64)
        } else {
            0.0
        }
    }

    /// Estimate tokens saved based on rough conversion (1 token ≈ 4 bytes)
    pub fn estimated_tokens_saved(&self) -> u64 {
        let bytes_saved = self.total_input_bytes.saturating_sub(self.total_output_bytes);
        bytes_saved / 4  // Rough estimate
    }

    pub fn avg_duration_ms(&self) -> u64 {
        if self.total_operations > 0 {
            let total: u64 = self.recent_operations.iter().map(|r| r.duration_ms).sum();
            total / self.total_operations.min(self.recent_operations.len() as u64)
        } else {
            0
        }
    }
}

impl Default for FilteringMetrics {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_operation() {
        let mut metrics = FilteringMetrics::new();
        metrics.record_operation("filter".to_string(), 1000, 500, 100);

        assert_eq!(metrics.total_operations, 1);
        assert_eq!(metrics.total_input_bytes, 1000);
        assert_eq!(metrics.total_output_bytes, 500);
    }

    #[test]
    fn test_reduction_ratio() {
        let mut metrics = FilteringMetrics::new();
        metrics.record_operation("filter".to_string(), 1000, 500, 100);

        let ratio = metrics.avg_reduction_ratio();
        assert!((ratio - 0.5).abs() < 0.01);  // 50% reduction
    }

    #[test]
    fn test_tokens_saved() {
        let mut metrics = FilteringMetrics::new();
        metrics.record_operation("filter".to_string(), 1000, 500, 100);

        let tokens = metrics.estimated_tokens_saved();
        assert_eq!(tokens, 125);  // (1000 - 500) / 4
    }

    #[test]
    fn test_operation_distribution() {
        let mut metrics = FilteringMetrics::new();
        metrics.record_operation("filter".to_string(), 1000, 500, 100);
        metrics.record_operation("map".to_string(), 500, 400, 50);
        metrics.record_operation("filter".to_string(), 800, 400, 80);

        assert_eq!(metrics.operation_distribution.get("filter"), Some(&2));
        assert_eq!(metrics.operation_distribution.get("map"), Some(&1));
    }

    #[test]
    fn test_recent_operations_limit() {
        let mut metrics = FilteringMetrics::new();
        for i in 0..150 {
            metrics.record_operation(
                format!("op_{}", i % 5),
                1000 + i as u64,
                500 + i as u64,
                50,
            );
        }

        assert_eq!(metrics.total_operations, 150);
        assert_eq!(metrics.recent_operations.len(), 100);
    }
}
