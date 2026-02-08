use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::Utc;
use serde_json::Value;
use tokio::task::spawn_blocking;

use crate::config::defaults::get_config_dir;
use crate::config::loader::VTCodeConfig;
use crate::utils::file_utils::{
    ensure_dir_exists, ensure_dir_exists_sync, read_json_file, read_json_file_sync,
    write_json_file, write_json_file_sync,
};

use super::types::{TeamConfig, TeamMailboxMessage, TeamTaskList};

#[derive(Debug, Clone)]
pub struct TeamStoragePaths {
    pub base_dir: PathBuf,
    pub teams_dir: PathBuf,
    pub tasks_dir: PathBuf,
}

impl TeamStoragePaths {
    pub fn new(base_dir: PathBuf) -> Self {
        let teams_dir = base_dir.join("teams");
        let tasks_dir = base_dir.join("tasks");
        Self {
            base_dir,
            teams_dir,
            tasks_dir,
        }
    }

    pub fn team_dir(&self, team_name: &str) -> PathBuf {
        self.teams_dir.join(sanitize_component(team_name))
    }

    pub fn team_config_path(&self, team_name: &str) -> PathBuf {
        self.team_dir(team_name).join("config.json")
    }

    pub fn mailbox_dir(&self, team_name: &str) -> PathBuf {
        self.team_dir(team_name).join("mailbox")
    }

    pub fn mailbox_path(&self, team_name: &str, recipient: &str) -> PathBuf {
        self.mailbox_dir(team_name)
            .join(format!("{}.jsonl", sanitize_component(recipient)))
    }

    pub fn tasks_path(&self, team_name: &str) -> PathBuf {
        self.tasks_dir
            .join(sanitize_component(team_name))
            .join("tasks.json")
    }

    pub fn tasks_lock_path(&self, team_name: &str) -> PathBuf {
        self.tasks_dir
            .join(sanitize_component(team_name))
            .join("tasks.lock")
    }
}

#[derive(Debug, Clone)]
pub struct TeamStorage {
    paths: TeamStoragePaths,
}

impl TeamStorage {
    pub async fn from_config(vt_cfg: Option<&VTCodeConfig>) -> Result<Self> {
        let base_dir = resolve_base_dir(vt_cfg)?;
        let paths = TeamStoragePaths::new(base_dir);
        ensure_dir_exists(&paths.teams_dir).await?;
        ensure_dir_exists(&paths.tasks_dir).await?;
        Ok(Self { paths })
    }

    pub fn paths(&self) -> &TeamStoragePaths {
        &self.paths
    }

    pub async fn load_team_config(&self, team_name: &str) -> Result<Option<TeamConfig>> {
        let path = self.paths.team_config_path(team_name);
        if !path.exists() {
            return Ok(None);
        }
        let config = read_json_file(&path).await?;
        Ok(Some(config))
    }

    pub async fn save_team_config(&self, config: &TeamConfig) -> Result<()> {
        let path = self.paths.team_config_path(&config.name);
        if let Some(parent) = path.parent() {
            ensure_dir_exists(parent).await?;
        }
        write_json_file(&path, config).await
    }

    pub async fn load_tasks(&self, team_name: &str) -> Result<TeamTaskList> {
        let path = self.paths.tasks_path(team_name);
        if !path.exists() {
            return Ok(TeamTaskList::default());
        }
        read_json_file(&path).await
    }

    pub async fn save_tasks(&self, team_name: &str, tasks: &TeamTaskList) -> Result<()> {
        let path = self.paths.tasks_path(team_name);
        if let Some(parent) = path.parent() {
            ensure_dir_exists(parent).await?;
        }
        write_json_file(&path, tasks).await
    }

    pub async fn append_mailbox_message(
        &self,
        team_name: &str,
        recipient: &str,
        message: &TeamMailboxMessage,
    ) -> Result<()> {
        let path = self.paths.mailbox_path(team_name, recipient);
        let serialized = serde_json::to_string(message).with_context(|| {
            format!("Failed to serialize mailbox message for {}", path.display())
        })?;
        let line = format!("{}\n", serialized);

        let path_clone = path.clone();
        spawn_blocking(move || -> Result<()> {
            if let Some(parent) = path_clone.parent() {
                ensure_dir_exists_sync(parent)?;
            }
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path_clone)
                .with_context(|| format!("Failed to open mailbox file {}", path_clone.display()))?;
            file.write_all(line.as_bytes()).with_context(|| {
                format!("Failed to write mailbox file {}", path_clone.display())
            })?;
            Ok(())
        })
        .await??;

