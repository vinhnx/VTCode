//! Unified builder for tool responses
//!
//! Provides a consistent way to construct tool execution results with
//! support for dual-channel output, structured metadata, and standardized error reporting.

use hashbrown::HashMap;
use serde_json::{Value, json};
use std::path::PathBuf;

use crate::tools::result::{ToolMetadataBuilder, ToolResult};

/// Builder for standardized tool responses
pub struct ToolResponseBuilder {
    tool_name: String,
    success: bool,
    message: Option<String>,
    content: Option<String>,
    stdout: Option<String>,
    modified_files: Vec<String>,
    has_more: bool,
    llm_content: Option<String>,
    ui_content: Option<String>,
    error: Option<String>,
    metadata: ToolMetadataBuilder,
    custom_fields: HashMap<String, Value>,
}

impl ToolResponseBuilder {
    /// Create a new builder for the given tool
    pub fn new(tool_name: impl Into<String>) -> Self {
        Self {
            tool_name: tool_name.into(),
            success: true,
            message: None,
            content: None,
            stdout: None,
            modified_files: Vec::new(),
            has_more: false,
            llm_content: None,
            ui_content: None,
            error: None,
            metadata: ToolMetadataBuilder::new(),
            custom_fields: HashMap::new(),
        }
    }

    /// Mark the execution as successful
    pub fn success(mut self) -> Self {
        self.success = true;
        self
    }

    /// Mark the execution as failed with an error message
    pub fn failure(mut self, error: impl Into<String>) -> Self {
        self.success = false;
        self.error = Some(error.into());
        self
    }

    /// Set a user-friendly status message
    pub fn message(mut self, message: impl Into<String>) -> Self {
        self.message = Some(message.into());
        self
    }

    /// Set the main content (used for both LLM and UI if not overridden)
    pub fn content(mut self, content: impl Into<String>) -> Self {
        self.content = Some(content.into());
        self
    }

    /// Set standard output from a process
    pub fn stdout(mut self, stdout: impl Into<String>) -> Self {
        self.stdout = Some(stdout.into());
        self
    }

    /// Add a modified file to the list
    pub fn modified_file(mut self, path: impl Into<String>) -> Self {
        self.modified_files.push(path.into());
        self
    }

    /// Add multiple modified files
    pub fn modified_files(mut self, paths: Vec<String>) -> Self {
        self.modified_files.extend(paths);
        self
    }

    /// Set whether there are more results available
    pub fn has_more(mut self, has_more: bool) -> Self {
        self.has_more = has_more;
        self
    }

    /// Set explicit dual-channel content
    pub fn dual_content(mut self, llm: impl Into<String>, ui: impl Into<String>) -> Self {
        self.llm_content = Some(llm.into());
        self.ui_content = Some(ui.into());
        self
    }

    /// Add a file reference to the metadata (for UI linking)
    pub fn file(mut self, path: impl Into<PathBuf>) -> Self {
        self.metadata = self.metadata.file(path.into());
        self
    }

    /// Add multiple file references
    pub fn files(mut self, paths: Vec<PathBuf>) -> Self {
        self.metadata = self.metadata.files(paths);
        self
    }

    /// Add structured data to the metadata
    pub fn data(mut self, key: impl Into<String>, value: Value) -> Self {
        self.metadata = self.metadata.data(key, value);
        self
    }

    /// Add a custom top-level field to the final JSON response
    pub fn field(mut self, key: impl Into<String>, value: Value) -> Self {
        self.custom_fields.insert(key.into(), value);
        self
    }

