//! Subagent configuration schema and parsing
//!
//! Subagents are specialized AI agents that can be invoked for specific tasks.
//! Built-in subagents are shipped with the binary.
//!
//! # Built-in subagents include:
//! - explore: Fast read-only codebase search
//! - plan: Research for planning mode
//! - general: Multi-step tasks with full capabilities
//! - code-reviewer: Code quality and security review
//! - debugger: Error investigation and fixes

use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use tracing::debug;

/// Permission mode for subagent tool execution
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SubagentPermissionMode {
    /// Normal permission prompts
    #[default]
    Default,
    /// Auto-accept file edits
    AcceptEdits,
    /// Bypass all permission prompts (dangerous)
    BypassPermissions,
    /// Plan mode - research only, no modifications
    Plan,
    /// Ignore permission errors and continue
    Ignore,
}

impl fmt::Display for SubagentPermissionMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Default => write!(f, "default"),
            Self::AcceptEdits => write!(f, "acceptEdits"),
            Self::BypassPermissions => write!(f, "bypassPermissions"),
            Self::Plan => write!(f, "plan"),
            Self::Ignore => write!(f, "ignore"),
        }
    }
}

impl FromStr for SubagentPermissionMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "default" => Ok(Self::Default),
            "acceptedits" | "accept_edits" | "accept-edits" => Ok(Self::AcceptEdits),
            "bypasspermissions" | "bypass_permissions" | "bypass-permissions" => {
                Ok(Self::BypassPermissions)
            }
            "plan" => Ok(Self::Plan),
            "ignore" => Ok(Self::Ignore),
            _ => Err(format!("Unknown permission mode: {}", s)),
        }
    }
}

/// Model selection for subagent
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SubagentModel {
    /// Inherit model from parent conversation
    Inherit,
    /// Use a specific model alias (sonnet, opus, haiku)
    Alias(String),
    /// Use a specific model ID
    ModelId(String),
}

impl Default for SubagentModel {
    fn default() -> Self {
        Self::Alias("sonnet".to_string())
    }
}

impl fmt::Display for SubagentModel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Inherit => write!(f, "inherit"),
            Self::Alias(alias) => write!(f, "{}", alias),
            Self::ModelId(id) => write!(f, "{}", id),
        }
    }
}

impl FromStr for SubagentModel {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.eq_ignore_ascii_case("inherit") {
            Ok(Self::Inherit)
        } else if matches!(s.to_lowercase().as_str(), "sonnet" | "opus" | "haiku") {
            Ok(Self::Alias(s.to_lowercase()))
        } else {
            Ok(Self::ModelId(s.to_string()))
        }
    }
}

/// Source location of a subagent definition
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SubagentSource {
    /// Built-in subagent shipped with the binary
    Builtin,
    /// User-level subagent from ~/.vtcode/agents/
    User,
    /// Project-level subagent from .vtcode/agents/
    Project,
    /// Plugin-provided subagent
    Plugin(String),
}

impl fmt::Display for SubagentSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Builtin => write!(f, "builtin"),
            Self::User => write!(f, "user"),
            Self::Project => write!(f, "project"),
            Self::Plugin(name) => write!(f, "plugin:{}", name),
        }
    }
}

/// YAML frontmatter parsed from subagent markdown file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubagentFrontmatter {
    /// Unique identifier (lowercase, hyphens allowed)
    pub name: String,

    /// Natural language description of when to use this subagent
    pub description: String,

    /// Comma-separated list of allowed tools (inherits all if omitted)
    #[serde(default)]
    pub tools: Option<String>,

    /// Model to use (alias, model ID, or "inherit")
    #[serde(default)]
    pub model: Option<String>,

    /// Permission mode for tool execution
    #[serde(default, rename = "permissionMode")]
    pub permission_mode: Option<String>,

    /// Comma-separated list of skills to auto-load
    #[serde(default)]
    pub skills: Option<String>,
}

/// Complete subagent configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubagentConfig {
    /// Unique identifier
    pub name: String,

    /// Human-readable description for delegation
    pub description: String,

    /// Allowed tools (None = inherit all from parent)
    pub tools: Option<Vec<String>>,

    /// Model selection
    pub model: SubagentModel,

    /// Permission mode
    pub permission_mode: SubagentPermissionMode,

    /// Skills to auto-load
    pub skills: Vec<String>,

    /// System prompt (markdown body)
    pub system_prompt: String,

    /// Source location
    pub source: SubagentSource,

    /// File path (if loaded from file)
    pub file_path: Option<PathBuf>,
}

