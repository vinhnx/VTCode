use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::telemetry::perf::PerfSpan;
use crate::utils::async_line_writer::AsyncLineWriter;

const TRAJECTORY_PREFIX: &str = "trajectory-";
const TRAJECTORY_EXTENSION: &str = "jsonl";
const SECONDS_PER_DAY: u64 = 24 * 60 * 60;
const BYTES_PER_MB: u64 = 1024 * 1024;

#[derive(Clone)]
pub struct TrajectoryLogger {
    enabled: bool,
    writer: Option<Arc<AsyncLineWriter>>,
}

#[derive(Debug, Clone, Copy)]
pub struct TrajectoryRetention {
    pub max_files: usize,
    pub max_age_days: u64,
    pub max_total_size_bytes: u64,
}

impl Default for TrajectoryRetention {
    fn default() -> Self {
        use vtcode_config::constants::defaults;
        Self {
            max_files: defaults::DEFAULT_TRAJECTORY_MAX_FILES,
            max_age_days: defaults::DEFAULT_TRAJECTORY_MAX_AGE_DAYS,
            max_total_size_bytes: defaults::DEFAULT_TRAJECTORY_MAX_SIZE_MB
                .saturating_mul(BYTES_PER_MB),
        }
    }
}

impl TrajectoryLogger {
    pub fn new(workspace: &Path) -> Self {
        Self::with_retention(workspace, TrajectoryRetention::default())
    }

    pub fn with_retention(workspace: &Path, retention: TrajectoryRetention) -> Self {
        let dir = workspace.join(".vtcode").join("logs");
        rotate_current_trajectory(&dir);
        prune_trajectory_logs_best_effort(&dir, retention);
        let path = dir.join("trajectory.jsonl");
        let writer = AsyncLineWriter::new(path).ok().map(Arc::new);
        let enabled = writer.is_some();
        Self { enabled, writer }
    }

    pub fn disabled() -> Self {
        Self {
            enabled: false,
            writer: None,
        }
    }

    pub fn log<T: Serialize>(&self, record: &T) {
        if !self.enabled {
            return;
        }
        let mut perf = PerfSpan::new("vtcode.perf.trajectory_log_ms");
        perf.tag("mode", "async");
        if let Ok(line) = serde_json::to_string(record)
            && let Some(writer) = self.writer.as_ref()
        {
            writer.write_line(line);
        }
    }

    #[cfg(test)]
    pub fn flush(&self) {
        if let Some(writer) = self.writer.as_ref() {
            writer.flush();
        }
    }

    pub fn log_route(&self, turn: usize, selected_model: &str, class: &str, input_preview: &str) {
        #[derive(Serialize)]
        struct RouteRec<'a> {
            kind: &'static str,
            turn: usize,
            selected_model: &'a str,
            class: &'a str,
            input_preview: &'a str,
            ts: i64,
        }
        let rec = RouteRec {
            kind: "route",
            turn,
            selected_model,
            class,
            input_preview,
            ts: chrono::Utc::now().timestamp(),
        };
        self.log(&rec);
    }

    pub fn log_tool_call(&self, turn: usize, name: &str, args: &serde_json::Value, ok: bool) {
        #[derive(Serialize)]
        struct ToolRec<'a> {
            kind: &'static str,
            turn: usize,
            name: &'a str,
            args: serde_json::Value,
            ok: bool,
            ts: i64,
        }
        let rec = ToolRec {
            kind: "tool",
            turn,
            name,
            args: args.clone(),
            ok,
            ts: chrono::Utc::now().timestamp(),
        };
        self.log(&rec);
    }
}

fn rotate_current_trajectory(dir: &Path) {
    let current = dir.join("trajectory.jsonl");
    if !current.exists() {
        return;
    }
    let metadata = match fs::metadata(&current) {
        Ok(m) => m,
        Err(_) => return,
    };
    if metadata.len() == 0 {
        return;
    }
    let timestamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");
    let rotated_name = format!("{TRAJECTORY_PREFIX}{timestamp}.{TRAJECTORY_EXTENSION}");
    let rotated_path = dir.join(rotated_name);
    let _ = fs::rename(&current, &rotated_path);
}

fn is_trajectory_file(path: &Path) -> bool {
    let name = match path.file_name().and_then(|n| n.to_str()) {
        Some(n) => n,
        None => return false,
    };
    name.starts_with(TRAJECTORY_PREFIX) && name.ends_with(&format!(".{TRAJECTORY_EXTENSION}"))
}

struct FileEntry {
    path: PathBuf,
    modified: SystemTime,
    size: u64,
}

fn prune_trajectory_logs_best_effort(dir: &Path, limits: TrajectoryRetention) {
    if let Err(err) = prune_trajectory_logs(dir, limits) {
        tracing::debug!(
            "Failed to prune trajectory logs in {}: {}",
            dir.display(),
            err
        );
    }
}

