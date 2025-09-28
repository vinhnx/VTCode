//! Simple bash-like search tool
//!
//! This tool provides direct search capabilities similar to common
//! bash commands such as grep, find, ls, and cat.

use super::traits::Tool;
use crate::config::constants::tools;
use crate::simple_indexer::SimpleIndexer;
use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::{Value, json};
use std::{path::PathBuf, process::Stdio, time::Duration};
use tokio::{process::Command, time::timeout};

/// Simple bash-like search tool
#[derive(Clone)]
pub struct SimpleSearchTool {
    indexer: SimpleIndexer,
}

impl SimpleSearchTool {
    /// Create a new simple search tool
    pub fn new(workspace_root: PathBuf) -> Self {
        let indexer = SimpleIndexer::new(workspace_root.clone());
        indexer.init().unwrap_or_else(|e| {
            eprintln!("Warning: Failed to initialize indexer: {}", e);
        });

        Self { indexer }
    }

    /// Execute command and capture its stdout
    async fn execute_pty_command(
        &self,
        command: &str,
        args: Vec<String>,
        timeout_secs: Option<u64>,
    ) -> Result<String> {
        let full_command_parts = std::iter::once(command.to_string())
            .chain(args.clone())
            .collect::<Vec<String>>();
        self.validate_command(&full_command_parts)?;

        let full_command = if args.is_empty() {
            command.to_string()
        } else {
            format!("{} {}", command, args.join(" "))
        };

        let work_dir = self.indexer.workspace_root().to_path_buf();
        let mut cmd = Command::new(command);
        if !args.is_empty() {
            cmd.args(&args);
        }
        cmd.current_dir(&work_dir);
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let duration = Duration::from_secs(timeout_secs.unwrap_or(30));
        let output = timeout(duration, cmd.output())
            .await
            .with_context(|| {
                format!(
                    "command '{}' timed out after {}s",
                    full_command,
                    duration.as_secs()
                )
            })?
            .with_context(|| format!("Failed to execute command: {}", full_command))?;

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// Validate command for security
    fn validate_command(&self, command_parts: &[String]) -> Result<()> {
        if command_parts.is_empty() {
            return Err(anyhow::anyhow!("Command cannot be empty"));
        }

        let program = &command_parts[0];

        // For SimpleSearchTool, we only allow safe read-only commands
        let allowed_commands = [
            "grep", "find", "ls", "cat", "head", "tail", "wc", "sort", "uniq", "cut", "tr", "fold",
        ];

        if !allowed_commands.contains(&program.as_str()) {
            return Err(anyhow::anyhow!(
                "Command '{}' is not allowed in SimpleSearchTool. \
                 Only safe read-only commands are permitted: {}",
                program,
                allowed_commands.join(", ")
            ));
        }

        // Additional validation for specific commands
        let full_command = command_parts.join(" ");

        // Prevent access to sensitive directories
        let sensitive_paths = [
            "/etc/", "/usr/", "/var/", "/root/", "/boot/", "/sys/", "/proc/", "/home/",
        ];
        for path in &sensitive_paths {
            if full_command.contains(path) {
                return Err(anyhow::anyhow!(
                    "Access to system directory '{}' is not allowed. \
                     Work within your project workspace only.",
                    path.trim_end_matches('/')
                ));
            }
        }

        // Prevent dangerous grep/find patterns
        if (program == "grep" || program == "find")
            && (full_command.contains(" -exec")
                || full_command.contains(" -delete")
                || full_command.contains(" -execdir"))
        {
            return Err(anyhow::anyhow!(
                "Dangerous execution patterns in {} command are not allowed.",
                program
            ));
        }

        Ok(())
    }

    /// Execute grep-like search
    async fn grep(&self, args: Value) -> Result<Value> {
        let pattern = args
            .get("pattern")
            .and_then(|v| v.as_str())
            .context("pattern is required for grep")?;

        let file_pattern = args.get("file_pattern").and_then(|v| v.as_str());
        let max_results = args
            .get("max_results")
            .and_then(|v| v.as_u64())
            .unwrap_or(50) as usize;

        // Build grep command
        let mut cmd_args = vec![pattern.to_string()];
        if let Some(file_pat) = file_pattern {
            cmd_args.push("--include".to_string());
            cmd_args.push(format!("*{}*", file_pat));
        }
        cmd_args.push("-r".to_string()); // recursive
        cmd_args.push("-n".to_string()); // line numbers
        cmd_args.push(".".to_string()); // current directory

        let output = self
            .execute_pty_command("grep", cmd_args, Some(30))
            .await
            .context("Failed to execute grep")?;

        // Parse and limit results
        let lines: Vec<&str> = output.lines().collect();
        let limited_lines: Vec<&str> = lines.into_iter().take(max_results).collect();

        Ok(json!({
            "command": "grep",
            "pattern": pattern,
            "results": limited_lines,
            "count": limited_lines.len(),
            "mode": "pty",
            "pty_enabled": true
        }))
    }

    /// Execute find-like file search
    async fn find(&self, args: Value) -> Result<Value> {
        let pattern = args
            .get("pattern")
            .and_then(|v| v.as_str())
            .context("pattern is required for find")?;

        // Build find command
        let cmd_args = vec![
            ".".to_string(),
            "-name".to_string(),
            format!("*{}*", pattern),
            "-type".to_string(),
            "f".to_string(),
        ];

        let output = self
            .execute_pty_command("find", cmd_args, Some(30))
            .await
            .context("Failed to execute find")?;

        let files: Vec<&str> = output.lines().collect();

        Ok(json!({
            "command": "find",
            "pattern": pattern,
            "files": files,
            "count": files.len(),
            "mode": "pty",
            "pty_enabled": true
        }))
    }

    /// Execute ls-like directory listing
    async fn ls(&self, args: Value) -> Result<Value> {
        let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");

        let show_hidden = args
            .get("show_hidden")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        // Build ls command
        let mut cmd_args = vec![];
        if show_hidden {
            cmd_args.push("-la".to_string());
        } else {
            cmd_args.push("-l".to_string());
        }
        cmd_args.push(path.to_string());

        let output = self
            .execute_pty_command("ls", cmd_args, Some(10))
            .await
            .context("Failed to execute ls")?;

        let files: Vec<&str> = output.lines().collect();

        Ok(json!({
            "command": "ls",
            "path": path,
            "files": files,
            "count": files.len(),
            "show_hidden": show_hidden,
            "mode": "pty",
            "pty_enabled": true
        }))
    }

    /// Execute cat-like file content reading
    async fn cat(&self, args: Value) -> Result<Value> {
        let file_path = args
            .get("file_path")
            .and_then(|v| v.as_str())
            .context("file_path is required for cat")?;

        let start_line = args
            .get("start_line")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize);
        let end_line = args
            .get("end_line")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize);

