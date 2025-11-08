use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SdkMetrics {
    pub total_generations: u64,
    pub total_generation_time_ms: u64,
    pub cache_hits: u64,
    pub tools_total_generated: u64,
    pub max_tools_per_generation: u64,
}

impl SdkMetrics {
    pub fn new() -> Self {
        Self {
            total_generations: 0,
            total_generation_time_ms: 0,
            cache_hits: 0,
            tools_total_generated: 0,
            max_tools_per_generation: 0,
        }
    }

    pub fn record_generation(&mut self, generation_time_ms: u64, tools_count: u64) {
        self.total_generations += 1;
        self.total_generation_time_ms += generation_time_ms;
        self.tools_total_generated += tools_count;
        if tools_count > self.max_tools_per_generation {
            self.max_tools_per_generation = tools_count;
        }
    }

    pub fn record_cache_hit(&mut self) {
        self.cache_hits += 1;
    }

    pub fn avg_generation_time_ms(&self) -> u64 {
        if self.total_generations > 0 {
            self.total_generation_time_ms / self.total_generations
        } else {
            0
        }
    }

    pub fn avg_tools_per_generation(&self) -> u64 {
        if self.total_generations > 0 {
            self.tools_total_generated / self.total_generations
        } else {
            0
        }
    }

    pub fn cache_hit_rate(&self) -> f64 {
        if self.total_generations > 0 {
            self.cache_hits as f64 / self.total_generations as f64
        } else {
            0.0
        }
    }
}

impl Default for SdkMetrics {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_generation() {
        let mut metrics = SdkMetrics::new();
        metrics.record_generation(50, 10);

        assert_eq!(metrics.total_generations, 1);
        assert_eq!(metrics.total_generation_time_ms, 50);
        assert_eq!(metrics.tools_total_generated, 10);
    }

    #[test]
    fn test_avg_generation_time() {
        let mut metrics = SdkMetrics::new();
        metrics.record_generation(50, 10);
        metrics.record_generation(60, 12);
        metrics.record_generation(40, 8);

        assert_eq!(metrics.avg_generation_time_ms(), 50);
    }

    #[test]
    fn test_max_tools() {
        let mut metrics = SdkMetrics::new();
        metrics.record_generation(50, 10);
        metrics.record_generation(60, 25);
        metrics.record_generation(40, 8);

        assert_eq!(metrics.max_tools_per_generation, 25);
    }

    #[test]
    fn test_cache_hit_rate() {
        let mut metrics = SdkMetrics::new();
        metrics.record_generation(50, 10);
        metrics.record_generation(60, 12);
        metrics.record_cache_hit();
        metrics.record_cache_hit();

        assert_eq!(metrics.total_generations, 2);
        assert_eq!(metrics.cache_hits, 2);
        assert!(metrics.cache_hit_rate() > 0.0);
    }
}
