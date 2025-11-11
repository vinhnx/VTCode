use std::collections::HashMap;
/// Token computation metrics and profiling
///
/// Provides accurate token counting with profiling to understand
/// where tokens are being used and optimize context window usage.
use std::time::{Duration, Instant};

/// Token counting statistics
#[derive(Debug, Clone)]
pub struct TokenMetrics {
    /// Average chars per token (varies by model and content)
    pub avg_chars_per_token: f64,
    /// Total tokens counted in this session
    pub total_tokens: usize,
    /// Total characters analyzed
    pub total_chars: usize,
    /// Time spent in token counting
    pub total_time: Duration,
    /// Breakdown by content type
    pub by_type: HashMap<String, TokenTypeMetrics>,
}

/// Metrics for a specific token type/content category
#[derive(Debug, Clone)]
pub struct TokenTypeMetrics {
    /// Name of content type (e.g., "code", "docs", "tool_output")
    pub name: String,
    /// Tokens for this type
    pub tokens: usize,
    /// Characters for this type
    pub chars: usize,
    /// Count of messages/sections
    pub count: usize,
    /// Time spent analyzing
    pub time_ms: u64,
}

impl TokenMetrics {
    pub fn new() -> Self {
        Self {
            avg_chars_per_token: 4.0, // Start with Claude/GPT-4 estimate
            total_tokens: 0,
            total_chars: 0,
            total_time: Duration::ZERO,
            by_type: HashMap::new(),
        }
    }

    /// Update metrics with a new measurement
    pub fn record(&mut self, content_type: &str, tokens: usize, chars: usize, elapsed: Duration) {
        self.total_tokens += tokens;
        self.total_chars += chars;
        self.total_time += elapsed;

        // Update running average
        if self.total_chars > 0 {
            self.avg_chars_per_token = (self.total_chars as f64) / (self.total_tokens as f64);
        }

        let entry = self
            .by_type
            .entry(content_type.to_string())
            .or_insert(TokenTypeMetrics {
                name: content_type.to_string(),
                tokens: 0,
                chars: 0,
                count: 0,
                time_ms: 0,
            });

        entry.tokens += tokens;
        entry.chars += chars;
        entry.count += 1;
        entry.time_ms += elapsed.as_millis() as u64;
    }

    /// Get top content types by token usage
    pub fn top_types(&self, limit: usize) -> Vec<&TokenTypeMetrics> {
        let mut types: Vec<_> = self.by_type.values().collect();
        types.sort_by(|a, b| b.tokens.cmp(&a.tokens));
        types.into_iter().take(limit).collect()
    }

    /// Format metrics for display
    pub fn format_summary(&self) -> String {
        let mut output = String::new();

        output.push_str(&format!("📊 Token Metrics Summary\n"));
        output.push_str(&format!("  Total tokens: {}\n", self.total_tokens));
        output.push_str(&format!("  Total chars: {}\n", self.total_chars));
        output.push_str(&format!(
            "  Avg chars/token: {:.2}\n",
            self.avg_chars_per_token
        ));
        output.push_str(&format!(
            "  Total time: {:.2}ms\n",
            self.total_time.as_secs_f64() * 1000.0
        ));

        if !self.by_type.is_empty() {
            output.push_str("\n  By Type:\n");
            for metric in self.top_types(5) {
                output.push_str(&format!(
                    "    {}: {} tokens ({} chars, {} ms)\n",
                    metric.name, metric.tokens, metric.chars, metric.time_ms
                ));
            }
        }

        output
    }
}

impl Default for TokenMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Token counter with profiling
pub struct TokenCounter {
    metrics: TokenMetrics,
}

impl TokenCounter {
    pub fn new() -> Self {
        Self {
            metrics: TokenMetrics::new(),
        }
    }

    /// Count tokens in text with profiling
    pub fn count_with_profiling(&mut self, content_type: &str, text: &str) -> usize {
        let start = Instant::now();
        let chars = text.len();

        // Use improved token counting:
        // - Code typically 3-4 chars/token (more punctuation/symbols)
        // - Documentation 4.5-5 chars/token (more words)
        // - JSON/tool output 3-4 chars/token (mixed)
        let estimated_tokens = match content_type {
            "code" | "command" => (chars as f64 / 3.5) as usize,
            "docs" | "documentation" | "markdown" => (chars as f64 / 4.5) as usize,
            "json" | "tool_output" => (chars as f64 / 3.8) as usize,
            "conversation" | "message" => (chars as f64 / 4.0) as usize,
            _ => (chars as f64 / self.metrics.avg_chars_per_token) as usize,
        };

        let elapsed = start.elapsed();
        self.metrics
            .record(content_type, estimated_tokens.max(1), chars, elapsed);

        estimated_tokens.max(1)
    }

