//! Bash-like tool for command execution with sandbox integration
//!
//! This tool provides bash-like functionality for running common
//! commands and tools that require a shell environment. It includes
//! integration with Anthropic's sandbox runtime for secure command
//! execution with configurable permissions.
//!
//! # Sandbox Integration
//!
//! When sandboxing is enabled via the `SandboxProfile`, all commands are
//! executed through the Anthropic sandbox runtime (`srt`) which provides:
//! - Filesystem isolation within the project workspace
//! - Network access control via domain allowlists
//! - Prevention of access to sensitive system locations
//! - Secure execution environment for AI agent commands

use super::traits::Tool;
use crate::config::constants::tools;
use crate::execpolicy::sanitize_working_dir;
use crate::sandbox::SandboxProfile;
use crate::tools::pty::{PtyCommandRequest, PtyManager};
use anyhow::{Context, Result};
use async_trait::async_trait;
use portable_pty::PtySize;
use serde_json::{Value, json};
use shell_words::join;
use std::{path::PathBuf, process::Stdio, time::Duration};
use tokio::{process::Command, time::timeout};

/// Bash-like tool for command execution with optional Anthropic sandbox runtime integration
///
/// This tool provides secure command execution with the option to run commands through
/// Anthropic's sandbox runtime for enhanced security. When a `SandboxProfile` is set,
/// all commands are executed in a restricted environment with configurable filesystem
/// and network permissions.
#[derive(Clone)]
pub struct BashTool {
    workspace_root: PathBuf,
    pty_manager: Option<PtyManager>,
    sandbox_profile: Option<SandboxProfile>,
}

impl BashTool {
    /// Create a new bash tool
    pub fn new(workspace_root: PathBuf) -> Self {
        Self {
            workspace_root,
            pty_manager: None,
            sandbox_profile: None,
        }
    }

    /// Attach a PTY manager so commands can execute in true PTY mode.
    pub fn set_pty_manager(&mut self, manager: PtyManager) {
        if let Some(profile) = &self.sandbox_profile {
            manager.set_sandbox_profile(Some(profile.clone()));
        }
        self.pty_manager = Some(manager);
    }

    pub fn set_sandbox_profile(&mut self, profile: Option<SandboxProfile>) {
        self.sandbox_profile = profile.clone();
        if let Some(manager) = &self.pty_manager {
            manager.set_sandbox_profile(profile);
        }
    }

    /// Execute command and capture its output
    async fn execute_command(
        &self,
        command: &str,
        args: Vec<String>,
        working_dir: Option<&str>,
        timeout_secs: Option<u64>,
        prefer_pty: bool,
    ) -> Result<Value> {
        let command_parts = std::iter::once(command.to_string())
            .chain(args.iter().cloned())
            .collect::<Vec<String>>();
        self.validate_command(&command_parts)?;

        let full_command = if args.is_empty() {
            command.to_string()
        } else {
            format!("{} {}", command, args.join(" "))
        };

        if prefer_pty {
            if let Some(manager) = self.pty_manager.as_ref().filter(|mgr| mgr.config().enabled) {
                let working_dir_path = manager
                    .resolve_working_dir(working_dir)
                    .context("failed to resolve working directory for PTY command")?;
                let config = manager.config();
                let timeout_value = timeout_secs.unwrap_or(config.command_timeout_seconds);
                if timeout_value == 0 {
                    anyhow::bail!("timeout_secs must be greater than zero");
                }

                let request = PtyCommandRequest {
                    command: command_parts.clone(),
                    working_dir: working_dir_path.clone(),
                    timeout: Duration::from_secs(timeout_value),
                    size: PtySize {
                        rows: config.default_rows,
                        cols: config.default_cols,
                        pixel_width: 0,
                        pixel_height: 0,
                    },
                };

                let result = manager
                    .run_command(request)
                    .await
                    .with_context(|| format!("failed to execute PTY command: {}", full_command))?;

                let exit_code = result.exit_code;
                let duration = result.duration;
                let size = result.size;
                let output_text = result.output;
                let success = exit_code == 0;
                let working_directory = manager.describe_working_dir(&working_dir_path);

                return Ok(json!({
                    "success": success,
                    "exit_code": exit_code,
                    "stdout": output_text.clone(),
                    "stderr": "",
                    "output": output_text,
                    "mode": "pty",
                    "pty_enabled": true,
                    "command": full_command,
                    "working_directory": working_directory,
                    "timeout_secs": timeout_value,
                    "duration_ms": duration.as_millis(),
                    "pty": {
                        "rows": size.rows,
                        "cols": size.cols,
                    },
                }));
            }
        }

        if let Some(profile) = &self.sandbox_profile {
            return self
                .execute_sandboxed(
                    profile,
                    &command_parts,
                    &full_command,
                    working_dir,
                    timeout_secs,
                )
                .await;
        }

        let work_dir = sanitize_working_dir(&self.workspace_root, working_dir)
            .context("failed to resolve working directory for command")?;

        let mut cmd = Command::new(command);
        if !args.is_empty() {
            cmd.args(&args);
        }
        cmd.current_dir(&work_dir);
        // Ensure tools like git bypass interactive pagers.
        cmd.env("PAGER", "cat");
        cmd.env("GIT_PAGER", "cat");
        cmd.env("LESS", "R");
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let timeout_value = timeout_secs.unwrap_or(30);
        let duration = Duration::from_secs(timeout_value);
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
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let success = output.status.success();

        Ok(json!({
            "success": success,
            "exit_code": output.status.code().unwrap_or_default(),
            "stdout": stdout.clone(),
            "stderr": stderr,
            "output": stdout,
            "mode": "terminal",
            "pty_enabled": false,
            "command": full_command,
            "working_directory": work_dir.display().to_string(),
            "timeout_secs": timeout_value,
        }))
    }

