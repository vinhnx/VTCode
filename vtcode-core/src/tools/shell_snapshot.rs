//! Shell environment snapshot for avoiding repeated login script execution.
//!
//! This module provides a mechanism to capture a fully-initialized shell environment
//! (after login scripts have run) and reuse it for subsequent command executions,
//! significantly reducing command startup time.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Context, Result, anyhow};
use parking_lot::RwLock;
use tokio::process::Command;
use tokio::sync::Mutex as TokioMutex;
use tracing::{debug, info};

use super::shell::resolve_fallback_shell;

/// Environment variables that should not be captured in snapshots.
/// These are volatile or session-specific and would cause issues if reused.
const EXCLUDED_ENV_VARS: &[&str] = &[
    "PWD",
    "OLDPWD",
    "SHLVL",
    "_",
    "TERM",
    "TERM_PROGRAM",
    "TERM_SESSION_ID",
    "SHELL_SESSION_ID",
    "TERM_PROGRAM_VERSION",
    "COLUMNS",
    "LINES",
    "WINDOWID",
    "DISPLAY",
    "SSH_CLIENT",
    "SSH_CONNECTION",
    "SSH_TTY",
    "STY",
    "TMUX",
    "TMUX_PANE",
    "ITERM_SESSION_ID",
    "ITERM_PROFILE",
    "KONSOLE_DBUS_SERVICE",
    "KONSOLE_DBUS_SESSION",
    "KONSOLE_VERSION",
    "GNOME_TERMINAL_SCREEN",
    "GNOME_TERMINAL_SERVICE",
    "VTE_VERSION",
    "COLORTERM",
    "WT_SESSION",
    "WT_PROFILE_ID",
    "LC_TERMINAL",
    "LC_TERMINAL_VERSION",
    "SECURITYSESSIONID",
    "Apple_PubSub_Socket_Render",
    "LaunchInstanceID",
    "RANDOM",
    "LINENO",
    "SECONDS",
    "EPOCHREALTIME",
    "EPOCHSECONDS",
    "BASHPID",
    "PPID",
    "BASH_COMMAND",
    "BASH_SUBSHELL",
];

/// Markers used to delimit the environment dump in shell output.
const ENV_BEGIN_MARKER: &str = "__VTCODE_ENV_BEGIN__";
const ENV_END_MARKER: &str = "__VTCODE_ENV_END__";

/// Default snapshot TTL (24 hours).
const DEFAULT_SNAPSHOT_TTL: Duration = Duration::from_secs(24 * 60 * 60);

/// Maximum age before considering refresh (5 minutes for development).
const REFRESH_CHECK_INTERVAL: Duration = Duration::from_secs(5 * 60);

/// Detected shell kind for platform-specific behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellKind {
    Bash,
    Zsh,
    Sh,
    Fish,
    Unknown,
}

impl ShellKind {
    /// Detect shell kind from the shell path.
    pub fn from_path(shell_path: &str) -> Self {
        let shell_name = Path::new(shell_path)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("");

        match shell_name {
            name if name.contains("bash") => ShellKind::Bash,
            name if name.contains("zsh") => ShellKind::Zsh,
            name if name.contains("fish") => ShellKind::Fish,
            "sh" | "dash" | "ash" => ShellKind::Sh,
            _ => ShellKind::Unknown,
        }
    }

    /// Get the login shell configuration files to monitor for changes.
    pub fn config_files(&self) -> Vec<PathBuf> {
        let home = dirs::home_dir().unwrap_or_default();
        match self {
            ShellKind::Bash => vec![
                PathBuf::from("/etc/profile"),
                home.join(".bash_profile"),
                home.join(".bash_login"),
                home.join(".profile"),
                home.join(".bashrc"),
            ],
            ShellKind::Zsh => vec![
                PathBuf::from("/etc/zshenv"),
                PathBuf::from("/etc/zprofile"),
                PathBuf::from("/etc/zshrc"),
                PathBuf::from("/etc/zlogin"),
                home.join(".zshenv"),
                home.join(".zprofile"),
                home.join(".zshrc"),
                home.join(".zlogin"),
            ],
            ShellKind::Fish => vec![
                PathBuf::from("/etc/fish/config.fish"),
                home.join(".config/fish/config.fish"),
            ],
            ShellKind::Sh | ShellKind::Unknown => {
                vec![PathBuf::from("/etc/profile"), home.join(".profile")]
            }
        }
    }
}