    /// Build the legacy JSON Value response
    pub fn build_json(self) -> Value {
        let mut res = json!({
            "success": self.success,
            "status": if self.success { "success" } else { "error" },
        });

        let Some(obj) = res.as_object_mut() else {
            return res;
        };

        if let Some(msg) = self.message {
            obj.insert("message".to_string(), json!(msg));
        }

        if let Some(err) = self.error {
            obj.insert("error".to_string(), json!(err));
        }

        let content_value = self.content;
        if let Some(c) = content_value.as_ref() {
            obj.insert("content".to_string(), json!(c));
        }

        if let Some(s) = self.stdout {
            let duplicates_content = content_value.as_deref() == Some(s.as_str());
            if !duplicates_content {
                obj.insert("stdout".to_string(), json!(s));
            }
        }

        if !self.modified_files.is_empty() {
            obj.insert("modified_files".to_string(), json!(self.modified_files));
        }

        if self.has_more {
            obj.insert("has_more".to_string(), json!(true));
        }

        // Build and merge metadata. Keys already surfaced as top-level custom
        // fields stay top-level only: duplicating them inside `metadata.data`
        // re-serializes the same values on every tool result (read_file paid
        // that tax for `content_kind`/`encoding` on every call). Only explicit
        // `.field()` twins dedup — a `.data()` entry sharing a name with a
        // builder-structural key (success/status/message/...) must survive.
        let mut meta = self.metadata.build();
        meta.data.retain(|key, _| !self.custom_fields.contains_key(key));
        if !meta.data.is_empty() || !meta.files.is_empty() || !meta.lines.is_empty() {
            obj.insert("metadata".to_string(), json!(meta));
        }

        // Add custom top-level fields
        for (k, v) in self.custom_fields {
            obj.insert(k, v);
        }

        res
    }

    /// Build the modern dual-channel ToolResult
    pub fn build_result(self) -> ToolResult {
        if !self.success {
            return ToolResult::error(self.tool_name, self.error.unwrap_or_else(|| "Unknown error".to_string()));
        }

        let llm = self.llm_content.or_else(|| self.content.clone()).unwrap_or_default();
        let ui = self.ui_content.or_else(|| self.content.clone()).unwrap_or_default();

        let mut res = ToolResult::new(self.tool_name, llm, ui);
        res.metadata = self.metadata.build();

        // Add custom fields to metadata data map
        for (k, v) in self.custom_fields {
            res.metadata.data.insert(k, v);
        }

        res
    }
}

#[cfg(test)]
mod tests {
    use super::ToolResponseBuilder;
    use serde_json::json;

    #[test]
    fn build_json_omits_stdout_when_same_as_content() {
        let value = ToolResponseBuilder::new("test").content("same").stdout("same").build_json();

        assert_eq!(value.get("content").and_then(|v| v.as_str()), Some("same"));
        assert!(value.get("stdout").is_none());
    }

    #[test]
    fn build_json_keeps_metadata_only_keys_and_deduplicates_field_keys() {
        let value = ToolResponseBuilder::new("read_file")
            .field("content_kind", json!("text"))
            .field("encoding", json!("utf8"))
            .data("content_kind", json!("text"))
            .data("encoding", json!("utf8"))
            .data("size_bytes", json!(1024))
            .build_json();

        // Duplicated keys stay top-level only; metadata-only keys survive.
        assert_eq!(value["content_kind"], json!("text"));
        assert_eq!(value["encoding"], json!("utf8"));
        assert_eq!(value["metadata"]["data"]["size_bytes"], json!(1024));
        assert!(value["metadata"]["data"].get("content_kind").is_none());
        assert!(value["metadata"]["data"].get("encoding").is_none());
    }

    #[test]
    fn build_json_omits_metadata_object_when_all_data_keys_are_duplicates() {
        let value = ToolResponseBuilder::new("read_file")
            .field("content_kind", json!("text"))
            .data("content_kind", json!("text"))
            .build_json();

        assert_eq!(value["content_kind"], json!("text"));
        assert!(value.get("metadata").is_none());
    }

    #[test]
    fn build_json_keeps_data_keys_colliding_with_structural_names() {
        // A `.data()` entry must not vanish because it shares a name with a
        // builder-structural key; only explicit `.field()` twins dedup.
        let value = ToolResponseBuilder::new("read_file")
            .message("done")
            .field("kind", json!("top"))
            .data("message", json!("from data"))
            .data("kind", json!("from data"))
            .build_json();

        assert_eq!(value["message"], json!("done"));
        assert_eq!(value["kind"], json!("top"));
        assert_eq!(value["metadata"]["data"]["message"], json!("from data"));
        assert!(value["metadata"]["data"].get("kind").is_none());
    }
}
