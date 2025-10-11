//! Bash-like tool for command execution
//!
//! This tool provides bash-like functionality for running common
//! commands and tools that require a shell environment.

use super::traits::Tool;
use crate::config::CommandsConfig;
use crate::config::constants::tools;
use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::{Value, json};
use std::{path::PathBuf, time::Duration};

use crate::utils::process::{ProcessRequest, run_process};

/// Bash-like tool for command execution
#[derive(Clone)]
pub struct BashTool {
    workspace_root: PathBuf,
    commands_config: CommandsConfig,
}

impl BashTool {
    /// Create a new bash tool
    pub fn new(workspace_root: PathBuf, commands_config: CommandsConfig) -> Self {
        Self {
            workspace_root,
            commands_config,
        }
    }

    /// Execute command and capture its output
    async fn execute_pty_command(
        &self,
        command: &str,
        args: Vec<String>,
        timeout_secs: Option<u64>,
    ) -> Result<Value> {
        let full_command_parts = std::iter::once(command.to_string())
            .chain(args.clone())
            .collect::<Vec<String>>();
        self.validate_command(&full_command_parts)?;

        let full_command = if args.is_empty() {
            command.to_string()
        } else {
            format!("{} {}", command, args.join(" "))
        };

        let work_dir = self.workspace_root.clone();
        let duration = Duration::from_secs(timeout_secs.unwrap_or(30));
        let output = run_process(ProcessRequest {
            program: command,
            args: &args,
            display: &full_command,
            current_dir: Some(work_dir.as_path()),
            timeout: duration,
            stdin: None,
            max_stdout_bytes: self.commands_config.max_stdout_bytes,
            max_stderr_bytes: self.commands_config.max_stderr_bytes,
        })
        .await?;

        Ok(json!({
            "success": output.success,
            "exit_code": output.exit_code,
            "stdout": output.stdout,
            "stderr": output.stderr,
            "stdout_bytes": output.stdout_bytes,
            "stderr_bytes": output.stderr_bytes,
            "stdout_truncated": output.stdout_truncated,
            "stderr_truncated": output.stderr_truncated,
            "timed_out": output.timed_out,
            "duration_ms": output.duration.as_millis(),
            "mode": "terminal",
            "pty_enabled": false,
            "command": full_command,
            "working_directory": work_dir.display().to_string()
        }))
    }

    /// Validate command for security
    fn validate_command(&self, command_parts: &[String]) -> Result<()> {
        if command_parts.is_empty() {
            return Err(anyhow::anyhow!("Command cannot be empty"));
        }

        let program = &command_parts[0];

        // Basic security checks - dangerous commands that should be blocked
        let dangerous_commands = [
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
            "curl",
            "wget",
            "ftp",
            "scp",
            "rsync", // Network commands
            "ssh",
            "telnet",
            "nc",
            "ncat",
            "socat", // Remote access
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

        if dangerous_commands.contains(&program.as_str()) {
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
            return Err(anyhow::anyhow!(
                "Network download commands are restricted. \
                 Use local file operations only."
            ));
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

        if !allowed_commands.contains(&program.as_str()) {
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

        self.execute_pty_command("ls", cmd_args, Some(10)).await
    }

    /// Execute pwd command
    async fn execute_pwd(&self) -> Result<Value> {
        self.execute_pty_command("pwd", vec![], Some(5)).await
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

        self.execute_pty_command("grep", cmd_args, Some(30)).await
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

        self.execute_pty_command("find", cmd_args, Some(30)).await
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
                .execute_pty_command("sh", vec!["-c".to_string(), sed_cmd], Some(10))
                .await;
        }

        let cmd_args = vec![path.to_string()];

        self.execute_pty_command("cat", cmd_args, Some(10)).await
    }

    /// Execute head command
    async fn execute_head(&self, args: Value) -> Result<Value> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .context("path is required for head")?;

        let lines = args.get("lines").and_then(|v| v.as_u64()).unwrap_or(10);

        let cmd_args = vec!["-n".to_string(), lines.to_string(), path.to_string()];

        self.execute_pty_command("head", cmd_args, Some(10)).await
    }

    /// Execute tail command
    async fn execute_tail(&self, args: Value) -> Result<Value> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .context("path is required for tail")?;

        let lines = args.get("lines").and_then(|v| v.as_u64()).unwrap_or(10);

        let cmd_args = vec!["-n".to_string(), lines.to_string(), path.to_string()];

        self.execute_pty_command("tail", cmd_args, Some(10)).await
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

        self.execute_pty_command("mkdir", cmd_args, Some(10)).await
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

        self.execute_pty_command("rm", cmd_args, Some(10)).await
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

        self.execute_pty_command("cp", cmd_args, Some(30)).await
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

        self.execute_pty_command("mv", cmd_args, Some(10)).await
    }

    /// Execute stat command
    async fn execute_stat(&self, args: Value) -> Result<Value> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .context("path is required for stat")?;

        let cmd_args = vec!["-la".to_string(), path.to_string()];

        self.execute_pty_command("ls", cmd_args, Some(10)).await
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

        self.execute_pty_command(command, cmd_args, Some(30)).await
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
