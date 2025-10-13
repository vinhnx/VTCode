//! Simple markdown-based storage system
//!
//! This module provides simple storage capabilities using markdown files
//! instead of complex databases. Perfect for storing project metadata,
//! search results, and other simple data structures.

use anyhow::{Context, Result, ensure};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use tempfile::Builder;

#[derive(Clone, Debug)]
pub struct MarkdownStorageOptions {
    pub extension: String,
    pub include_json_section: bool,
    pub include_yaml_section: bool,
    pub include_raw_section: bool,
}

impl Default for MarkdownStorageOptions {
    fn default() -> Self {
        Self {
            extension: "md".to_string(),
            include_json_section: true,
            include_yaml_section: true,
            include_raw_section: true,
        }
    }
}

impl MarkdownStorageOptions {
    pub fn with_extension<S: Into<String>>(mut self, extension: S) -> Self {
        self.extension = extension.into();
        self
    }

    pub fn with_json_section(mut self, include: bool) -> Self {
        self.include_json_section = include;
        self
    }

    pub fn with_yaml_section(mut self, include: bool) -> Self {
        self.include_yaml_section = include;
        self
    }

    pub fn with_raw_section(mut self, include: bool) -> Self {
        self.include_raw_section = include;
        self
    }
}

/// Simple markdown storage manager
#[derive(Clone)]
pub struct MarkdownStorage {
    storage_dir: PathBuf,
    options: MarkdownStorageOptions,
}

impl MarkdownStorage {
    /// Create a new markdown storage instance
    pub fn new(storage_dir: PathBuf) -> Self {
        Self {
            storage_dir,
            options: MarkdownStorageOptions::default(),
        }
    }

    pub fn with_options(storage_dir: PathBuf, options: MarkdownStorageOptions) -> Self {
        Self {
            storage_dir,
            options,
        }
    }

    /// Initialize storage directory
    pub fn init(&self) -> Result<()> {
        fs::create_dir_all(&self.storage_dir)
            .with_context(|| format!("failed to initialize {}", self.storage_dir.display()))?;
        Ok(())
    }

    /// Store data as markdown
    pub fn store<T: Serialize>(&self, key: &str, data: &T, title: &str) -> Result<()> {
        ensure!(
            !self.options.extension.is_empty(),
            "Markdown storage file extension cannot be empty"
        );

        let file_path = self.file_path(key);
        let markdown = self.serialize_to_markdown(data, title)?;
        self.write_atomic(&file_path, markdown.as_bytes())
    }

    /// Load data from markdown
    pub fn load<T: for<'de> Deserialize<'de>>(&self, key: &str) -> Result<T> {
        let file_path = self.file_path(key);
        let content = fs::read_to_string(&file_path)
            .with_context(|| format!("failed to read {}", file_path.display()))?;
        self.deserialize_from_markdown(&content)
    }

    /// List all stored items
    pub fn list(&self) -> Result<Vec<String>> {
        let mut items = Vec::new();

        for entry in fs::read_dir(&self.storage_dir)
            .with_context(|| format!("failed to read directory {}", self.storage_dir.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            if self.has_expected_extension(&path) {
                if let Some(file_name) = path.file_stem() {
                    if let Some(name) = file_name.to_str() {
                        items.push(name.to_string());
                    }
                }
            }
        }

        Ok(items)
    }

    /// Delete stored item
    pub fn delete(&self, key: &str) -> Result<()> {
        let file_path = self.file_path(key);
        if file_path.exists() {
            fs::remove_file(&file_path)
                .with_context(|| format!("failed to delete {}", file_path.display()))?;
        }
        Ok(())
    }

    /// Check if item exists
    pub fn exists(&self, key: &str) -> bool {
        self.file_path(key).exists()
    }

    // Helper methods

    fn file_path(&self, key: &str) -> PathBuf {
        self.storage_dir
            .join(format!("{}.{}", key, self.options.extension))
    }

    fn has_expected_extension(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.eq_ignore_ascii_case(&self.options.extension))
            .unwrap_or(false)
    }

    fn serialize_to_markdown<T: Serialize>(&self, data: &T, title: &str) -> Result<String> {
        let json = serde_json::to_string_pretty(data)?;
        let yaml = serde_yaml::to_string(data)?;

        let mut markdown = format!("# {}\n\n", title);

        if self.options.include_json_section {
            markdown.push_str("## JSON\n\n```json\n");
            markdown.push_str(&json);
            markdown.push_str("\n```\n\n");
        }

        if self.options.include_yaml_section {
            markdown.push_str("## YAML\n\n```yaml\n");
            markdown.push_str(&yaml);
            markdown.push_str("\n```\n\n");
        }

        if self.options.include_raw_section {
            markdown.push_str("## Raw Data\n\n");
            markdown.push_str(&self.format_raw_data(data));
            markdown.push('\n');
        }

        Ok(markdown)
    }

    fn deserialize_from_markdown<T: for<'de> Deserialize<'de>>(&self, content: &str) -> Result<T> {
        // Try to extract JSON from markdown code blocks
        if let Some(json_block) = self.extract_code_block(content, "json") {
            return serde_json::from_str(json_block).context("Failed to parse JSON from markdown");
        }

        // Try to extract YAML from markdown code blocks
        if let Some(yaml_block) = self.extract_code_block(content, "yaml") {
            return serde_yaml::from_str(yaml_block).context("Failed to parse YAML from markdown");
        }

        Err(anyhow::anyhow!("No valid JSON or YAML found in markdown"))
    }

