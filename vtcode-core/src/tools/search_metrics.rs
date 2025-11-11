/// Search operation metrics and optimization
///
/// Tracks token cost of search results to enable intelligent caching,
/// sampling, and summarization of expensive searches in large codebases.
use std::collections::HashMap;

/// Metrics for a single search operation
#[derive(Debug, Clone)]
pub struct SearchMetric {
    /// Search pattern used
    pub pattern: String,
    /// Number of matches found
    pub match_count: usize,
    /// Estimated tokens for results
    pub result_tokens: usize,
    /// Time to execute search
    pub duration_ms: u64,
    /// Files searched
    pub files_searched: usize,
    /// Whether search was expensive (high token cost)
    pub is_expensive: bool,
}

/// Tracks search operation metrics for optimization
#[derive(Debug, Clone)]
pub struct SearchMetrics {
    /// All recorded searches indexed by pattern
    searches: HashMap<String, SearchMetric>,
    /// Total token cost of all searches
    pub total_tokens: usize,
    /// Total searches executed
    pub total_searches: usize,
    /// Token threshold for "expensive" searches (default: 10000)
    expensive_threshold: usize,
}

impl SearchMetrics {
    /// Create new search metrics tracker
    pub fn new() -> Self {
        Self {
            searches: HashMap::new(),
            total_tokens: 0,
            total_searches: 0,
            expensive_threshold: 10000,
        }
    }

    /// Set token threshold for expensive searches
    pub fn with_expensive_threshold(mut self, threshold: usize) -> Self {
        self.expensive_threshold = threshold;
        self
    }

    /// Record a search operation
    pub fn record_search(
        &mut self,
        pattern: &str,
        match_count: usize,
        result_chars: usize,
        duration_ms: u64,
        files_searched: usize,
    ) {
        // Estimate tokens from character count (using default 4.0 chars/token)
        let estimated_tokens = (result_chars as f64 / 4.0).ceil() as usize;
        let is_expensive = estimated_tokens > self.expensive_threshold;

        let metric = SearchMetric {
            pattern: pattern.to_string(),
            match_count,
            result_tokens: estimated_tokens,
            duration_ms,
            files_searched,
            is_expensive,
        };

        self.total_tokens += estimated_tokens;
        self.total_searches += 1;
        self.searches.insert(pattern.to_string(), metric);
    }

    /// Get metric for a specific pattern
    pub fn get_search(&self, pattern: &str) -> Option<&SearchMetric> {
        self.searches.get(pattern)
    }

    /// Find most expensive searches
    pub fn expensive_searches(&self, limit: usize) -> Vec<&SearchMetric> {
        let mut searches: Vec<_> = self.searches.values().filter(|s| s.is_expensive).collect();
        searches.sort_by(|a, b| b.result_tokens.cmp(&a.result_tokens));
        searches.into_iter().take(limit).collect()
    }

    /// Find slowest searches
    pub fn slowest_searches(&self, limit: usize) -> Vec<&SearchMetric> {
        let mut searches: Vec<_> = self.searches.values().collect();
        searches.sort_by(|a, b| b.duration_ms.cmp(&a.duration_ms));
        searches.into_iter().take(limit).collect()
    }

    /// Calculate average tokens per search
    pub fn avg_tokens_per_search(&self) -> f64 {
        if self.total_searches == 0 {
            0.0
        } else {
            self.total_tokens as f64 / self.total_searches as f64
        }
    }

    /// Check if search should be sampled (too many results)
    pub fn should_sample_results(&self, pattern: &str) -> bool {
        self.get_search(pattern)
            .map(|m| m.is_expensive)
            .unwrap_or(false)
    }

    /// Estimate sampling ratio for expensive search
    ///
    /// Returns a value between 0.1 (10% of results) and 1.0 (no sampling)
    pub fn estimate_sampling_ratio(&self, pattern: &str) -> f64 {
        if let Some(metric) = self.get_search(pattern) {
            if !metric.is_expensive {
                return 1.0;
            }

            // Linear interpolation: at threshold = 1.0, at 2x threshold = 0.1
            let ratio = self.expensive_threshold as f64 / metric.result_tokens as f64;
            (ratio * 0.9 + 0.1).min(1.0).max(0.1)
        } else {
            1.0 // No sampling if not tracked
        }
    }

    /// Format metrics for display
    pub fn format_summary(&self) -> String {
        let mut output = String::new();
        output.push_str("🔍 Search Metrics Summary\n");
        output.push_str(&format!("  Total searches: {}\n", self.total_searches));
        output.push_str(&format!("  Total tokens: {}\n", self.total_tokens));
        output.push_str(&format!(
            "  Avg tokens/search: {:.0}\n",
            self.avg_tokens_per_search()
        ));
        output.push_str(&format!(
            "  Expensive searches: {}\n",
            self.searches.values().filter(|s| s.is_expensive).count()
        ));

        let expensive = self.expensive_searches(3);
        if !expensive.is_empty() {
            output.push_str("\n  Most expensive searches:\n");
            for (i, metric) in expensive.iter().enumerate() {
                output.push_str(&format!(
                    "    {}. '{}': {} tokens ({} matches)\n",
                    i + 1,
                    metric.pattern,
                    metric.result_tokens,
                    metric.match_count
                ));
            }
        }

        output
    }

