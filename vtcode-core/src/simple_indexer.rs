//! Simple file indexer using regex and markdown storage
//!
//! This module provides a simple, direct approach to code indexing and retrieval
//! using regex patterns and markdown files for storage. No complex embeddings
//! or databases - just direct file operations like a human using bash.

use anyhow::Result;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Simple file index entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileIndex {
    /// File path
    pub path: String,
    /// File content hash for change detection
    pub hash: String,
    /// Last modified timestamp
    pub modified: u64,
    /// File size
    pub size: u64,
    /// Language/extension
    pub language: String,
    /// Simple tags
    pub tags: Vec<String>,
}

/// Simple search result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub file_path: String,
    pub line_number: usize,
    pub line_content: String,
    pub matches: Vec<String>,
}

/// Simple file indexer
#[derive(Clone)]
pub struct SimpleIndexer {
    /// Index storage directory
    index_dir: PathBuf,
    /// Workspace root
    workspace_root: PathBuf,
    /// In-memory index cache
    index_cache: HashMap<String, FileIndex>,
}

impl SimpleIndexer {
    /// Create a new simple indexer
    pub fn new(workspace_root: PathBuf) -> Self {
        let index_dir = workspace_root.join(".vtcode").join("index");

        Self {
            index_dir,
            workspace_root,
            index_cache: HashMap::new(),
        }
    }

    /// Initialize the index directory
    pub fn init(&self) -> Result<()> {
        fs::create_dir_all(&self.index_dir)?;
        Ok(())
    }

    /// Get the workspace root path
    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    /// Index a single file
    pub fn index_file(&mut self, file_path: &Path) -> Result<()> {
        if !file_path.exists() || !file_path.is_file() {
            return Ok(());
        }

        let content = match fs::read_to_string(file_path) {
            Ok(text) => text,
            Err(err) => {
                if err.kind() == ErrorKind::InvalidData {
                    return Ok(());
                }
                return Err(err.into());
            }
        };
        let hash = self.calculate_hash(&content);
        let modified = self.get_modified_time(file_path)?;
        let size = content.len() as u64;
        let language = self.detect_language(file_path);

        let index = FileIndex {
            path: file_path.to_string_lossy().to_string(),
            hash,
            modified,
            size,
            language,
            tags: vec![],
        };

        self.index_cache
            .insert(file_path.to_string_lossy().to_string(), index.clone());

        // Save to markdown file
        self.save_index_to_markdown(&index)?;

        Ok(())
    }

    /// Index all files in directory recursively
    pub fn index_directory(&mut self, dir_path: &Path) -> Result<()> {
        let mut file_paths = Vec::new();

        // First pass: collect all file paths
        self.walk_directory(dir_path, &mut |file_path| {
            file_paths.push(file_path.to_path_buf());
            Ok(())
        })?;

        // Second pass: index each file
        for file_path in file_paths {
            self.index_file(&file_path)?;
        }

        Ok(())
    }

    /// Search files using regex pattern
    pub fn search(&self, pattern: &str, path_filter: Option<&str>) -> Result<Vec<SearchResult>> {
        let regex = Regex::new(pattern)?;

        let mut results = Vec::new();

        // Search through indexed files
        for (file_path, _) in &self.index_cache {
            if let Some(filter) = path_filter {
                if !file_path.contains(filter) {
                    continue;
                }
            }

            if let Ok(content) = fs::read_to_string(file_path) {
                for (line_num, line) in content.lines().enumerate() {
                    if regex.is_match(line) {
                        let matches: Vec<String> = regex
                            .find_iter(line)
                            .map(|m| m.as_str().to_string())
                            .collect();

                        results.push(SearchResult {
                            file_path: file_path.clone(),
                            line_number: line_num + 1,
                            line_content: line.to_string(),
                            matches,
                        });
                    }
                }
            }
        }

        Ok(results)
    }

    /// Find files by name pattern
    pub fn find_files(&self, pattern: &str) -> Result<Vec<String>> {
        let regex = Regex::new(pattern)?;
        let mut results = Vec::new();

        for file_path in self.index_cache.keys() {
            if regex.is_match(file_path) {
                results.push(file_path.clone());
            }
        }

        Ok(results)
    }

