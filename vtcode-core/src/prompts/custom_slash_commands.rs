use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::warn;

/// Configuration for custom slash commands
#[derive(Debug, Clone, Default)]
pub struct CustomSlashCommandConfig {
    pub enabled: bool,
    pub directory: String,
    pub extra_directories: Vec<String>,
    pub max_file_size_kb: usize,
}

impl CustomSlashCommandConfig {
    pub fn default() -> Self {
        Self {
            enabled: true,
            directory: "~/.vtcode/commands".to_string(),
            extra_directories: vec![],
            max_file_size_kb: 64, // 64KB default
        }
    }
}

/// Registry for custom slash commands loaded from markdown files
#[derive(Debug, Clone, Default)]
pub struct CustomSlashCommandRegistry {
    enabled: bool,
    directories: Vec<PathBuf>,
    commands: BTreeMap<String, CustomSlashCommand>,
}

impl CustomSlashCommandRegistry {
    pub async fn load(config: Option<&CustomSlashCommandConfig>, workspace: &Path) -> Result<Self> {
        let settings = config
            .cloned()
            .unwrap_or_else(|| CustomSlashCommandConfig::default());

        if !settings.enabled {
            return Ok(Self {
                enabled: false,
                directories: vec![],
                commands: BTreeMap::new(),
            });
        }

        let directories = resolve_directories(&settings, workspace);
        let max_bytes = if settings.max_file_size_kb == 0 {
            usize::MAX
        } else {
            settings.max_file_size_kb.saturating_mul(1024)
        };

        let mut commands = BTreeMap::new();

        // Load commands from all directories
        for directory in &directories {
            if !fs::try_exists(directory).await.unwrap_or(false) {
                continue;
            }

            if !directory.is_dir() {
                warn!(
                    "custom slash command path `{}` is not a directory - skipping",
                    directory.display()
                );
                continue;
            }

            match fs::read_dir(directory).await {
                Ok(mut entries) => {
                    while let Ok(Some(entry)) = entries.next_entry().await {
                        let path = entry.path();
                        if !path.is_file() || !is_markdown_file(&path) {
                            continue;
                        }

                        match CustomSlashCommand::from_file(&path, max_bytes).await {
                            Ok(Some(command)) => {
                                let key = command.name.to_ascii_lowercase();
                                if commands.contains_key(&key) {
                                    warn!(
                                        "duplicate custom slash command `{}` detected at {}; keeping first occurrence",
                                        command.name,
                                        path.display()
                                    );
                                    continue;
                                }
                                commands.insert(key, command);
                            }
                            Ok(None) => {}
                            Err(err) => {
                                warn!(
                                    "failed to load custom slash command from {}: {err:#}",
                                    path.display()
                                );
                            }
                        }
                    }
                }
                Err(err) => {
                    warn!(
                        "failed to read custom slash command directory `{}`: {err}",
                        directory.display()
                    );
                }
            }
        }

        Ok(Self {
            enabled: true,
            directories,
            commands,
        })
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    pub fn directories(&self) -> &[PathBuf] {
        &self.directories
    }

    pub fn iter(&self) -> impl Iterator<Item = &CustomSlashCommand> {
        self.commands.values()
    }

    pub fn get(&self, name: &str) -> Option<&CustomSlashCommand> {
        self.commands.get(&name.to_ascii_lowercase())
    }

    pub fn get_command_names(&self) -> Vec<String> {
        self.commands.keys().cloned().collect()
    }
}

use regex::Regex;

/// A custom slash command loaded from a markdown file
#[derive(Debug, Clone)]
pub struct CustomSlashCommand {
    pub name: String,
    pub description: Option<String>,
    pub argument_hint: Option<String>,
    pub allowed_tools: Option<Vec<String>>,
    pub disable_model_invocation: bool,
    pub model: Option<String>,
    pub path: PathBuf,
    pub content: String,
    pub has_bash_execution: bool, // Whether the command contains bash execution (!`command`)
}

impl CustomSlashCommand {
    async fn from_file(path: &Path, max_bytes: usize) -> Result<Option<Self>> {
        let Some(stem) = path.file_stem().and_then(|value| value.to_str()) else {
            warn!(
                "skipping custom slash command with non-UTF-8 filename: {}",
                path.display()
            );
            return Ok(None);
        };

        if stem.trim().is_empty() {
            warn!(
                "skipping custom slash command with empty name at {}",
                path.display()
            );
            return Ok(None);
        }

        if stem.chars().any(|ch| ch.is_whitespace() || ch == ':') {
            warn!(
                "custom slash command names must not contain whitespace or colons; `{}` skipped",
                stem
            );
            return Ok(None);
        }

        let metadata = fs::metadata(path).await.map_err(|e| {
            anyhow::anyhow!("failed to read metadata for {}: {}", path.display(), e)
        })?;
        if metadata.len() as usize > max_bytes {
            warn!(
                "custom slash command `{}` exceeds max_file_size_kb ({:.1} KB) - skipping",
                stem,
                metadata.len() as f64 / 1024.0
            );
            return Ok(None);
        }

        let contents = fs::read_to_string(path).await.map_err(|e| {
            anyhow::anyhow!(
                "failed to read custom slash command from {}: {}",
                path.display(),
                e
            )
        })?;

        Self::from_contents(stem, path, &contents)
    }