impl SubagentConfig {
    /// Parse a subagent from markdown content with YAML frontmatter
    pub fn from_markdown(
        content: &str,
        source: SubagentSource,
        file_path: Option<PathBuf>,
    ) -> Result<Self, SubagentParseError> {
        debug!(
            ?source,
            ?file_path,
            content_len = content.len(),
            "Parsing subagent from markdown"
        );
        // Extract YAML frontmatter between --- delimiters
        let content = content.trim();
        if !content.starts_with("---") {
            return Err(SubagentParseError::MissingFrontmatter);
        }

        let after_start = &content[3..];
        let end_pos = after_start
            .find("\n---")
            .ok_or(SubagentParseError::MissingFrontmatter)?;

        let yaml_content = &after_start[..end_pos].trim();
        let body_start = 3 + end_pos + 4; // Skip "---\n" + yaml + "\n---"
        let system_prompt = content
            .get(body_start..)
            .map(|s| s.trim())
            .unwrap_or("")
            .to_string();

        // Parse YAML frontmatter
        let frontmatter: SubagentFrontmatter =
            serde_yaml::from_str(yaml_content).map_err(SubagentParseError::YamlError)?;

        // Parse tools list
        let tools = frontmatter.tools.map(|t| {
            t.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        });

        // Parse model
        let model = frontmatter
            .model
            .map(|m| SubagentModel::from_str(&m).unwrap())
            .unwrap_or_default();

        // Parse permission mode
        let permission_mode = frontmatter
            .permission_mode
            .map(|p| SubagentPermissionMode::from_str(&p).unwrap_or_default())
            .unwrap_or_default();

        // Parse skills list
        let skills = frontmatter
            .skills
            .map(|s| {
                s.split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default();

        let config = Self {
            name: frontmatter.name.clone(),
            description: frontmatter.description.clone(),
            tools,
            model,
            permission_mode,
            skills,
            system_prompt,
            source,
            file_path,
        };
        debug!(
            name = %config.name,
            ?config.model,
            ?config.permission_mode,
            tools_count = config.tools.as_ref().map(|t| t.len()),
            "Parsed subagent config"
        );
        Ok(config)
    }

    /// Parse subagent from JSON (for CLI --agents flag)
    pub fn from_json(name: &str, value: &serde_json::Value) -> Result<Self, SubagentParseError> {
        let description = value
            .get("description")
            .and_then(|v| v.as_str())
            .ok_or_else(|| SubagentParseError::MissingField("description".to_string()))?
            .to_string();

        let system_prompt = value
            .get("prompt")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let tools = value.get("tools").and_then(|v| {
            v.as_array().map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
        });

        let model = value
            .get("model")
            .and_then(|v| v.as_str())
            .map(|m| SubagentModel::from_str(m).unwrap())
            .unwrap_or_default();

        let permission_mode = value
            .get("permissionMode")
            .and_then(|v| v.as_str())
            .map(|p| SubagentPermissionMode::from_str(p).unwrap_or_default())
            .unwrap_or_default();

        let skills = value
            .get("skills")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        Ok(Self {
            name: name.to_string(),
            description,
            tools,
            model,
            permission_mode,
            skills,
            system_prompt,
            source: SubagentSource::User, // Default to User source for JSON-parsed agents
            file_path: None,
        })
    }

    /// Check if this subagent has access to a specific tool
    pub fn has_tool_access(&self, tool_name: &str) -> bool {
        match &self.tools {
            None => true, // Inherits all tools
            Some(tools) => tools.iter().any(|t| t == tool_name),
        }
    }

    /// Get the list of allowed tools, or None if all tools are allowed
    pub fn allowed_tools(&self) -> Option<&[String]> {
        self.tools.as_deref()
    }

    /// Check if this is a read-only subagent (like Explore)
    pub fn is_read_only(&self) -> bool {
        self.permission_mode == SubagentPermissionMode::Plan
    }
}

/// Errors that can occur when parsing subagent configurations
#[derive(Debug)]
pub enum SubagentParseError {
    /// Missing YAML frontmatter
    MissingFrontmatter,
    /// YAML parsing error
    YamlError(serde_yaml::Error),
    /// Missing required field
    MissingField(String),
    /// IO error when reading file
    IoError(std::io::Error),
}

impl fmt::Display for SubagentParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingFrontmatter => write!(f, "Missing YAML frontmatter (---...---)"),
            Self::YamlError(e) => write!(f, "YAML parse error: {}", e),
            Self::MissingField(field) => write!(f, "Missing required field: {}", field),
            Self::IoError(e) => write!(f, "IO error: {}", e),
        }
    }
}

impl std::error::Error for SubagentParseError {}

impl From<std::io::Error> for SubagentParseError {
    fn from(e: std::io::Error) -> Self {
        Self::IoError(e)
    }
}

