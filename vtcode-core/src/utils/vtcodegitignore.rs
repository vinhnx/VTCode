use anyhow::{Result, anyhow};
use glob::Pattern;
use std::path::{Path, PathBuf};
use tokio::fs;

/// Represents a .vtcodegitignore file with pattern matching capabilities
#[derive(Debug, Clone)]
pub struct VTCodeGitignore {
    /// Root directory where .vtcodegitignore was found
    root_dir: PathBuf,
    /// Compiled glob patterns for matching
    patterns: Vec<CompiledPattern>,
    /// Whether the .vtcodegitignore file exists and was loaded
    loaded: bool,
}

/// A compiled pattern with its original string and compiled glob
#[derive(Debug, Clone)]
struct CompiledPattern {
    /// Original pattern string from the file
    original: String,
    /// Compiled glob pattern
    pattern: Pattern,
    /// Whether this is a negation pattern (starts with !)
    negated: bool,
}

impl VTCodeGitignore {
    /// Create a new VTCodeGitignore instance by looking for .vtcodegitignore in the current directory
    pub async fn new() -> Result<Self> {
        let current_dir = std::env::current_dir()
            .map_err(|e| anyhow!("Failed to get current directory: {}", e))?;

        Self::from_directory(&current_dir).await
    }

    /// Create a VTCodeGitignore instance from a specific directory
    pub async fn from_directory(root_dir: &Path) -> Result<Self> {
        let gitignore_path = root_dir.join(".vtcodegitignore");

        let mut patterns = Vec::new();
        let mut loaded = false;

        if gitignore_path.exists() {
            match Self::load_patterns(&gitignore_path).await {
                Ok(loaded_patterns) => {
                    patterns = loaded_patterns;
                    loaded = true;
                }
                Err(e) => {
                    // Log warning but don't fail - just treat as no patterns
                    tracing::warn!("Failed to load .vtcodegitignore: {}", e);
                }
            }
        }

        Ok(Self {
            root_dir: root_dir.to_path_buf(),
            patterns,
            loaded,
        })
    }

    /// Load patterns from the .vtcodegitignore file
    async fn load_patterns(file_path: &Path) -> Result<Vec<CompiledPattern>> {
        let content = fs::read_to_string(file_path)
            .await
            .map_err(|e| anyhow!("Failed to read .vtcodegitignore: {}", e))?;

        let mut patterns = Vec::new();

        for (line_num, line) in content.lines().enumerate() {
            let line = line.trim();

            // Skip empty lines and comments
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            // Parse the pattern
            let (pattern_str, negated) = if let Some(stripped) = line.strip_prefix('!') {
                (stripped.to_string(), true)
            } else {
                (line.to_string(), false)
            };

            // Convert gitignore patterns to glob patterns
            let glob_pattern = Self::convert_gitignore_to_glob(&pattern_str);

            match Pattern::new(&glob_pattern) {
                Ok(pattern) => {
                    patterns.push(CompiledPattern {
                        original: pattern_str,
                        pattern,
                        negated,
                    });
                }
                Err(e) => {
                    return Err(anyhow!(
                        "Invalid pattern on line {}: '{}': {}",
                        line_num + 1,
                        pattern_str,
                        e
                    ));
                }
            }
        }

        Ok(patterns)
    }

    /// Convert gitignore pattern syntax to glob pattern syntax
    fn convert_gitignore_to_glob(pattern: &str) -> String {
        let mut result = pattern.to_string();

        // Handle directory-only patterns (ending with /)
        if result.ends_with('/') {
            result = format!("{}/**", result.trim_end_matches('/'));
        }

        // Handle patterns that don't start with / or **/
        if !result.starts_with('/') && !result.starts_with("**/") && !result.contains('/') {
            // Simple filename pattern - make it match anywhere
            result = format!("**/{}", result);
        }

        result
    }