    /// Count tokens in batch
    pub fn count_batch(&mut self, items: Vec<(String, String)>) -> HashMap<String, usize> {
        let mut results = HashMap::new();
        for (content_type, text) in items {
            let tokens = self.count_with_profiling(&content_type, &text);
            results.insert(content_type, tokens);
        }
        results
    }

    /// Get current metrics
    pub fn metrics(&self) -> &TokenMetrics {
        &self.metrics
    }

    /// Get mutable metrics (for session state integration)
    pub fn metrics_mut(&mut self) -> &mut TokenMetrics {
        &mut self.metrics
    }

    /// Reset metrics
    pub fn reset(&mut self) {
        self.metrics = TokenMetrics::new();
    }
}

impl Default for TokenCounter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_creates_metrics() {
        let metrics = TokenMetrics::new();
        assert_eq!(metrics.total_tokens, 0);
        assert_eq!(metrics.total_chars, 0);
    }

    #[test]
    fn test_records_measurement() {
        let mut metrics = TokenMetrics::new();
        metrics.record("code", 100, 350, Duration::from_millis(5));

        assert_eq!(metrics.total_tokens, 100);
        assert_eq!(metrics.total_chars, 350);
        assert!((metrics.avg_chars_per_token - 3.5).abs() < 0.1);
    }

    #[test]
    fn test_counts_code_tokens() {
        let mut counter = TokenCounter::new();
        let code = "fn main() { println!(\"hello\"); }";

        let tokens = counter.count_with_profiling("code", code);

        // Code is ~35 chars, at 3.5 chars/token = ~10 tokens
        assert!(tokens >= 8 && tokens <= 12);
    }

    #[test]
    fn test_counts_documentation_tokens() {
        let mut counter = TokenCounter::new();
        let docs = "This is a long piece of documentation about the system.";

        let tokens = counter.count_with_profiling("docs", docs);

        // Docs are ~55 chars, at 4.5 chars/token = ~12 tokens
        assert!(tokens >= 10 && tokens <= 15);
    }

    #[test]
    fn test_batch_counting() {
        let mut counter = TokenCounter::new();
        let items = vec![
            ("code".to_string(), "fn test() {}".to_string()),
            ("docs".to_string(), "Documentation text".to_string()),
        ];

        let results = counter.count_batch(items);
        assert_eq!(results.len(), 2);
        assert!(results["code"] > 0);
        assert!(results["docs"] > 0);
    }

    #[test]
    fn test_updates_running_average() {
        let mut counter = TokenCounter::new();

        // First measurement: 100 tokens from 400 chars = 4 chars/token
        counter.count_with_profiling("code", "a".repeat(400).as_str());
        assert_eq!(counter.metrics().total_tokens, 100);

        // Second measurement: should update average
        counter.count_with_profiling("docs", "b".repeat(450).as_str());
        assert_eq!(counter.metrics().total_tokens, 200);
        assert!(counter.metrics().avg_chars_per_token > 2.0);
    }

    #[test]
    fn test_top_types() {
        let mut metrics = TokenMetrics::new();
        metrics.record("code", 1000, 3500, Duration::from_millis(10));
        metrics.record("docs", 200, 900, Duration::from_millis(5));
        metrics.record("json", 300, 1200, Duration::from_millis(6));

        let top = metrics.top_types(2);
        assert_eq!(top.len(), 2);
        assert_eq!(top[0].name, "code");
        assert_eq!(top[1].name, "json");
    }

    #[test]
    fn test_formats_summary() {
        let mut metrics = TokenMetrics::new();
        metrics.record("code", 100, 350, Duration::from_millis(5));

        let summary = metrics.format_summary();
        assert!(summary.contains("Total tokens: 100"));
        assert!(summary.contains("code"));
    }

    #[test]
    fn test_reset() {
        let mut counter = TokenCounter::new();
        counter.count_with_profiling("code", "test");
        assert!(counter.metrics().total_tokens > 0);

        counter.reset();
        assert_eq!(counter.metrics().total_tokens, 0);
        assert_eq!(counter.metrics().total_chars, 0);
    }

    #[test]
    fn test_minimum_token_count() {
        let mut counter = TokenCounter::new();
        // Even empty or tiny content should count as 1 token
        let tokens = counter.count_with_profiling("code", "a");
        assert!(tokens >= 1);
    }
}
