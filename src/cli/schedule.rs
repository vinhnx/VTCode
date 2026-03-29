use anyhow::{Result, bail};
use chrono::Local;
use vtcode_core::cli::args::{ScheduleCreateArgs, ScheduleSubcommand};
use vtcode_core::scheduler::{
    DURABLE_SCHEDULER_RUNTIME_HINT, DurableTaskStore, ScheduleCreateInput, SchedulerDaemon,
    durable_task_is_overdue, install_service_file, scheduled_tasks_enabled, uninstall_service_file,
};

use crate::startup::StartupContext;

pub(crate) async fn handle_schedule_command(
    startup: &StartupContext,
    command: ScheduleSubcommand,
) -> Result<()> {
    match command {
        ScheduleSubcommand::Create(args) => handle_create(startup, args).await,
        ScheduleSubcommand::List => handle_list(startup).await,
        ScheduleSubcommand::Delete { id } => handle_delete(startup, &id).await,
        ScheduleSubcommand::Serve => handle_serve(startup).await,
        ScheduleSubcommand::InstallService => handle_install_service(startup).await,
        ScheduleSubcommand::UninstallService => handle_uninstall_service().await,
    }
}

async fn handle_create(startup: &StartupContext, args: ScheduleCreateArgs) -> Result<()> {
    ensure_scheduler_enabled(startup)?;
    let store = DurableTaskStore::new_default()?;
    let input = schedule_create_input(args);
    let default_workspace = input
        .prompt
        .as_ref()
        .map(|_| startup.workspace.clone())
        .filter(|_| input.workspace.is_none());
    let summary = store.create_from_input(input, Local::now(), default_workspace)?;
    println!("Created scheduled task {}", summary.id);
    println!("Name: {}", summary.name);
    println!("Kind: {}", summary.action_kind);
    println!("Schedule: {}", summary.schedule);
    if let Some(next_run_at) = summary.next_run_at {
        println!(
            "Next run: {}",
            next_run_at
                .with_timezone(&Local)
                .format("%Y-%m-%d %H:%M:%S")
        );
    }
    println!("{DURABLE_SCHEDULER_RUNTIME_HINT}");
    println!("Use `vtcode schedule install-service` to keep durable tasks running after restart.");
    Ok(())
}

async fn handle_list(startup: &StartupContext) -> Result<()> {
    ensure_scheduler_enabled(startup)?;
    let store = DurableTaskStore::new_default()?;
    let tasks = store.list()?;
    if tasks.is_empty() {
        println!("No durable scheduled tasks.");
        return Ok(());
    }

    let now = chrono::Utc::now();
    let mut has_overdue_tasks = false;
    for task in tasks {
        let next_run = task
            .next_run_at
            .map(|value| {
                value
                    .with_timezone(&Local)
                    .format("%Y-%m-%d %H:%M:%S")
                    .to_string()
            })
            .unwrap_or_else(|| "none".to_string());
        let is_overdue = durable_task_is_overdue(
            task.next_run_at,
            task.last_run_at,
            task.last_status.is_some(),
            now,
        );
        let last_status = if is_overdue {
            has_overdue_tasks = true;
            "overdue".to_string()
        } else {
            task.last_status.unwrap_or_else(|| "never_run".to_string())
        };
        println!(
            "{}  {}  {}  next={}  status={}",
            task.id, task.name, task.schedule, next_run, last_status
        );
    }
    if has_overdue_tasks {
        println!("{DURABLE_SCHEDULER_RUNTIME_HINT}");
    }

    Ok(())
}

async fn handle_delete(startup: &StartupContext, id: &str) -> Result<()> {
    ensure_scheduler_enabled(startup)?;
    let store = DurableTaskStore::new_default()?;
    let Some(task) = store.delete(id)? else {
        bail!("No scheduled task with id '{}'", id);
    };
    println!("Deleted scheduled task {} ({})", task.id, task.name);
    Ok(())
}

async fn handle_serve(startup: &StartupContext) -> Result<()> {
    ensure_scheduler_enabled(startup)?;
    vtcode_core::notifications::apply_global_notification_config_from_vtcode(&startup.config)?;
    let store = DurableTaskStore::new_default()?;
    let executable = std::env::current_exe()?;
    println!("VT Code scheduler daemon running.");
    println!("Executable: {}", executable.display());
    let daemon = SchedulerDaemon::new(store, executable);
    daemon.serve_forever().await
}

async fn handle_install_service(startup: &StartupContext) -> Result<()> {
    ensure_scheduler_enabled(startup)?;
    let executable = std::env::current_exe()?;
    let plan = install_service_file(&executable)?;
    println!(
        "Installed scheduler service file at {}",
        plan.path.display()
    );
    match plan.manager {
        vtcode_core::scheduler::ServiceManager::Launchd => {
            println!(
                "Next step: run `launchctl bootstrap gui/$(id -u) {}`",
                shell_words::quote(plan.path.display().to_string().as_str())
            );
        }
        vtcode_core::scheduler::ServiceManager::SystemdUser => {
            println!("Next step: run `systemctl --user daemon-reload`");
            println!(
                "Then run: `systemctl --user enable --now {}`",
                plan.path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("vtcode-scheduler.service")
            );
        }
    }
    Ok(())
}

async fn handle_uninstall_service() -> Result<()> {
    if let Some((manager, path, removed)) = uninstall_service_file()? {
        if removed {
            println!("Removed scheduler service file at {}", path.display());
        } else {
            println!("Scheduler service file is not installed.");
        }
        match manager {
            vtcode_core::scheduler::ServiceManager::Launchd => {
                println!(
                    "If the service is loaded, run `launchctl bootout gui/$(id -u) {}`.",
                    shell_words::quote(path.display().to_string().as_str())
                );
            }
            vtcode_core::scheduler::ServiceManager::SystemdUser => {
                println!(
                    "If the service is enabled, run `systemctl --user disable --now vtcode-scheduler.service`."
                );
            }
        }
        return Ok(());
    }

    println!("Durable scheduler services are unsupported on this platform.");
    Ok(())
}

fn ensure_scheduler_enabled(startup: &StartupContext) -> Result<()> {
    if scheduled_tasks_enabled(startup.config.automation.scheduled_tasks.enabled) {
        return Ok(());
    }
    bail!(
        "Scheduled tasks are disabled. Enable [automation.scheduled_tasks].enabled or unset VTCODE_DISABLE_CRON."
    )
}

fn schedule_create_input(args: ScheduleCreateArgs) -> ScheduleCreateInput {
    ScheduleCreateInput {
        name: args.name,
        prompt: args.prompt,
        reminder: args.reminder,
        every: args.every,
        cron: args.cron,
        at: args.at,
        workspace: args.workspace,
    }
}