    fn extract_code_block<'a>(&self, content: &'a str, language: &str) -> Option<&'a str> {
        let start_pattern = format!("```{}", language);
        let end_pattern = "```";

        if let Some(start_idx) = content.find(&start_pattern) {
            let code_start = start_idx + start_pattern.len();
            if let Some(end_idx) = content[code_start..].find(end_pattern) {
                let code_end = code_start + end_idx;
                return Some(content[code_start..code_end].trim());
            }
        }

        None
    }

    fn format_raw_data<T: Serialize>(&self, data: &T) -> String {
        match serde_json::to_value(data) {
            Ok(serde_json::Value::Object(map)) => {
                let mut lines = Vec::new();
                for (key, value) in map {
                    lines.push(format!("- **{}**: {}", key, self.format_value(&value)));
                }
                lines.join("\n")
            }
            _ => "Complex data structure".to_string(),
        }
    }

    fn format_value(&self, value: &serde_json::Value) -> String {
        match value {
            serde_json::Value::String(s) => format!("\"{}\"", s),
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::Bool(b) => b.to_string(),
            serde_json::Value::Array(arr) => format!("[{} items]", arr.len()),
            serde_json::Value::Object(obj) => format!("{{{} fields}}", obj.len()),
            serde_json::Value::Null => "null".to_string(),
        }
    }

    fn write_atomic(&self, path: &Path, contents: &[u8]) -> Result<()> {
        let parent = path
            .parent()
            .context("markdown storage path must have a parent directory")?;
        let mut temp_file = Builder::new()
            .prefix("markdown")
            .tempfile_in(parent)
            .context("failed to create temporary markdown storage file")?;
        temp_file
            .write_all(contents)
            .context("failed to write temporary markdown storage file")?;
        temp_file
            .as_file_mut()
            .sync_all()
            .context("failed to flush markdown storage file to disk")?;
        temp_file
            .persist(path)
            .map_err(|error| error.error)
            .context("failed to persist markdown storage file")?;
        Ok(())
    }
}

/// Simple key-value storage using markdown
pub struct SimpleKVStorage {
    storage: MarkdownStorage,
}

impl SimpleKVStorage {
    pub fn new(storage_dir: PathBuf) -> Self {
        Self {
            storage: MarkdownStorage::new(storage_dir),
        }
    }

    pub fn with_options(storage_dir: PathBuf, options: MarkdownStorageOptions) -> Self {
        Self {
            storage: MarkdownStorage::with_options(storage_dir, options),
        }
    }

    pub fn init(&self) -> Result<()> {
        self.storage.init()
    }

    pub fn put(&self, key: &str, value: &str) -> Result<()> {
        let data = IndexMap::from([("value".to_string(), value.to_string())]);
        self.storage
            .store(key, &data, &format!("Key-Value: {}", key))
    }

    pub fn get(&self, key: &str) -> Result<String> {
        let data: IndexMap<String, String> = self.storage.load(key)?;
        data.get("value")
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Value not found for key: {}", key))
    }

    pub fn delete(&self, key: &str) -> Result<()> {
        self.storage.delete(key)
    }

    pub fn list_keys(&self) -> Result<Vec<String>> {
        self.storage.list()
    }
}

/// Simple project metadata storage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectData {
    pub name: String,
    pub description: Option<String>,
    pub version: String,
    pub tags: Vec<String>,
    pub metadata: IndexMap<String, String>,
}

impl ProjectData {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            description: None,
            version: "1.0.0".to_string(),
            tags: vec![],
            metadata: IndexMap::new(),
        }
    }
}

/// Project storage using markdown
#[derive(Clone)]
pub struct ProjectStorage {
    storage: MarkdownStorage,
}

impl ProjectStorage {
    pub fn new(storage_dir: PathBuf) -> Self {
        Self {
            storage: MarkdownStorage::new(storage_dir),
        }
    }

    pub fn with_options(storage_dir: PathBuf, options: MarkdownStorageOptions) -> Self {
        Self {
            storage: MarkdownStorage::with_options(storage_dir, options),
        }
    }

    pub fn init(&self) -> Result<()> {
        self.storage.init()
    }

    pub fn save_project(&self, project: &ProjectData) -> Result<()> {
        self.storage.store(
            &project.name,
            project,
            &format!("Project: {}", project.name),
        )
    }

    pub fn load_project(&self, name: &str) -> Result<ProjectData> {
        self.storage.load(name)
    }

    pub fn list_projects(&self) -> Result<Vec<String>> {
        self.storage.list()
    }

    pub fn delete_project(&self, name: &str) -> Result<()> {
        self.storage.delete(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use indexmap::IndexMap;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn custom_extension_is_used_for_storage() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let options = MarkdownStorageOptions::default().with_extension("note");
        let storage = MarkdownStorage::with_options(temp_dir.path().into(), options);
        storage.init()?;

        storage.store(
            "example",
            &IndexMap::from([("value".to_string(), "123".to_string())]),
            "Example",
        )?;

        let file_path = temp_dir.path().join("example.note");
        assert!(file_path.exists());
        Ok(())
    }

    #[test]
    fn disabled_sections_are_omitted_from_markdown() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let options = MarkdownStorageOptions::default()
            .with_json_section(false)
            .with_yaml_section(false)
            .with_raw_section(false);
        let storage = MarkdownStorage::with_options(temp_dir.path().into(), options);
        storage.init()?;

        let payload = IndexMap::from([("value".to_string(), "123".to_string())]);
        storage.store("example", &payload, "Example")?;

        let file_path = temp_dir.path().join("example.md");
        let content = fs::read_to_string(file_path)?;
        assert!(!content.contains("```json"));
        assert!(!content.contains("```yaml"));
        assert!(!content.contains("## Raw Data"));
        Ok(())
    }
}
