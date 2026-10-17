use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use std::path::Path;
use vtcode_commons::VtCodePaths;
use vtcode_commons::utils::parse_bool_env_value;

pub const MAX_SCHEDULED_TASKS: usize = 50;
pub const SESSION_TASK_EXPIRY_HOURS: i64 = 72;
pub const DISABLE_CRON_ENV: &str = "VTCODE_DISABLE_CRON";
pub const DURABLE_SCHEDULER_RUNTIME_HINT: &str = "Durable tasks fire while VT Code is open, `vtcode schedule serve` is running, or the installed scheduler service is active.";

const SERVICE_NAME: &str = "vtcode-scheduler";
const LAUNCHD_LABEL: &str = "com.vtcode.scheduler";

#[cfg(test)]
mod test_env_overrides {
    use std::sync::{LazyLock, Mutex};

    static DISABLE_CRON: LazyLock<Mutex<Option<String>>> = LazyLock::new(|| Mutex::new(None));

    pub(super) fn get() -> Option<String> {
        DISABLE_CRON.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub(super) fn set(value: Option<&str>) {
        if let Ok(mut slot) = DISABLE_CRON.lock() {
            *slot = value.map(ToString::to_string);
        }
    }
}

#[cfg(test)]
mod tests;

mod cron;
mod daemon;
mod service;
mod session;
mod spec;
mod store;

pub use self::daemon::SchedulerDaemon;
pub use self::service::{
    ServiceInstallPlan, ServiceManager, install_service_file, render_launchd_plist, render_service_install_plan,
    render_systemd_unit, uninstall_service_file,
};
#[cfg(test)]
pub(crate) use self::session::resolve_scheduled_workspace_path_with_home;
pub use self::session::{
    ScheduleCreateInput, SessionLanguageCommand, SessionScheduler, parse_local_datetime, parse_schedule_create_args,
    parse_schedule_create_tokens, parse_session_language_command,
};
pub use self::spec::{
    Cron5, DueSessionPrompt, FixedInterval, OneShot, ScheduleSpec, ScheduledTaskAction, ScheduledTaskDefinition,
    ScheduledTaskRecord, ScheduledTaskRuntimeState, ScheduledTaskSummary, TaskRunStatus,
};
pub use self::store::{DurableTaskStore, SchedulerPaths};

pub fn scheduled_tasks_enabled(enabled_in_config: bool) -> bool {
    #[cfg(test)]
    if let Some(value) = test_env_overrides::get() {
        if parse_bool_env_value(&value) == Some(true) {
            return false;
        }
    }

    if let Ok(value) = std::env::var(DISABLE_CRON_ENV) {
        if parse_bool_env_value(&value) == Some(true) {
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

fn initialize_runtime_state(definition: &ScheduledTaskDefinition) -> Result<ScheduledTaskRuntimeState> {
    let next_base_run_at = definition.schedule.first_base_fire_at(definition.created_at)?;
    let next_run_at = next_base_run_at
        .map(|base| definition.schedule.jittered_fire_at(&definition.id, base))
        .transpose()?;
    Ok(ScheduledTaskRuntimeState {
        next_base_run_at,
        next_run_at,
        ..ScheduledTaskRuntimeState::default()
    })
}

#[derive(Debug, Clone, Copy)]
struct NextScheduledRun {
    base_fire_at: DateTime<Utc>,
    fire_at: DateTime<Utc>,
}

fn due_session_prompt(record: &ScheduledTaskRecord) -> Option<DueSessionPrompt> {
    let ScheduledTaskAction::Prompt { prompt } = &record.definition.action else {
        return None;
    };

    Some(DueSessionPrompt {
        id: record.definition.id.clone(),
        name: record.definition.name.clone(),
        prompt: prompt.clone(),
    })
}

fn next_scheduled_run(
    definition: &ScheduledTaskDefinition,
    last_base_run_at: Option<DateTime<Utc>>,
) -> Result<Option<NextScheduledRun>> {
    let Some(last_base_run_at) = last_base_run_at else {
        return Ok(None);
    };

    let Some(next_base_run_at) = definition.schedule.next_base_fire_after(last_base_run_at)? else {
        return Ok(None);
    };

    if definition.expires_at.is_some_and(|expiry| next_base_run_at > expiry) {
        return Ok(None);
    }

    Ok(Some(NextScheduledRun {
        fire_at: definition.schedule.jittered_fire_at(&definition.id, next_base_run_at)?,
        base_fire_at: next_base_run_at,
    }))
}

fn advance_record_runtime(
    record: &mut ScheduledTaskRecord,
    ran_at: DateTime<Utc>,
    status: TaskRunStatus,
) -> Result<bool> {
    record.runtime.last_run_at = Some(ran_at);
    record.runtime.last_status = Some(status);

    match next_scheduled_run(&record.definition, record.runtime.next_base_run_at)? {
        Some(next_run) => {
            record.runtime.next_base_run_at = Some(next_run.base_fire_at);
            record.runtime.next_run_at = Some(next_run.fire_at);
            Ok(false)
        }
        None => {
            record.runtime.next_base_run_at = None;
            record.runtime.next_run_at = None;
            Ok(true)
        }
    }
}

fn atomic_write(path: &Path, content: &[u8]) -> Result<()> {
    VtCodePaths::write_private_file_atomic(path, content)
        .with_context(|| format!("Failed to atomically write {}", path.display()))
}