    async fn execute_sandboxed(
        &self,
        profile: &SandboxProfile,
        command_parts: &[String],
        full_command: &str,
        working_dir: Option<&str>,
        timeout_secs: Option<u64>,
    ) -> Result<Value> {
        let work_dir = sanitize_working_dir(&self.workspace_root, working_dir)
            .context("failed to resolve working directory for sandboxed command")?;
        let command_string = join(command_parts.iter().map(|part| part.as_str()));

        let mut cmd = Command::new(profile.binary());
        cmd.arg("--settings");
        cmd.arg(profile.settings());
        cmd.arg(&command_string);
        cmd.current_dir(&work_dir);
        cmd.env("PAGER", "cat");
        cmd.env("GIT_PAGER", "cat");
        cmd.env("LESS", "R");
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let timeout_value = timeout_secs.unwrap_or(30);
        let duration = Duration::from_secs(timeout_value);
        let output = timeout(duration, cmd.output())
            .await
            .with_context(|| {
                format!(
                    "sandboxed command '{}' timed out after {}s",
                    full_command,
                    duration.as_secs()
                )
            })?
            .with_context(|| format!("Failed to execute sandboxed command: {}", full_command))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let success = output.status.success();

        Ok(json!({
            "success": success,
            "exit_code": output.status.code().unwrap_or_default(),
            "stdout": stdout.clone(),
            "stderr": stderr,
            "output": stdout,
            "mode": "sandbox",
            "pty_enabled": false,
            "sandbox_enabled": true,
            "command": full_command,
            "working_directory": work_dir.display().to_string(),
            "timeout_secs": timeout_value,
            "sandbox": {
                "binary": profile.binary().display().to_string(),
                "settings_path": profile.settings().display().to_string(),
                "command_string": command_string,
            },
        }))
    }