fn prune_trajectory_logs(dir: &Path, limits: TrajectoryRetention) -> anyhow::Result<()> {
    if !dir.exists() {
        return Ok(());
    }

    let mut entries = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        if !is_trajectory_file(&path) {
            continue;
        }
        let metadata = match entry.metadata() {
            Ok(m) if m.is_file() => m,
            _ => continue,
        };
        entries.push(FileEntry {
            path,
            modified: metadata.modified().unwrap_or(UNIX_EPOCH),
            size: metadata.len(),
        });
    }

    if entries.is_empty() {
        return Ok(());
    }

    let now = SystemTime::now();
    let age_cutoff = if limits.max_age_days == 0 {
        now
    } else {
        now.checked_sub(Duration::from_secs(
            limits.max_age_days.saturating_mul(SECONDS_PER_DAY),
        ))
        .unwrap_or(UNIX_EPOCH)
    };

    let (expired, mut retained): (Vec<_>, Vec<_>) = entries
        .into_iter()
        .partition(|entry| entry.modified <= age_cutoff);
    remove_files(expired);

    retained.sort_by(|a, b| b.modified.cmp(&a.modified));

    if limits.max_files > 0 && retained.len() > limits.max_files {
        let overflow = retained.split_off(limits.max_files);
        remove_files(overflow);
    }

    if limits.max_total_size_bytes == 0 || retained.is_empty() {
        return Ok(());
    }

    let mut total_size = 0u64;
    let mut size_overflow = Vec::new();
    let mut keep = Vec::with_capacity(retained.len());
    for entry in retained {
        let projected = total_size.saturating_add(entry.size);
        if keep.is_empty() || projected <= limits.max_total_size_bytes {
            total_size = projected;
            keep.push(entry);
        } else {
            size_overflow.push(entry);
        }
    }
    remove_files(size_overflow);

    Ok(())
}

fn remove_files(entries: Vec<FileEntry>) {
    for entry in entries {
        if let Err(err) = fs::remove_file(&entry.path) {
            tracing::debug!(
                "Failed to remove trajectory log {}: {}",
                entry.path.display(),
                err
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_trajectory_logger_log_route_integration() {
        let temp_dir = TempDir::new().unwrap();
        let logger = TrajectoryLogger::new(temp_dir.path());

        logger.log_route(
            1,
            "gemini-3-flash-preview",
            "standard",
            "test user input for logging",
        );
        logger.flush();

        let log_path = temp_dir.path().join(".vtcode/logs/trajectory.jsonl");
        assert!(log_path.exists());

        let content = fs::read_to_string(log_path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 1);

        let record: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(record["kind"], "route");
        assert_eq!(record["turn"], 1);
        assert_eq!(record["selected_model"], "gemini-3-flash-preview");
        assert_eq!(record["class"], "standard");
        assert_eq!(record["input_preview"], "test user input for logging");
        assert!(record["ts"].is_number());
    }

    #[test]
    fn test_rotation_renames_existing_log() {
        let temp_dir = TempDir::new().unwrap();
        let logs_dir = temp_dir.path().join(".vtcode").join("logs");
        fs::create_dir_all(&logs_dir).unwrap();

        let current = logs_dir.join("trajectory.jsonl");
        fs::write(&current, r#"{"kind":"route","turn":1}"#).unwrap();

        let _logger = TrajectoryLogger::new(temp_dir.path());

        let rotated: Vec<_> = fs::read_dir(&logs_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| is_trajectory_file(&e.path()))
            .collect();
        assert_eq!(rotated.len(), 1, "Old log should be rotated");
        assert!(current.exists(), "New current file should be created");
    }

    #[test]
    fn test_prune_removes_old_files() {
        let temp_dir = TempDir::new().unwrap();
        let logs_dir = temp_dir.path().join(".vtcode").join("logs");
        fs::create_dir_all(&logs_dir).unwrap();

        for i in 0..5 {
            let name = format!("trajectory-2024010{}T000000Z.jsonl", i);
            fs::write(logs_dir.join(name), "data").unwrap();
        }

        let limits = TrajectoryRetention {
            max_files: 3,
            max_age_days: 0,
            max_total_size_bytes: 100 * BYTES_PER_MB,
        };

        prune_trajectory_logs(&logs_dir, limits).unwrap();

        let remaining: Vec<_> = fs::read_dir(&logs_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| is_trajectory_file(&e.path()))
            .collect();
        assert!(remaining.len() <= 3, "Should keep at most 3 files");
    }

    #[test]
    fn test_empty_trajectory_not_rotated() {
        let temp_dir = TempDir::new().unwrap();
        let logs_dir = temp_dir.path().join(".vtcode").join("logs");
        fs::create_dir_all(&logs_dir).unwrap();

        let current = logs_dir.join("trajectory.jsonl");
        fs::write(&current, "").unwrap();

        let _logger = TrajectoryLogger::new(temp_dir.path());

        let rotated: Vec<_> = fs::read_dir(&logs_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| is_trajectory_file(&e.path()))
            .collect();
        assert_eq!(rotated.len(), 0, "Empty file should not be rotated");
    }
}