        Ok(())
    }

    pub async fn read_mailbox_since(
        &self,
        team_name: &str,
        recipient: &str,
        offset: u64,
    ) -> Result<(Vec<TeamMailboxMessage>, u64)> {
        let path = self.paths.mailbox_path(team_name, recipient);
        if !path.exists() {
            return Ok((Vec::new(), offset));
        }

        let path_clone = path.clone();
        let (messages, new_offset) =
            spawn_blocking(move || -> Result<(Vec<TeamMailboxMessage>, u64)> {
                use std::io::{Read, Seek, SeekFrom};
                let mut file = std::fs::OpenOptions::new()
                    .read(true)
                    .open(&path_clone)
                    .with_context(|| {
                        format!("Failed to open mailbox file {}", path_clone.display())
                    })?;
                let len = file.metadata()?.len();
                let start = offset.min(len);
                file.seek(SeekFrom::Start(start))?;
                let mut content = String::new();
                file.read_to_string(&mut content)?;
                let mut parsed = Vec::new();
                for line in content.lines() {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    let message: TeamMailboxMessage =
                        serde_json::from_str(trimmed).with_context(|| {
                            format!("Failed to parse mailbox line for {}", path_clone.display())
                        })?;
                    parsed.push(message);
                }
                Ok((parsed, len))
            })
            .await??;

        Ok((messages, new_offset))
    }

    pub async fn with_task_lock<F, T>(&self, team_name: &str, f: F) -> Result<T>
    where
        F: FnOnce(&mut TeamTaskList) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let tasks_path = self.paths.tasks_path(team_name);
        let lock_path = self.paths.tasks_lock_path(team_name);
        spawn_blocking(move || -> Result<T> {
            use fs2::FileExt;
            use std::fs::OpenOptions;

            if let Some(parent) = lock_path.parent() {
                ensure_dir_exists_sync(parent)?;
            }

            let lock_file = OpenOptions::new()
                .create(true)
                .read(true)
                .write(true)
                .open(&lock_path)
                .with_context(|| format!("Failed to open lock file {}", lock_path.display()))?;

            lock_file
                .lock_exclusive()
                .with_context(|| format!("Failed to lock {}", lock_path.display()))?;

            let mut tasks = if tasks_path.exists() {
                read_json_file_sync(&tasks_path)?
            } else {
                TeamTaskList::default()
            };

            let result = f(&mut tasks)?;
            if let Some(parent) = tasks_path.parent() {
                ensure_dir_exists_sync(parent)?;
            }
            write_json_file_sync(&tasks_path, &tasks)?;
            lock_file.unlock().ok();
            Ok(result)
        })
        .await?
    }
}

fn resolve_base_dir(vt_cfg: Option<&VTCodeConfig>) -> Result<PathBuf> {
    if let Some(cfg) = vt_cfg
        && let Some(value) = cfg.agent_teams.storage_dir.as_ref()
        && !value.trim().is_empty()
    {
        let config_dir = get_config_dir().unwrap_or_else(|| fallback_config_dir());
        return Ok(resolve_storage_override(value, &config_dir));
    }

    Ok(get_config_dir().unwrap_or_else(fallback_config_dir))
}

fn fallback_config_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".vtcode")
}

fn resolve_storage_override(value: &str, config_dir: &Path) -> PathBuf {
    let trimmed = value.trim();
    if let Some(rest) = trimmed.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }

    let candidate = PathBuf::from(trimmed);
    if candidate.is_relative() {
        return config_dir.join(candidate);
    }

    candidate
}

fn sanitize_component(value: &str) -> String {
    let mut normalized = String::new();
    let mut last_was_separator = false;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            normalized.push(ch.to_ascii_lowercase());
            last_was_separator = false;
        } else if matches!(ch, '-' | '_') {
            if !last_was_separator {
                normalized.push(ch);
                last_was_separator = true;
            }
        } else if !last_was_separator {
            normalized.push('-');
            last_was_separator = true;
        }
    }

    let trimmed = normalized.trim_matches(|c| c == '-' || c == '_');
    if trimmed.is_empty() {
        format!("team-{}", Utc::now().timestamp_millis())
    } else {
        trimmed.to_owned()
    }
}

pub fn build_task_completion_details(
    task_id: u64,
    teammate: Option<&str>,
    summary: Option<&str>,
) -> Value {
    serde_json::json!({
        "task_id": task_id,
        "assigned_to": teammate,
        "summary": summary,
    })
}