    /// Validate command for security
    fn validate_command(&self, command_parts: &[String]) -> Result<()> {
        if command_parts.is_empty() {
            return Err(anyhow::anyhow!("Command cannot be empty"));
        }

        let program = &command_parts[0];
        let sandbox_enabled = self.sandbox_profile.is_some();

        // Basic security checks - dangerous commands that should be blocked
        let always_blocked_commands = [
            "rm",
            "rmdir",
            "del",
            "format",
            "fdisk",
            "mkfs",
            "dd",
            "shred",
            "wipe",
            "srm",
            "unlink",
            "chmod",
            "chown",
            "passwd",
            "usermod",
            "userdel",
            "systemctl",
            "service",
            "kill",
            "killall",
            "pkill",
            "reboot",
            "shutdown",
            "halt",
            "poweroff",
            "sudo",
            "su",
            "doas",
            "runas",
            "mount",
            "umount",
            "fsck",
            "tune2fs", // Filesystem operations
            "iptables",
            "ufw",
            "firewalld", // Firewall
            "crontab",
            "at", // Scheduling
            "docker",
            "podman",
            "kubectl", // Container/orchestration
        ];

        if always_blocked_commands.contains(&program.as_str()) {
            return Err(anyhow::anyhow!(
                "Dangerous command not allowed: '{}'. This command could potentially harm your system. \
                 Use file operation tools instead for safe file management.",
                program
            ));
        }

        let is_network_command = matches!(
            program.as_str(),
            // Network commands are allowed when the sandbox is active and managing access.
            "curl" | "wget" | "ftp" | "scp" | "rsync" | "ssh" | "telnet" | "nc" | "ncat" | "socat"
        );

        if is_network_command && !sandbox_enabled {
            return Err(anyhow::anyhow!(
                "Dangerous command not allowed: '{}'. This command could potentially harm your system. \
                 Use file operation tools instead for safe file management.",
                program
            ));
        }

        // Check for suspicious patterns in the full command
        let full_command = command_parts.join(" ");

        // Block recursive delete operations
        if full_command.contains("rm -rf")
            || full_command.contains("rm -r")
                && (full_command.contains(" /") || full_command.contains(" ~"))
            || full_command.contains("rmdir")
                && (full_command.contains(" /") || full_command.contains(" ~"))
        {
            return Err(anyhow::anyhow!(
                "Potentially dangerous recursive delete operation detected. \
                 Use file operation tools for safe file management."
            ));
        }

        // Block privilege escalation attempts
        if full_command.contains("sudo ")
            || full_command.contains("su ")
            || full_command.contains("doas ")
            || full_command.contains("runas ")
        {
            return Err(anyhow::anyhow!(
                "Privilege escalation commands are not allowed. \
                 All operations run with current user privileges."
            ));
        }

        // Block network operations that could exfiltrate data
        if (full_command.contains("curl ") || full_command.contains("wget "))
            && (full_command.contains("http://")
                || full_command.contains("https://")
                || full_command.contains("ftp://"))
        {
            if !sandbox_enabled {
                return Err(anyhow::anyhow!(
                    "Network download commands are restricted. \
                     Use local file operations only."
                ));
            }
        }

        // Block commands that modify system configuration
        if full_command.contains(" > /etc/")
            || full_command.contains(" >> /etc/")
            || full_command.contains(" > /usr/")
            || full_command.contains(" >> /usr/")
            || full_command.contains(" > /var/")
            || full_command.contains(" >> /var/")
        {
            return Err(anyhow::anyhow!(
                "System configuration file modifications are not allowed. \
                 Use user-specific configuration files only."
            ));
        }

        // Block commands that access sensitive directories
        let sensitive_paths = [
            "/etc/", "/usr/", "/var/", "/root/", "/boot/", "/sys/", "/proc/",
        ];
        for path in &sensitive_paths {
            if full_command.contains(path)
                && (full_command.contains("rm ")
                    || full_command.contains("mv ")
                    || full_command.contains("cp ")
                    || full_command.contains("chmod ")
                    || full_command.contains("chown "))
            {
                return Err(anyhow::anyhow!(
                    "Operations on system directories '{}' are not allowed. \
                     Work within your project workspace only.",
                    path.trim_end_matches('/')
                ));
            }
        }

        // Allow only safe commands that are commonly needed for development
        let allowed_commands = [
            "ls", "pwd", "cat", "head", "tail", "grep", "find", "wc", "sort", "uniq", "cut", "awk",
            "sed", "echo", "printf", "seq", "basename", "dirname", "date", "cal", "bc", "expr",
            "test", "[", "]", "true", "false", "sleep", "which", "type", "file", "stat", "du",
            "df", "ps", "top", "htop", "tree", "less", "more", "tac", "rev", "tr", "fold", "paste",
            "join", "comm", "diff", "patch", "gzip", "gunzip", "bzip2", "bunzip2", "xz", "unxz",
            "tar", "zip", "unzip", "gzip", "bzip2", "git", "hg",
            "svn", // Version control (read-only operations)
            "make", "cmake", "ninja", // Build systems
            "cargo", "npm", "yarn", "pnpm", // Package managers
            "python", "python3", "node", "ruby", "perl", "php", "java", "javac", "scala", "kotlin",
            "go", "rustc", "gcc", "g++", "clang", "clang++", // Compilers
        ];

        let sandbox_allowed_commands = [
            "curl", "wget", "ftp", "scp", "rsync", "ssh", "telnet", "nc", "ncat", "socat",
        ];

        let command_allowed = allowed_commands.contains(&program.as_str())
            || (sandbox_enabled && sandbox_allowed_commands.contains(&program.as_str()));

        if !command_allowed {
            return Err(anyhow::anyhow!(
                "Command '{}' is not in the allowed commands list. \
                 Only safe development and analysis commands are permitted. \
                 Use specialized tools for file operations, searches, and builds.",
                program
            ));
        }

        Ok(())
    }