/// Configuration for the subagent system in vtcode.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SubagentsConfig {
    /// Enable the subagent system
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// Maximum concurrent subagents
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent: usize,

    /// Default timeout for subagent execution (seconds)
    #[serde(default = "default_timeout_seconds")]
    pub default_timeout_seconds: u64,

    /// Default model for subagents (if not specified in subagent config)
    #[serde(default)]
    pub default_model: Option<String>,

    /// Additional directories to search for subagent definitions
    #[serde(default)]
    pub additional_agent_dirs: Vec<PathBuf>,
}

fn default_enabled() -> bool {
    true
}

fn default_max_concurrent() -> usize {
    3
}

fn default_timeout_seconds() -> u64 {
    300 // 5 minutes
}

impl Default for SubagentsConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            max_concurrent: default_max_concurrent(),
            default_timeout_seconds: default_timeout_seconds(),
            default_model: None,
            additional_agent_dirs: Vec::new(),
        }
    }
}

/// Load subagent from a markdown file
pub fn load_subagent_from_file(
    path: &Path,
    source: SubagentSource,
) -> Result<SubagentConfig, SubagentParseError> {
    let content = std::fs::read_to_string(path)?;
    SubagentConfig::from_markdown(&content, source, Some(path.to_path_buf()))
}

/// Discover all subagent files in a directory
pub fn discover_subagents_in_dir(
    dir: &Path,
    source: SubagentSource,
) -> Vec<Result<SubagentConfig, SubagentParseError>> {
    let mut results = Vec::new();

    if !dir.exists() || !dir.is_dir() {
        return results;
    }

    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "md").unwrap_or(false) {
                results.push(load_subagent_from_file(&path, source.clone()));
            }
        }
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_parse_subagent_markdown() {
        let content = r#"---
name: code-reviewer
description: Expert code reviewer for quality and security
tools: read_file, grep_file, list_files
model: sonnet
permissionMode: default
skills: rust-patterns
---

You are a senior code reviewer.
Focus on quality, security, and best practices.
"#;

        let config = SubagentConfig::from_markdown(content, SubagentSource::User, None).unwrap();

        assert_eq!(config.name, "code-reviewer");
        assert_eq!(
            config.description,
            "Expert code reviewer for quality and security"
        );
        assert_eq!(
            config.tools,
            Some(vec![
                "read_file".to_string(),
                "grep_file".to_string(),
                "list_files".to_string()
            ])
        );
        assert_eq!(config.model, SubagentModel::Alias("sonnet".to_string()));
        assert_eq!(config.permission_mode, SubagentPermissionMode::Default);
        assert_eq!(config.skills, vec!["rust-patterns".to_string()]);
        assert!(config.system_prompt.contains("senior code reviewer"));
    }

    #[test]
    fn test_parse_subagent_inherit_model() {
        let content = r#"---
name: explorer
description: Codebase explorer
model: inherit
---

Explore the codebase.
"#;

        let config = SubagentConfig::from_markdown(content, SubagentSource::Project, None).unwrap();
        assert_eq!(config.model, SubagentModel::Inherit);
    }

    #[test]
    fn test_parse_subagent_json() {
        let json = serde_json::json!({
            "description": "Test subagent",
            "prompt": "You are a test agent.",
            "tools": ["read_file", "write_file"],
            "model": "opus"
        });

        let config = SubagentConfig::from_json("test-agent", &json).unwrap();
        assert_eq!(config.name, "test-agent");
        assert_eq!(config.description, "Test subagent");
        assert_eq!(
            config.tools,
            Some(vec!["read_file".to_string(), "write_file".to_string()])
        );
        assert_eq!(config.model, SubagentModel::Alias("opus".to_string()));
    }

    #[test]
    fn test_tool_access() {
        let config = SubagentConfig {
            name: "test".to_string(),
            description: "test".to_string(),
            tools: Some(vec!["read_file".to_string(), "grep_file".to_string()]),
            model: SubagentModel::default(),
            permission_mode: SubagentPermissionMode::default(),
            skills: vec![],
            system_prompt: String::new(),
            source: SubagentSource::User,
            file_path: None,
        };

        assert!(config.has_tool_access("read_file"));
        assert!(config.has_tool_access("grep_file"));
        assert!(!config.has_tool_access("write_file"));
    }

    #[test]
    fn test_inherit_all_tools() {
        let config = SubagentConfig {
            name: "test".to_string(),
            description: "test".to_string(),
            tools: None, // Inherit all
            model: SubagentModel::default(),
            permission_mode: SubagentPermissionMode::default(),
            skills: vec![],
            system_prompt: String::new(),
            source: SubagentSource::User,
            file_path: None,
        };

        assert!(config.has_tool_access("read_file"));
        assert!(config.has_tool_access("any_tool"));
    }
}
