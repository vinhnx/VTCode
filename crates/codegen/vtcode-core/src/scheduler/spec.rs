use anyhow::{Context, Result, bail};
use chrono::{DateTime, Duration as ChronoDuration, Local, Timelike, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use super::cron::ParsedCron;

const SESSION_JITTER_CAP_SECS: u64 = 15 * 60;
const ONE_SHOT_TOP_OF_HOUR_JITTER_SECS: u64 = 90;

static NEXT_TASK_COUNTER: AtomicU64 = AtomicU64::new(1);

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
            Self::OneShot(spec) => {
                format!("once at {}", spec.at.with_timezone(&Local).format("%Y-%m-%d %H:%M"))
            }
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

    pub fn next_base_fire_after(&self, last_base_fire_at: DateTime<Utc>) -> Result<Option<DateTime<Utc>>> {
        let last_base_local = last_base_fire_at.with_timezone(&Local);
        let next_local = match self {
            Self::Cron5(spec) => spec.next_after(last_base_local)?,
            Self::FixedInterval(spec) => Some(last_base_local + spec.chrono_duration()?),
            Self::OneShot(_) => None,
        };
        Ok(next_local.map(|value| value.with_timezone(&Utc)))
    }

    pub(crate) fn jittered_fire_at(&self, id: &str, base_fire_at: DateTime<Utc>) -> Result<DateTime<Utc>> {
        let base_local = base_fire_at.with_timezone(&Local);
        let hash = stable_hash_u64(id.as_bytes());
        match self {
            Self::Cron5(_) | Self::FixedInterval(_) => {
                let Some(period) = self.nominal_period()? else {
                    return Ok(base_fire_at);
                };
                #[allow(
                    clippy::cast_sign_loss,
                    reason = "Intentional compatibility, platform, or test-only suppression."
                )]
                let period_secs = period.num_seconds().max(0) as u64;
                #[allow(
                    clippy::cast_sign_loss,
                    reason = "Intentional compatibility, platform, or test-only suppression."
                )]
                let max_delay = (((period_secs as f64) * 0.10).floor()).max(0.0) as u64;
                let max_delay = max_delay.min(SESSION_JITTER_CAP_SECS);
                let delay_secs = if max_delay == 0 { 0 } else { hash % (max_delay + 1) };
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

    pub(crate) fn parsed(&self) -> Result<ParsedCron> {
        ParsedCron::parse(&self.expression)
    }

    pub(crate) fn next_after(&self, after: DateTime<Local>) -> Result<Option<DateTime<Local>>> {
        self.parsed()?.next_after(after)
    }

    pub(crate) fn approx_period(&self) -> Result<Option<ChronoDuration>> {
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
        Ok(Self { seconds: duration.as_secs() })
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
    pub(crate) fn summary(&self) -> ScheduledTaskSummary {
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

fn summarize_task_name(summary: &str) -> String {
    let trimmed = summary.trim();
    if trimmed.is_empty() {
        return "Scheduled task".to_string();
    }
    let compact = vtcode_commons::formatting::collapse_whitespace(trimmed);
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