    /// Execute ls command
    async fn execute_ls(&self, args: Value) -> Result<Value> {
        let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
        let show_hidden = args
            .get("show_hidden")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let mut cmd_args = vec![path.to_string()];
        if show_hidden {
            cmd_args.insert(0, "-la".to_string());
        } else {
            cmd_args.insert(0, "-l".to_string());
        }

        self.execute_command("ls", cmd_args, None, Some(10), false)
            .await
    }

    /// Execute pwd command
    async fn execute_pwd(&self) -> Result<Value> {
        self.execute_command("pwd", vec![], None, Some(5), false)
            .await
    }

    /// Execute grep command
    async fn execute_grep(&self, args: Value) -> Result<Value> {
        let pattern = args
            .get("pattern")
            .and_then(|v| v.as_str())
            .context("pattern is required for grep")?;

        let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
        let recursive = args
            .get("recursive")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let mut cmd_args = vec![pattern.to_string(), path.to_string()];
        if recursive {
            cmd_args.insert(0, "-r".to_string());
        }

        self.execute_command("grep", cmd_args, None, Some(30), false)
            .await
    }

    /// Execute find command
    async fn execute_find(&self, args: Value) -> Result<Value> {
        let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
        let name_pattern = args.get("name_pattern").and_then(|v| v.as_str());
        let type_filter = args.get("type_filter").and_then(|v| v.as_str());

        let mut cmd_args = vec![path.to_string()];
        if let Some(pattern) = name_pattern {
            cmd_args.push("-name".to_string());
            cmd_args.push(pattern.to_string());
        }
        if let Some(filter) = type_filter {
            cmd_args.push("-type".to_string());
            cmd_args.push(filter.to_string());
        }

        self.execute_command("find", cmd_args, None, Some(30), false)
            .await
    }

    /// Execute cat command
    async fn execute_cat(&self, args: Value) -> Result<Value> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .context("path is required for cat")?;

        let start_line = args.get("start_line").and_then(|v| v.as_u64());
        let end_line = args.get("end_line").and_then(|v| v.as_u64());

        if let (Some(start), Some(end)) = (start_line, end_line) {
            // Use sed to extract line range
            let sed_cmd = format!("sed -n '{}','{}'p {}", start, end, path);
            return self
                .execute_command("sh", vec!["-c".to_string(), sed_cmd], None, Some(10), false)
                .await;
        }

        let cmd_args = vec![path.to_string()];

