//! Memory pressure classification system

use std::fmt;

/// Memory pressure levels
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryPressure {
    /// Normal memory usage (< 400 MB)
    Normal,
    /// Warning level (400-600 MB) - reduce TTL, trigger selective eviction
    Warning,
    /// Critical level (>= 600 MB) - aggressive eviction, clear caches
    Critical,
}

impl MemoryPressure {
    /// Classify memory pressure based on RSS bytes
    pub fn from_rss(rss_bytes: usize) -> Self {
        let soft_limit = vtcode_config::constants::memory::SOFT_LIMIT_BYTES;
        let hard_limit = vtcode_config::constants::memory::HARD_LIMIT_BYTES;

        if rss_bytes >= hard_limit {
            MemoryPressure::Critical
        } else if rss_bytes >= soft_limit {
            MemoryPressure::Warning
        } else {
            MemoryPressure::Normal
        }
    }

    /// Check if eviction should be triggered
    pub fn should_evict(&self) -> bool {
        matches!(self, MemoryPressure::Warning | MemoryPressure::Critical)
    }

    /// Check if aggressive eviction should be triggered
    pub fn should_evict_aggressively(&self) -> bool {
        matches!(self, MemoryPressure::Critical)
    }

    /// Get human-readable description
    pub fn description(&self) -> &'static str {
        match self {
            MemoryPressure::Normal => "Normal (memory usage is healthy)",
            MemoryPressure::Warning => "Warning (approaching soft limit, consider cleanup)",
            MemoryPressure::Critical => "Critical (at hard limit, aggressive cleanup recommended)",
        }
    }

    /// Get TTL reduction factor for cache management
    pub fn ttl_reduction_factor(&self) -> f64 {
        match self {
            MemoryPressure::Normal => 1.0,
            MemoryPressure::Warning => {
                vtcode_config::constants::memory::WARNING_TTL_REDUCTION_FACTOR
            }
            MemoryPressure::Critical => {
                vtcode_config::constants::memory::CRITICAL_TTL_REDUCTION_FACTOR
            }
        }
    }
}

impl fmt::Display for MemoryPressure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MemoryPressure::Normal => write!(f, "Normal"),
            MemoryPressure::Warning => write!(f, "Warning"),
            MemoryPressure::Critical => write!(f, "Critical"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_pressure_from_rss_normal() {
        let soft_limit = vtcode_config::constants::memory::SOFT_LIMIT_BYTES;
        let pressure = MemoryPressure::from_rss(soft_limit - 1024); // Just under soft limit
        assert_eq!(pressure, MemoryPressure::Normal);
    }

    #[test]
    fn test_memory_pressure_from_rss_warning() {
        let soft_limit = vtcode_config::constants::memory::SOFT_LIMIT_BYTES;
        let hard_limit = vtcode_config::constants::memory::HARD_LIMIT_BYTES;
        let mid_point = (soft_limit + hard_limit) / 2;
        let pressure = MemoryPressure::from_rss(mid_point);
        assert_eq!(pressure, MemoryPressure::Warning);
    }

    #[test]
    fn test_memory_pressure_from_rss_critical() {
        let hard_limit = vtcode_config::constants::memory::HARD_LIMIT_BYTES;
        let pressure = MemoryPressure::from_rss(hard_limit + 1024); // Just over hard limit
        assert_eq!(pressure, MemoryPressure::Critical);
    }

    #[test]
    fn test_should_evict() {
        assert!(!MemoryPressure::Normal.should_evict());
        assert!(MemoryPressure::Warning.should_evict());
        assert!(MemoryPressure::Critical.should_evict());
    }

    #[test]
    fn test_should_evict_aggressively() {
        assert!(!MemoryPressure::Normal.should_evict_aggressively());
        assert!(!MemoryPressure::Warning.should_evict_aggressively());
        assert!(MemoryPressure::Critical.should_evict_aggressively());
    }

    #[test]
    fn test_ttl_reduction_factors() {
        let normal_factor = MemoryPressure::Normal.ttl_reduction_factor();
        let warning_factor = MemoryPressure::Warning.ttl_reduction_factor();
        let critical_factor = MemoryPressure::Critical.ttl_reduction_factor();

        // Factors should be in decreasing order
        assert!(normal_factor > warning_factor);
        assert!(warning_factor > critical_factor);

        // Values should be between 0 and 1
        assert!(normal_factor <= 1.0);
        assert!(critical_factor > 0.0);
    }

    #[test]
    fn test_display() {
        assert_eq!(format!("{}", MemoryPressure::Normal), "Normal");
        assert_eq!(format!("{}", MemoryPressure::Warning), "Warning");
        assert_eq!(format!("{}", MemoryPressure::Critical), "Critical");
    }
}
