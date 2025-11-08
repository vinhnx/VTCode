use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryMetrics {
    pub total_queries: u64,
    pub successful_queries: u64,
    pub failed_queries: u64,
    pub total_time_ms: u64,
    pub cache_hits: u64,
    pub recent_queries: VecDeque<QueryRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryRecord {
    pub keyword: String,
    pub result_count: u64,
    pub response_time_ms: u64,
    pub timestamp: DateTime<Utc>,
    pub success: bool,
}

impl DiscoveryMetrics {
    pub fn new() -> Self {
        Self {
            total_queries: 0,
            successful_queries: 0,
            failed_queries: 0,
            total_time_ms: 0,
            cache_hits: 0,
            recent_queries: VecDeque::with_capacity(100),
        }
    }

    pub fn record_query(&mut self, keyword: String, result_count: u64, response_time_ms: u64) {
        self.total_queries += 1;
        self.successful_queries += 1;
        self.total_time_ms += response_time_ms;

        let record = QueryRecord {
            keyword,
            result_count,
            response_time_ms,
            timestamp: Utc::now(),
            success: true,
        };

        if self.recent_queries.len() >= 100 {
            self.recent_queries.pop_front();
        }
        self.recent_queries.push_back(record);
    }

    pub fn record_failure(&mut self, keyword: String) {
        self.total_queries += 1;
        self.failed_queries += 1;

        let record = QueryRecord {
            keyword,
            result_count: 0,
            response_time_ms: 0,
            timestamp: Utc::now(),
            success: false,
        };

        if self.recent_queries.len() >= 100 {
            self.recent_queries.pop_front();
        }
        self.recent_queries.push_back(record);
    }

    pub fn record_cache_hit(&mut self) {
        self.cache_hits += 1;
    }

    pub fn avg_response_time_ms(&self) -> u64 {
        if self.successful_queries > 0 {
            self.total_time_ms / self.successful_queries
        } else {
            0
        }
    }

    pub fn hit_rate(&self) -> f64 {
        if self.total_queries > 0 {
            self.successful_queries as f64 / self.total_queries as f64
        } else {
            0.0
        }
    }

    pub fn cache_hit_rate(&self) -> f64 {
        if self.total_queries > 0 {
            self.cache_hits as f64 / self.total_queries as f64
        } else {
            0.0
        }
    }
}

impl Default for DiscoveryMetrics {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_query() {
        let mut metrics = DiscoveryMetrics::new();
        metrics.record_query("file".to_string(), 5, 50);

        assert_eq!(metrics.total_queries, 1);
        assert_eq!(metrics.successful_queries, 1);
        assert_eq!(metrics.total_time_ms, 50);
        assert_eq!(metrics.avg_response_time_ms(), 50);
    }

    #[test]
    fn test_record_failure() {
        let mut metrics = DiscoveryMetrics::new();
        metrics.record_failure("invalid".to_string());

        assert_eq!(metrics.total_queries, 1);
        assert_eq!(metrics.failed_queries, 1);
        assert_eq!(metrics.hit_rate(), 0.0);
    }

    #[test]
    fn test_hit_rate() {
        let mut metrics = DiscoveryMetrics::new();
        metrics.record_query("test".to_string(), 3, 30);
        metrics.record_query("test2".to_string(), 2, 25);
        metrics.record_failure("test3".to_string());

        assert_eq!(metrics.total_queries, 3);
        assert_eq!(metrics.hit_rate(), 2.0 / 3.0);
    }

    #[test]
    fn test_cache_hit_rate() {
        let mut metrics = DiscoveryMetrics::new();
        metrics.record_query("test".to_string(), 3, 30);
        metrics.record_cache_hit();
        metrics.record_cache_hit();

        assert_eq!(metrics.cache_hits, 2);
        assert!(metrics.cache_hit_rate() > 0.0);
    }

    #[test]
    fn test_recent_queries_limit() {
        let mut metrics = DiscoveryMetrics::new();
        for i in 0..150 {
            metrics.record_query(format!("query_{}", i), 1, 10);
        }

        assert_eq!(metrics.total_queries, 150);
        assert_eq!(metrics.recent_queries.len(), 100);
    }
}
