use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::{Deserialize, Serialize};

use crate::utils::session_archive::SessionMessage;

const MAX_DESCRIPTION_LEN: usize = 160;
const SECONDS_PER_DAY: u64 = 24 * 60 * 60;
pub const DEFAULT_CHECKPOINTS_ENABLED: bool = true;
pub const DEFAULT_MAX_SNAPSHOTS: usize = 50;
pub const DEFAULT_MAX_AGE_DAYS: u64 = 30;

fn sanitize_relative_path(path: &Path) -> Option<PathBuf> {
    if path.is_absolute() {
        return None;
    }

    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => normalized.push(part),
            Component::ParentDir => {
                if !normalized.pop() {
                    return None;
                }
            }
            Component::Prefix(_) | Component::RootDir => {
                return None;
            }
        }
    }
    Some(normalized)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotMetadata {
    pub id: String,
    pub turn_number: usize,
    pub created_at: u64,
    pub description: String,
    pub message_count: usize,
    pub file_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum FileEncoding {
    Utf8,
    Base64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileSnapshot {
    pub path: String,
    pub deleted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encoding: Option<FileEncoding>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoredSnapshot {
    pub metadata: SnapshotMetadata,
    pub conversation: Vec<SessionMessage>,
    pub files: Vec<FileSnapshot>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevertScope {
    Conversation,
    Code,
    Both,
}

impl RevertScope {
    pub fn includes_code(self) -> bool {
        matches!(self, Self::Code | Self::Both)
    }

    pub fn includes_conversation(self) -> bool {
        matches!(self, Self::Conversation | Self::Both)
    }
}

pub struct SnapshotConfig {
    pub enabled: bool,
    pub workspace: PathBuf,
    pub storage_dir: Option<PathBuf>,
    pub max_snapshots: usize,
    pub max_age_days: Option<u64>,
}

impl SnapshotConfig {
    pub fn new(workspace: PathBuf) -> Self {
        Self {
            enabled: DEFAULT_CHECKPOINTS_ENABLED,
            workspace,
            storage_dir: None,
            max_snapshots: DEFAULT_MAX_SNAPSHOTS,
            max_age_days: Some(DEFAULT_MAX_AGE_DAYS),
        }
    }

    fn storage_dir(&self) -> PathBuf {
        self.storage_dir
            .clone()
            .unwrap_or_else(|| self.workspace.join(".vtcode").join("checkpoints"))
    }
}

pub struct SnapshotManager {
    enabled: bool,
    workspace: PathBuf,
    canonical_workspace: PathBuf,
    storage_dir: PathBuf,
    max_snapshots: usize,
    max_age_days: Option<u64>,
}

impl SnapshotManager {
    pub fn new(config: SnapshotConfig) -> Result<Self> {
        let storage_dir = config.storage_dir();
        let canonical_workspace =
            fs::canonicalize(&config.workspace).unwrap_or_else(|_| config.workspace.clone());

        if config.enabled {
            fs::create_dir_all(&storage_dir).with_context(|| {
                format!(
                    "failed to create checkpoint directory: {}",
                    storage_dir.display()
                )
            })?;
        }
        Ok(Self {
            enabled: config.enabled,
            workspace: config.workspace,
            canonical_workspace,
            storage_dir,
            max_snapshots: config.max_snapshots,
            max_age_days: config.max_age_days,
        })
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    fn snapshot_path(&self, turn_number: usize) -> PathBuf {
        self.storage_dir.join(format!("turn_{}.json", turn_number))
    }

    fn normalize_path(&self, path: &Path) -> Option<PathBuf> {
        if path.is_absolute() {
            if let Ok(canonical_path) = fs::canonicalize(path) {
                if let Ok(stripped) = canonical_path.strip_prefix(&self.canonical_workspace) {
                    return sanitize_relative_path(stripped);
                }
            }

            if let Ok(stripped) = path.strip_prefix(&self.workspace) {
                return sanitize_relative_path(stripped);
            }

            return None;
        }

        sanitize_relative_path(path)
    }

    fn read_snapshot_files(&self) -> Result<Vec<(usize, PathBuf)>> {
        let mut entries = Vec::new();
        if !self.storage_dir.exists() {
            return Ok(entries);
        }
        for entry in fs::read_dir(&self.storage_dir).with_context(|| {
            format!(
                "failed to read checkpoint directory: {}",
                self.storage_dir.display()
            )
        })? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let stem = match path.file_stem().and_then(|stem| stem.to_str()) {
                Some(value) => value,
                None => continue,
            };
            let turn_str = match stem.strip_prefix("turn_") {
                Some(value) => value,
                None => continue,
            };
            if let Ok(turn) = turn_str.parse::<usize>() {
                entries.push((turn, path));
            }
        }
        entries.sort_by_key(|(turn, _)| *turn);
        Ok(entries)
    }

    fn encode_file(bytes: &[u8]) -> (FileEncoding, String) {
        match std::str::from_utf8(bytes) {
            Ok(text) => (FileEncoding::Utf8, text.to_string()),
            Err(_) => (FileEncoding::Base64, BASE64.encode(bytes)),
        }
    }

    fn decode_file(encoding: FileEncoding, data: &str) -> Result<Vec<u8>> {
        match encoding {
            FileEncoding::Utf8 => Ok(data.as_bytes().to_vec()),
            FileEncoding::Base64 => BASE64
                .decode(data)
                .context("failed to decode base64 file contents"),
        }
    }

    fn truncate_description(description: &str) -> String {
        let first_line = description.lines().next().unwrap_or("").trim();
        if first_line.chars().count() <= MAX_DESCRIPTION_LEN {
            return first_line.to_string();
        }
        first_line
            .chars()
            .take(MAX_DESCRIPTION_LEN.saturating_sub(1))
            .chain(std::iter::once('…'))
            .collect()
    }

    fn current_timestamp() -> Result<u64> {
        Ok(SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("system clock before UNIX_EPOCH")?
            .as_secs())
    }

    pub fn next_turn_number(&self) -> Result<usize> {
        Ok(self
            .read_snapshot_files()?
            .into_iter()
            .map(|(turn, _)| turn)
            .max()
            .unwrap_or(0)
            .saturating_add(1))
    }

    pub fn create_snapshot(
        &self,
        turn_number: usize,
        description: &str,
        conversation: &[SessionMessage],
        modified_files: &BTreeSet<PathBuf>,
    ) -> Result<Option<SnapshotMetadata>> {
        if !self.enabled {
            return Ok(None);
        }

        let timestamp = Self::current_timestamp()?;
        let mut files = Vec::new();

        for path in modified_files {
            let relative = match self.normalize_path(path) {
                Some(value) => value,
                None => continue,
            };
            let absolute = self.workspace.join(&relative);
            if absolute.exists() {
                let bytes = fs::read(&absolute).with_context(|| {
                    format!("failed to read file for checkpoint: {}", absolute.display())
                })?;
                let (encoding, data) = Self::encode_file(&bytes);
                files.push(FileSnapshot {
                    path: relative.to_string_lossy().replace('\\', "/"),
                    deleted: false,
                    encoding: Some(encoding),
                    data: Some(data),
                });
            } else {
                files.push(FileSnapshot {
                    path: relative.to_string_lossy().replace('\\', "/"),
                    deleted: true,
                    encoding: None,
                    data: None,
                });
            }
        }

        let metadata = SnapshotMetadata {
            id: format!("turn_{}", turn_number),
            turn_number,
            created_at: timestamp,
            description: Self::truncate_description(description),
            message_count: conversation.len(),
            file_count: files.len(),
        };

        let stored = StoredSnapshot {
            metadata: metadata.clone(),
            conversation: conversation.to_vec(),
            files,
        };

        let path = self.snapshot_path(turn_number);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!(
                    "failed to ensure checkpoint directory: {}",
                    parent.display()
                )
            })?;
        }

        let data = serde_json::to_vec_pretty(&stored).context("failed to serialize checkpoint")?;
        let mut file = fs::File::create(&path)
            .with_context(|| format!("failed to create checkpoint: {}", path.display()))?;
        file.write_all(&data)
            .with_context(|| format!("failed to write checkpoint: {}", path.display()))?;

        self.cleanup_old_snapshots()?;

        Ok(Some(metadata))
    }

    pub fn list_snapshots(&self) -> Result<Vec<SnapshotMetadata>> {
        if !self.enabled {
            return Ok(Vec::new());
        }
        self.cleanup_old_snapshots()?;
        let mut snapshots = Vec::new();
        for (_, path) in self.read_snapshot_files()? {
            let data = fs::read(&path)
                .with_context(|| format!("failed to read checkpoint: {}", path.display()))?;
            let stored: StoredSnapshot = serde_json::from_slice(&data)
                .with_context(|| format!("failed to parse checkpoint: {}", path.display()))?;
            snapshots.push(stored.metadata);
        }
        snapshots.sort_by(|a, b| b.turn_number.cmp(&a.turn_number));
        Ok(snapshots)
    }

    pub fn load_snapshot(&self, turn_number: usize) -> Result<Option<StoredSnapshot>> {
        if !self.enabled {
            return Ok(None);
        }
        let path = self.snapshot_path(turn_number);
        if !path.exists() {
            return Ok(None);
        }
        let data = fs::read(&path)
            .with_context(|| format!("failed to read checkpoint: {}", path.display()))?;
        let stored = serde_json::from_slice(&data)
            .with_context(|| format!("failed to parse checkpoint: {}", path.display()))?;
        Ok(Some(stored))
    }

    pub fn restore_snapshot(
        &self,
        turn_number: usize,
        scope: RevertScope,
    ) -> Result<Option<CheckpointRestore>> {
        let Some(stored) = self.load_snapshot(turn_number)? else {
            return Ok(None);
        };

        if scope.includes_code() {
            for snapshot in &stored.files {
                let relative = Path::new(&snapshot.path);
                let Some(sanitized) = sanitize_relative_path(relative) else {
                    continue;
                };
                let absolute = self.workspace.join(&sanitized);
                if snapshot.deleted {
                    if absolute.exists() {
                        fs::remove_file(&absolute).with_context(|| {
                            format!(
                                "failed to remove file during checkpoint restore: {}",
                                absolute.display()
                            )
                        })?;
                    }
                    continue;
                }

                if let Some(parent) = absolute.parent() {
                    fs::create_dir_all(parent).with_context(|| {
                        format!(
                            "failed to create directories for restore: {}",
                            parent.display()
                        )
                    })?;
                }

                let encoding = snapshot.encoding.clone().unwrap_or(FileEncoding::Utf8);
                let data = snapshot.data.as_deref().unwrap_or_default();
                let bytes = Self::decode_file(encoding, data)?;
                fs::write(&absolute, &bytes).with_context(|| {
                    format!("failed to write restored file: {}", absolute.display())
                })?;
            }
        }

        let conversation = if scope.includes_conversation() {
            stored.conversation.clone()
        } else {
            Vec::new()
        };

        Ok(Some(CheckpointRestore {
            metadata: stored.metadata,
            conversation,
        }))
    }

    pub fn cleanup_old_snapshots(&self) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }

        let mut entries = self.read_snapshot_files()?;

        if let Some(cutoff) = self.retention_cutoff_secs()? {
            let stale_entries = entries.clone();
            for (_, path) in stale_entries {
                let data = match fs::read(&path) {
                    Ok(data) => data,
                    Err(err) => {
                        eprintln!(
                            "Warning: failed to read checkpoint {}: {}",
                            path.display(),
                            err
                        );
                        continue;
                    }
                };
                let stored: StoredSnapshot = match serde_json::from_slice(&data) {
                    Ok(value) => value,
                    Err(err) => {
                        eprintln!(
                            "Warning: failed to parse checkpoint {}: {}",
                            path.display(),
                            err
                        );
                        continue;
                    }
                };
                if stored.metadata.created_at <= cutoff
                    && let Err(err) = fs::remove_file(&path)
                {
                    eprintln!(
                        "Warning: failed to remove expired checkpoint {}: {}",
                        path.display(),
                        err
                    );
                }
            }
            entries = self.read_snapshot_files()?;
        }

        if self.max_snapshots == 0 || entries.len() <= self.max_snapshots {
            return Ok(());
        }

        let excess = entries.len() - self.max_snapshots;
        for (_, path) in entries.into_iter().take(excess) {
            if let Err(err) = fs::remove_file(&path) {
                eprintln!(
                    "Warning: failed to remove old checkpoint {}: {}",
                    path.display(),
                    err
                );
            }
        }
        Ok(())
    }

    fn retention_cutoff_secs(&self) -> Result<Option<u64>> {
        let Some(days) = self.max_age_days else {
            return Ok(None);
        };

        let now = Self::current_timestamp()?;
        if days == 0 {
            return Ok(Some(now));
        }

        let seconds = days.saturating_mul(SECONDS_PER_DAY);
        let cutoff_instant = SystemTime::now()
            .checked_sub(Duration::from_secs(seconds))
            .unwrap_or(SystemTime::UNIX_EPOCH);
        let cutoff = cutoff_instant
            .duration_since(UNIX_EPOCH)
            .context("system clock before UNIX_EPOCH")?
            .as_secs();
        Ok(Some(cutoff))
    }

    pub fn parse_revert_scope(value: &str) -> Option<RevertScope> {
        match value.to_ascii_lowercase().as_str() {
            "conversation" | "chat" => Some(RevertScope::Conversation),
            "code" | "files" => Some(RevertScope::Code),
            "both" | "full" => Some(RevertScope::Both),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CheckpointRestore {
    pub metadata: SnapshotMetadata,
    pub conversation: Vec<SessionMessage>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_manager() -> (TempDir, SnapshotManager) {
        let dir = TempDir::new().expect("tempdir");
        let workspace = dir.path().to_path_buf();
        let manager =
            SnapshotManager::new(SnapshotConfig::new(workspace.clone())).expect("manager");
        (dir, manager)
    }

    #[test]
    fn create_and_list_snapshots() -> Result<()> {
        let (_dir, manager) = setup_manager();
        let mut conversation = Vec::new();
        conversation.push(SessionMessage::new(
            crate::llm::provider::MessageRole::User,
            "Hello",
        ));
        let mut files = BTreeSet::new();
        manager
            .create_snapshot(1, "First turn", &conversation, &files)?
            .expect("metadata");
        conversation.push(SessionMessage::new(
            crate::llm::provider::MessageRole::Assistant,
            "Hi",
        ));
        manager
            .create_snapshot(2, "Second turn", &conversation, &files)?
            .expect("metadata");

        let snapshots = manager.list_snapshots()?;
        assert_eq!(snapshots.len(), 2);
        assert_eq!(snapshots[0].turn_number, 2);
        assert_eq!(snapshots[1].turn_number, 1);
        Ok(())
    }

    #[test]
    fn snapshot_restores_file_contents() -> Result<()> {
        let (dir, manager) = setup_manager();
        let workspace = dir.path();
        let file_path = workspace.join("example.txt");
        fs::write(&file_path, "v1")?;

        let mut files = BTreeSet::new();
        files.insert(PathBuf::from("example.txt"));
        let conversation = vec![SessionMessage::new(
            crate::llm::provider::MessageRole::User,
            "edit example",
        )];
        manager
            .create_snapshot(1, "save", &conversation, &files)?
            .expect("metadata");

        fs::write(&file_path, "v2")?;
        manager
            .restore_snapshot(1, RevertScope::Code)?
            .expect("restore");
        let restored = fs::read_to_string(&file_path)?;
        assert_eq!(restored, "v1");
        Ok(())
    }

    #[test]
    fn snapshot_handles_deleted_files() -> Result<()> {
        let (dir, manager) = setup_manager();
        let workspace = dir.path();
        let file_path = workspace.join("remove.txt");
        fs::write(&file_path, "data")?;

        let mut files = BTreeSet::new();
        files.insert(PathBuf::from("remove.txt"));
        let conversation = vec![SessionMessage::new(
            crate::llm::provider::MessageRole::User,
            "remove",
        )];
        manager
            .create_snapshot(1, "save", &conversation, &files)?
            .expect("metadata");

        fs::remove_file(&file_path)?;
        manager
            .restore_snapshot(1, RevertScope::Code)?
            .expect("restore");
        assert!(file_path.exists());
        let content = fs::read_to_string(&file_path)?;
        assert_eq!(content, "data");
        Ok(())
    }

    #[test]
    fn cleanup_respects_limit() -> Result<()> {
        let (_dir, manager) = setup_manager();
        let conversation = vec![SessionMessage::new(
            crate::llm::provider::MessageRole::User,
            "hi",
        )];
        let files = BTreeSet::new();

        for turn in 1..=5 {
            manager
                .create_snapshot(turn, "turn", &conversation, &files)?
                .expect("metadata");
        }

        // Manager default limit is 50, shrink artificially for test
        let mut config = SnapshotConfig::new(manager.workspace.clone());
        config.max_snapshots = 3;
        let trimmed = SnapshotManager::new(config)?;
        trimmed.cleanup_old_snapshots()?;
        let listed = trimmed.list_snapshots()?;
        assert_eq!(listed.len(), 3);
        assert_eq!(listed[0].turn_number, 5);
        assert_eq!(listed[2].turn_number, 3);
        Ok(())
    }

    #[test]
    fn snapshot_normalizes_absolute_paths() -> Result<()> {
        let (dir, manager) = setup_manager();
        let workspace = dir.path();
        let absolute = workspace.join("abs.txt");
        fs::write(&absolute, "contents")?;

        let mut files = BTreeSet::new();
        files.insert(absolute.clone());
        let conversation = vec![SessionMessage::new(
            crate::llm::provider::MessageRole::User,
            "absolute",
        )];

        manager
            .create_snapshot(1, "abs", &conversation, &files)?
            .expect("metadata");

        let stored = manager.load_snapshot(1)?.expect("stored snapshot");
        assert_eq!(stored.files.len(), 1);
        assert_eq!(stored.files[0].path, "abs.txt");
        assert!(!stored.files[0].deleted);
        Ok(())
    }

    #[test]
    fn cleanup_removes_expired_snapshots() -> Result<()> {
        let (_dir, manager) = setup_manager();
        let conversation = vec![SessionMessage::new(
            crate::llm::provider::MessageRole::User,
            "cleanup",
        )];
        let files = BTreeSet::new();

        manager
            .create_snapshot(1, "old", &conversation, &files)?
            .expect("metadata");

        let snapshot_path = manager.snapshot_path(1);
        let mut stored: StoredSnapshot = serde_json::from_slice(&fs::read(&snapshot_path)?)?;
        stored.metadata.created_at = 1;
        let updated = serde_json::to_vec_pretty(&stored)?;
        fs::write(&snapshot_path, updated)?;

        let mut config = SnapshotConfig::new(manager.workspace.clone());
        config.max_age_days = Some(1);
        let janitor = SnapshotManager::new(config)?;
        janitor.cleanup_old_snapshots()?;

        assert!(janitor.load_snapshot(1)?.is_none());
        Ok(())
    }

    #[test]
    fn parse_revert_scope_variants() {
        assert_eq!(
            SnapshotManager::parse_revert_scope("conversation"),
            Some(RevertScope::Conversation)
        );
        assert_eq!(
            SnapshotManager::parse_revert_scope("code"),
            Some(RevertScope::Code)
        );
        assert_eq!(
            SnapshotManager::parse_revert_scope("full"),
            Some(RevertScope::Both)
        );
        assert_eq!(SnapshotManager::parse_revert_scope("unknown"), None);
    }
}