        let mut cmd_args = vec![];
        if let (Some(start), Some(end)) = (start_line, end_line) {
            // Use sed to extract line range
            let sed_cmd = format!("sed -n '{}','{}'p {}", start, end, file_path);
            cmd_args = vec!["-c".to_string(), sed_cmd];
            let output = self
                .execute_pty_command("sh", cmd_args, Some(10))
                .await
                .context("Failed to execute sed")?;
            return Ok(json!({
                "command": "cat",
                "file_path": file_path,
                "content": output,
                "start_line": start,
                "end_line": end,
                "mode": "pty",
                "pty_enabled": true
            }));
        }

        cmd_args.push(file_path.to_string());
        let output = self
            .execute_pty_command("cat", cmd_args, Some(10))
            .await
            .context("Failed to execute cat")?;

        Ok(json!({
            "command": "cat",
            "file_path": file_path,
            "content": output,
            "start_line": start_line,
            "end_line": end_line,
            "mode": "pty",
            "pty_enabled": true
        }))
    }

    /// Execute head-like file preview
    async fn head(&self, args: Value) -> Result<Value> {
        let file_path = args
            .get("file_path")
            .and_then(|v| v.as_str())
            .context("file_path is required for head")?;

        let lines = args.get("lines").and_then(|v| v.as_u64()).unwrap_or(10) as usize;

        let cmd_args = vec!["-n".to_string(), lines.to_string(), file_path.to_string()];

        let output = self
            .execute_pty_command("head", cmd_args, Some(10))
            .await
            .context("Failed to execute head")?;

        Ok(json!({
            "command": "head",
            "file_path": file_path,
            "content": output,
            "lines": lines,
            "mode": "pty",
            "pty_enabled": true
        }))
    }

    /// Execute tail-like file preview
    async fn tail(&self, args: Value) -> Result<Value> {
        let file_path = args
            .get("file_path")
            .and_then(|v| v.as_str())
            .context("file_path is required for tail")?;

        let lines = args.get("lines").and_then(|v| v.as_u64()).unwrap_or(10) as usize;

        let cmd_args = vec!["-n".to_string(), lines.to_string(), file_path.to_string()];

        let output = self
            .execute_pty_command("tail", cmd_args, Some(10))
            .await
            .context("Failed to execute tail")?;

        Ok(json!({
            "command": "tail",
            "file_path": file_path,
            "content": output,
            "lines": lines,
            "mode": "pty",
            "pty_enabled": true
        }))
    }
}

#[async_trait]
impl Tool for SimpleSearchTool {
    async fn execute(&self, args: Value) -> Result<Value> {
        let command = args
            .get("command")
            .and_then(|v| v.as_str())
            .unwrap_or("grep");

        match command {
            "grep" => self.grep(args).await,
            "find" => self.find(args).await,
            "ls" => self.ls(args).await,
            "cat" => self.cat(args).await,
            "head" => self.head(args).await,
            "tail" => self.tail(args).await,
            _ => Err(anyhow::anyhow!("Unknown command: {}", command)),
        }
    }

    fn name(&self) -> &'static str {
        tools::SIMPLE_SEARCH
    }

    fn description(&self) -> &'static str {
        "Simple bash-like search and file operations with security validation: grep, find, ls, cat, head, tail, index. \
         Only safe read-only operations are allowed - no file modifications or dangerous commands."
    }
}
