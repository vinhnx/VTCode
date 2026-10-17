use anyhow::{Context, Result, bail};
use chrono::{DateTime, Local, Utc};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::session::ScheduleCreateInput;
use super::spec::{ScheduledTaskDefinition, ScheduledTaskRecord, ScheduledTaskRuntimeState, ScheduledTaskSummary};
use super::{MAX_SCHEDULED_TASKS, atomic_write, initialize_runtime_state};
use vtcode_commons::VtCodePaths;

const CLAIM_STALE_SECS: u64 = 15 * 60;
const DURABLE_STORE_DIR: &str = "scheduled_tasks";

#[derive(Debug, Clone)]
pub struct SchedulerPaths {
    pub config_root: PathBuf,
    pub data_root: PathBuf,
}

impl SchedulerPaths {
    pub fn new_default() -> Result<Self> {
        let paths = VtCodePaths::resolve().context("Failed to resolve VT Code paths")?;
        let config_root = paths.config_path(DURABLE_STORE_DIR)?;
        let data_root = paths.state_path(DURABLE_STORE_DIR)?;
        Ok(Self { config_root, data_root })
    }

    #[must_use]
    pub fn tasks_dir(&self) -> PathBuf {
        self.config_root.join("tasks")
    }

    #[must_use]
    pub fn state_dir(&self) -> PathBuf {
        self.data_root.join("state")
    }

    #[must_use]
    pub fn claims_dir(&self) -> PathBuf {
        self.data_root.join("claims")
    }

    #[must_use]
    pub fn runs_dir(&self) -> PathBuf {
        self.data_root.join("runs")
    }

    pub fn ensure_dirs(&self) -> Result<()> {
        for dir in [
            &self.config_root,
            &self.data_root,
            &self.tasks_dir(),
            &self.state_dir(),
            &self.claims_dir(),
            &self.runs_dir(),
        ] {
            VtCodePaths::ensure_user_dir(dir)
                .with_context(|| format!("Failed to create scheduler directory {}", dir.display()))?;
        }
        Ok(())
    }

    #[must_use]
    pub fn definition_path(&self, id: &str) -> PathBuf {
        self.tasks_dir().join(format!("{id}.toml"))
    }

    #[must_use]
    pub fn runtime_path(&self, id: &str) -> PathBuf {
        self.state_dir().join(format!("{id}.json"))
    }

    #[must_use]
    pub fn claim_path(&self, id: &str) -> PathBuf {
        self.claims_dir().join(format!("{id}.claim"))
    }

    #[must_use]
    pub fn artifact_dir(&self, id: &str, run_at: DateTime<Utc>) -> PathBuf {
        self.runs_dir().join(id).join(run_at.format("%Y%m%dT%H%M%SZ").to_string())
    }
}

#[derive(Debug, Clone)]
pub struct DurableTaskStore {
    paths: SchedulerPaths,
}

impl DurableTaskStore {
    #[must_use]
    pub fn with_paths(paths: SchedulerPaths) -> Self {
        Self { paths }
    }

    pub fn new_default() -> Result<Self> {
        Ok(Self { paths: SchedulerPaths::new_default()? })
    }

    #[must_use]
    pub fn paths(&self) -> &SchedulerPaths {
        &self.paths
    }

    pub fn create(&self, definition: ScheduledTaskDefinition) -> Result<ScheduledTaskSummary> {
        self.paths.ensure_dirs()?;
        let current_count = self.definition_paths()?.len();
        if current_count >= MAX_SCHEDULED_TASKS {
            bail!("VT Code supports at most {MAX_SCHEDULED_TASKS} durable scheduled tasks");
        }

        let runtime = initialize_runtime_state(&definition)?;
        self.write_definition(&definition)?;
        self.write_runtime(&definition.id, &runtime)?;
        Ok(ScheduledTaskRecord { definition, runtime }.summary())
    }

    pub fn create_from_input(
        &self,
        input: ScheduleCreateInput,
        now: DateTime<Local>,
        default_workspace: Option<PathBuf>,
    ) -> Result<ScheduledTaskSummary> {
        let definition = input.build_definition(now, default_workspace)?;
        self.create(definition)
    }

    pub fn list(&self) -> Result<Vec<ScheduledTaskSummary>> {
        let mut records = self.load_records()?;
        records.sort_by_key(|record| record.runtime.next_run_at);
        Ok(records.into_iter().map(|record| record.summary()).collect())
    }

    pub fn delete(&self, id: &str) -> Result<Option<ScheduledTaskSummary>> {
        let Some(record) = self.load_record(id)? else {
            return Ok(None);
        };
        let _ = fs::remove_file(self.paths.definition_path(id));
        let _ = fs::remove_file(self.paths.runtime_path(id));
        let _ = fs::remove_file(self.paths.claim_path(id));
        Ok(Some(record.summary()))
    }

    pub fn load_record(&self, id: &str) -> Result<Option<ScheduledTaskRecord>> {
        let definition_path = self.paths.definition_path(id);
        if !definition_path.exists() {
            return Ok(None);
        }
        let definition = read_definition(&definition_path)?;
        let runtime = match self.read_runtime(&definition.id)? {
            Some(runtime) => runtime,
            None => initialize_runtime_state(&definition)?,
        };
        Ok(Some(ScheduledTaskRecord { definition, runtime }))
    }

