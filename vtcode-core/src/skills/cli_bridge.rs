//! CLI Tool Bridge for External Tool Integration
//!
//! Bridges external CLI tools to VT Code's skill system, enabling integration
//! of any command-line tool with proper documentation into the agent harness.
//!
//! ## Features
//!
//! - **Progressive Disclosure**: Load tool documentation only when needed
//! - **JSON I/O**: Structured input/output via JSON when available
//! - **Fallback Support**: Graceful degradation to text output
//! - **Validation**: Schema-based validation for tool arguments
//! - **Streaming**: Support for long-running operations
//!
//! ## Tool Discovery
//!
//! Tools are discovered by scanning for:
//! - Executable files with accompanying README.md
//! - tool.json metadata files
//! - Standard installation paths (/usr/local/bin, ~/.local/bin, etc.)

use crate::skills::types::{Skill, SkillManifest, SkillResource};
use crate::tool_policy::ToolPolicy;
use crate::tools::traits::Tool;
use crate::utils::async_utils;
use crate::utils::file_utils::{read_file_with_context_sync, read_json_file_sync};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;
use tracing::{debug, info, warn};

/// Configuration for a CLI tool skill
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliToolConfig {
    /// Tool name (must be unique)
    pub name: String,

    /// Brief description
    pub description: String,

    /// Path to the executable
    pub executable_path: PathBuf,

    /// Path to README/documentation
    pub readme_path: Option<PathBuf>,

    /// Path to JSON schema for arguments
    pub schema_path: Option<PathBuf>,

    /// Timeout for execution (seconds)
    pub timeout_seconds: Option<u64>,

    /// Whether tool supports JSON I/O
    pub supports_json: bool,

    /// Environment variables to set
    pub environment: Option<std::collections::HashMap<String, String>>,

    /// Working directory for execution
    pub working_dir: Option<PathBuf>,
}

/// Tool execution result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliToolResult {
    /// Exit code
    pub exit_code: i32,

    /// Standard output
    pub stdout: String,

    /// Standard error
    pub stderr: String,

    /// Parsed JSON output (if available)
    pub json_output: Option<Value>,

    /// Execution time in milliseconds
    pub execution_time_ms: u64,
}

/// Bridge between CLI tools and VT Code skills
#[derive(Debug, Clone)]
pub struct CliToolBridge {
    pub config: CliToolConfig,
    instructions: String,
    schema: Option<Value>,
}

impl CliToolBridge {
    /// Create a new CLI tool bridge from configuration
    pub fn new(config: CliToolConfig) -> Result<Self> {
        let instructions = Self::load_readme(&config)?;
        let schema = Self::load_schema(&config)?;

        Ok(CliToolBridge {
            config,
            instructions,
            schema,
        })
    }

    /// Create a bridge from a tool directory
    pub fn from_directory(tool_dir: &Path) -> Result<Self> {
        let config_path = tool_dir.join("tool.json");
        let config: CliToolConfig = if config_path.exists() {
            read_json_file_sync(&config_path)?
        } else {
            // Auto-discover tool configuration
            Self::auto_discover_config(tool_dir)?
        };

        Self::new(config)
    }

    /// Auto-discover tool configuration from directory
    fn auto_discover_config(tool_dir: &Path) -> Result<CliToolConfig> {
        // Look for executable files
        let executables = Self::find_executables(tool_dir)?;
        if executables.is_empty() {
            return Err(anyhow!(
                "No executable files found in {}",
                tool_dir.display()
            ));
        }

        // Look for README files
        let readme_files = Self::find_readmes(tool_dir)?;

        // Use first executable and README (if found)
        let executable_path = executables[0].clone();
        let readme_path = readme_files.first().cloned();

        // Try to determine tool name from executable
        let name = executable_path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| anyhow!("Invalid executable filename"))?
            .to_string();