    fn from_contents(name: &str, path: &Path, contents: &str) -> Result<Option<Self>> {
        let (frontmatter, body) = split_frontmatter(contents).map_err(|e| {
            anyhow::anyhow!("failed to parse frontmatter in {}: {}", path.display(), e)
        })?;

        if body.trim().is_empty() {
            warn!(
                "custom slash command `{}` has no content after frontmatter; skipping",
                name
            );
            return Ok(None);
        }

        let has_bash_execution = body.contains("!`") && body.contains("`");

        let command = CustomSlashCommand {
            name: name.to_owned(),
            description: frontmatter.as_ref().and_then(|fm| fm.description.clone()),
            argument_hint: frontmatter.as_ref().and_then(|fm| fm.argument_hint.clone()),
            allowed_tools: frontmatter
                .as_ref()
                .and_then(|fm| fm.allowed_tools.as_ref())
                .and_then(|field| normalize_allowed_tools_list(field).ok()),
            disable_model_invocation: frontmatter
                .as_ref()
                .map(|fm| fm.disable_model_invocation)
                .unwrap_or(false),
            model: frontmatter.as_ref().and_then(|fm| fm.model.clone()),
            path: path.to_path_buf(),
            content: body.trim_start().to_string(),
            has_bash_execution,
        };

        Ok(Some(command))
    }

    /// Expand the command content with arguments
    pub fn expand_content(&self, args: &str) -> String {
        let mut content = self.content.clone();

        // Replace $ARGUMENTS with all arguments
        if !args.trim().is_empty() {
            content = content.replace("$ARGUMENTS", args);
        }

        // Replace $1, $2, etc. with positional arguments
        let positional_args: Vec<&str> = args.split_whitespace().collect();
        for (i, arg) in positional_args.iter().enumerate() {
            let placeholder = format!("${}", i + 1);
            content = content.replace(&placeholder, arg);
        }

        content
    }

    /// Process the command content, executing bash commands and expanding arguments
    pub fn process_content(&self, args: &str) -> Result<String> {
        let expanded_content = self.expand_content(args);

        // If the command has bash execution, process the bash commands
        if self.has_bash_execution {
            self.execute_bash_commands(&expanded_content)
        } else {
            Ok(expanded_content)
        }
    }

    /// Execute bash commands in the content and replace them with their output
    fn execute_bash_commands(&self, content: &str) -> Result<String> {
        // Find all occurrences of !`command` patterns
        let re = Regex::new(r"!`([^`]+)`")?;
        let mut result = content.to_string();

        for cap in re.captures_iter(content) {
            if let Some(command_match) = cap.get(1) {
                let command = command_match.as_str();

                // Execute the command and get output
                let output = self.run_bash_command(command)?;

                // Replace the !`command` with the output
                let placeholder = format!("!`{}`", command);
                result = result.replace(&placeholder, &output);
            }
        }

        Ok(result)
    }