/// Fingerprint of a configuration file for change detection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileFingerprint {
    pub path: PathBuf,
    pub mtime: Option<SystemTime>,
    pub size: Option<u64>,
}

impl FileFingerprint {
    /// Create a fingerprint for a file.
    pub fn from_path(path: PathBuf) -> Self {
        let (mtime, size) = std::fs::metadata(&path)
            .ok()
            .map(|m| (m.modified().ok(), Some(m.len())))
            .unwrap_or((None, None));

        Self { path, mtime, size }
    }

    /// Check if the file has changed since this fingerprint was taken.
    pub fn has_changed(&self) -> bool {
        let current = Self::from_path(self.path.clone());
        self.mtime != current.mtime || self.size != current.size
    }
}

/// A captured shell environment snapshot.
#[derive(Debug, Clone)]
pub struct ShellSnapshot {
    /// The shell path used for this snapshot.
    pub shell_path: String,
    /// Detected shell kind.
    pub shell_kind: ShellKind,
    /// Captured environment variables.
    pub env: HashMap<String, String>,
    /// When this snapshot was captured.
    pub captured_at: Instant,
    /// Fingerprints of configuration files at capture time.
    pub config_fingerprints: Vec<FileFingerprint>,
}

impl ShellSnapshot {
    /// Check if this snapshot is still valid.
    pub fn is_valid(&self, shell_path: &str, ttl: Duration) -> bool {
        if self.shell_path != shell_path {
            debug!("Snapshot invalid: shell path changed");
            return false;
        }

        if self.captured_at.elapsed() > ttl {
            debug!("Snapshot invalid: TTL expired");
            return false;
        }

        for fp in &self.config_fingerprints {
            if fp.has_changed() {
                debug!("Snapshot invalid: config file changed: {:?}", fp.path);
                return false;
            }
        }

        true
    }

    /// Get the PATH from the snapshot.
    pub fn path(&self) -> Option<&str> {
        self.env.get("PATH").map(|s| s.as_str())
    }
}

/// Manager for shell environment snapshots.
pub struct ShellSnapshotManager {
    /// The current snapshot (if any).
    snapshot: RwLock<Option<Arc<ShellSnapshot>>>,
    /// Lock for capture operations to prevent stampedes.
    capture_lock: TokioMutex<()>,
    /// Snapshot TTL.
    ttl: Duration,
    /// Last time we checked for refresh.
    last_refresh_check: RwLock<Instant>,
}

impl Default for ShellSnapshotManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ShellSnapshotManager {
    /// Create a new snapshot manager.
    pub fn new() -> Self {
        Self {
            snapshot: RwLock::new(None),
            capture_lock: TokioMutex::new(()),
            ttl: DEFAULT_SNAPSHOT_TTL,
            last_refresh_check: RwLock::new(Instant::now()),
        }
    }

    /// Create a snapshot manager with a custom TTL.
    pub fn with_ttl(ttl: Duration) -> Self {
        Self {
            snapshot: RwLock::new(None),
            capture_lock: TokioMutex::new(()),
            ttl,
            last_refresh_check: RwLock::new(Instant::now()),
        }
    }