        Ok(CliToolConfig {
            name: name.clone(),
            description: format!("CLI tool: {}", name),
            executable_path,
            readme_path,
            schema_path: None,
            timeout_seconds: Some(30),
            supports_json: false, // Will be tested during execution
            environment: None,
            working_dir: Some(tool_dir.to_path_buf()),
        })
    }

    /// Find executable files in directory
    fn find_executables(dir: &Path) -> Result<Vec<PathBuf>> {
        let mut executables = vec![];

        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_file() {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let metadata = entry.metadata()?;
                    let permissions = metadata.permissions();
                    if permissions.mode() & 0o111 != 0 {
                        executables.push(path);
                    }
                }

                #[cfg(windows)]
                {
                    if let Some(ext) = path.extension() {
                        if ext == "exe" || ext == "bat" || ext == "cmd" {
                            executables.push(path);
                        }
                    }
                }
            }
        }

        Ok(executables)
    }

    /// Find README files in directory
    fn find_readmes(dir: &Path) -> Result<Vec<PathBuf>> {
        let mut readmes = vec![];

        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            if let Some(_name) = path.file_name().and_then(|n| n.to_str()).filter(|n| {
                path.is_file() && n.to_lowercase().starts_with("readme") && n.ends_with(".md")
            }) {
                readmes.push(path);
            }
        }

        Ok(readmes)
    }

    /// Load README/documentation content
    fn load_readme(config: &CliToolConfig) -> Result<String> {
        if let Some(readme_path) = config.readme_path.as_ref().filter(|p| p.exists()) {
            return read_file_with_context_sync(readme_path, "README file");
        }

        // Generate basic instructions if no README
        Ok(format!(
            "# {}\n\nCLI tool: {}\n\nExecute with provided arguments.\n",
            config.name,
            config.executable_path.display()
        ))
    }

    /// Load JSON schema for validation
    fn load_schema(config: &CliToolConfig) -> Result<Option<Value>> {
        if let Some(schema_path) = config.schema_path.as_ref().filter(|p| p.exists()) {
            return Ok(Some(read_json_file_sync(schema_path)?));
        }

        Ok(None)
    }

    /// Execute the CLI tool with given arguments
    pub async fn execute_internal(&self, args: Value) -> Result<CliToolResult> {
        info!(
            "Executing CLI tool: {} with args: {:?}",
            self.config.name, args
        );

        let start_time = std::time::Instant::now();

        // Validate arguments against schema if available
        if let Some(schema) = &self.schema {
            self.validate_args(&args, schema)?;
        }

        // Build command
        let mut cmd = Command::new(&self.config.executable_path);

        // Set working directory
        if let Some(working_dir) = &self.config.working_dir {
            cmd.current_dir(working_dir);
        }

        // Set environment variables
        if let Some(env) = &self.config.environment {
            for (key, value) in env {
                cmd.env(key, value);
            }
        }

        // Configure I/O
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // Add arguments based on configuration and input
        self.configure_arguments(&mut cmd, &args)?;

        // Execute with timeout
        let timeout_duration = Duration::from_secs(self.config.timeout_seconds.unwrap_or(30));
        let output_result =
            async_utils::with_timeout(cmd.output(), timeout_duration, "CLI tool execution")
                .await??;

        let execution_time_ms = start_time.elapsed().as_millis() as u64;

        // Parse output
        let stdout = String::from_utf8_lossy(&output_result.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output_result.stderr).to_string();

        // Try to parse JSON output if supported
        let json_output = if self.config.supports_json {
            serde_json::from_str(&stdout).ok()
        } else {
            None
        };

        Ok(CliToolResult {
            exit_code: output_result.status.code().unwrap_or(-1),
            stdout,
            stderr,
            json_output,
            execution_time_ms,
        })
    }

    /// Configure command arguments based on input
    fn configure_arguments(&self, cmd: &mut Command, args: &Value) -> Result<()> {
        if args.is_null() || args == &Value::Null {
            return Ok(());
        }

        // Handle different argument formats
        match args {
            Value::String(s) => {
                // Single string argument
                cmd.arg(s);
            }
            Value::Array(arr) => {
                // Array of arguments
                for arg in arr {
                    if let Some(s) = arg.as_str() {
                        cmd.arg(s);
                    }
                }
            }
            Value::Object(map) => {
                // Named arguments - convert to command-line flags
                for (key, value) in map {
                    if let Some(s) = value.as_str() {
                        cmd.arg(format!("--{}", key));
                        cmd.arg(s);
                    } else if value.is_boolean() && value.as_bool().unwrap() {
                        cmd.arg(format!("--{}", key));
                    }
                }
            }
            _ => {
                // Fallback: serialize to JSON and pass as single argument
                let json_str = serde_json::to_string(args)?;
                cmd.arg(json_str);
            }
        }

        Ok(())
    }

    /// Validate arguments against JSON schema
    fn validate_args(&self, args: &Value, schema: &Value) -> Result<()> {
        // Basic validation - in production, use jsonschema crate
        debug!("Validating args against schema: {:?}", schema);

        // For now, just check required fields
        if let Some(required) = schema.get("required").and_then(|v| v.as_array()) {
            for field in required {
                if let Some(field_name) = field.as_str().filter(|f| args.get(*f).is_none()) {
                    return Err(anyhow!("Missing required field: {}", field_name));
                }
            }
        }

        Ok(())
    }

    /// Test if tool supports JSON I/O
    pub async fn test_json_support(&self) -> Result<bool> {
        debug!("Testing JSON support for tool: {}", self.config.name);

        // Try to execute with --help-json or similar flag
        let mut cmd = Command::new(&self.config.executable_path);
        cmd.arg("--help-json")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let result = cmd.output().await;

        match result {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                // Check if output is valid JSON
                Ok(serde_json::from_str::<Value>(&stdout).is_ok())
            }
            Err(_) => Ok(false),
        }
    }

    /// Convert to VT Code Skill
    pub fn to_skill(&self) -> Result<Skill> {
        let manifest = SkillManifest {
            name: self.config.name.clone(),
            description: self.config.description.clone(),
            version: Some("1.0.0".to_string()),
            author: Some("VT Code CLI Bridge".to_string()),
            variety: crate::skills::types::SkillVariety::SystemUtility,
            ..Default::default()
        };

        let mut skill = Skill::new(
            manifest,
            self.config.executable_path.parent().unwrap().to_path_buf(),
            self.instructions.clone(),
        )?;

        // Add schema as resource if available
        if let Some(schema) = &self.schema {
            skill.add_resource(
                "schema.json".to_string(),
                SkillResource {
                    path: "schema.json".to_string(),
                    resource_type: crate::skills::types::ResourceType::Reference,
                    content: Some(schema.to_string().into_bytes()),
                },
            );
        }

        Ok(skill)
    }
}