    /// Run a bash command and return its output
    fn run_bash_command(&self, command: &str) -> Result<String> {
        // Comprehensive command validation for security
        use std::process::Command;

        self.validate_command(command)?;

        let output = Command::new("sh")
            .arg("-c")
            .arg(command)
            .output()
            .map_err(|e| anyhow::anyhow!("Failed to execute command: {}: {}", command, e))?;

        let stdout = String::from_utf8(output.stdout)
            .map_err(|e| anyhow::anyhow!("Failed to parse stdout: {}", e))?;
        let stderr = String::from_utf8(output.stderr)
            .map_err(|e| anyhow::anyhow!("Failed to parse stderr: {}", e))?;

        // Combine stdout and stderr, trimming whitespace
        let result = format!("{}{}", stdout, stderr).trim().to_string();

        Ok(result)
    }

    /// Validate a command for security risks
    fn validate_command(&self, command: &str) -> Result<()> {
        use regex::Regex;

        // Block absolutely forbidden commands that cannot be used in any context
        let forbidden_commands = [
            ("dd", "Low-level disk writing"),
            ("mkfs", "Filesystem creation"),
            ("fdisk", "Disk partitioning"),
            ("parted", "Partition modification"),
            ("mount", "Filesystem mounting"),
            ("umount", "Filesystem unmounting"),
            ("rm", "File deletion"),
            ("mv", "File moving"),
            ("cp", "File copying"),
            ("chmod", "Permission modification"),
            ("chown", "Ownership modification"),
        ];

        let cmd_trimmed = command.trim_start();
        for (forbidden, reason) in &forbidden_commands {
            // Check if command starts with the forbidden command followed by space or end
            if cmd_trimmed.starts_with(forbidden) {
                let len = forbidden.len();
                if cmd_trimmed.len() == len
                    || !cmd_trimmed[len..]
                        .chars()
                        .next()
                        .unwrap_or(' ')
                        .is_alphanumeric()
                {
                    return Err(anyhow::anyhow!(
                        "Command execution forbidden for safety: {} ({})",
                        forbidden,
                        reason
                    ));
                }
            }
        }

        // Block access to sensitive system directories
        let sensitive_paths = [
            "/etc/",
            "/proc/",
            "/sys/",
            "/dev/",
            "/boot/",
            "/root/",
            "/var/log/",
        ];
        for path in &sensitive_paths {
            if command.contains(path) {
                return Err(anyhow::anyhow!(
                    "Command access to sensitive path forbidden: {}",
                    path
                ));
            }
        }

        // Block path traversal attempts
        if command.contains("..") {
            return Err(anyhow::anyhow!("Path traversal sequences not allowed"));
        }

        // Block redirection to special files
        if let Ok(redirect_re) = Regex::new(r"[><]\s*(?:/dev/|/proc/|/sys/)") {
            if redirect_re.is_match(command) {
                return Err(anyhow::anyhow!("Redirection to system files not allowed"));
            }
        }

        Ok(())
    }
}

#[derive(Debug, Deserialize, Default)]
struct CustomSlashCommandFrontmatter {
    #[serde(default)]
    description: Option<String>,
    #[serde(default, alias = "argument_hint", alias = "argument-hint")]
    argument_hint: Option<String>,
    #[serde(default, alias = "allowed-tools", alias = "allowed_tools")]
    allowed_tools: Option<AllowedToolsField>,
    #[serde(
        default,
        alias = "disable-model-invocation",
        alias = "disable_model_invocation"
    )]
    disable_model_invocation: bool,
    #[serde(default, alias = "model")]
    model: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum AllowedToolsField {
    List(Vec<String>),
    String(String),
}

/// Parse frontmatter from markdown content
fn split_frontmatter(contents: &str) -> Result<(Option<CustomSlashCommandFrontmatter>, &str)> {
    let Some(remaining) = contents.strip_prefix("---") else {
        return Ok((None, contents));
    };

    let remainder = if let Some(rest) = remaining.strip_prefix("\r\n") {
        rest
    } else if let Some(rest) = remaining.strip_prefix('\n') {
        rest
    } else {
        return Ok((None, contents));
    };

    let mut end_offset = None;
    let mut consumed = 0usize;
    for line in remainder.split_inclusive(['\n']) {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed == "---" {
            end_offset = Some(consumed);
            consumed += line.len();
            break;
        }
        consumed += line.len();
    }

    let Some(end) = end_offset else {
        return Ok((None, contents));
    };

    let frontmatter_raw = &remainder[..end];
    let body_start = consumed;
    let body = &remainder[body_start..];
    let frontmatter: CustomSlashCommandFrontmatter =
        serde_yaml::from_str(frontmatter_raw).context("invalid YAML frontmatter")?;
    Ok((Some(frontmatter), body))
}

