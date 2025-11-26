//! Simple bash-like command runner
//!
//! This module provides simple, direct command execution that acts like
//! a human using bash commands. No complex abstractions, just direct
//! execution of common shell commands.

use anyhow::{Context, Result};
use std::path::PathBuf;
use std::process::Command;

/// Simple bash-like command runner
#[derive(Clone)]
pub struct BashRunner {
    /// Working directory
    working_dir: PathBuf,
}

impl BashRunner {
    /// Create a new bash runner
    pub fn new(working_dir: PathBuf) -> Self {
        Self { working_dir }
    }

    /// Change directory (like cd)
    pub fn cd(&mut self, path: &str) -> Result<()> {
        let new_path = if path.starts_with('/') {
            PathBuf::from(path)
        } else {
            self.working_dir.join(path)
        };

        if !new_path.exists() {
            return Err(anyhow::anyhow!("Directory does not exist: {}", path));
        }

        if !new_path.is_dir() {
            return Err(anyhow::anyhow!("Path is not a directory: {}", path));
        }

        self.working_dir = new_path.canonicalize()?;
        Ok(())
    }

    /// List directory contents (like ls)
    pub fn ls(&self, path: Option<&str>, show_hidden: bool) -> Result<String> {
        let target_path = path
            .map(|p| self.resolve_path(p))
            .unwrap_or_else(|| self.working_dir.clone());

        let mut cmd = Command::new("ls");
        if show_hidden {
            cmd.arg("-la");
        } else {
            cmd.arg("-l");
        }
        cmd.arg(&target_path);

        let output = cmd
            .output()
            .with_context(|| "Failed to execute ls command".to_string())?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            Err(anyhow::anyhow!(
                "ls failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ))
        }
    }

    /// Print working directory (like pwd)
    pub fn pwd(&self) -> String {
        self.working_dir.to_string_lossy().into_owned()
    }

    /// Create directory (like mkdir)
    pub fn mkdir(&self, path: &str, parents: bool) -> Result<()> {
        let target_path = self.resolve_path(path);

        let mut cmd = Command::new("mkdir");
        if parents {
            cmd.arg("-p");
        }
        cmd.arg(&target_path);

        let output = cmd
            .output()
            .with_context(|| "Failed to execute mkdir command".to_string())?;

        if output.status.success() {
            Ok(())
        } else {
            Err(anyhow::anyhow!(
                "mkdir failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ))
        }
    }

    /// Remove files/directories (like rm)
    pub fn rm(&self, path: &str, recursive: bool, force: bool) -> Result<()> {
        let target_path = self.resolve_path(path);

        let mut cmd = Command::new("rm");
        if recursive {
            cmd.arg("-r");
        }
        if force {
            cmd.arg("-f");
        }
        cmd.arg(&target_path);

        let output = cmd
            .output()
            .with_context(|| "Failed to execute rm command".to_string())?;

        if output.status.success() {
            Ok(())
        } else {
            Err(anyhow::anyhow!(
                "rm failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ))
        }
    }

    /// Copy files/directories (like cp)
    pub fn cp(&self, source: &str, dest: &str, recursive: bool) -> Result<()> {
        let source_path = self.resolve_path(source);
        let dest_path = self.resolve_path(dest);

        let mut cmd = Command::new("cp");
        if recursive {
            cmd.arg("-r");
        }
        cmd.arg(&source_path).arg(&dest_path);

        let output = cmd
            .output()
            .with_context(|| "Failed to execute cp command".to_string())?;

        if output.status.success() {
            Ok(())
        } else {
            Err(anyhow::anyhow!(
                "cp failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ))
        }
    }

    /// Move/rename files/directories (like mv)
    pub fn mv(&self, source: &str, dest: &str) -> Result<()> {
        let source_path = self.resolve_path(source);
        let dest_path = self.resolve_path(dest);

        let output = Command::new("mv")
            .arg(&source_path)
            .arg(&dest_path)
            .output()
            .with_context(|| "Failed to execute mv command".to_string())?;

        if output.status.success() {
            Ok(())
        } else {
            Err(anyhow::anyhow!(
                "mv failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ))
        }
    }

    /// Search for text in files (like grep)
    pub fn grep(&self, pattern: &str, path: Option<&str>, recursive: bool) -> Result<String> {
        let target_path = path
            .map(|p| self.resolve_path(p))
            .unwrap_or_else(|| self.working_dir.clone());

        let mut cmd = Command::new("grep");
        cmd.arg("-n"); // Show line numbers
        if recursive {
            cmd.arg("-r");
        }
        cmd.arg(pattern).arg(&target_path);

        let output = cmd
            .output()
            .with_context(|| "Failed to execute grep command".to_string())?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            // grep returns non-zero when no matches found, which is not an error for us
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.is_empty() {
                Ok(String::new()) // No matches found
            } else {
                Err(anyhow::anyhow!("grep failed: {}", stderr))
            }
        }
    }

    /// Find files (like find)
    pub fn find(
        &self,
        path: Option<&str>,
        name_pattern: Option<&str>,
        type_filter: Option<&str>,
    ) -> Result<String> {
        let target_path = path
            .map(|p| self.resolve_path(p))
            .unwrap_or_else(|| self.working_dir.clone());

        let mut cmd = Command::new("find");
        cmd.arg(&target_path);

        if let Some(pattern) = name_pattern {
            cmd.arg("-name").arg(pattern);
        }

        if let Some(type_filter) = type_filter {
            cmd.arg("-type").arg(type_filter);
        }

        let output = cmd
            .output()
            .with_context(|| "Failed to execute find command".to_string())?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            Err(anyhow::anyhow!(
                "find failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ))
        }
    }

    /// Show file contents (like cat)
    pub fn cat(
        &self,
        path: &str,
        start_line: Option<usize>,
        end_line: Option<usize>,
    ) -> Result<String> {
        let file_path = self.resolve_path(path);

        if let (Some(start), Some(end)) = (start_line, end_line) {
            // Use sed to extract specific lines
            let range = format!("{}q;{}q", start, end);
            let output = Command::new("sed")
                .arg("-n")
                .arg(&range)
                .arg(&file_path)
                .output()
                .with_context(|| "Failed to execute sed command".to_string())?;

            if output.status.success() {
                Ok(String::from_utf8_lossy(&output.stdout).to_string())
            } else {
                Err(anyhow::anyhow!(
                    "sed failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                ))
            }
        } else {
            // Simple cat
            let output = Command::new("cat")
                .arg(&file_path)
                .output()
                .with_context(|| "Failed to execute cat command".to_string())?;

            if output.status.success() {
                Ok(String::from_utf8_lossy(&output.stdout).to_string())
            } else {
                Err(anyhow::anyhow!(
                    "cat failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                ))
            }
        }
    }

    /// Show first/last lines of file (like head/tail)
    pub fn head(&self, path: &str, lines: usize) -> Result<String> {
        let file_path = self.resolve_path(path);

        let output = Command::new("head")
            .arg("-n")
            .arg(lines.to_string())
            .arg(&file_path)
            .output()
            .with_context(|| "Failed to execute head command".to_string())?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            Err(anyhow::anyhow!(
                "head failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ))
        }
    }

    pub fn tail(&self, path: &str, lines: usize) -> Result<String> {
        let file_path = self.resolve_path(path);

        let output = Command::new("tail")
            .arg("-n")
            .arg(lines.to_string())
            .arg(&file_path)
            .output()
            .with_context(|| "Failed to execute tail command".to_string())?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            Err(anyhow::anyhow!(
                "tail failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ))
        }
    }

    /// Get file info (like ls -la but for single file)
    pub fn stat(&self, path: &str) -> Result<String> {
        let file_path = self.resolve_path(path);

        let output = Command::new("ls")
            .arg("-la")
            .arg(&file_path)
            .output()
            .with_context(|| "Failed to execute ls command".to_string())?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            Err(anyhow::anyhow!(
                "stat failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ))
        }
    }

    /// Execute arbitrary command
    pub fn run(&self, command: &str, args: &[&str]) -> Result<String> {
        let output = Command::new(command)
            .args(args)
            .current_dir(&self.working_dir)
            .output()
            .with_context(|| format!("Failed to execute command: {}", command))?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.is_empty() {
                Ok(String::new())
            } else {
                Err(anyhow::anyhow!("Command failed: {}", stderr))
            }
        }
    }

    // Helper method
    fn resolve_path(&self, path: &str) -> PathBuf {
        if path.starts_with('/') {
            PathBuf::from(path)
        } else {
            self.working_dir.join(path)
        }
    }
}