    /// Check if a file path should be excluded based on the .vtcodegitignore patterns
    pub fn should_exclude(&self, file_path: &Path) -> bool {
        if !self.loaded || self.patterns.is_empty() {
            return false;
        }

        // Convert to relative path from the root directory
        let relative_path = match file_path.strip_prefix(&self.root_dir) {
            Ok(rel) => rel,
            Err(_) => {
                // If we can't make it relative, use the full path
                file_path
            }
        };

        let path_str = relative_path.to_string_lossy();

        // Default to not excluded
        let mut excluded = false;

        for pattern in &self.patterns {
            if pattern.pattern.matches(&path_str) {
                if pattern.original.ends_with('/') && file_path.is_file() {
                    // Directory-only rules should not exclude individual files.
                    continue;
                }
                if pattern.negated {
                    // Negation pattern - include this file
                    excluded = false;
                } else {
                    // Normal pattern - exclude this file
                    excluded = true;
                }
            }
        }

        excluded
    }

    /// Filter a list of file paths based on .vtcodegitignore patterns
    pub fn filter_paths(&self, paths: Vec<PathBuf>) -> Vec<PathBuf> {
        if !self.loaded {
            return paths;
        }

        paths
            .into_iter()
            .filter(|path| !self.should_exclude(path))
            .collect()
    }

    /// Check if the .vtcodegitignore file was loaded successfully
    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    /// Get the number of patterns loaded
    pub fn pattern_count(&self) -> usize {
        self.patterns.len()
    }

    /// Get the root directory
    pub fn root_dir(&self) -> &Path {
        &self.root_dir
    }
}

impl Default for VTCodeGitignore {
    fn default() -> Self {
        Self {
            root_dir: PathBuf::new(),
            patterns: Vec::new(),
            loaded: false,
        }
    }
}

/// Global .vtcodegitignore instance for easy access
static VTCODE_GITIGNORE: once_cell::sync::Lazy<tokio::sync::RwLock<VTCodeGitignore>> =
    once_cell::sync::Lazy::new(|| tokio::sync::RwLock::new(VTCodeGitignore::default()));

/// Initialize the global .vtcodegitignore instance
pub async fn initialize_vtcode_gitignore() -> Result<()> {
    let gitignore = VTCodeGitignore::new().await?;
    let mut global_gitignore = VTCODE_GITIGNORE.write().await;
    *global_gitignore = gitignore;
    Ok(())
}

/// Get the global .vtcodegitignore instance
pub async fn get_global_vtcode_gitignore() -> tokio::sync::RwLockReadGuard<'static, VTCodeGitignore>
{
    VTCODE_GITIGNORE.read().await
}

/// Check if a file should be excluded by the global .vtcodegitignore
pub async fn should_exclude_file(file_path: &Path) -> bool {
    let gitignore = get_global_vtcode_gitignore().await;
    gitignore.should_exclude(file_path)
}

/// Filter paths using the global .vtcodegitignore
pub async fn filter_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let gitignore = get_global_vtcode_gitignore().await;
    gitignore.filter_paths(paths)
}