    /// Get an existing valid snapshot or capture a new one.
    pub async fn get_or_capture(&self) -> Result<Arc<ShellSnapshot>> {
        let shell_path = resolve_fallback_shell();

        {
            let snapshot = self.snapshot.read();
            if let Some(ref snap) = *snapshot
                && snap.is_valid(&shell_path, self.ttl)
            {
                return Ok(Arc::clone(snap));
            }
        }

        let _guard = self.capture_lock.lock().await;

        {
            let snapshot = self.snapshot.read();
            if let Some(ref snap) = *snapshot
                && snap.is_valid(&shell_path, self.ttl)
            {
                return Ok(Arc::clone(snap));
            }
        }

        let new_snapshot = Arc::new(capture_shell_snapshot(&shell_path).await?);

        {
            let mut snapshot = self.snapshot.write();
            *snapshot = Some(Arc::clone(&new_snapshot));
        }

        info!(
            "Captured shell environment snapshot ({} variables)",
            new_snapshot.env.len()
        );
        Ok(new_snapshot)
    }

    /// Get the current snapshot if valid, without capturing.
    pub fn get_if_valid(&self) -> Option<Arc<ShellSnapshot>> {
        let shell_path = resolve_fallback_shell();
        let snapshot = self.snapshot.read();
        snapshot.as_ref().and_then(|snap| {
            if snap.is_valid(&shell_path, self.ttl) {
                Some(Arc::clone(snap))
            } else {
                None
            }
        })
    }

    /// Invalidate the current snapshot.
    pub fn invalidate(&self) {
        let mut snapshot = self.snapshot.write();
        *snapshot = None;
        debug!("Shell snapshot invalidated");
    }

    /// Check if refresh is needed (rate-limited).
    pub fn should_refresh(&self) -> bool {
        let mut last_check = self.last_refresh_check.write();
        if last_check.elapsed() < REFRESH_CHECK_INTERVAL {
            return false;
        }
        *last_check = Instant::now();

        let shell_path = resolve_fallback_shell();
        let snapshot = self.snapshot.read();
        match &*snapshot {
            Some(snap) => !snap.is_valid(&shell_path, self.ttl),
            None => true,
        }
    }

    /// Get snapshot statistics for diagnostics.
    pub fn stats(&self) -> SnapshotStats {
        let snapshot = self.snapshot.read();
        match &*snapshot {
            Some(snap) => SnapshotStats {
                has_snapshot: true,
                shell_path: Some(snap.shell_path.clone()),
                shell_kind: Some(snap.shell_kind),
                env_count: snap.env.len(),
                age_secs: snap.captured_at.elapsed().as_secs(),
                config_files_monitored: snap.config_fingerprints.len(),
            },
            None => SnapshotStats {
                has_snapshot: false,
                shell_path: None,
                shell_kind: None,
                env_count: 0,
                age_secs: 0,
                config_files_monitored: 0,
            },
        }
    }
}

/// Statistics about the current snapshot state.
#[derive(Debug, Clone)]
pub struct SnapshotStats {
    pub has_snapshot: bool,
    pub shell_path: Option<String>,
    pub shell_kind: Option<ShellKind>,
    pub env_count: usize,
    pub age_secs: u64,
    pub config_files_monitored: usize,
}