    /// Clear all metrics
    pub fn reset(&mut self) {
        self.searches.clear();
        self.total_tokens = 0;
        self.total_searches = 0;
    }

    /// Get stats for monitoring
    pub fn stats(&self) -> SearchMetricsStats {
        let expensive_count = self.searches.values().filter(|s| s.is_expensive).count();
        SearchMetricsStats {
            total_searches: self.total_searches,
            total_tokens: self.total_tokens,
            expensive_searches: expensive_count,
            avg_tokens_per_search: self.avg_tokens_per_search(),
        }
    }
}

/// Statistics for search metrics
#[derive(Debug, Clone)]
pub struct SearchMetricsStats {
    pub total_searches: usize,
    pub total_tokens: usize,
    pub expensive_searches: usize,
    pub avg_tokens_per_search: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_creates_metrics() {
        let metrics = SearchMetrics::new();
        assert_eq!(metrics.total_searches, 0);
        assert_eq!(metrics.total_tokens, 0);
    }

    #[test]
    fn test_records_search() {
        let mut metrics = SearchMetrics::new();
        metrics.record_search("fn main", 5, 1000, 10, 3);

        assert_eq!(metrics.total_searches, 1);
        assert!(metrics.total_tokens > 0);

        let metric = metrics.get_search("fn main").unwrap();
        assert_eq!(metric.match_count, 5);
        assert_eq!(metric.files_searched, 3);
    }

    #[test]
    fn test_identifies_expensive_searches() {
        let mut metrics = SearchMetrics::with_expensive_threshold(5000).new();
        // This will be expensive (12500 tokens estimated)
        metrics.record_search("common_pattern", 100, 50000, 50, 50);

        let metric = metrics.get_search("common_pattern").unwrap();
        assert!(metric.is_expensive);
    }

    #[test]
    fn test_expensive_searches() {
        let mut metrics = SearchMetrics::with_expensive_threshold(5000).new();
        metrics.record_search("pattern1", 10, 10000, 20, 5);
        metrics.record_search("pattern2", 5, 2000, 10, 2);
        metrics.record_search("pattern3", 50, 30000, 100, 20);

        let expensive = metrics.expensive_searches(2);
        assert_eq!(expensive.len(), 2);
        assert!(expensive[0].result_tokens > expensive[1].result_tokens);
    }

    #[test]
    fn test_slowest_searches() {
        let mut metrics = SearchMetrics::new();
        metrics.record_search("fast", 10, 1000, 5, 2);
        metrics.record_search("slow", 10, 1000, 100, 2);
        metrics.record_search("medium", 10, 1000, 50, 2);

        let slowest = metrics.slowest_searches(2);
        assert_eq!(slowest.len(), 2);
        assert!(slowest[0].duration_ms > slowest[1].duration_ms);
    }

    #[test]
    fn test_sampling_ratio() {
        let mut metrics = SearchMetrics::with_expensive_threshold(10000).new();

        // Non-expensive search should not be sampled
        metrics.record_search("cheap", 10, 5000, 10, 5);
        assert_eq!(metrics.estimate_sampling_ratio("cheap"), 1.0);

        // Expensive search should be sampled
        metrics.record_search("expensive", 100, 50000, 100, 50);
        let ratio = metrics.estimate_sampling_ratio("expensive");
        assert!(ratio < 1.0);
        assert!(ratio >= 0.1);
    }

    #[test]
    fn test_average_tokens() {
        let mut metrics = SearchMetrics::new();
        metrics.record_search("search1", 10, 4000, 10, 5);
        metrics.record_search("search2", 5, 8000, 20, 3);

        let avg = metrics.avg_tokens_per_search();
        assert!(avg > 0.0);
        // Should be approximately (1000 + 2000) / 2 = 1500
    }

    #[test]
    fn test_should_sample_results() {
        let mut metrics = SearchMetrics::with_expensive_threshold(5000).new();
        metrics.record_search("cheap", 10, 2000, 10, 5);
        metrics.record_search("expensive", 100, 50000, 100, 50);

        assert!(!metrics.should_sample_results("cheap"));
        assert!(metrics.should_sample_results("expensive"));
    }

    #[test]
    fn test_format_summary() {
        let mut metrics = SearchMetrics::new();
        metrics.record_search("pattern1", 10, 4000, 10, 5);

        let summary = metrics.format_summary();
        assert!(summary.contains("Search Metrics"));
        assert!(summary.contains("Total searches: 1"));
    }

    #[test]
    fn test_reset() {
        let mut metrics = SearchMetrics::new();
        metrics.record_search("pattern1", 10, 4000, 10, 5);

        metrics.reset();
        assert_eq!(metrics.total_searches, 0);
        assert_eq!(metrics.total_tokens, 0);
        assert!(metrics.get_search("pattern1").is_none());
    }

    #[test]
    fn test_stats() {
        let mut metrics = SearchMetrics::with_expensive_threshold(5000).new();
        metrics.record_search("cheap", 10, 2000, 10, 5);
        metrics.record_search("expensive", 100, 50000, 100, 50);

        let stats = metrics.stats();
        assert_eq!(stats.total_searches, 2);
        assert_eq!(stats.expensive_searches, 1);
        assert!(stats.avg_tokens_per_search > 0.0);
    }
}