/// Reload the global .vtcodegitignore from disk
pub async fn reload_vtcode_gitignore() -> Result<()> {
    initialize_vtcode_gitignore().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::TempDir;

    /// Test the vtcodegitignore functionality in isolation
    #[tokio::test]
    async fn test_vtcodegitignore_integration() {
        // Create a temporary directory for testing
        let temp_dir = TempDir::new().unwrap();
        let gitignore_path = temp_dir.path().join(".vtcodegitignore");

        // Create a .vtcodegitignore file
        let mut file = File::create(&gitignore_path).unwrap();
        writeln!(file, "*.log").unwrap();
        writeln!(file, "target/").unwrap();
        writeln!(file, "!important.log").unwrap();

        // Test that the file was created
        assert!(gitignore_path.exists());

        // Test pattern matching logic
        let gitignore = VTCodeGitignore::from_directory(temp_dir.path())
            .await
            .unwrap();
        assert!(gitignore.is_loaded());
        assert_eq!(gitignore.pattern_count(), 3);

        // Test file exclusion
        assert!(gitignore.should_exclude(&temp_dir.path().join("debug.log")));
        assert!(gitignore.should_exclude(&temp_dir.path().join("target/binary")));
        assert!(!gitignore.should_exclude(&temp_dir.path().join("important.log")));
        assert!(!gitignore.should_exclude(&temp_dir.path().join("source.rs")));

        println!("✓ VTCodeGitignore functionality works correctly!");
    }

    #[tokio::test]
    async fn test_basic_pattern_matching() {
        let temp_dir = TempDir::new().unwrap();
        let gitignore_path = temp_dir.path().join(".vtcodegitignore");

        // Create a simple .vtcodegitignore
        let mut file = File::create(&gitignore_path).unwrap();
        writeln!(file, "*.log").unwrap();
        writeln!(file, "target/").unwrap();
        writeln!(file, "!important.log").unwrap();

        let gitignore = VTCodeGitignore::from_directory(temp_dir.path())
            .await
            .unwrap();
        assert!(gitignore.is_loaded());
        assert_eq!(gitignore.pattern_count(), 3);

        // Test pattern matching
        assert!(gitignore.should_exclude(&temp_dir.path().join("debug.log")));
        assert!(gitignore.should_exclude(&temp_dir.path().join("target/debug.exe")));
        assert!(!gitignore.should_exclude(&temp_dir.path().join("important.log")));
        assert!(!gitignore.should_exclude(&temp_dir.path().join("source.rs")));
    }

    #[tokio::test]
    async fn test_no_gitignore_file() {
        let temp_dir = TempDir::new().unwrap();
        let gitignore = VTCodeGitignore::from_directory(temp_dir.path())
            .await
            .unwrap();
        assert!(!gitignore.is_loaded());
        assert_eq!(gitignore.pattern_count(), 0);
        assert!(!gitignore.should_exclude(&temp_dir.path().join("anyfile.txt")));
    }

    #[tokio::test]
    async fn test_empty_gitignore_file() {
        let temp_dir = TempDir::new().unwrap();
        let gitignore_path = temp_dir.path().join(".vtcodegitignore");

        // Create an empty .vtcodegitignore
        File::create(&gitignore_path).unwrap();

        let gitignore = VTCodeGitignore::from_directory(temp_dir.path())
            .await
            .unwrap();
        assert!(gitignore.is_loaded());
        assert_eq!(gitignore.pattern_count(), 0);
    }

    #[tokio::test]
    async fn test_comments_and_empty_lines() {
        let temp_dir = TempDir::new().unwrap();
        let gitignore_path = temp_dir.path().join(".vtcodegitignore");

        // Create .vtcodegitignore with comments and empty lines
        let mut file = File::create(&gitignore_path).unwrap();
        writeln!(file, "# This is a comment").unwrap();
        writeln!(file, "").unwrap();
        writeln!(file, "*.tmp").unwrap();
        writeln!(file, "# Another comment").unwrap();
        writeln!(file, "").unwrap();

        let gitignore = VTCodeGitignore::from_directory(temp_dir.path())
            .await
            .unwrap();
        assert!(gitignore.is_loaded());
        assert_eq!(gitignore.pattern_count(), 1); // Only the *.tmp pattern should be loaded

        assert!(gitignore.should_exclude(&temp_dir.path().join("file.tmp")));
        assert!(!gitignore.should_exclude(&temp_dir.path().join("file.txt")));
    }

    #[tokio::test]
    async fn test_path_filtering() {
        let temp_dir = TempDir::new().unwrap();
        let gitignore_path = temp_dir.path().join(".vtcodegitignore");

        // Create .vtcodegitignore
        let mut file = File::create(&gitignore_path).unwrap();
        writeln!(file, "*.log").unwrap();
        writeln!(file, "temp/").unwrap();

        let gitignore = VTCodeGitignore::from_directory(temp_dir.path())
            .await
            .unwrap();

        let paths = vec![
            temp_dir.path().join("app.log"),
            temp_dir.path().join("source.rs"),
            temp_dir.path().join("temp/cache.dat"),
            temp_dir.path().join("important.txt"),
        ];

        let filtered = gitignore.filter_paths(paths);

        assert_eq!(filtered.len(), 2);
        assert!(filtered.contains(&temp_dir.path().join("source.rs")));
        assert!(filtered.contains(&temp_dir.path().join("important.txt")));
        assert!(!filtered.contains(&temp_dir.path().join("app.log")));
        assert!(!filtered.contains(&temp_dir.path().join("temp/cache.dat")));
    }
}