#[async_trait]
impl Tool for CliToolBridge {
    fn name(&self) -> &'static str {
        // We need a 'static str, so we leak the name.
        // Since tools are discovered once and kept, this is acceptable.
        Box::leak(self.config.name.clone().into_boxed_str())
    }

    fn description(&self) -> &'static str {
        Box::leak(self.config.description.clone().into_boxed_str())
    }

    fn parameter_schema(&self) -> Option<Value> {
        self.schema.clone()
    }

    fn default_permission(&self) -> ToolPolicy {
        ToolPolicy::Prompt
    }

    async fn execute(&self, args: Value) -> Result<Value> {
        let result = self.execute_internal(args).await?;
        Ok(serde_json::to_value(result)?)
    }
}

/// Discover CLI tools in standard locations
pub fn discover_cli_tools() -> Result<Vec<CliToolConfig>> {
    let mut tools = vec![];

    // Standard locations to search
    let search_paths = vec![
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/usr/bin"),
        PathBuf::from("~/.local/bin").expand_home()?,
        PathBuf::from("./tools"),
        PathBuf::from("./vendor/tools"),
    ];

    for path in search_paths {
        if path.exists() && path.is_dir() {
            match discover_tools_in_directory(&path) {
                Ok(dir_tools) => tools.extend(dir_tools),
                Err(e) => warn!("Failed to discover tools in {}: {}", path.display(), e),
            }
        }
    }

    info!("Discovered {} CLI tools", tools.len());
    Ok(tools)
}

/// Discover tools in a specific directory
fn discover_tools_in_directory(dir: &Path) -> Result<Vec<CliToolConfig>> {
    let mut tools = vec![];

    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_file() {
            // Check if it's an executable
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let metadata = entry.metadata()?;
                let permissions = metadata.permissions();
                if permissions.mode() & 0o111 == 0 {
                    continue;
                }
            }

            #[cfg(windows)]
            {
                if let Some(ext) = path.extension() {
                    if ext != "exe" && ext != "bat" && ext != "cmd" {
                        continue;
                    }
                } else {
                    continue;
                }
            }

            // Look for accompanying README
            let readme_path = dir.join(format!(
                "{}.md",
                path.file_stem().unwrap().to_str().unwrap()
            ));

            let config = CliToolConfig {
                name: path.file_stem().unwrap().to_str().unwrap().to_string(),
                description: format!("CLI tool: {}", path.display()),
                executable_path: path.clone(),
                readme_path: if readme_path.exists() {
                    Some(readme_path)
                } else {
                    None
                },
                schema_path: None,
                timeout_seconds: Some(30),
                supports_json: false,
                environment: None,
                working_dir: Some(dir.to_path_buf()),
            };

            tools.push(config);
        }
    }

    Ok(tools)
}

/// Extension trait for PathBuf to expand home directory
trait PathExt {
    fn expand_home(&self) -> Result<PathBuf>;
}

impl PathExt for PathBuf {
    fn expand_home(&self) -> Result<PathBuf> {
        if let Some(home) = std::env::var("HOME").ok().filter(|_| self.starts_with("~")) {
            return Ok(PathBuf::from(home).join(self.strip_prefix("~").unwrap()));
        }
        Ok(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[allow(unused_imports)]
    use tempfile::TempDir;

    #[test]
    fn test_cli_tool_config_creation() {
        let config = CliToolConfig {
            name: "test-tool".to_string(),
            description: "Test tool".to_string(),
            executable_path: PathBuf::from("/bin/echo"),
            readme_path: None,
            schema_path: None,
            timeout_seconds: Some(10),
            supports_json: false,
            environment: None,
            working_dir: None,
        };

        assert_eq!(config.name, "test-tool");
        assert_eq!(config.timeout_seconds, Some(10));
    }

    #[tokio::test]
    async fn test_simple_tool_execution() {
        let config = CliToolConfig {
            name: "echo".to_string(),
            description: "Echo command".to_string(),
            executable_path: PathBuf::from("/bin/echo"),
            readme_path: None,
            schema_path: None,
            timeout_seconds: Some(5),
            supports_json: false,
            environment: None,
            working_dir: None,
        };

        let bridge = CliToolBridge::new(config).unwrap();
        let result = bridge
            .execute_internal(Value::String("hello world".to_string()))
            .await
            .unwrap();

        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("hello world"));
    }
}