        self.execute_command("cat", cmd_args, None, Some(10), false)
            .await
    }

    /// Execute head command
    async fn execute_head(&self, args: Value) -> Result<Value> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .context("path is required for head")?;

        let lines = args.get("lines").and_then(|v| v.as_u64()).unwrap_or(10);

        let cmd_args = vec!["-n".to_string(), lines.to_string(), path.to_string()];

        self.execute_command("head", cmd_args, None, Some(10), false)
            .await
    }

    /// Execute tail command
    async fn execute_tail(&self, args: Value) -> Result<Value> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .context("path is required for tail")?;

        let lines = args.get("lines").and_then(|v| v.as_u64()).unwrap_or(10);

        let cmd_args = vec!["-n".to_string(), lines.to_string(), path.to_string()];

        self.execute_command("tail", cmd_args, None, Some(10), false)
            .await
    }

    /// Execute mkdir command
    async fn execute_mkdir(&self, args: Value) -> Result<Value> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .context("path is required for mkdir")?;

        let parents = args
            .get("parents")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let mut cmd_args = vec![path.to_string()];
        if parents {
            cmd_args.insert(0, "-p".to_string());
        }

        self.execute_command("mkdir", cmd_args, None, Some(10), false)
            .await
    }

    /// Execute rm command
    async fn execute_rm(&self, args: Value) -> Result<Value> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .context("path is required for rm")?;

        let recursive = args
            .get("recursive")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let force = args.get("force").and_then(|v| v.as_bool()).unwrap_or(false);

        let mut cmd_args = vec![];
        if recursive {
            cmd_args.push("-r".to_string());
        }
        if force {
            cmd_args.push("-f".to_string());
        }
        cmd_args.push(path.to_string());

        self.execute_command("rm", cmd_args, None, Some(10), false)
            .await
    }

    /// Execute cp command
    async fn execute_cp(&self, args: Value) -> Result<Value> {
        let source = args
            .get("source")
            .and_then(|v| v.as_str())
            .context("source is required for cp")?;

        let dest = args
            .get("dest")
            .and_then(|v| v.as_str())
            .context("dest is required for cp")?;

        let recursive = args
            .get("recursive")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let mut cmd_args = vec![];
        if recursive {
            cmd_args.push("-r".to_string());
        }
        cmd_args.push(source.to_string());
        cmd_args.push(dest.to_string());

        self.execute_command("cp", cmd_args, None, Some(30), false)
            .await
    }

    /// Execute mv command
    async fn execute_mv(&self, args: Value) -> Result<Value> {
        let source = args
            .get("source")
            .and_then(|v| v.as_str())
            .context("source is required for mv")?;

        let dest = args
            .get("dest")
            .and_then(|v| v.as_str())
            .context("dest is required for mv")?;

        let cmd_args = vec![source.to_string(), dest.to_string()];

        self.execute_command("mv", cmd_args, None, Some(10), false)
            .await
    }

    /// Execute stat command
    async fn execute_stat(&self, args: Value) -> Result<Value> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .context("path is required for stat")?;

        let cmd_args = vec!["-la".to_string(), path.to_string()];

        self.execute_command("ls", cmd_args, None, Some(10), false)
            .await
    }

    /// Execute arbitrary command
    async fn execute_run(&self, args: Value) -> Result<Value> {
        let command = args
            .get("command")
            .and_then(|v| v.as_str())
            .context("command is required for run")?;

        let cmd_args = args
            .get("args")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| s.to_string())
                    .collect::<Vec<String>>()
            })
            .unwrap_or_default();

        let timeout_override = args.get("timeout_secs").and_then(|v| v.as_u64());
        let working_dir = args.get("working_dir").and_then(|v| v.as_str());

        self.execute_command(command, cmd_args, working_dir, timeout_override, true)
            .await
    }
}

#[async_trait]
impl Tool for BashTool {
    async fn execute(&self, args: Value) -> Result<Value> {
        let command = args
            .get("bash_command")
            .and_then(|v| v.as_str())
            .unwrap_or("ls");

        match command {
            "ls" => self.execute_ls(args).await,
            "pwd" => self.execute_pwd().await,
            "grep" => self.execute_grep(args).await,
            "find" => self.execute_find(args).await,
            "cat" => self.execute_cat(args).await,
            "head" => self.execute_head(args).await,
            "tail" => self.execute_tail(args).await,
            "mkdir" => self.execute_mkdir(args).await,
            "rm" => self.execute_rm(args).await,
            "cp" => self.execute_cp(args).await,
            "mv" => self.execute_mv(args).await,
            "stat" => self.execute_stat(args).await,
            "run" => self.execute_run(args).await,
            _ => Err(anyhow::anyhow!("Unknown bash command: {}", command)),
        }
    }

    fn name(&self) -> &'static str {
        tools::BASH
    }

    fn description(&self) -> &'static str {
        "Bash-like commands with security validation: ls, pwd, grep, find, cat, head, tail, mkdir, rm, cp, mv, stat, run. \
         Dangerous commands (rm, sudo, network operations, system modifications) are blocked for safety."
    }
}
