//! Cached Style Parser for Performance Optimization
//!
//! This module provides caching for frequently parsed anstyle strings to avoid
//! repeated parsing overhead, especially useful for theme parsing and
//! frequently used style configurations.

use std::collections::HashMap;
use std::sync::RwLock;
use anstyle::Style as AnsiStyle;
use anyhow::{Context, Result};

/// Thread-safe cached parser for Git and LS_COLORS style strings
pub struct CachedStyleParser {
    git_cache: RwLock<HashMap<String, AnsiStyle>>,
    ls_colors_cache: RwLock<HashMap<String, AnsiStyle>>,
}

impl CachedStyleParser {
    /// Create a new cached style parser
    pub fn new() -> Self {
        Self {
            git_cache: RwLock::new(HashMap::new()),
            ls_colors_cache: RwLock::new(HashMap::new()),
        }
    }

    /// Parse and cache a Git-style color string (e.g., "bold red blue")
    pub fn parse_git_style(&self, input: &str) -> Result<AnsiStyle> {
        // Check cache first
        {
            let cache = self.git_cache.read().unwrap();
            if let Some(cached) = cache.get(input) {
                return Ok(*cached);
            }
        }

        // Parse and cache result
        let result = anstyle_git::parse(input)
            .map_err(|e| anyhow::anyhow!("Failed to parse Git style '{}': {:?}", input, e))?;

        {
            let mut cache = self.git_cache.write().unwrap();
            cache.insert(input.to_string(), result);
        }

        Ok(result)
    }

    /// Parse and cache an LS_COLORS-style string (e.g., "01;34")
    pub fn parse_ls_colors(&self, input: &str) -> Result<AnsiStyle> {
        // Check cache first
        {
            let cache = self.ls_colors_cache.read().unwrap();
            if let Some(cached) = cache.get(input) {
                return Ok(*cached);
            }
        }

        // Parse and cache result
        let result = anstyle_ls::parse(input)
            .ok_or_else(|| anyhow::anyhow!("Failed to parse LS_COLORS '{}'", input))?;

        {
            let mut cache = self.ls_colors_cache.write().unwrap();
            cache.insert(input.to_string(), result);
        }

        Ok(result)
    }

    /// Parse using Git syntax first, then LS_COLORS as fallback, with caching
    pub fn parse_flexible(&self, input: &str) -> Result<AnsiStyle> {
        // Try Git syntax first
        match self.parse_git_style(input) {
            Ok(style) => Ok(style),
            Err(_) => {
                // Fall back to LS_COLORS if Git parsing fails
                self.parse_ls_colors(input)
                    .with_context(|| format!("Could not parse style string: '{}'", input))
            }
        }
    }

    /// Clear all cached styles
    pub fn clear_cache(&self) {
        {
            let mut git_cache = self.git_cache.write().unwrap();
            git_cache.clear();
        }
        {
            let mut ls_colors_cache = self.ls_colors_cache.write().unwrap();
            ls_colors_cache.clear();
        }
    }

    /// Get cache statistics
    pub fn cache_stats(&self) -> (usize, usize) {
        let git_count = self.git_cache.read().unwrap().len();
        let ls_colors_count = self.ls_colors_cache.read().unwrap().len();
        (git_count, ls_colors_count)
    }
}

impl Default for CachedStyleParser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_git_style() {
        let parser = CachedStyleParser::new();
        let result = parser.parse_git_style("bold red").unwrap();
        
        assert!(result.get_effects().contains(anstyle::Effects::BOLD));
    }

    #[test]
    fn test_parse_ls_colors() {
        let parser = CachedStyleParser::new();
        let result = parser.parse_ls_colors("34").unwrap(); // Blue
        
        assert!(result.get_fg_color().is_some());
    }

    #[test]
    fn test_parse_flexible_git_first() {
        let parser = CachedStyleParser::new();
        let result = parser.parse_flexible("bold green").unwrap();
        
        assert!(result.get_effects().contains(anstyle::Effects::BOLD));
    }

    #[test]
    fn test_parse_flexible_ls_fallback() {
        let parser = CachedStyleParser::new();
        let result = parser.parse_flexible("01;34").unwrap(); // Bold blue in ANSI codes
        
        assert!(result.get_effects().contains(anstyle::Effects::BOLD));
    }

    #[test]
    fn test_caching_behavior() {
        let parser = CachedStyleParser::new();
        
        // Parse same string twice - should use cache on second call
        let _result1 = parser.parse_git_style("red").unwrap();
        let _result2 = parser.parse_git_style("red").unwrap();
        
        let (git_count, _) = parser.cache_stats();
        assert_eq!(git_count, 1); // Only one cached entry for "red"
    }

    #[test]
    fn test_cache_clear() {
        let parser = CachedStyleParser::new();
        let _result = parser.parse_git_style("blue").unwrap();
        
        assert_eq!(parser.cache_stats().0, 1); // One cached entry
        
        parser.clear_cache();
        
        assert_eq!(parser.cache_stats().0, 0); // Cache cleared
    }

    #[test]
    fn test_multiple_cache_entries() {
        let parser = CachedStyleParser::new();
        let _result1 = parser.parse_git_style("bold red").unwrap();
        let _result2 = parser.parse_git_style("italic green").unwrap();
        let _result3 = parser.parse_ls_colors("34").unwrap();
        
        let (git_count, ls_count) = parser.cache_stats();
        assert_eq!(git_count, 2); // Two Git style entries
        assert_eq!(ls_count, 1);  // One LS_COLORS entry
    }
}