    pub fn update_runtime(&self, record: &ScheduledTaskRecord) -> Result<()> {
        self.write_runtime(&record.definition.id, &record.runtime)
    }

    pub(crate) async fn load_records_async(&self) -> Result<Vec<ScheduledTaskRecord>> {
        let store = self.clone();
        tokio::task::spawn_blocking(move || store.load_records())
            .await
            .context("Scheduled task loading task panicked")?
    }

    pub(crate) async fn update_runtime_async(&self, record: ScheduledTaskRecord) -> Result<()> {
        let store = self.clone();
        tokio::task::spawn_blocking(move || store.update_runtime(&record))
            .await
            .context("Scheduled task runtime update task panicked")?
    }

    fn load_records(&self) -> Result<Vec<ScheduledTaskRecord>> {
        self.paths.ensure_dirs()?;
        let mut records = Vec::new();
        for definition_path in self.definition_paths()? {
            let definition = read_definition(&definition_path)?;
            let runtime = self
                .read_runtime(&definition.id)?
                .unwrap_or(initialize_runtime_state(&definition)?);
            records.push(ScheduledTaskRecord { definition, runtime });
        }
        Ok(records)
    }

    fn definition_paths(&self) -> Result<Vec<PathBuf>> {
        self.paths.ensure_dirs()?;
        let mut paths = Vec::new();
        for entry in fs::read_dir(self.paths.tasks_dir())
            .with_context(|| format!("Failed to read {}", self.paths.tasks_dir().display()))?
        {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) == Some("toml") {
                paths.push(path);
            }
        }
        paths.sort();
        Ok(paths)
    }

    fn write_definition(&self, definition: &ScheduledTaskDefinition) -> Result<()> {
        let serialized = toml::to_string_pretty(definition).context("Failed to serialize task definition")?;
        atomic_write(&self.paths.definition_path(&definition.id), serialized.as_bytes())
    }

    fn read_runtime(&self, id: &str) -> Result<Option<ScheduledTaskRuntimeState>> {
        let path = self.paths.runtime_path(id);
        if !path.exists() {
            return Ok(None);
        }
        let raw = fs::read_to_string(&path).with_context(|| format!("Failed to read {}", path.display()))?;
        let runtime = serde_json::from_str(&raw).with_context(|| format!("Failed to parse {}", path.display()))?;
        Ok(Some(runtime))
    }

    fn write_runtime(&self, id: &str, runtime: &ScheduledTaskRuntimeState) -> Result<()> {
        let serialized = serde_json::to_vec_pretty(runtime).context("Failed to serialize runtime state")?;
        atomic_write(&self.paths.runtime_path(id), &serialized)
    }
}

fn read_definition(path: &Path) -> Result<ScheduledTaskDefinition> {
    let raw = fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;
    toml::from_str(&raw).with_context(|| format!("Failed to parse {}", path.display()))
}

fn try_acquire_claim(paths: &SchedulerPaths, id: &str) -> Result<bool> {
    paths.ensure_dirs()?;
    let path = paths.claim_path(id);
    match VtCodePaths::create_private_file(&path) {
        Ok(mut file) => {
            let timestamp = Utc::now().to_rfc3339();
            file.write_all(timestamp.as_bytes())
                .with_context(|| format!("Failed to write {}", path.display()))?;
            Ok(true)
        }
        Err(error)
            if error.chain().any(|cause| {
                cause
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|io_error| io_error.kind() == std::io::ErrorKind::AlreadyExists)
            }) =>
        {
            if claim_is_stale(&path)? {
                let _ = fs::remove_file(&path);
                return try_acquire_claim(paths, id);
            }
            Ok(false)
        }
        Err(error) => Err(error).with_context(|| format!("Failed to create {}", path.display())),
    }
}

pub(crate) async fn try_acquire_claim_async(paths: SchedulerPaths, id: String) -> Result<bool> {
    tokio::task::spawn_blocking(move || try_acquire_claim(&paths, &id))
        .await
        .context("Scheduled task claim acquisition task panicked")?
}

fn claim_is_stale(path: &Path) -> Result<bool> {
    let metadata = fs::metadata(path).with_context(|| format!("Failed to stat {}", path.display()))?;
    let modified = metadata
        .modified()
        .with_context(|| format!("Failed to read modification time for {}", path.display()))?;
    let elapsed = modified.elapsed().unwrap_or_default();
    Ok(elapsed >= Duration::from_secs(CLAIM_STALE_SECS))
}

fn release_claim(paths: &SchedulerPaths, id: &str) -> Result<()> {
    let path = paths.claim_path(id);
    if path.exists() {
        fs::remove_file(&path).with_context(|| format!("Failed to remove {}", path.display()))?;
    }
    Ok(())
}

pub(crate) async fn release_claim_async(paths: SchedulerPaths, id: String) -> Result<()> {
    tokio::task::spawn_blocking(move || release_claim(&paths, &id))
        .await
        .context("Scheduled task claim release task panicked")?
}