    /// Get file content with line numbers
    pub fn get_file_content(
        &self,
        file_path: &str,
        start_line: Option<usize>,
        end_line: Option<usize>,
    ) -> Result<String> {
        let content = fs::read_to_string(file_path)?;
        let lines: Vec<&str> = content.lines().collect();

        let start = start_line.unwrap_or(1).saturating_sub(1);
        let end = end_line.unwrap_or(lines.len());

        let selected_lines = &lines[start..end.min(lines.len())];

        let mut result = String::new();
        for (i, line) in selected_lines.iter().enumerate() {
            result.push_str(&format!("{}: {}\n", start + i + 1, line));
        }

        Ok(result)
    }

    /// List files in directory (like ls)
    pub fn list_files(&self, dir_path: &str, show_hidden: bool) -> Result<Vec<String>> {
        let path = Path::new(dir_path);
        if !path.exists() {
            return Ok(vec![]);
        }

        let mut files = Vec::new();

        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let file_name = entry.file_name().to_string_lossy().to_string();

            if !show_hidden && file_name.starts_with('.') {
                continue;
            }

            files.push(file_name);
        }

        Ok(files)
    }

    /// Grep-like search (like grep command)
    pub fn grep(&self, pattern: &str, file_pattern: Option<&str>) -> Result<Vec<SearchResult>> {
        let regex = Regex::new(pattern)?;
        let mut results = Vec::new();

        for (file_path, _) in &self.index_cache {
            if let Some(fp) = file_pattern {
                if !file_path.contains(fp) {
                    continue;
                }
            }

            if let Ok(content) = fs::read_to_string(file_path) {
                for (line_num, line) in content.lines().enumerate() {
                    if regex.is_match(line) {
                        results.push(SearchResult {
                            file_path: file_path.clone(),
                            line_number: line_num + 1,
                            line_content: line.to_string(),
                            matches: vec![line.to_string()],
                        });
                    }
                }
            }
        }

        Ok(results)
    }

    // Helper methods

    fn walk_directory<F>(&mut self, dir_path: &Path, callback: &mut F) -> Result<()>
    where
        F: FnMut(&Path) -> Result<()>,
    {
        let vtcode_dir = self.workspace_root.join(".vtcode");
        let external_root = vtcode_dir.join("external");
        self.walk_directory_internal(dir_path, &vtcode_dir, &external_root, callback)
    }

    fn walk_directory_internal<F>(
        &mut self,
        dir_path: &Path,
        vtcode_dir: &Path,
        external_root: &Path,
        callback: &mut F,
    ) -> Result<()>
    where
        F: FnMut(&Path) -> Result<()>,
    {
        if !dir_path.exists() {
            return Ok(());
        }

        for entry in fs::read_dir(dir_path)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() {
                if path == vtcode_dir {
                    let external_path = path.join("external");
                    if external_path.exists() {
                        self.walk_directory_internal(
                            &external_path,
                            vtcode_dir,
                            external_root,
                            callback,
                        )?;
                    }
                    continue;
                }

                if path.starts_with(vtcode_dir) && !path.starts_with(external_root) {
                    continue;
                }

                if let Some(name) = path.file_name() {
                    let name_str = name.to_string_lossy();
                    if name_str.starts_with('.') {
                        continue;
                    }
                    if name_str == "target" || name_str == "node_modules" {
                        continue;
                    }
                }

                self.walk_directory_internal(&path, vtcode_dir, external_root, callback)?;
            } else if path.is_file() {
                callback(&path)?;
            }
        }

        Ok(())
    }

    fn calculate_hash(&self, content: &str) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        content.hash(&mut hasher);
        format!("{:x}", hasher.finish())
    }

    fn get_modified_time(&self, file_path: &Path) -> Result<u64> {
        let metadata = fs::metadata(file_path)?;
        let modified = metadata.modified()?;
        Ok(modified.duration_since(SystemTime::UNIX_EPOCH)?.as_secs())
    }

    fn detect_language(&self, file_path: &Path) -> String {
        file_path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("unknown")
            .to_string()
    }

    fn save_index_to_markdown(&self, index: &FileIndex) -> Result<()> {
        let file_name = format!("{}.md", self.calculate_hash(&index.path));
        let index_path = self.index_dir.join(file_name);

        let markdown = format!(
            "# File Index: {}\n\n\
            - **Path**: {}\n\
            - **Hash**: {}\n\
            - **Modified**: {}\n\
            - **Size**: {} bytes\n\
            - **Language**: {}\n\
            - **Tags**: {}\n\n",
            index.path,
            index.path,
            index.hash,
            index.modified,
            index.size,
            index.language,
            index.tags.join(", ")
        );

        fs::write(index_path, markdown)?;
        Ok(())
    }
}