/// Capture a shell environment snapshot by running a login shell.
async fn capture_shell_snapshot(shell_path: &str) -> Result<ShellSnapshot> {
    let shell_kind = ShellKind::from_path(shell_path);

    let capture_script = format!(
        "printf '{}\\n'; env -0; printf '\\n{}\\n'",
        ENV_BEGIN_MARKER, ENV_END_MARKER
    );

    let output = Command::new(shell_path)
        .args(["-lc", &capture_script])
        .output()
        .await
        .with_context(|| format!("Failed to run login shell: {shell_path}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!(
            "Login shell exited with status {}: {}",
            output.status.code().unwrap_or(-1),
            stderr.trim()
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let env = parse_env_output(&stdout)?;

    let config_fingerprints: Vec<FileFingerprint> = shell_kind
        .config_files()
        .into_iter()
        .filter(|p| p.exists())
        .map(FileFingerprint::from_path)
        .collect();

    Ok(ShellSnapshot {
        shell_path: shell_path.to_string(),
        shell_kind,
        env,
        captured_at: Instant::now(),
        config_fingerprints,
    })
}

/// Parse the environment output from the capture script.
fn parse_env_output(output: &str) -> Result<HashMap<String, String>> {
    let begin_idx = output
        .find(ENV_BEGIN_MARKER)
        .ok_or_else(|| anyhow!("Missing begin marker in env output"))?;
    let end_idx = output
        .find(ENV_END_MARKER)
        .ok_or_else(|| anyhow!("Missing end marker in env output"))?;

    if end_idx <= begin_idx {
        return Err(anyhow!("Invalid marker positions in env output"));
    }

    let env_section = &output[begin_idx + ENV_BEGIN_MARKER.len()..end_idx];
    let env_section = env_section.trim();

    let mut env = HashMap::new();
    for entry in env_section.split('\0') {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }

        if let Some(eq_pos) = entry.find('=') {
            let key = &entry[..eq_pos];
            let value = &entry[eq_pos + 1..];

            if !should_exclude_env_var(key) {
                env.insert(key.to_string(), value.to_string());
            }
        }
    }

    if env.is_empty() {
        return Err(anyhow!("No environment variables captured"));
    }

    Ok(env)
}

/// Check if an environment variable should be excluded from the snapshot.
fn should_exclude_env_var(key: &str) -> bool {
    if EXCLUDED_ENV_VARS.contains(&key) {
        return true;
    }

    if key.starts_with("BASH_") && key != "BASH_VERSION" {
        return true;
    }
    if key.starts_with("ZSH_") {
        return true;
    }

    false
}

/// Global singleton for the shell snapshot manager.
static GLOBAL_SNAPSHOT_MANAGER: once_cell::sync::Lazy<ShellSnapshotManager> =
    once_cell::sync::Lazy::new(ShellSnapshotManager::new);

/// Get the global shell snapshot manager.
pub fn global_snapshot_manager() -> &'static ShellSnapshotManager {
    &GLOBAL_SNAPSHOT_MANAGER
}

/// Apply a snapshot's environment to a command builder.
pub fn apply_snapshot_env(command: &mut Command, snapshot: &ShellSnapshot, clear_env: bool) {
    if clear_env {
        command.env_clear();
    }

    for (key, value) in &snapshot.env {
        command.env(key, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shell_kind_detection() {
        assert_eq!(ShellKind::from_path("/bin/bash"), ShellKind::Bash);
        assert_eq!(ShellKind::from_path("/usr/bin/zsh"), ShellKind::Zsh);
        assert_eq!(ShellKind::from_path("/bin/sh"), ShellKind::Sh);
        assert_eq!(ShellKind::from_path("/usr/local/bin/fish"), ShellKind::Fish);
        assert_eq!(ShellKind::from_path("/unknown/shell"), ShellKind::Unknown);
    }

    #[test]
    fn test_parse_env_output() {
        let output = format!(
            "some noise\n{}\nHOME=/home/user\0PATH=/usr/bin\0EXCLUDED=yes\0\n{}\nmore noise",
            ENV_BEGIN_MARKER, ENV_END_MARKER
        );
        let env = parse_env_output(&output).unwrap();
        assert_eq!(env.get("HOME"), Some(&"/home/user".to_string()));
        assert_eq!(env.get("PATH"), Some(&"/usr/bin".to_string()));
    }

    #[test]
    fn test_excluded_env_vars() {
        assert!(should_exclude_env_var("PWD"));
        assert!(should_exclude_env_var("SHLVL"));
        assert!(should_exclude_env_var("BASH_COMMAND"));
        assert!(!should_exclude_env_var("BASH_VERSION"));
        assert!(!should_exclude_env_var("HOME"));
        assert!(!should_exclude_env_var("PATH"));
    }

    #[test]
    fn test_file_fingerprint() {
        let fp = FileFingerprint::from_path(PathBuf::from("/nonexistent/file"));
        assert!(fp.mtime.is_none());
        assert!(fp.size.is_none());
    }

    #[test]
    fn test_snapshot_manager_stats() {
        let manager = ShellSnapshotManager::new();
        let stats = manager.stats();
        assert!(!stats.has_snapshot);
        assert_eq!(stats.env_count, 0);
    }
}
