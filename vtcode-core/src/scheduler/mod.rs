use anyhow::{Context, Result, anyhow, bail};
use chrono::{
    DateTime, Datelike, Duration as ChronoDuration, Local, LocalResult, NaiveDateTime, NaiveTime,
    TimeZone, Timelike, Utc,
};
use humantime::parse_duration as parse_human_duration;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::process::Command;

use crate::config::defaults::{get_config_dir, get_data_dir};
use crate::notifications::{NotificationEvent, send_global_notification};
use crate::utils::path::normalize_path;

pub const DEFAULT_LOOP_INTERVAL_MINUTES: u64 = 10;
pub const MAX_SCHEDULED_TASKS: usize = 50;
pub const SESSION_TASK_EXPIRY_HOURS: i64 = 72;
pub const DISABLE_CRON_ENV: &str = "VTCODE_DISABLE_CRON";
pub const DURABLE_SCHEDULER_RUNTIME_HINT: &str = "Durable tasks fire while VT Code is open, `vtcode schedule serve` is running, or the installed scheduler service is active.";

const SESSION_JITTER_CAP_SECS: u64 = 15 * 60;
const ONE_SHOT_TOP_OF_HOUR_JITTER_SECS: u64 = 90;
const CLAIM_STALE_SECS: u64 = 15 * 60;
const SERVICE_NAME: &str = "vtcode-scheduler";
const LAUNCHD_LABEL: &str = "com.vtcode.scheduler";
const DURABLE_STORE_DIR: &str = "scheduled_tasks";

static NEXT_TASK_COUNTER: AtomicU64 = AtomicU64::new(1);

#[cfg(test)]
mod test_env_overrides {
    use std::sync::{LazyLock, Mutex};

    static DISABLE_CRON: LazyLock<Mutex<Option<String>>> = LazyLock::new(|| Mutex::new(None));

    pub(super) fn get() -> Option<String> {
        DISABLE_CRON.lock().ok().and_then(|value| value.clone())
    }

    pub(super) fn set(value: Option<&str>) {
        if let Ok(mut slot) = DISABLE_CRON.lock() {
            *slot = value.map(ToString::to_string);
        }
    }
}

static LEADING_INTERVAL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?ix)
        ^\s*
        (?P<count>\d+)
        \s*
        (?P<unit>s|sec|secs|second|seconds|m|min|mins|minute|minutes|h|hr|hrs|hour|hours|d|day|days)
        \s+
        (?P<prompt>.+)
        $",
    )
    .expect("leading interval regex")
});

static TRAILING_INTERVAL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?ix)
        ^\s*
        (?P<prompt>.+?)
        \s+
        every
        \s+
        (?P<count>\d+)
        \s*
        (?P<unit>s|sec|secs|second|seconds|m|min|mins|minute|minutes|h|hr|hrs|hour|hours|d|day|days)
        \s*
        $",
    )
    .expect("trailing interval regex")
});

static REMIND_AT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?ix)^\s*remind\s+me\s+at\s+(?P<when>.+?)\s+to\s+(?P<prompt>.+)\s*$")
        .expect("remind-at regex")
});

static REMIND_IN_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?ix)
        ^\s*in\s+
        (?P<count>\d+)
        \s*
        (?P<unit>minutes|minute|hours|hour|days|day)
        \s*,?\s*
        (?P<prompt>.+)
        \s*$
    ",
    )
    .expect("remind-in regex")
});

static TIME_ONLY_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?ix)^\s*(?P<hour>\d{1,2})(?::(?P<minute>\d{2}))?\s*(?P<ampm>am|pm)?\s*$")
        .expect("time-only regex")
});

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ScheduledTaskAction {
    Prompt { prompt: String },
    Reminder { message: String },
}

impl ScheduledTaskAction {
    #[must_use]
    pub fn summary(&self) -> &str {
        match self {
            Self::Prompt { prompt } => prompt,
            Self::Reminder { message } => message,
        }
    }

