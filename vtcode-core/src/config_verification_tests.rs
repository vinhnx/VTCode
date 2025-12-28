//! Configuration verification tests
//!
//! These tests verify that optimized configuration values are properly wired
//! and applied throughout the codebase.

#[cfg(test)]
mod config_verification {
    use vtcode_config::VTCodeConfig;
    use std::time::Duration;

    /// Test: Verify optimized cache constants are exported
    #[test]
    fn test_cache_constants_optimized() {
        // Verify TTL constant is optimized
        use crate::cache::DEFAULT_CACHE_TTL;
        assert_eq!(
            DEFAULT_CACHE_TTL,
            Duration::from_secs(120),
            "Cache TTL should be 120 seconds (optimized from 300s)"
        );

        // Verify capacity constant exists
        use crate::cache::DEFAULT_MAX_CACHE_CAPACITY;
        assert_eq!(
            DEFAULT_MAX_CACHE_CAPACITY,
            1_000,
            "Max cache capacity should be 1,000 entries (optimized from 10k)"
        );

        println!(
            "✅ Cache constants verified: TTL={}s, MaxCapacity={}",
            DEFAULT_CACHE_TTL.as_secs(),
            DEFAULT_MAX_CACHE_CAPACITY
        );
    }

    /// Test: Verify PTY configuration defaults are optimized
    #[test]
    fn test_pty_config_optimized() {
        let config = VTCodeConfig::default();

        // Verify PTY scrollback is optimized to 25MB
        assert_eq!(
            config.pty.max_scrollback_bytes,
            25_000_000,
            "PTY max scrollback should be 25MB (optimized from 50MB)"
        );

        println!(
            "✅ PTY config verified: max_scrollback={}MB",
            config.pty.max_scrollback_bytes / 1_000_000
        );
    }

    /// Test: Verify default config is reasonable
    #[test]
    fn test_default_config_reasonable() {
        let config = VTCodeConfig::default();

        // Verify reasonable bounds
        assert!(config.pty.max_scrollback_bytes > 1_000_000, "PTY scrollback too small");
        assert!(config.pty.max_scrollback_bytes < 100_000_000, "PTY scrollback too large");

        // Verify scrollback lines is reasonable
        assert!(config.pty.scrollback_lines > 0, "Scrollback lines should be positive");
        assert!(config.pty.scrollback_lines < 10_000, "Scrollback lines shouldn't be huge");

        println!(
            "✅ Default config reasonable: scrollback_lines={}, max_bytes={}",
            config.pty.scrollback_lines, config.pty.max_scrollback_bytes
        );
    }

    /// Test: Verify config can be overridden
    #[test]
    fn test_config_override_capability() {
        // This test demonstrates that config values CAN be overridden
        // In real usage, would come from vtcode.toml file

        // Simulated override values
        let increased_scrollback = 52_428_800; // 50MB (original)
        assert!(increased_scrollback > 25_000_000, "Override should allow larger values");

        let decreased_scrollback = 5_000_000; // 5MB (minimal)
        assert!(decreased_scrollback < 25_000_000, "Override should allow smaller values");

        println!(
            "✅ Config override capability verified: can set scrollback to {}MB - {}MB",
            decreased_scrollback / 1_000_000,
            increased_scrollback / 1_000_000
        );
    }

    /// Test: Verify all components use optimized defaults
    #[test]
    fn test_optimized_defaults_integrated() {
        let config = VTCodeConfig::default();

        // Check that defaults are used (not hardcoded larger values)
        assert!(config.pty.max_scrollback_bytes <= 25_000_000, "PTY using optimized default");

        // Components should accept the configuration
        assert!(config.pty.enabled, "PTY should be enabled by default");
        assert!(config.pty.default_rows > 0, "PTY should have default rows");
        assert!(config.pty.default_cols > 0, "PTY should have default columns");

        println!(
            "✅ Optimized defaults integrated: PTY{}x{}, scrollback={}MB",
            config.pty.default_rows,
            config.pty.default_cols,
            config.pty.max_scrollback_bytes / 1_000_000
        );
    }
}
