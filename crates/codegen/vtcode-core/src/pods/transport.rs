use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use std::process::Stdio;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

/// Captured command output from a pod transport.
#[derive(Debug, Clone, Default)]
pub struct CommandOutput {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

#[async_trait]
pub trait PodTransport: Send + Sync {
    async fn exec_capture(&self, ssh_target: &str, command: &str) -> Result<CommandOutput>;
    async fn write_file(&self, ssh_target: &str, remote_path: &str, contents: &str) -> Result<()>;
    async fn exec_stream(&self, ssh_target: &str, command: &str) -> Result<()>;
}

/// SSH-backed transport used by the real CLI.
#[derive(Debug, Clone, Default)]
pub struct SshTransport;

#[async_trait]
impl PodTransport for SshTransport {
    async fn exec_capture(&self, ssh_target: &str, command: &str) -> Result<CommandOutput> {
        let mut ssh = build_ssh_command(ssh_target, command)?;
        ssh.stdout(Stdio::piped()).stderr(Stdio::piped());

        let output = ssh
            .output()
            .await
            .with_context(|| format!("failed to execute SSH command: {command}"))?;

        Ok(CommandOutput {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        })
    }

    async fn write_file(&self, ssh_target: &str, remote_path: &str, contents: &str) -> Result<()> {
        let remote_command = format!("cat > {remote_path}");
        let mut ssh = build_ssh_command(ssh_target, &remote_command)?;
        ssh.stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());

        let mut child = ssh
            .spawn()
            .with_context(|| format!("failed to spawn SSH writer for {remote_path}"))?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(contents.as_bytes())
                .await
                .with_context(|| format!("failed to write remote file {remote_path}"))?;
        } else {
            return Err(anyhow!("SSH writer did not provide stdin"));
        }

        let output = child
            .wait_with_output()
            .await
            .with_context(|| format!("failed to finish SSH writer for {remote_path}"))?;

        if output.status.success() {
            Ok(())
        } else {
            Err(anyhow!(
                "remote file write failed for {remote_path}: {}",
                String::from_utf8_lossy(&output.stderr)
            ))
        }
    }

    async fn exec_stream(&self, ssh_target: &str, command: &str) -> Result<()> {
        let mut ssh = build_ssh_command(ssh_target, command)?;
        ssh.stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());

        let status = ssh
            .status()
            .await
            .with_context(|| format!("failed to stream SSH command: {command}"))?;

        if status.success() {
            Ok(())
        } else {
            Err(anyhow!("SSH stream command failed with status {status}"))
        }
    }
}

fn build_ssh_command(ssh_target: &str, remote_command: &str) -> Result<Command> {
    let parts = shell_words::split(ssh_target)
        .with_context(|| format!("failed to parse SSH target: {ssh_target}"))?;

    let Some((program, args)) = parts.split_first() else {
        return Err(anyhow!("SSH target is empty"));
    };

    let mut command = Command::new(program);
    command.args(args);
    command.arg(remote_command);
    Ok(command)
}

#[cfg(test)]
mod tests {}