    #[must_use]
    pub fn kind_label(&self) -> &'static str {
        match self {
            Self::Prompt { .. } => "prompt",
            Self::Reminder { .. } => "reminder",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ScheduleSpec {
    Cron5(Cron5),
    FixedInterval(FixedInterval),
    OneShot(OneShot),
}

impl ScheduleSpec {
    pub fn cron5(expression: impl Into<String>) -> Result<Self> {
        Ok(Self::Cron5(Cron5::parse(expression)?))
    }

    pub fn fixed_interval(duration: Duration) -> Result<Self> {
        Ok(Self::FixedInterval(FixedInterval::from_duration(duration)?))
    }

    pub fn one_shot(at: DateTime<Utc>) -> Self {
        Self::OneShot(OneShot { at })
    }

    #[must_use]
    pub fn is_recurring(&self) -> bool {
        !matches!(self, Self::OneShot(_))
    }

    #[must_use]
    pub fn human_description(&self) -> String {
        match self {
            Self::Cron5(spec) => format!("cron {}", spec.expression),
            Self::FixedInterval(spec) => spec.human_description(),
            Self::OneShot(spec) => format!(
                "once at {}",
                spec.at.with_timezone(&Local).format("%Y-%m-%d %H:%M")
            ),
        }
    }

    pub fn first_base_fire_at(&self, created_at: DateTime<Utc>) -> Result<Option<DateTime<Utc>>> {
        let created_local = created_at.with_timezone(&Local);
        let base_local = match self {
            Self::Cron5(spec) => spec.next_after(created_local)?,
            Self::FixedInterval(spec) => Some(created_local + spec.chrono_duration()?),
            Self::OneShot(spec) => Some(spec.at.with_timezone(&Local)),
        };
        Ok(base_local.map(|value| value.with_timezone(&Utc)))
    }

    pub fn next_base_fire_after(
        &self,
        last_base_fire_at: DateTime<Utc>,
    ) -> Result<Option<DateTime<Utc>>> {
        let last_base_local = last_base_fire_at.with_timezone(&Local);
        let next_local = match self {
            Self::Cron5(spec) => spec.next_after(last_base_local)?,
            Self::FixedInterval(spec) => Some(last_base_local + spec.chrono_duration()?),
            Self::OneShot(_) => None,
        };
        Ok(next_local.map(|value| value.with_timezone(&Utc)))
    }

    fn jittered_fire_at(&self, id: &str, base_fire_at: DateTime<Utc>) -> Result<DateTime<Utc>> {
        let base_local = base_fire_at.with_timezone(&Local);
        let hash = stable_hash_u64(id.as_bytes());
        match self {
            Self::Cron5(_) | Self::FixedInterval(_) => {
                let Some(period) = self.nominal_period()? else {
                    return Ok(base_fire_at);
                };
                let period_secs = period.num_seconds().max(0) as u64;
                let max_delay = ((period_secs as f64) * 0.10).floor() as u64;
                let max_delay = max_delay.min(SESSION_JITTER_CAP_SECS);
                let delay_secs = if max_delay == 0 {
                    0
                } else {
                    hash % (max_delay + 1)
                };
                Ok(base_fire_at + ChronoDuration::seconds(delay_secs as i64))
            }
            Self::OneShot(_) if matches!(base_local.minute(), 0 | 30) => {
                let lead_secs = hash % (ONE_SHOT_TOP_OF_HOUR_JITTER_SECS + 1);
                Ok(base_fire_at - ChronoDuration::seconds(lead_secs as i64))
            }
            Self::OneShot(_) => Ok(base_fire_at),
        }
    }

    fn nominal_period(&self) -> Result<Option<ChronoDuration>> {
        match self {
            Self::FixedInterval(spec) => Ok(Some(spec.chrono_duration()?)),
            Self::Cron5(spec) => spec.approx_period(),
            Self::OneShot(_) => Ok(None),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Cron5 {
    pub expression: String,
}

impl Cron5 {
    pub fn parse(expression: impl Into<String>) -> Result<Self> {
        let expression = expression.into();
        ParsedCron::parse(&expression)?;
        Ok(Self { expression })
    }

    fn parsed(&self) -> Result<ParsedCron> {
        ParsedCron::parse(&self.expression)
    }

    fn next_after(&self, after: DateTime<Local>) -> Result<Option<DateTime<Local>>> {
        self.parsed()?.next_after(after)
    }

    fn approx_period(&self) -> Result<Option<ChronoDuration>> {
        let now = Local::now();
        let Some(first) = self.next_after(now)? else {
            return Ok(None);
        };
        let Some(second) = self.next_after(first)? else {
            return Ok(None);
        };
        Ok(Some(second - first))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FixedInterval {
    pub seconds: u64,
}

impl FixedInterval {
    pub fn from_duration(duration: Duration) -> Result<Self> {
        if duration.as_secs() < 60 {
            bail!("Fixed intervals must be at least 1 minute");
        }
        Ok(Self {
            seconds: duration.as_secs(),
        })
    }

    pub fn chrono_duration(&self) -> Result<ChronoDuration> {
        let seconds = i64::try_from(self.seconds).context("interval is too large")?;
        Ok(ChronoDuration::seconds(seconds))
    }

    #[must_use]
    pub fn human_description(&self) -> String {
        humanize_interval(self.seconds)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OneShot {
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScheduledTaskDefinition {
    pub id: String,
    pub name: String,
    pub schedule: ScheduleSpec,
    pub action: ScheduledTaskAction,
    pub workspace: Option<PathBuf>,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
}

impl ScheduledTaskDefinition {
    pub fn new(
        name: Option<String>,
        schedule: ScheduleSpec,
        action: ScheduledTaskAction,
        workspace: Option<PathBuf>,
        created_at: DateTime<Utc>,
        expires_at: Option<DateTime<Utc>>,
    ) -> Result<Self> {
        let rendered_name = name
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| summarize_task_name(action.summary()));
        let id = generate_task_id(&rendered_name, action.summary(), created_at);
        Ok(Self {
            id,
            name: rendered_name,
            schedule,
            action,
            workspace,
            created_at,
            expires_at,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum TaskRunStatus {
    Triggered,
    ReminderSent,
    Success,
    Failed { message: String },
}

impl fmt::Display for TaskRunStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Triggered => write!(f, "triggered"),
            Self::ReminderSent => write!(f, "reminder_sent"),
            Self::Success => write!(f, "success"),
            Self::Failed { message } => write!(f, "failed: {message}"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ScheduledTaskRuntimeState {
    pub last_run_at: Option<DateTime<Utc>>,
    pub next_base_run_at: Option<DateTime<Utc>>,
    pub next_run_at: Option<DateTime<Utc>>,
    pub last_status: Option<TaskRunStatus>,
    pub last_artifact_dir: Option<PathBuf>,
    pub last_events_file: Option<PathBuf>,
    pub last_message_file: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduledTaskRecord {
    pub definition: ScheduledTaskDefinition,
    pub runtime: ScheduledTaskRuntimeState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScheduledTaskSummary {
    pub id: String,
    pub name: String,
    pub action_kind: String,
    pub schedule: String,
    pub workspace: Option<PathBuf>,
    pub recurring: bool,
    pub next_run_at: Option<DateTime<Utc>>,
    pub last_run_at: Option<DateTime<Utc>>,
    pub last_status: Option<String>,
}

impl ScheduledTaskRecord {
    fn summary(&self) -> ScheduledTaskSummary {
        ScheduledTaskSummary {
            id: self.definition.id.clone(),
            name: self.definition.name.clone(),
            action_kind: self.definition.action.kind_label().to_string(),
            schedule: self.definition.schedule.human_description(),
            workspace: self.definition.workspace.clone(),
            recurring: self.definition.schedule.is_recurring(),
            next_run_at: self.runtime.next_run_at,
            last_run_at: self.runtime.last_run_at,
            last_status: self.runtime.last_status.as_ref().map(ToString::to_string),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DueSessionPrompt {
    pub id: String,
    pub name: String,
    pub prompt: String,
}

#[derive(Debug, Default, Clone)]
pub struct SessionScheduler {
    tasks: BTreeMap<String, ScheduledTaskRecord>,
}

impl SessionScheduler {
    #[must_use]
    pub fn new() -> Self {
        Self {
            tasks: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.tasks.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }

    pub fn create_prompt_task(
        &mut self,
        name: Option<String>,
        prompt: String,
        schedule: ScheduleSpec,
        created_at: DateTime<Utc>,
    ) -> Result<ScheduledTaskSummary> {
        self.ensure_capacity()?;
        let expires_at = schedule
            .is_recurring()
            .then(|| created_at + ChronoDuration::hours(SESSION_TASK_EXPIRY_HOURS));
        let definition = ScheduledTaskDefinition::new(
            name,
            schedule,
            ScheduledTaskAction::Prompt { prompt },
            None,
            created_at,
            expires_at,
        )?;
        let runtime = initialize_runtime_state(&definition)?;
        let record = ScheduledTaskRecord {
            definition: definition.clone(),
            runtime,
        };
        let summary = record.summary();
        self.tasks.insert(definition.id.clone(), record);
        Ok(summary)
    }

    pub fn list(&self) -> Vec<ScheduledTaskSummary> {
        self.tasks
            .values()
            .map(ScheduledTaskRecord::summary)
            .collect()
    }

    pub fn delete(&mut self, query: &str) -> Option<ScheduledTaskSummary> {
        let query = query.trim();
        if query.is_empty() {
            return None;
        }
        if let Some(record) = self.tasks.remove(query) {
            return Some(record.summary());
        }
        let key = self.tasks.iter().find_map(|(id, record)| {
            record
                .definition
                .name
                .eq_ignore_ascii_case(query)
                .then(|| id.clone())
        })?;
        self.tasks.remove(&key).map(|record| record.summary())
    }

    pub fn collect_due_prompts(&mut self, now: DateTime<Utc>) -> Result<Vec<DueSessionPrompt>> {
        let mut due = Vec::new();
        let mut completed = Vec::new();
        for record in self.tasks.values_mut() {
            let Some(next_run_at) = record.runtime.next_run_at else {
                continue;
            };
            if now < next_run_at {
                continue;
            }

            if let ScheduledTaskAction::Prompt { prompt } = &record.definition.action {
                due.push(DueSessionPrompt {
                    id: record.definition.id.clone(),
                    name: record.definition.name.clone(),
                    prompt: prompt.clone(),
                });
            }

            record.runtime.last_run_at = Some(now);
            record.runtime.last_status = Some(TaskRunStatus::Triggered);

            let Some(last_base_run_at) = record.runtime.next_base_run_at else {
                record.runtime.next_run_at = None;
                completed.push(record.definition.id.clone());
                continue;
            };

            let next_base_run_at = record
                .definition
                .schedule
                .next_base_fire_after(last_base_run_at)?;
            let should_remove = next_base_run_at.is_none_or(|next| {
                record
                    .definition
                    .expires_at
                    .is_some_and(|expiry| next > expiry)
            });
            if should_remove {
                record.runtime.next_base_run_at = None;
                record.runtime.next_run_at = None;
                completed.push(record.definition.id.clone());
                continue;
            }

            let next_base_run_at = next_base_run_at.expect("checked above");
            record.runtime.next_base_run_at = Some(next_base_run_at);
            record.runtime.next_run_at = Some(
                record
                    .definition
                    .schedule
                    .jittered_fire_at(&record.definition.id, next_base_run_at)?,
            );
        }

        for id in completed {
            self.tasks.remove(&id);
        }

        Ok(due)
    }

    fn ensure_capacity(&self) -> Result<()> {
        if self.tasks.len() >= MAX_SCHEDULED_TASKS {
            bail!(
                "A session can hold at most {} scheduled tasks",
                MAX_SCHEDULED_TASKS
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopCommand {
    pub prompt: String,
    pub interval: FixedInterval,
    pub normalization_note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionLanguageCommand {
    CreateOneShotPrompt {
        prompt: String,
        run_at: DateTime<Utc>,
    },
    ListTasks,
    CancelTask {
        query: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduleCreateInput {
    pub name: Option<String>,
    pub prompt: Option<String>,
    pub reminder: Option<String>,
    pub every: Option<String>,
    pub cron: Option<String>,
    pub at: Option<String>,
    pub workspace: Option<PathBuf>,
}

impl ScheduleCreateInput {
    pub fn build_definition(
        self,
        now: DateTime<Local>,
        default_workspace: Option<PathBuf>,
    ) -> Result<ScheduledTaskDefinition> {
        let action = match (self.prompt, self.reminder) {
            (Some(prompt), None) => ScheduledTaskAction::Prompt { prompt },
            (None, Some(reminder)) => ScheduledTaskAction::Reminder { message: reminder },
            (Some(_), Some(_)) => bail!("Choose either --prompt or --reminder"),
            (None, None) => bail!("One of --prompt or --reminder is required"),
        };

        let schedule = match (self.every, self.cron, self.at) {
            (Some(raw), None, None) => {
                let duration = parse_human_duration(raw.trim())
                    .with_context(|| format!("Invalid --every duration: {}", raw.trim()))?;
                ScheduleSpec::fixed_interval(duration)?
            }
            (None, Some(expression), None) => ScheduleSpec::cron5(expression)?,
            (None, None, Some(raw)) => {
                ScheduleSpec::one_shot(parse_local_datetime(raw.trim(), now)?)
            }
            _ => bail!("Choose exactly one of --every, --cron, or --at"),
        };

        let workspace = match (&action, self.workspace.or(default_workspace)) {
            (ScheduledTaskAction::Prompt { .. }, Some(path)) => Some(path),
            (ScheduledTaskAction::Prompt { .. }, None) => {
                bail!("Prompt tasks require a workspace (pass --workspace or create from chat)")
            }
            (ScheduledTaskAction::Reminder { .. }, path) => path,
        }
        .map(|path| resolve_scheduled_workspace_path(&path))
        .transpose()?;

        if matches!(action, ScheduledTaskAction::Prompt { .. })
            && workspace.as_ref().is_some_and(|path| !path.is_dir())
        {
            let workspace = workspace.as_ref().expect("prompt workspace should exist");
            bail!(
                "Prompt task workspace does not exist or is not a directory: {}",
                workspace.display()
            );
        }

        ScheduledTaskDefinition::new(
            self.name,
            schedule,
            action,
            workspace,
            now.with_timezone(&Utc),
            None,
        )
    }
}

fn resolve_scheduled_workspace_path(path: &Path) -> Result<PathBuf> {
    resolve_scheduled_workspace_path_with_home(path, dirs::home_dir().as_deref())
}

fn resolve_scheduled_workspace_path_with_home(
    path: &Path,
    home_dir: Option<&Path>,
) -> Result<PathBuf> {
    let expanded = expand_scheduled_workspace_home(path, home_dir);
    let absolute = if expanded.is_absolute() {
        expanded
    } else {
        std::env::current_dir()
            .context("Failed to resolve current directory for scheduled task workspace")?
            .join(expanded)
    };
    Ok(normalize_path(&absolute))
}

fn expand_scheduled_workspace_home(path: &Path, home_dir: Option<&Path>) -> PathBuf {
    let Some(raw) = path.to_str() else {
        return path.to_path_buf();
    };

    if raw == "~" {
        return home_dir
            .map(Path::to_path_buf)
            .unwrap_or_else(|| path.to_path_buf());
    }

    if let Some(rest) = raw.strip_prefix("~/")
        && let Some(home_dir) = home_dir
    {
        return home_dir.join(rest);
    }

    path.to_path_buf()
}

#[derive(Debug, Clone)]
pub struct SchedulerPaths {
    pub config_root: PathBuf,
    pub data_root: PathBuf,
}

impl SchedulerPaths {
    pub fn new_default() -> Result<Self> {
        let config_root = get_config_dir()
            .ok_or_else(|| anyhow!("Failed to resolve VT Code config directory"))?
            .join(DURABLE_STORE_DIR);
        let data_root = get_data_dir()
            .ok_or_else(|| anyhow!("Failed to resolve VT Code data directory"))?
            .join(DURABLE_STORE_DIR);
        Ok(Self {
            config_root,
            data_root,
        })
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
            fs::create_dir_all(dir).with_context(|| {
                format!("Failed to create scheduler directory {}", dir.display())
            })?;
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
        self.runs_dir()
            .join(id)
            .join(run_at.format("%Y%m%dT%H%M%SZ").to_string())
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
        Ok(Self {
            paths: SchedulerPaths::new_default()?,
        })
    }

    #[must_use]
    pub fn paths(&self) -> &SchedulerPaths {
        &self.paths
    }

    pub fn create(&self, definition: ScheduledTaskDefinition) -> Result<ScheduledTaskSummary> {
        self.paths.ensure_dirs()?;
        let current_count = self.definition_paths()?.len();
        if current_count >= MAX_SCHEDULED_TASKS {
            bail!(
                "VT Code supports at most {} durable scheduled tasks",
                MAX_SCHEDULED_TASKS
            );
        }

        let runtime = initialize_runtime_state(&definition)?;
        self.write_definition(&definition)?;
        self.write_runtime(&definition.id, &runtime)?;
        Ok(ScheduledTaskRecord {
            definition,
            runtime,
        }
        .summary())
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
        Ok(Some(ScheduledTaskRecord {
            definition,
            runtime,
        }))
    }

    pub fn update_runtime(&self, record: &ScheduledTaskRecord) -> Result<()> {
        self.write_runtime(&record.definition.id, &record.runtime)
    }

    fn load_records(&self) -> Result<Vec<ScheduledTaskRecord>> {
        self.paths.ensure_dirs()?;
        let mut records = Vec::new();
        for definition_path in self.definition_paths()? {
            let definition = read_definition(&definition_path)?;
            let runtime = self
                .read_runtime(&definition.id)?
                .unwrap_or(initialize_runtime_state(&definition)?);
            records.push(ScheduledTaskRecord {
                definition,
                runtime,
            });
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
        let serialized =
            toml::to_string_pretty(definition).context("Failed to serialize task definition")?;
        atomic_write(
            &self.paths.definition_path(&definition.id),
            serialized.as_bytes(),
        )
    }

    fn read_runtime(&self, id: &str) -> Result<Option<ScheduledTaskRuntimeState>> {
        let path = self.paths.runtime_path(id);
        if !path.exists() {
            return Ok(None);
        }
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        let runtime = serde_json::from_str(&raw)
            .with_context(|| format!("Failed to parse {}", path.display()))?;
        Ok(Some(runtime))
    }

    fn write_runtime(&self, id: &str, runtime: &ScheduledTaskRuntimeState) -> Result<()> {
        let serialized =
            serde_json::to_vec_pretty(runtime).context("Failed to serialize runtime state")?;
        atomic_write(&self.paths.runtime_path(id), &serialized)
    }
}

#[derive(Debug, Clone)]
pub struct SchedulerDaemon {
    store: DurableTaskStore,
    executable_path: PathBuf,
}

impl SchedulerDaemon {
    #[must_use]
    pub fn new(store: DurableTaskStore, executable_path: PathBuf) -> Self {
        Self {
            store,
            executable_path,
        }
    }

    pub async fn serve_forever(&self) -> Result<()> {
        loop {
            self.run_due_tasks_once().await?;
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }

    pub async fn run_due_tasks_once(&self) -> Result<usize> {
        let now = Utc::now();
        let mut records = self.store.load_records()?;
        let mut executed = 0usize;

        records.sort_by_key(|record| record.runtime.next_run_at);
        for mut record in records {
            let Some(next_run_at) = record.runtime.next_run_at else {
                continue;
            };
            if now < next_run_at {
                continue;
            }
            if !try_acquire_claim(self.store.paths(), &record.definition.id)? {
                continue;
            }

            let result = self.execute_record(&record, now).await;
            let release_result = release_claim(self.store.paths(), &record.definition.id);
            let run_outcome = result?;
            release_result?;

            apply_run_outcome(&mut record, run_outcome)?;
            self.store.update_runtime(&record)?;
            executed = executed.saturating_add(1);
        }

        Ok(executed)
    }

    async fn execute_record(
        &self,
        record: &ScheduledTaskRecord,
        run_at: DateTime<Utc>,
    ) -> Result<RunOutcome> {
        match &record.definition.action {
            ScheduledTaskAction::Reminder { message } => {
                let notification = send_global_notification(NotificationEvent::IdlePrompt {
                    title: format!("Scheduled reminder: {}", record.definition.name),
                    message: message.clone(),
                })
                .await;
                let status = match notification {
                    Ok(()) => TaskRunStatus::ReminderSent,
                    Err(error) => TaskRunStatus::Failed {
                        message: format!("failed to send reminder notification: {error:#}"),
                    },
                };
                Ok(RunOutcome {
                    ran_at: run_at,
                    status,
                    artifact_dir: None,
                    events_file: None,
                    last_message_file: None,
                })
            }
            ScheduledTaskAction::Prompt { prompt } => {
                let artifact_dir = self
                    .store
                    .paths()
                    .artifact_dir(&record.definition.id, run_at);
                let events_file = artifact_dir.join("events.jsonl");
                let last_message_file = artifact_dir.join("last-message.txt");
                let workspace = record
                    .definition
                    .workspace
                    .as_deref()
                    .map(resolve_scheduled_workspace_path)
                    .transpose()
                    .with_context(|| {
                        let workspace = record
                            .definition
                            .workspace
                            .as_ref()
                            .map(|path| path.display().to_string())
                            .unwrap_or_else(|| "<none>".to_string());
                        format!(
                            "Failed to resolve scheduled task workspace {} for task {}",
                            workspace, record.definition.id
                        )
                    });
                let execution = async {
                    fs::create_dir_all(&artifact_dir).with_context(|| {
                        format!(
                            "Failed to create run artifact dir {}",
                            artifact_dir.display()
                        )
                    })?;

                    let mut command = Command::new(&self.executable_path);
                    command
                        .arg("exec")
                        .arg("--events")
                        .arg(&events_file)
                        .arg("--last-message-file")
                        .arg(&last_message_file)
                        .arg(prompt)
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null());

                    if let Some(workspace) = workspace? {
                        command.current_dir(&workspace);
                    }

                    command.status().await.with_context(|| {
                        let workspace = record
                            .definition
                            .workspace
                            .as_ref()
                            .map(|path| path.display().to_string())
                            .unwrap_or_else(|| "<none>".to_string());
                        format!(
                            "Failed to spawn scheduled VT Code exec for task {} using {} in {}",
                            record.definition.id,
                            self.executable_path.display(),
                            workspace
                        )
                    })
                }
                .await;
                let run_status = match execution {
                    Ok(status) if status.success() => TaskRunStatus::Success,
                    Ok(status) => TaskRunStatus::Failed {
                        message: format!("vtcode exec exited with {}", status),
                    },
                    Err(error) => TaskRunStatus::Failed {
                        message: format!("{error:#}"),
                    },
                };
                Ok(RunOutcome {
                    ran_at: run_at,
                    status: run_status,
                    artifact_dir: artifact_dir.is_dir().then_some(artifact_dir),
                    events_file: events_file.is_file().then_some(events_file),
                    last_message_file: last_message_file.is_file().then_some(last_message_file),
                })
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceManager {
    Launchd,
    SystemdUser,
}

impl ServiceManager {
    #[must_use]
    pub fn current() -> Option<Self> {
        #[cfg(target_os = "macos")]
        {
            return Some(Self::Launchd);
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            return Some(Self::SystemdUser);
        }
        #[allow(unreachable_code)]
        None
    }
}

#[derive(Debug, Clone)]
pub struct ServiceInstallPlan {
    pub manager: ServiceManager,
    pub path: PathBuf,
    pub contents: String,
}

pub fn render_service_install_plan(executable_path: &Path) -> Result<ServiceInstallPlan> {
    let manager = ServiceManager::current()
        .ok_or_else(|| anyhow!("Durable scheduler services are unsupported on this platform"))?;
    let path = match manager {
        ServiceManager::Launchd => dirs::home_dir()
            .ok_or_else(|| anyhow!("Failed to resolve home directory"))?
            .join("Library/LaunchAgents")
            .join(format!("{LAUNCHD_LABEL}.plist")),
        ServiceManager::SystemdUser => dirs::home_dir()
            .ok_or_else(|| anyhow!("Failed to resolve home directory"))?
            .join(".config/systemd/user")
            .join(format!("{SERVICE_NAME}.service")),
    };
    let contents = match manager {
        ServiceManager::Launchd => render_launchd_plist(executable_path),
        ServiceManager::SystemdUser => render_systemd_unit(executable_path),
    };
    Ok(ServiceInstallPlan {
        manager,
        path,
        contents,
    })
}

pub fn install_service_file(executable_path: &Path) -> Result<ServiceInstallPlan> {
    let plan = render_service_install_plan(executable_path)?;
    if let Some(parent) = plan.path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    atomic_write(&plan.path, plan.contents.as_bytes())?;
    Ok(plan)
}

pub fn uninstall_service_file() -> Result<Option<(ServiceManager, PathBuf, bool)>> {
    let Some(manager) = ServiceManager::current() else {
        return Ok(None);
    };
    let path = render_service_install_plan(Path::new("/tmp/vtcode"))?.path;
    if path.exists() {
        fs::remove_file(&path).with_context(|| format!("Failed to remove {}", path.display()))?;
        return Ok(Some((manager, path, true)));
    }
    Ok(Some((manager, path, false)))
}

#[must_use]
pub fn render_launchd_plist(executable_path: &Path) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
  <dict>
    <key>Label</key>
    <string>{LAUNCHD_LABEL}</string>
    <key>ProgramArguments</key>
    <array>
      <string>{}</string>
      <string>schedule</string>
      <string>serve</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
  </dict>
</plist>
"#,
        xml_escape(executable_path.display().to_string())
    )
}

#[must_use]
pub fn render_systemd_unit(executable_path: &Path) -> String {
    format!(
        "[Unit]\nDescription=VT Code scheduler\n\n[Service]\nType=simple\nExecStart={} schedule serve\nRestart=always\nRestartSec=5\n\n[Install]\nWantedBy=default.target\n",
        shell_words::quote(executable_path.to_string_lossy().as_ref())
    )
}

pub fn scheduled_tasks_enabled(enabled_in_config: bool) -> bool {
    #[cfg(test)]
    if let Some(value) = test_env_overrides::get() {
        let normalized = value.trim().to_ascii_lowercase();
        if matches!(normalized.as_str(), "1" | "true" | "yes" | "on") {
            return false;
        }
    }

    if let Ok(value) = std::env::var(DISABLE_CRON_ENV) {
        let normalized = value.trim().to_ascii_lowercase();
        if matches!(normalized.as_str(), "1" | "true" | "yes" | "on") {
            return false;
        }
    }
    enabled_in_config
}

#[must_use]
pub fn durable_task_is_overdue(
    next_run_at: Option<DateTime<Utc>>,
    last_run_at: Option<DateTime<Utc>>,
    has_last_status: bool,
    now: DateTime<Utc>,
) -> bool {
    !has_last_status && last_run_at.is_none() && next_run_at.is_some_and(|next_run| next_run <= now)
}

pub fn parse_loop_command(args: &str) -> Result<LoopCommand> {
    let trimmed = args.trim();
    if trimmed.is_empty() {
        bail!("Usage: /loop [interval] <prompt>");
    }

    let (raw_prompt, count, unit) = if let Some(captures) = LEADING_INTERVAL_RE.captures(trimmed) {
        (
            captures["prompt"].trim().to_string(),
            captures["count"].parse::<u64>()?,
            captures["unit"].to_string(),
        )
    } else if let Some(captures) = TRAILING_INTERVAL_RE.captures(trimmed) {
        (
            captures["prompt"].trim().to_string(),
            captures["count"].parse::<u64>()?,
            captures["unit"].to_string(),
        )
    } else {
        (
            trimmed.to_string(),
            DEFAULT_LOOP_INTERVAL_MINUTES,
            "minutes".to_string(),
        )
    };

    if raw_prompt.trim().is_empty() {
        bail!("Usage: /loop [interval] <prompt>");
    }

    let (seconds, note) = normalize_interval_spec(count, &unit)?;
    Ok(LoopCommand {
        prompt: raw_prompt,
        interval: FixedInterval { seconds },
        normalization_note: note,
    })
}

pub fn parse_session_language_command(
    input: &str,
    now: DateTime<Local>,
) -> Option<Result<SessionLanguageCommand>> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }

    let normalized = trimmed
        .trim_end_matches(['?', '.', '!'])
        .to_ascii_lowercase();
    if normalized == "what scheduled tasks do i have" {
        return Some(Ok(SessionLanguageCommand::ListTasks));
    }

    if let Some(captures) = REMIND_AT_RE.captures(trimmed) {
        let when = captures
            .name("when")
            .map(|value| value.as_str())
            .unwrap_or_default();
        let prompt = captures
            .name("prompt")
            .map(|value| value.as_str().trim().to_string())
            .unwrap_or_default();
        return Some(
            parse_local_datetime(when, now)
                .map(|run_at| SessionLanguageCommand::CreateOneShotPrompt { prompt, run_at }),
        );
    }

    if let Some(captures) = REMIND_IN_RE.captures(trimmed) {
        let count = captures["count"].parse::<i64>().ok()?;
        let prompt = captures["prompt"].trim().to_string();
        let delta = match captures["unit"].to_ascii_lowercase().as_str() {
            "minute" | "minutes" => ChronoDuration::minutes(count),
            "hour" | "hours" => ChronoDuration::hours(count),
            "day" | "days" => ChronoDuration::days(count),
            _ => return None,
        };
        return Some(Ok(SessionLanguageCommand::CreateOneShotPrompt {
            prompt,
            run_at: (now + delta).with_timezone(&Utc),
        }));
    }

    if let Some(query) = trimmed.strip_prefix("cancel ") {
        return Some(Ok(SessionLanguageCommand::CancelTask {
            query: query.trim().to_string(),
        }));
    }

    None
}

pub fn parse_schedule_create_args(args: &str) -> Result<ScheduleCreateInput> {
    let tokens =
        shell_words::split(args).with_context(|| format!("Failed to parse arguments: {args}"))?;
    parse_schedule_create_tokens(&tokens)
}

pub fn parse_schedule_create_tokens(tokens: &[String]) -> Result<ScheduleCreateInput> {
    let mut name = None;
    let mut prompt = None;
    let mut reminder = None;
    let mut every = None;
    let mut cron = None;
    let mut at = None;
    let mut workspace = None;
    let mut index = 0usize;

    while index < tokens.len() {
        let token = &tokens[index];
        let (flag, inline_value) = if let Some((left, right)) = token.split_once('=') {
            (left, Some(right.to_string()))
        } else {
            (token.as_str(), None)
        };

        let take_value = |idx: &mut usize| -> Result<String> {
            if let Some(value) = inline_value.clone() {
                return Ok(value);
            }
            let Some(value) = tokens.get(*idx + 1) else {
                bail!("Missing value for {flag}");
            };
            *idx += 1;
            Ok(value.clone())
        };

        match flag {
            "--name" => name = Some(take_value(&mut index)?),
            "--prompt" => prompt = Some(take_value(&mut index)?),
            "--reminder" => reminder = Some(take_value(&mut index)?),
            "--every" => every = Some(take_value(&mut index)?),
            "--cron" => cron = Some(take_value(&mut index)?),
            "--at" => at = Some(take_value(&mut index)?),
            "--workspace" => workspace = Some(PathBuf::from(take_value(&mut index)?)),
            "--help" | "help" => {
                bail!(
                    "Usage: /schedule create --prompt <text>|--reminder <text> --every <dur>|--cron <expr>|--at <time> [--name <label>] [--workspace <path>]"
                );
            }
            _ => bail!("Unknown option: {token}"),
        }
        index += 1;
    }

    Ok(ScheduleCreateInput {
        name,
        prompt,
        reminder,
        every,
        cron,
        at,
        workspace,
    })
}

fn initialize_runtime_state(
    definition: &ScheduledTaskDefinition,
) -> Result<ScheduledTaskRuntimeState> {
    let next_base_run_at = definition
        .schedule
        .first_base_fire_at(definition.created_at)?;
    let next_run_at = next_base_run_at
        .map(|base| definition.schedule.jittered_fire_at(&definition.id, base))
        .transpose()?;
    Ok(ScheduledTaskRuntimeState {
        next_base_run_at,
        next_run_at,
        ..ScheduledTaskRuntimeState::default()
    })
}

fn apply_run_outcome(record: &mut ScheduledTaskRecord, outcome: RunOutcome) -> Result<()> {
    record.runtime.last_run_at = Some(outcome.ran_at);
    record.runtime.last_status = Some(outcome.status);
    record.runtime.last_artifact_dir = outcome.artifact_dir;
    record.runtime.last_events_file = outcome.events_file;
    record.runtime.last_message_file = outcome.last_message_file;

    let Some(last_base_run_at) = record.runtime.next_base_run_at else {
        record.runtime.next_run_at = None;
        return Ok(());
    };

    let next_base_run_at = record
        .definition
        .schedule
        .next_base_fire_after(last_base_run_at)?;
    if let Some(next_base_run_at) = next_base_run_at {
        record.runtime.next_base_run_at = Some(next_base_run_at);
        record.runtime.next_run_at = Some(
            record
                .definition
                .schedule
                .jittered_fire_at(&record.definition.id, next_base_run_at)?,
        );
    } else {
        record.runtime.next_base_run_at = None;
        record.runtime.next_run_at = None;
    }

    Ok(())
}

#[derive(Debug, Clone)]
struct RunOutcome {
    ran_at: DateTime<Utc>,
    status: TaskRunStatus,
    artifact_dir: Option<PathBuf>,
    events_file: Option<PathBuf>,
    last_message_file: Option<PathBuf>,
}

fn read_definition(path: &Path) -> Result<ScheduledTaskDefinition> {
    let raw =
        fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;
    toml::from_str(&raw).with_context(|| format!("Failed to parse {}", path.display()))
}

fn atomic_write(path: &Path, content: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }

    let temp_name = format!(
        ".{}.tmp-{}",
        path.file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("task"),
        NEXT_TASK_COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    let temp_path = path.with_file_name(temp_name);
    fs::write(&temp_path, content)
        .with_context(|| format!("Failed to write {}", temp_path.display()))?;
    fs::rename(&temp_path, path)
        .with_context(|| format!("Failed to replace {}", path.display()))?;
    Ok(())
}

fn try_acquire_claim(paths: &SchedulerPaths, id: &str) -> Result<bool> {
    paths.ensure_dirs()?;
    let path = paths.claim_path(id);
    match OpenOptions::new().write(true).create_new(true).open(&path) {
        Ok(mut file) => {
            let timestamp = Utc::now().to_rfc3339();
            file.write_all(timestamp.as_bytes())
                .with_context(|| format!("Failed to write {}", path.display()))?;
            Ok(true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            if claim_is_stale(&path)? {
                let _ = fs::remove_file(&path);
                return try_acquire_claim(paths, id);
            }
            Ok(false)
        }
        Err(error) => Err(error).with_context(|| format!("Failed to create {}", path.display())),
    }
}

fn claim_is_stale(path: &Path) -> Result<bool> {
    let metadata =
        fs::metadata(path).with_context(|| format!("Failed to stat {}", path.display()))?;
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

fn summarize_task_name(summary: &str) -> String {
    let trimmed = summary.trim();
    if trimmed.is_empty() {
        return "Scheduled task".to_string();
    }
    let compact = trimmed.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut output = String::new();
    for ch in compact.chars().take(32) {
        output.push(ch);
    }
    output
}

fn generate_task_id(name: &str, summary: &str, created_at: DateTime<Utc>) -> String {
    let counter = NEXT_TASK_COUNTER.fetch_add(1, Ordering::Relaxed);
    let seed = format!(
        "{name}|{summary}|{}|{}|{}",
        created_at.timestamp_nanos_opt().unwrap_or_default(),
        std::process::id(),
        counter
    );
    format!("{:08x}", stable_hash_u64(seed.as_bytes()) as u32)
}

fn stable_hash_u64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn humanize_interval(seconds: u64) -> String {
    match seconds {
        value if value % 86_400 == 0 => {
            let days = value / 86_400;
            if days == 1 {
                "every 1 day".to_string()
            } else {
                format!("every {days} days")
            }
        }
        value if value % 3_600 == 0 => {
            let hours = value / 3_600;
            if hours == 1 {
                "every 1 hour".to_string()
            } else {
                format!("every {hours} hours")
            }
        }
        value => {
            let minutes = value / 60;
            if minutes == 1 {
                "every 1 minute".to_string()
            } else {
                format!("every {minutes} minutes")
            }
        }
    }
}

fn normalize_interval_spec(count: u64, unit: &str) -> Result<(u64, Option<String>)> {
    if count == 0 {
        bail!("Intervals must be greater than zero");
    }

    let unit = unit.to_ascii_lowercase();
    let (raw_minutes, original) = match unit.as_str() {
        "s" | "sec" | "secs" | "second" | "seconds" => {
            let minutes = count.div_ceil(60);
            (minutes, format!("{count}s"))
        }
        "m" | "min" | "mins" | "minute" | "minutes" => (count, format!("{count}m")),
        "h" | "hr" | "hrs" | "hour" | "hours" => (count * 60, format!("{count}h")),
        "d" | "day" | "days" => (count * 60 * 24, format!("{count}d")),
        _ => bail!("Unsupported interval unit: {unit}"),
    };

    let normalized_minutes = normalize_clean_minutes(raw_minutes);
    let note = if normalized_minutes != raw_minutes {
        Some(format!(
            "Rounded {original} to {} for scheduler cadence.",
            humanize_interval(normalized_minutes * 60)
        ))
    } else if raw_minutes != count && unit.starts_with('s') {
        Some(format!(
            "Rounded {original} up to {} because VT Code schedules at minute granularity.",
            humanize_interval(normalized_minutes * 60)
        ))
    } else {
        None
    };

    Ok((normalized_minutes * 60, note))
}

fn normalize_clean_minutes(raw_minutes: u64) -> u64 {
    const CLEAN_MINUTES: &[u64] = &[
        1, 2, 3, 4, 5, 6, 10, 12, 15, 20, 30, 60, 120, 180, 240, 360, 480, 720, 1_440,
    ];
    if CLEAN_MINUTES.contains(&raw_minutes) {
        return raw_minutes;
    }
    CLEAN_MINUTES
        .iter()
        .copied()
        .min_by_key(|candidate| {
            let distance = candidate.abs_diff(raw_minutes);
            (distance, *candidate < raw_minutes, *candidate)
        })
        .unwrap_or(raw_minutes)
}

pub fn parse_local_datetime(raw: &str, now: DateTime<Local>) -> Result<DateTime<Utc>> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        bail!("Time value cannot be empty");
    }

    if let Ok(parsed) = DateTime::parse_from_rfc3339(trimmed) {
        return Ok(parsed.with_timezone(&Utc));
    }

    for format in ["%Y-%m-%d %H:%M", "%Y-%m-%dT%H:%M", "%Y-%m-%d %H:%M:%S"] {
        if let Ok(naive) = NaiveDateTime::parse_from_str(trimmed, format) {
            return localize_naive_datetime(naive).map(|value| value.with_timezone(&Utc));
        }
    }

    if let Some(captures) = TIME_ONLY_RE.captures(trimmed) {
        let mut hour = captures["hour"].parse::<u32>()?;
        let minute = captures
            .name("minute")
            .map(|value| value.as_str().parse::<u32>())
            .transpose()?
            .unwrap_or(0);
        if minute >= 60 {
            bail!("Invalid minute in time value");
        }
        if let Some(ampm) = captures
            .name("ampm")
            .map(|value| value.as_str().to_ascii_lowercase())
        {
            if hour == 0 || hour > 12 {
                bail!("Invalid 12-hour clock value");
            }
            if ampm == "pm" && hour != 12 {
                hour += 12;
            }
            if ampm == "am" && hour == 12 {
                hour = 0;
            }
        } else if hour >= 24 {
            bail!("Invalid 24-hour clock value");
        }

        let time = NaiveTime::from_hms_opt(hour, minute, 0)
            .ok_or_else(|| anyhow!("Invalid time value"))?;
        let today = now.date_naive();
        let naive = today.and_time(time);
        let mut localized = localize_naive_datetime(naive)?;
        if localized <= now {
            localized = localize_naive_datetime((today + ChronoDuration::days(1)).and_time(time))?;
        }
        return Ok(localized.with_timezone(&Utc));
    }

    bail!("Unsupported time format. Use RFC3339, YYYY-MM-DD HH:MM, or a local time like 3pm")
}

fn localize_naive_datetime(naive: NaiveDateTime) -> Result<DateTime<Local>> {
    match Local.from_local_datetime(&naive) {
        LocalResult::Single(value) => Ok(value),
        LocalResult::Ambiguous(first, _) => Ok(first),
        LocalResult::None => bail!("Local time does not exist due to timezone transition"),
    }
}

fn xml_escape(value: String) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[derive(Debug, Clone)]
struct ParsedCron {
    minute: CronField,
    hour: CronField,
    day_of_month: CronField,
    month: CronField,
    day_of_week: CronField,
}

impl ParsedCron {
    fn parse(expression: &str) -> Result<Self> {
        let parts = expression.split_whitespace().collect::<Vec<_>>();
        if parts.len() != 5 {
            bail!("Cron expressions require exactly 5 fields");
        }

        Ok(Self {
            minute: CronField::parse(parts[0], 0, 59, false)?,
            hour: CronField::parse(parts[1], 0, 23, false)?,
            day_of_month: CronField::parse(parts[2], 1, 31, false)?,
            month: CronField::parse(parts[3], 1, 12, false)?,
            day_of_week: CronField::parse(parts[4], 0, 7, true)?,
        })
    }

    fn next_after(&self, after: DateTime<Local>) -> Result<Option<DateTime<Local>>> {
        let mut candidate = after
            .with_second(0)
            .and_then(|value| value.with_nanosecond(0))
            .ok_or_else(|| anyhow!("Failed to normalize cron timestamp"))?
            + ChronoDuration::minutes(1);
        let horizon = candidate + ChronoDuration::days(366 * 5);

        while candidate <= horizon {
            if self.matches(candidate) {
                return Ok(Some(candidate));
            }
            candidate += ChronoDuration::minutes(1);
        }

        Ok(None)
    }

    fn matches(&self, value: DateTime<Local>) -> bool {
        let month = value.month();
        let dom = value.day();
        let minute = value.minute();
        let hour = value.hour();
        let dow = value.weekday().num_days_from_sunday();

        if !self.minute.contains(minute) || !self.hour.contains(hour) || !self.month.contains(month)
        {
            return false;
        }

        let dom_matches = self.day_of_month.contains(dom);
        let dow_matches = self.day_of_week.contains(dow);

        if self.day_of_month.is_wildcard && self.day_of_week.is_wildcard {
            return true;
        }
        if self.day_of_month.is_wildcard {
            return dow_matches;
        }
        if self.day_of_week.is_wildcard {
            return dom_matches;
        }

        dom_matches || dow_matches
    }
}

#[derive(Debug, Clone)]
struct CronField {
    values: BTreeSet<u32>,
    is_wildcard: bool,
}

impl CronField {
    fn parse(raw: &str, min: u32, max: u32, is_day_of_week: bool) -> Result<Self> {
        if raw.contains(['L', 'W', '?']) {
            bail!("Unsupported cron syntax in field '{raw}'");
        }
        if raw.chars().any(|ch| ch.is_ascii_alphabetic()) {
            bail!("Named cron aliases are not supported in '{raw}'");
        }

        let mut values = BTreeSet::new();
        let mut is_wildcard = false;
        for segment in raw.split(',') {
            let segment = segment.trim();
            if segment.is_empty() {
                bail!("Cron field contains an empty segment");
            }
            if segment == "*" {
                is_wildcard = true;
                values.extend(min..=max);
                continue;
            }

            let (base, step) = if let Some((left, right)) = segment.split_once('/') {
                let step = right
                    .parse::<u32>()
                    .with_context(|| format!("Invalid step value in '{segment}'"))?;
                if step == 0 {
                    bail!("Step value must be greater than zero");
                }
                (left, Some(step))
            } else {
                (segment, None)
            };

            let mut base_values = if base == "*" {
                is_wildcard = true;
                (min..=max).collect::<Vec<_>>()
            } else if let Some((left, right)) = base.split_once('-') {
                let start = parse_cron_number(left, min, max, is_day_of_week)?;
                let end = parse_cron_number(right, min, max, is_day_of_week)?;
                if start > end {
                    bail!("Invalid descending range '{base}'");
                }
                (start..=end).collect::<Vec<_>>()
            } else {
                let start = parse_cron_number(base, min, max, is_day_of_week)?;
                if let Some(step) = step {
                    let mut values = Vec::new();
                    let mut value = start;
                    while value <= max {
                        values.push(value);
                        match value.checked_add(step) {
                            Some(next) => value = next,
                            None => break,
                        }
                    }
                    values
                } else {
                    vec![start]
                }
            };

            if let Some(step) = step
                && (base == "*" || base.contains('-'))
            {
                let mut stepped = Vec::new();
                for (index, value) in base_values.iter().enumerate() {
                    if (index as u32).is_multiple_of(step) {
                        stepped.push(*value);
                    }
                }
                base_values = stepped;
            }

            values.extend(base_values);
        }

        Ok(Self {
            values,
            is_wildcard,
        })
    }

    fn contains(&self, value: u32) -> bool {
        self.values.contains(&value)
    }
}

fn parse_cron_number(raw: &str, min: u32, max: u32, is_day_of_week: bool) -> Result<u32> {
    let mut value = raw
        .parse::<u32>()
        .with_context(|| format!("Invalid cron value '{raw}'"))?;
    if is_day_of_week && value == 7 {
        value = 0;
    }
    if !(min..=max).contains(&value) {
        bail!("Cron value '{raw}' is out of range");
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn utc(y: i32, m: u32, d: u32, hh: u32, mm: u32, ss: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, hh, mm, ss)
            .single()
            .expect("valid timestamp")
    }

    #[test]
    fn loop_defaults_to_ten_minutes() {
        let parsed = parse_loop_command("check the build").expect("loop");
        assert_eq!(parsed.prompt, "check the build");
        assert_eq!(parsed.interval.seconds, 600);
        assert!(parsed.normalization_note.is_none());
    }

    #[test]
    fn loop_parses_leading_interval() {
        let parsed = parse_loop_command("30m check the build").expect("loop");
        assert_eq!(parsed.prompt, "check the build");
        assert_eq!(parsed.interval.seconds, 30 * 60);
    }

    #[test]
    fn loop_parses_trailing_every_clause() {
        let parsed = parse_loop_command("check the build every 2 hours").expect("loop");
        assert_eq!(parsed.prompt, "check the build");
        assert_eq!(parsed.interval.seconds, 2 * 60 * 60);
    }

    #[test]
    fn loop_rounds_seconds_up_to_minutes() {
        let parsed = parse_loop_command("45s check again").expect("loop");
        assert_eq!(parsed.interval.seconds, 60);
        assert!(parsed.normalization_note.is_some());
    }

    #[test]
    fn loop_rounds_unclean_minutes() {
        let parsed = parse_loop_command("7m check again").expect("loop");
        assert_eq!(parsed.interval.seconds, 6 * 60);
        assert!(parsed.normalization_note.is_some());
    }

    #[test]
    fn cron5_supports_vixie_or_semantics() {
        let cron = Cron5::parse("0 9 15 * 1").expect("cron");
        let monday = Local
            .with_ymd_and_hms(2026, 3, 30, 9, 0, 0)
            .single()
            .expect("monday");
        let dom = Local
            .with_ymd_and_hms(2026, 4, 15, 9, 0, 0)
            .single()
            .expect("dom");
        assert!(cron.parsed().expect("parsed").matches(monday));
        assert!(cron.parsed().expect("parsed").matches(dom));
    }

    #[test]
    fn cron5_rejects_extended_syntax() {
        assert!(Cron5::parse("0 9 ? * *").is_err());
        assert!(Cron5::parse("0 9 * JAN *").is_err());
        assert!(Cron5::parse("0 9 * * MON").is_err());
    }

    #[test]
    fn cron5_finds_next_matching_minute() {
        let cron = Cron5::parse("*/15 * * * *").expect("cron");
        let start = Local
            .with_ymd_and_hms(2026, 3, 28, 10, 7, 13)
            .single()
            .expect("start");
        let next = cron.next_after(start).expect("next").expect("some");
        assert_eq!(next.minute(), 15);
    }

    #[test]
    fn reminder_language_detects_at_time() {
        let now = Local
            .with_ymd_and_hms(2026, 3, 28, 13, 0, 0)
            .single()
            .expect("now");
        let command =
            parse_session_language_command("remind me at 3pm to push the release branch", now)
                .expect("command")
                .expect("parsed");
        match command {
            SessionLanguageCommand::CreateOneShotPrompt { prompt, .. } => {
                assert_eq!(prompt, "push the release branch");
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn reminder_language_detects_relative_time() {
        let now = Local
            .with_ymd_and_hms(2026, 3, 28, 13, 0, 0)
            .single()
            .expect("now");
        let command = parse_session_language_command(
            "in 45 minutes, check whether the integration tests passed",
            now,
        )
        .expect("command")
        .expect("parsed");
        match command {
            SessionLanguageCommand::CreateOneShotPrompt { prompt, run_at } => {
                assert_eq!(prompt, "check whether the integration tests passed");
                assert_eq!(
                    run_at,
                    (now + ChronoDuration::minutes(45)).with_timezone(&Utc)
                );
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn session_scheduler_expires_recurring_tasks_after_final_fire() {
        let created_at = utc(2026, 3, 28, 0, 0, 0);
        let mut scheduler = SessionScheduler::new();
        scheduler
            .create_prompt_task(
                Some("heartbeat".to_string()),
                "check".to_string(),
                ScheduleSpec::FixedInterval(FixedInterval { seconds: 60 * 60 }),
                created_at,
            )
            .expect("create");
        let record = scheduler.tasks.values_mut().next().expect("task");
        record.runtime.next_base_run_at = Some(created_at + ChronoDuration::hours(72));
        record.runtime.next_run_at = Some(created_at + ChronoDuration::hours(72));
        let due = scheduler
            .collect_due_prompts(created_at + ChronoDuration::hours(72))
            .expect("collect");
        assert_eq!(due.len(), 1);
        assert!(scheduler.is_empty());
    }

    #[test]
    fn session_scheduler_jitter_is_stable_for_task_id() {
        let definition = ScheduledTaskDefinition {
            id: "abcd1234".to_string(),
            name: "test".to_string(),
            schedule: ScheduleSpec::FixedInterval(FixedInterval { seconds: 600 }),
            action: ScheduledTaskAction::Prompt {
                prompt: "check".to_string(),
            },
            workspace: None,
            created_at: utc(2026, 3, 28, 0, 0, 0),
            expires_at: None,
        };
        let base = utc(2026, 3, 28, 1, 0, 0);
        let first = definition
            .schedule
            .jittered_fire_at(&definition.id, base)
            .expect("jitter");
        let second = definition
            .schedule
            .jittered_fire_at(&definition.id, base)
            .expect("jitter");
        assert_eq!(first, second);
    }

    #[test]
    fn disable_cron_env_overrides_enabled_config() {
        test_env_overrides::set(Some("1"));
        assert!(!scheduled_tasks_enabled(true));
        test_env_overrides::set(None);
    }

    #[test]
    fn durable_store_creates_and_lists_tasks() {
        let temp = tempdir().expect("tempdir");
        let store = DurableTaskStore::with_paths(SchedulerPaths {
            config_root: temp.path().join("cfg"),
            data_root: temp.path().join("data"),
        });
        let definition = ScheduledTaskDefinition::new(
            Some("daily".to_string()),
            ScheduleSpec::OneShot(OneShot {
                at: utc(2026, 3, 29, 9, 0, 0),
            }),
            ScheduledTaskAction::Reminder {
                message: "check release".to_string(),
            },
            None,
            utc(2026, 3, 28, 0, 0, 0),
            None,
        )
        .expect("definition");
        store.create(definition).expect("create");
        let tasks = store.list().expect("list");
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].name, "daily");
    }

    #[test]
    fn scheduled_workspace_resolution_expands_home_and_normalizes() {
        let resolved = resolve_scheduled_workspace_path_with_home(
            Path::new("~/projects/demo/../vtcode"),
            Some(Path::new("/tmp/home")),
        )
        .expect("resolve");
        assert_eq!(resolved, PathBuf::from("/tmp/home/projects/vtcode"));
    }

    #[test]
    fn schedule_create_definition_normalizes_prompt_workspace() {
        let temp = tempdir().expect("tempdir");
        let workspace = temp.path().join("workspace");
        fs::create_dir_all(&workspace).expect("workspace");
        let definition = ScheduleCreateInput {
            name: None,
            prompt: Some("check build".to_string()),
            reminder: None,
            every: Some("15m".to_string()),
            cron: None,
            at: None,
            workspace: Some(workspace.join(".").join("nested").join("..")),
        }
        .build_definition(Local::now(), None)
        .expect("definition");

        assert_eq!(definition.workspace.as_deref(), Some(workspace.as_path()));
    }

    #[test]
    fn schedule_create_definition_rejects_missing_prompt_workspace() {
        let error = ScheduleCreateInput {
            name: None,
            prompt: Some("check build".to_string()),
            reminder: None,
            every: Some("15m".to_string()),
            cron: None,
            at: None,
            workspace: Some(PathBuf::from("/path/that/does/not/exist")),
        }
        .build_definition(Local::now(), None)
        .expect_err("missing workspace should fail");

        assert!(
            error
                .to_string()
                .contains("Prompt task workspace does not exist or is not a directory")
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn scheduler_daemon_executes_due_prompt_task() {
        let temp = tempdir().expect("tempdir");
        let workspace = temp.path().join("workspace");
        fs::create_dir_all(&workspace).expect("workspace");
        let store = DurableTaskStore::with_paths(SchedulerPaths {
            config_root: temp.path().join("cfg"),
            data_root: temp.path().join("data"),
        });
        let now = Utc::now();
        let definition = ScheduledTaskDefinition::new(
            Some("hello".to_string()),
            ScheduleSpec::OneShot(OneShot {
                at: now - ChronoDuration::minutes(1),
            }),
            ScheduledTaskAction::Prompt {
                prompt: "say hello".to_string(),
            },
            Some(workspace),
            now - ChronoDuration::minutes(2),
            None,
        )
        .expect("definition");
        let summary = store.create(definition).expect("create");
        let executable = ["/usr/bin/true", "/bin/true"]
            .into_iter()
            .map(PathBuf::from)
            .find(|path| path.exists())
            .expect("true executable");
        let daemon = SchedulerDaemon::new(store.clone(), executable);

        let executed = daemon.run_due_tasks_once().await.expect("run");
        assert_eq!(executed, 1);

        let record = store
            .load_record(&summary.id)
            .expect("load")
            .expect("record");
        assert!(record.runtime.last_run_at.is_some());
        assert!(record.runtime.next_run_at.is_none());
        assert_eq!(
            record.runtime.last_status.as_ref().map(ToString::to_string),
            Some("success".to_string())
        );
        assert!(record.runtime.last_artifact_dir.is_some());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn scheduler_daemon_records_prompt_spawn_failures() {
        let temp = tempdir().expect("tempdir");
        let missing_workspace = temp.path().join("missing-workspace");
        let store = DurableTaskStore::with_paths(SchedulerPaths {
            config_root: temp.path().join("cfg"),
            data_root: temp.path().join("data"),
        });
        let now = Utc::now();
        let definition = ScheduledTaskDefinition::new(
            Some("broken".to_string()),
            ScheduleSpec::OneShot(OneShot {
                at: now - ChronoDuration::minutes(1),
            }),
            ScheduledTaskAction::Prompt {
                prompt: "say hello".to_string(),
            },
            Some(missing_workspace),
            now - ChronoDuration::minutes(2),
            None,
        )
        .expect("definition");
        let summary = store.create(definition).expect("create");
        let executable = ["/usr/bin/true", "/bin/true"]
            .into_iter()
            .map(PathBuf::from)
            .find(|path| path.exists())
            .expect("true executable");
        let daemon = SchedulerDaemon::new(store.clone(), executable);

        let executed = daemon.run_due_tasks_once().await.expect("run");
        assert_eq!(executed, 1);

        let record = store
            .load_record(&summary.id)
            .expect("load")
            .expect("record");
        assert!(record.runtime.last_run_at.is_some());
        assert!(record.runtime.next_run_at.is_none());
        assert!(matches!(
            record.runtime.last_status,
            Some(TaskRunStatus::Failed { .. })
        ));
    }

    #[test]
    fn durable_task_overdue_detection_requires_due_unrun_task() {
        let now = utc(2026, 3, 29, 0, 30, 0);
        assert!(durable_task_is_overdue(
            Some(utc(2026, 3, 29, 0, 22, 47)),
            None,
            false,
            now
        ));
        assert!(!durable_task_is_overdue(
            Some(utc(2026, 3, 29, 0, 40, 0)),
            None,
            false,
            now
        ));
        assert!(!durable_task_is_overdue(
            Some(utc(2026, 3, 29, 0, 22, 47)),
            Some(utc(2026, 3, 29, 0, 22, 47)),
            false,
            now
        ));
        assert!(!durable_task_is_overdue(
            Some(utc(2026, 3, 29, 0, 22, 47)),
            None,
            true,
            now
        ));
    }

    #[test]
    fn service_rendering_mentions_schedule_serve() {
        let launchd = render_launchd_plist(Path::new("/usr/local/bin/vtcode"));
        assert!(launchd.contains("schedule"));
        assert!(launchd.contains("serve"));
        let systemd = render_systemd_unit(Path::new("/usr/local/bin/vtcode"));
        assert!(systemd.contains("schedule serve"));
    }

    #[test]
    fn schedule_create_arg_parser_supports_workspace() {
        let parsed = parse_schedule_create_args(
            r#"--prompt "check build" --every 15m --workspace /tmp/demo --name "Build watch""#,
        )
        .expect("parse");
        assert_eq!(parsed.name.as_deref(), Some("Build watch"));
        assert_eq!(parsed.prompt.as_deref(), Some("check build"));
        assert_eq!(parsed.every.as_deref(), Some("15m"));
        assert_eq!(parsed.workspace, Some(PathBuf::from("/tmp/demo")));
    }
}