fn resolve_directories(config: &CustomSlashCommandConfig, workspace: &Path) -> Vec<PathBuf> {
    let mut resolved: BTreeSet<PathBuf> = BTreeSet::new();

    // Add primary directory
    resolved.insert(resolve_directory(&config.directory, workspace));

    // Add extra directories
    for extra in &config.extra_directories {
        resolved.insert(resolve_directory(extra, workspace));
    }

    // Add project-specific commands directory if it exists
    let project_commands = workspace.join(".vtcode").join("commands");
    if project_commands.exists() {
        resolved.insert(project_commands);
    }

    let project_claude_commands = workspace.join(".claude").join("commands");
    if project_claude_commands.exists() {
        resolved.insert(project_claude_commands);
    }

    let user_claude_commands = resolve_directory("~/.claude/commands", workspace);
    if user_claude_commands.exists() {
        resolved.insert(user_claude_commands);
    }

    resolved.into_iter().collect()
}

fn normalize_allowed_tools_list(field: &AllowedToolsField) -> Result<Vec<String>> {
    match field {
        AllowedToolsField::List(tools) => Ok(tools.clone()),
        AllowedToolsField::String(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                return Ok(vec![]);
            }
            let parts = if trimmed.contains(',') {
                trimmed
                    .split(',')
                    .map(|part| part.trim())
                    .filter(|part| !part.is_empty())
                    .map(|part| part.to_string())
                    .collect::<Vec<_>>()
            } else {
                trimmed
                    .split_whitespace()
                    .map(|part| part.to_string())
                    .collect::<Vec<_>>()
            };
            Ok(parts)
        }
    }
}

fn resolve_directory(value: &str, workspace: &Path) -> PathBuf {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return resolve_directory("~/.vtcode/commands", workspace);
    }

    if let Some(stripped) = trimmed
        .strip_prefix("~/")
        .or_else(|| trimmed.strip_prefix("~\\"))
    {
        if let Some(home) = dirs::home_dir() {
            return home.join(stripped);
        }
        return PathBuf::from(stripped);
    }

    let candidate = Path::new(trimmed);
    if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        workspace.join(candidate)
    }
}

fn is_markdown_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("md"))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_custom_slash_command_registry_loads_from_directory() {
        let temp = tempdir().unwrap();
        let commands_dir = temp.path().join("commands");
        fs::create_dir_all(&commands_dir).await.unwrap();
        fs::write(
            commands_dir.join("review.md"),
            "---\ndescription: Review code\nargument-hint: [file]\n---\nReview the file: $ARGUMENTS"
        ).await.unwrap();

        let mut cfg = CustomSlashCommandConfig::default();
        cfg.directory = commands_dir.to_string_lossy().into_owned();
        let registry = CustomSlashCommandRegistry::load(Some(&cfg), temp.path())
            .await
            .expect("load registry");

        assert!(registry.enabled());
        assert!(!registry.is_empty());
        let command = registry.get("review").unwrap();
        assert_eq!(command.name, "review");
        assert_eq!(command.description.as_deref(), Some("Review code"));
        assert_eq!(command.argument_hint.as_deref(), Some("[file]"));
    }

    #[tokio::test]
    async fn test_custom_slash_command_expansion() {
        let temp = tempdir().unwrap();
        let commands_dir = temp.path().join("commands");
        fs::create_dir_all(&commands_dir).await.unwrap();
        fs::write(
            commands_dir.join("test.md"),
            "---\ndescription: Test command\n---\nProcess $1 and $2 with $ARGUMENTS",
        )
        .await
        .unwrap();

        let mut cfg = CustomSlashCommandConfig::default();
        cfg.directory = commands_dir.to_string_lossy().into_owned();
        let registry = CustomSlashCommandRegistry::load(Some(&cfg), temp.path())
            .await
            .expect("load registry");

        let command = registry.get("test").unwrap();
        let expanded = command.expand_content("file1.txt file2.txt");

        assert!(expanded.contains("Process file1.txt and file2.txt"));
        assert!(expanded.contains("file1.txt file2.txt"));
    }
}
