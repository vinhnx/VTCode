use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use tokio::process::Command;
use tokio::sync::Mutex;
use tokio::time::timeout;
use vtcode_config::core::CustomProviderCommandAuthConfig;

#[derive(Clone, Debug)]
pub struct CustomProviderAuthHandle {
    config: CustomProviderCommandAuthConfig,
    workspace_root: Option<PathBuf>,
    state: Arc<Mutex<CustomProviderAuthState>>,
}

#[derive(Debug, Default)]
struct CustomProviderAuthState {
    cached_token: Option<CachedToken>,
}

#[derive(Debug, Clone)]
struct CachedToken {
    value: String,
    fetched_at: Instant,
}

impl CustomProviderAuthHandle {
    pub fn new(config: CustomProviderCommandAuthConfig, workspace_root: Option<PathBuf>) -> Self {
        Self {
            config,
            workspace_root,
            state: Arc::new(Mutex::new(CustomProviderAuthState::default())),
        }
    }

    pub async fn current_token(&self) -> Result<String> {
        let mut state = self.state.lock().await;
        if let Some(token) = state.cached_token.as_ref()
            && token.fetched_at.elapsed() < self.refresh_interval()
        {
            return Ok(token.value.clone());
        }

        let token = self.fetch_token().await?;
        state.cached_token = Some(CachedToken {
            value: token.clone(),
            fetched_at: Instant::now(),
        });
        Ok(token)
    }

    pub async fn force_refresh(&self) -> Result<String> {
        let token = self.fetch_token().await?;
        let mut state = self.state.lock().await;
        state.cached_token = Some(CachedToken {
            value: token.clone(),
            fetched_at: Instant::now(),
        });
        Ok(token)
    }

    fn refresh_interval(&self) -> Duration {
        Duration::from_millis(self.config.refresh_interval_ms)
    }

    fn timeout(&self) -> Duration {
        Duration::from_millis(self.config.timeout_ms)
    }

    fn resolve_cwd(&self) -> Option<PathBuf> {
        let cwd = self.config.cwd.as_ref()?;
        if cwd.is_absolute() {
            return Some(cwd.clone());
        }

        self.workspace_root
            .as_ref()
            .map(|workspace_root| workspace_root.join(cwd))
            .or_else(|| {
                std::env::current_dir()
                    .ok()
                    .map(|cwd_root| cwd_root.join(cwd))
            })
    }

    async fn fetch_token(&self) -> Result<String> {
        let mut command = Command::new(&self.config.command);
        command
            .args(&self.config.args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if let Some(cwd) = self.resolve_cwd() {
            command.current_dir(cwd);
        }

        let output = timeout(self.timeout(), command.output())
            .await
            .with_context(|| {
                format!(
                    "provider auth command timed out after {}ms",
                    self.config.timeout_ms
                )
            })?
            .with_context(|| {
                format!(
                    "failed to execute provider auth command `{}`",
                    self.config.command
                )
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stderr = stderr.trim();
            if stderr.is_empty() {
                bail!(
                    "provider auth command `{}` exited with status {}",
                    self.config.command,
                    output.status
                );
            }
            bail!(
                "provider auth command `{}` exited with status {}: {}",
                self.config.command,
                output.status,
                stderr
            );
        }

        let stdout = String::from_utf8(output.stdout).map_err(|err| {
            anyhow!(
                "provider auth command `{}` returned non-utf8 stdout: {err}",
                self.config.command
            )
        })?;
        let token = stdout.trim();
        if token.is_empty() {
            bail!(
                "provider auth command `{}` returned an empty token",
                self.config.command
            );
        }

        Ok(token.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::CustomProviderAuthHandle;
    use std::path::Path;
    use tempfile::TempDir;
    use vtcode_config::core::CustomProviderCommandAuthConfig;

    fn write_tokens_file(dir: &Path, tokens: &[&str]) {
        std::fs::write(dir.join("tokens.txt"), tokens.join("\n")).expect("write tokens file");
    }

    #[cfg(unix)]
    fn build_fixture(dir: &TempDir, tokens: &[&str]) -> CustomProviderCommandAuthConfig {
        use std::os::unix::fs::PermissionsExt;

        write_tokens_file(dir.path(), tokens);
        let script_path = dir.path().join("print-token.sh");
        std::fs::write(
            &script_path,
            r#"#!/bin/sh
first_line=$(sed -n '1p' tokens.txt)
printf ' %s \n' "$first_line"
tail -n +2 tokens.txt > tokens.next
mv tokens.next tokens.txt
"#,
        )
        .expect("write script");
        let mut permissions = std::fs::metadata(&script_path)
            .expect("script metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script_path, permissions).expect("set permissions");

        CustomProviderCommandAuthConfig {
            command: "./print-token.sh".to_string(),
            args: Vec::new(),
            cwd: Some(dir.path().to_path_buf()),
            timeout_ms: 1_000,
            refresh_interval_ms: 60_000,
        }
    }

    #[cfg(windows)]
    fn build_fixture(dir: &TempDir, tokens: &[&str]) -> CustomProviderCommandAuthConfig {
        write_tokens_file(dir.path(), tokens);
        let script_path = dir.path().join("print-token.ps1");
        std::fs::write(
            &script_path,
            r#"$lines = Get-Content -Path tokens.txt
if ($lines.Count -eq 0) { exit 1 }
Write-Output (" " + $lines[0] + " ")
$lines | Select-Object -Skip 1 | Set-Content -Path tokens.txt
"#,
        )
        .expect("write script");

        CustomProviderCommandAuthConfig {
            command: "powershell".to_string(),
            args: vec![
                "-NoProfile".to_string(),
                "-ExecutionPolicy".to_string(),
                "Bypass".to_string(),
                "-File".to_string(),
                script_path.to_string_lossy().into_owned(),
            ],
            cwd: Some(dir.path().to_path_buf()),
            timeout_ms: 1_000,
            refresh_interval_ms: 60_000,
        }
    }

    #[tokio::test]
    async fn current_token_trims_stdout_and_uses_cache() {
        let dir = TempDir::new().expect("tempdir");
        let handle = CustomProviderAuthHandle::new(build_fixture(&dir, &["first", "second"]), None);

        let first = handle.current_token().await.expect("first token");
        let second = handle.current_token().await.expect("cached token");

        assert_eq!(first, "first");
        assert_eq!(second, "first");
        let remaining = std::fs::read_to_string(dir.path().join("tokens.txt")).expect("tokens");
        assert_eq!(remaining.trim(), "second");
    }

    #[tokio::test]
    async fn force_refresh_reruns_command() {
        let dir = TempDir::new().expect("tempdir");
        let handle = CustomProviderAuthHandle::new(build_fixture(&dir, &["first", "second"]), None);

        let first = handle.current_token().await.expect("first token");
        let refreshed = handle.force_refresh().await.expect("refreshed token");

        assert_eq!(first, "first");
        assert_eq!(refreshed, "second");
    }
}
