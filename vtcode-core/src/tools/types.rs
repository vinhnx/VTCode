//! Common types used across the tool system

use serde::{Deserialize, Serialize};
use std::time::Instant;

/// Enhanced cache entry with performance tracking
#[derive(Debug, Clone)]
pub struct EnhancedCacheEntry<T> {
    pub data: T,
    pub timestamp: Instant,
    pub access_count: usize,
    pub size_bytes: usize,
    pub last_accessed: Instant,
    pub priority: u8, // 0=low, 1=medium, 2=high priority
}

impl<T> EnhancedCacheEntry<T> {
    pub fn new(data: T, size_bytes: usize) -> Self {
        let now = Instant::now();
        Self {
            data,
            timestamp: now,
            access_count: 1,
            size_bytes,
            last_accessed: now,
            priority: 1,
        }
    }

    pub fn access(&mut self) {
        self.access_count += 1;
        self.last_accessed = Instant::now();
    }
}

/// Cache statistics
#[derive(Debug, Clone, Default)]
pub struct EnhancedCacheStats {
    pub hits: usize,
    pub misses: usize,
    pub entries: usize,
    pub total_size_bytes: usize,
    pub evictions: usize,
    pub memory_evictions: usize,
    pub expired_evictions: usize,
    pub total_memory_saved: usize,
}

/// Operation statistics for performance monitoring
#[derive(Debug, Clone, Default)]
pub struct OperationStats {
    pub total_calls: usize,
    pub total_duration_ms: u64,
    pub avg_duration_ms: f64,
    pub success_count: usize,
    pub error_count: usize,
    pub last_called: Option<Instant>,
}

/// Input structures for various tools
#[derive(Debug, Deserialize)]
pub struct Input {
    pub path: String,
    #[serde(default)]
    pub max_bytes: Option<usize>,
    #[serde(default)]
    pub encoding: Option<String>,
    #[serde(default)]
    pub ast_grep_pattern: Option<String>,
    #[serde(default)]
    pub chunk_lines: Option<usize>,
    #[serde(default)]
    pub max_lines: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct WriteInput {
    pub path: String,
    pub content: String,
    #[serde(default)]
    pub encoding: Option<String>,
    #[serde(default = "default_write_mode")]
    pub mode: String,
    #[serde(default)]
    pub ast_grep_lint: bool,
    #[serde(default)]
    pub ast_grep_refactor: bool,
}

#[derive(Debug, Deserialize)]
pub struct EditInput {
    pub path: String,
    pub old_str: String,
    pub new_str: String,
    #[serde(default)]
    pub encoding: Option<String>,
    #[serde(default)]
    pub ast_grep_pattern: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ListInput {
    pub path: String,
    #[serde(default = "default_max_items")]
    pub max_items: usize,
    /// Optional page number for pagination (1-based)
    #[serde(default)]
    pub page: Option<usize>,
    /// Optional page size for pagination
    #[serde(default)]
    pub per_page: Option<usize>,
    /// Controls verbosity of tool output: "concise" (default) or "detailed"
    #[serde(default)]
    pub response_format: Option<String>,
    #[serde(default)]
    pub include_hidden: bool,
    #[serde(default)]
    pub ast_grep_pattern: Option<String>,
    // Enhanced file discovery parameters
    #[serde(default)]
    pub mode: Option<String>, // "list", "recursive", "find_name", "find_content"
    #[serde(default)]
    pub name_pattern: Option<String>, // For recursive and find_name modes
    #[serde(default)]
    pub content_pattern: Option<String>, // For find_content mode
    #[serde(default)]
    pub file_extensions: Option<Vec<String>>, // Filter by extensions
    #[serde(default)]
    pub case_sensitive: Option<bool>, // For pattern matching
}

#[derive(Debug, Deserialize, Serialize)]
pub struct EnhancedTerminalInput {
    pub command: Vec<String>,
    #[serde(default)]
    pub working_dir: Option<String>,
    #[serde(default)]
    pub timeout_secs: Option<u64>,
    #[serde(default)]
    pub mode: Option<String>, // "terminal", "pty"
    /// Controls verbosity of tool output: "concise" (default) or "detailed"
    #[serde(default)]
    pub response_format: Option<String>,
    #[serde(default)]
    pub raw_command: Option<String>,
    #[serde(default)]
    pub shell: Option<String>,
    #[serde(default)]
    pub login: Option<bool>,
}

/// PTY Session structure for managing interactive terminal sessions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VTCodePtySession {
    pub id: String,
    pub command: String,
    pub args: Vec<String>,
    pub working_dir: Option<String>,
    pub rows: u16,
    pub cols: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screen_contents: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scrollback: Option<String>,
}

// Default value functions
fn default_max_items() -> usize {
    1000
}
fn default_write_mode() -> String {
    "overwrite".to_string()
}

// Search path default
pub fn default_search_path() -> String {
    ".".to_string()
}
