//! Command resolution system
//! Maps command names to their actual filesystem paths
//! Used by policy evaluator to validate and log command locations

use std::collections::HashMap;
use std::path::PathBuf;
use tracing::{debug, warn};

/// Result of attempting to resolve a command to a filesystem path
#[derive(Debug, Clone)]
pub struct CommandResolution {
    /// The original command name (e.g., "cargo")
    pub command: String,

    /// Full path if found in system PATH (e.g., "/Users/user/.cargo/bin/cargo")
    pub resolved_path: Option<PathBuf>,

    /// Whether command was found in system PATH
    pub found: bool,

    /// Environment used for resolution
    pub search_paths: Vec<PathBuf>,
}

/// Resolver with built-in caching to avoid repeated PATH searches
pub struct CommandResolver {
    /// Cache of already-resolved commands
    cache: HashMap<String, CommandResolution>,

    /// Cache hit count for metrics
    cache_hits: usize,

    /// Cache miss count for metrics  
    cache_misses: usize,
}

impl CommandResolver {
    /// Create a new resolver with empty cache
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
            cache_hits: 0,
            cache_misses: 0,
        }
    }

    /// Resolve a command to its filesystem path
    ///
    /// # Example
    /// ```no_run
    /// let mut resolver = CommandResolver::new();
    /// let cargo = resolver.resolve("cargo");
    /// assert_eq!(cargo.command, "cargo");
    /// assert!(cargo.found);
    /// assert_eq!(cargo.resolved_path, Some("/Users/user/.cargo/bin/cargo".into()));
    /// ```
    pub fn resolve(&mut self, cmd: &str) -> CommandResolution {
        // Extract base command (first word only)
        let base_cmd = cmd.split_whitespace().next().unwrap_or(cmd);

        // Check cache first
        if let Some(cached) = self.cache.get(base_cmd) {
            self.cache_hits += 1;
            debug!(
                command = base_cmd,
                cache_hits = self.cache_hits,
                "Command resolution cache hit"
            );
            return cached.clone();
        }

        self.cache_misses += 1;

        // Try to find command in system PATH
        let resolution = if let Ok(path) = which::which(base_cmd) {
            CommandResolution {
                command: base_cmd.to_string(),
                resolved_path: Some(path.clone()),
                found: true,
                search_paths: Self::get_search_paths(),
            }
        } else {
            warn!(command = base_cmd, "Command not found in PATH");
            CommandResolution {
                command: base_cmd.to_string(),
                resolved_path: None,
                found: false,
                search_paths: Self::get_search_paths(),
            }
        };

        // Cache the result
        self.cache.insert(base_cmd.to_string(), resolution.clone());
        resolution
    }

    /// Get current PATH directories being searched
    fn get_search_paths() -> Vec<PathBuf> {
        std::env::var_os("PATH")
            .map(|paths| std::env::split_paths(&paths).collect())
            .unwrap_or_default()
    }

    /// Clear the resolution cache
    pub fn clear_cache(&mut self) {
        self.cache.clear();
        debug!("Command resolver cache cleared");
    }

    /// Get cache statistics
    pub fn cache_stats(&self) -> (usize, usize) {
        (self.cache_hits, self.cache_misses)
    }
}

impl Default for CommandResolver {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_common_command() {
        let mut resolver = CommandResolver::new();
        let ls = resolver.resolve("ls");
        assert_eq!(ls.command, "ls");
        // ls should be found on any Unix system
        assert!(ls.found);
    }

    #[test]
    fn test_cache_hits() {
        let mut resolver = CommandResolver::new();
        resolver.resolve("ls");
        resolver.resolve("ls");
        let (hits, misses) = resolver.cache_stats();
        assert_eq!(hits, 1);
        assert_eq!(misses, 1);
    }

    #[test]
    fn test_nonexistent_command() {
        let mut resolver = CommandResolver::new();
        let fake = resolver.resolve("this_command_definitely_does_not_exist_xyz");
        assert_eq!(fake.command, "this_command_definitely_does_not_exist_xyz");
        assert!(!fake.found);
    }

    #[test]
    fn test_extract_base_command() {
        let mut resolver = CommandResolver::new();
        // Should extract "cargo" from "cargo fmt"
        let resolution = resolver.resolve("cargo fmt --check");
        assert_eq!(resolution.command, "cargo");
    }
}
