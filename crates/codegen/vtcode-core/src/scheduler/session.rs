use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, Duration as ChronoDuration, Local, LocalResult, NaiveDateTime, NaiveTime, TimeZone, Utc};
use humantime::parse_duration as parse_human_duration;
use regex::Regex;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use super::spec::{
    DueSessionPrompt, ScheduleSpec, ScheduledTaskAction, ScheduledTaskDefinition, ScheduledTaskRecord,
    ScheduledTaskSummary, TaskRunStatus,
};
use super::{
    MAX_SCHEDULED_TASKS, SESSION_TASK_EXPIRY_HOURS, advance_record_runtime, due_session_prompt,
    initialize_runtime_state,
};
use crate::utils::path::normalize_path;

static REMIND_AT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?ix)^\s*remind\s+me\s+at\s+(?P<when>.+?)\s+to\s+(?P<prompt>.+)\s*$").expect("remind-at regex")
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
    Regex::new(r"(?ix)^\s*(?P<hour>\d{1,2})(?::(?P<minute>\d{2}))?\s*(?P<ampm>am|pm)?\s*$").expect("time-only regex")
});

#[derive(Debug, Default, Clone)]
pub struct SessionScheduler {
    pub(crate) tasks: BTreeMap<String, ScheduledTaskRecord>,
}

impl SessionScheduler {
    #[must_use]
    pub fn new() -> Self {
        Self { tasks: BTreeMap::new() }
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
        let record = ScheduledTaskRecord { definition: definition.clone(), runtime };
        let summary = record.summary();
        self.tasks.insert(definition.id.clone(), record);
        Ok(summary)
    }

    pub fn list(&self) -> Vec<ScheduledTaskSummary> {
        self.tasks.values().map(ScheduledTaskRecord::summary).collect()
    }

    pub fn delete(&mut self, query: &str) -> Option<ScheduledTaskSummary> {
        let query = query.trim();
        if query.is_empty() {
            return None;
        }
        if let Some(record) = self.tasks.remove(query) {
            return Some(record.summary());
        }
        let key = self
            .tasks
            .iter()
            .find_map(|(id, record)| record.definition.name.eq_ignore_ascii_case(query).then(|| id.clone()))?;
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

            if let Some(prompt) = due_session_prompt(record) {
                due.push(prompt);
            }

            if advance_record_runtime(record, now, TaskRunStatus::Triggered)? {
                completed.push(record.definition.id.clone());
            }
        }

        for id in completed {
            self.tasks.remove(&id);
        }

        Ok(due)
    }

    fn ensure_capacity(&self) -> Result<()> {
        if self.tasks.len() >= MAX_SCHEDULED_TASKS {
            bail!("A session can hold at most {MAX_SCHEDULED_TASKS} scheduled tasks");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionLanguageCommand {
    CreateOneShotPrompt { prompt: String, run_at: DateTime<Utc> },
    ListTasks,
    CancelTask { query: String },
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
            (None, None, Some(raw)) => ScheduleSpec::one_shot(parse_local_datetime(raw.trim(), now)?),
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

        if matches!(action, ScheduledTaskAction::Prompt { .. }) && workspace.as_ref().is_some_and(|path| !path.is_dir())
        {
            let workspace = workspace.as_ref().context("prompt workspace should exist")?;
            bail!("Prompt task workspace does not exist or is not a directory: {}", workspace.display());
        }

        ScheduledTaskDefinition::new(self.name, schedule, action, workspace, now.with_timezone(&Utc), None)
    }
}

pub(crate) fn resolve_scheduled_workspace_path(path: &Path) -> Result<PathBuf> {
    resolve_scheduled_workspace_path_with_home(path, dirs::home_dir().as_deref())
}

pub(crate) fn resolve_scheduled_workspace_path_with_home(path: &Path, home_dir: Option<&Path>) -> Result<PathBuf> {
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
        return home_dir.map(Path::to_path_buf).unwrap_or_else(|| path.to_path_buf());
    }

    if let Some(rest) = raw.strip_prefix("~/")
        && let Some(home_dir) = home_dir
    {
        return home_dir.join(rest);
    }

    path.to_path_buf()
}

pub fn parse_session_language_command(input: &str, now: DateTime<Local>) -> Option<Result<SessionLanguageCommand>> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }

    let normalized = trimmed.trim_end_matches(['?', '.', '!']).to_ascii_lowercase();
    if normalized == "what scheduled tasks do i have" {
        return Some(Ok(SessionLanguageCommand::ListTasks));
    }

    if let Some(captures) = REMIND_AT_RE.captures(trimmed) {
        let when = captures.name("when").map(|value| value.as_str()).unwrap_or_default();
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
        return Some(Ok(SessionLanguageCommand::CancelTask { query: query.trim().to_string() }));
    }

    None
}

pub fn parse_schedule_create_args(args: &str) -> Result<ScheduleCreateInput> {
    let tokens = shell_words::split(args).with_context(|| format!("Failed to parse arguments: {args}"))?;
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

    Ok(ScheduleCreateInput { name, prompt, reminder, every, cron, at, workspace })
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
        if let Some(ampm) = captures.name("ampm").map(|value| value.as_str().to_ascii_lowercase()) {
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

        let time = NaiveTime::from_hms_opt(hour, minute, 0).ok_or_else(|| anyhow!("Invalid time value"))?;
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
