use std::path::PathBuf;

use anyhow::Result;
use chrono::{Local, Utc};
use vtcode_core::scheduler::{
    DURABLE_SCHEDULER_RUNTIME_HINT, DurableTaskStore, ScheduleCreateInput, durable_task_is_overdue,
};
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_tui::app::{
    InlineListItem, InlineListSearchConfig, InlineListSelection, WizardModalMode, WizardStep,
};

use crate::agent::runloop::slash_commands::ScheduleCommandAction;
use crate::agent::runloop::unified::wizard_modal::{
    WizardModalOutcome, show_wizard_modal_and_wait,
};

use super::{SlashCommandContext, SlashCommandControl};

const SCHEDULE_ACTION_PREFIX: &str = "schedule.action.";
const SCHEDULE_ACTION_BACK: &str = "schedule.action.back";
const SCHEDULE_TASK_SHOW_PREFIX: &str = "schedule.task.show.";
const SCHEDULE_TASK_DELETE_PREFIX: &str = "schedule.task.delete.";
const SCHEDULE_INPUT_ID: &str = "schedule.input";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InteractiveTaskKind {
    Prompt,
    Reminder,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InteractiveScheduleKind {
    Every,
    Cron,
    At,
}

pub(crate) async fn handle_manage_schedule(
    mut ctx: SlashCommandContext<'_>,
    action: ScheduleCommandAction,
) -> Result<SlashCommandControl> {
    if !super::scheduler_enabled(ctx.vt_cfg.as_ref()) {
        ctx.renderer.line(
            MessageStyle::Info,
            "Scheduled tasks are disabled. Enable [automation.scheduled_tasks].enabled or unset VTCODE_DISABLE_CRON.",
        )?;
        return Ok(SlashCommandControl::Continue);
    }

    let store = DurableTaskStore::new_default()?;
    match action {
        ScheduleCommandAction::Interactive => {
            run_interactive_schedule_manager(&mut ctx, &store).await?
        }
        ScheduleCommandAction::Browse => browse_tasks(&mut ctx, &store, false).await?,
        ScheduleCommandAction::CreateInteractive => {
            if let Some(input) = prompt_schedule_create_input(&mut ctx).await? {
                create_task(&mut ctx, &store, input)?;
            }
        }
        ScheduleCommandAction::Create { input } => create_task(&mut ctx, &store, input)?,
        ScheduleCommandAction::DeleteInteractive => {
            delete_task_interactively(&mut ctx, &store).await?;
        }
        ScheduleCommandAction::Delete { id } => delete_task(&mut ctx, &store, &id)?,
    }

    ctx.renderer.line_if_not_empty(MessageStyle::Output)?;
    Ok(SlashCommandControl::Continue)
}

async fn run_interactive_schedule_manager(
    ctx: &mut SlashCommandContext<'_>,
    store: &DurableTaskStore,
) -> Result<()> {
    if !super::ui::ensure_selection_ui_available(ctx, "opening scheduled tasks")? {
        return Ok(());
    }
    if !ctx.renderer.supports_inline_ui() {
        browse_tasks(ctx, store, false).await?;
        return Ok(());
    }

    loop {
        show_schedule_manager_modal(ctx, store)?;
        let Some(selection) = super::ui::wait_for_list_modal_selection(ctx).await else {
            return Ok(());
        };

        let InlineListSelection::ConfigAction(action) = selection else {
            continue;
        };
        if action == SCHEDULE_ACTION_BACK {
            return Ok(());
        }

        let Some(action_key) = action.strip_prefix(SCHEDULE_ACTION_PREFIX) else {
            continue;
        };
        match action_key {
            "browse" => browse_tasks(ctx, store, false).await?,
            "create" => {
                if let Some(input) = prompt_schedule_create_input(ctx).await? {
                    create_task(ctx, store, input)?;
                }
            }
            "delete" => delete_task_interactively(ctx, store).await?,
            _ => {}
        }
    }
}

fn show_schedule_manager_modal(
    ctx: &mut SlashCommandContext<'_>,
    store: &DurableTaskStore,
) -> Result<()> {
    let tasks = store.list()?;
    let task_count = tasks.len();
    let browse_subtitle = if task_count == 0 {
        "No durable scheduled tasks yet.".to_string()
    } else if task_count == 1 {
        "Browse 1 durable scheduled task.".to_string()
    } else {
        format!("Browse {task_count} durable scheduled tasks.")
    };
    let delete_subtitle = if task_count == 0 {
        "No tasks available to delete.".to_string()
    } else {
        "Select a task to delete.".to_string()
    };

    let items = vec![
        InlineListItem {
            title: "Browse scheduled tasks".to_string(),
            subtitle: Some(browse_subtitle),
            badge: Some("Recommended".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{}browse",
                SCHEDULE_ACTION_PREFIX
            ))),
            search_value: Some("browse scheduled tasks list".to_string()),
        },
        InlineListItem {
            title: "Create scheduled task".to_string(),
            subtitle: Some(
                "Interactive flow for prompt tasks, reminders, and schedule options.".to_string(),
            ),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{}create",
                SCHEDULE_ACTION_PREFIX
            ))),
            search_value: Some("create scheduled task reminder prompt".to_string()),
        },
        InlineListItem {
            title: "Delete scheduled task".to_string(),
            subtitle: Some(delete_subtitle),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{}delete",
                SCHEDULE_ACTION_PREFIX
            ))),
            search_value: Some("delete remove cancel scheduled task".to_string()),
        },
        InlineListItem {
            title: "Back".to_string(),
            subtitle: Some("Close the scheduled-task manager.".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(
                SCHEDULE_ACTION_BACK.to_string(),
            )),
            search_value: Some("back close".to_string()),
        },
    ];

    ctx.handle.show_list_modal(
        "Scheduled tasks".to_string(),
        vec![
            "Manage durable scheduled tasks interactively.".to_string(),
            "Enter selects an action. Esc closes the manager.".to_string(),
        ],
        items,
        Some(InlineListSelection::ConfigAction(format!(
            "{}browse",
            SCHEDULE_ACTION_PREFIX
        ))),
        Some(InlineListSearchConfig {
            label: "Search actions".to_string(),
            placeholder: Some("browse, create, delete".to_string()),
        }),
    );
    Ok(())
}

async fn browse_tasks(
    ctx: &mut SlashCommandContext<'_>,
    store: &DurableTaskStore,
    delete_on_select: bool,
) -> Result<()> {
    let tasks = store.list()?;
    if tasks.is_empty() {
        ctx.renderer
            .line(MessageStyle::Info, "No durable scheduled tasks.")?;
        return Ok(());
    }
    let has_overdue_tasks = tasks.iter().any(summary_is_overdue);

    if !ctx.renderer.supports_inline_ui() {
        for task in tasks {
            render_task_summary(ctx, &task)?;
        }
        if has_overdue_tasks {
            ctx.renderer
                .line(MessageStyle::Warning, DURABLE_SCHEDULER_RUNTIME_HINT)?;
        }
        return Ok(());
    }
    if !super::ui::ensure_selection_ui_available(ctx, "browsing scheduled tasks")? {
        return Ok(());
    }

    let items = tasks
        .iter()
        .map(|task| task_modal_item(task, delete_on_select))
        .collect::<Vec<_>>();
    let selected = items.first().and_then(|item| item.selection.clone());
    let title = if delete_on_select {
        "Delete scheduled task"
    } else {
        "Scheduled tasks"
    };
    let instructions = if delete_on_select {
        vec![
            "Select a task to delete it immediately.".to_string(),
            "Esc cancels without changing anything.".to_string(),
        ]
    } else {
        let mut instructions = vec![
            "Select a task to show its details in the transcript.".to_string(),
            "Esc closes this list.".to_string(),
        ];
        if has_overdue_tasks {
            instructions.insert(
                0,
                "Overdue tasks are waiting for the durable scheduler daemon.".to_string(),
            );
        }
        instructions
    };

    ctx.handle.show_list_modal(
        title.to_string(),
        instructions,
        items,
        selected,
        Some(InlineListSearchConfig {
            label: "Search tasks".to_string(),
            placeholder: Some("name, schedule, prompt, reminder".to_string()),
        }),
    );

    let Some(selection) = super::ui::wait_for_list_modal_selection(ctx).await else {
        return Ok(());
    };
    let InlineListSelection::ConfigAction(action) = selection else {
        return Ok(());
    };

    if delete_on_select {
        if let Some(id) = action.strip_prefix(SCHEDULE_TASK_DELETE_PREFIX) {
            delete_task(ctx, store, id)?;
        }
        return Ok(());
    }

    if let Some(id) = action.strip_prefix(SCHEDULE_TASK_SHOW_PREFIX) {
        show_task_details(ctx, store, id)?;
    }
    Ok(())
}

async fn delete_task_interactively(
    ctx: &mut SlashCommandContext<'_>,
    store: &DurableTaskStore,
) -> Result<()> {
    if !ctx.renderer.supports_inline_ui() {
        ctx.renderer.line(
            MessageStyle::Info,
            "Interactive delete is available in the TUI only. Use `/schedule delete <task-id>` here.",
        )?;
        return Ok(());
    }
    if !super::ui::ensure_selection_ui_available(ctx, "selecting a scheduled task")? {
        return Ok(());
    }
    browse_tasks(ctx, store, true).await
}

fn create_task(
    ctx: &mut SlashCommandContext<'_>,
    store: &DurableTaskStore,
    input: ScheduleCreateInput,
) -> Result<()> {
    let default_workspace = input
        .prompt
        .as_ref()
        .map(|_| ctx.config.workspace.clone())
        .filter(|_| input.workspace.is_none());
    match store.create_from_input(input, Local::now(), default_workspace) {
        Ok(summary) => {
            ctx.renderer.line(
                MessageStyle::Info,
                &format!(
                    "Created durable scheduled task {} ({}) with {}.",
                    summary.id, summary.name, summary.schedule
                ),
            )?;
            ctx.renderer
                .line(MessageStyle::Info, DURABLE_SCHEDULER_RUNTIME_HINT)?;
            ctx.renderer.line(
                MessageStyle::Info,
                "Use `vtcode schedule install-service` to keep durable tasks running after restart.",
            )?;
        }
        Err(err) => {
            ctx.renderer.line(MessageStyle::Error, &err.to_string())?;
        }
    }
    Ok(())
}

fn delete_task(
    ctx: &mut SlashCommandContext<'_>,
    store: &DurableTaskStore,
    id: &str,
) -> Result<()> {
    if let Some(task) = store.delete(id)? {
        ctx.renderer.line(
            MessageStyle::Info,
            &format!(
                "Deleted durable scheduled task {} ({}).",
                task.id, task.name
            ),
        )?;
    } else {
        ctx.renderer.line(
            MessageStyle::Warning,
            &format!("No durable scheduled task found for '{}'.", id),
        )?;
    }
    Ok(())
}

fn show_task_details(
    ctx: &mut SlashCommandContext<'_>,
    store: &DurableTaskStore,
    id: &str,
) -> Result<()> {
    let Some(record) = store.load_record(id)? else {
        ctx.renderer.line(
            MessageStyle::Warning,
            &format!("No durable scheduled task found for '{}'.", id),
        )?;
        return Ok(());
    };

    let action_label = match record.definition.action.kind_label() {
        "prompt" => "Prompt",
        "reminder" => "Reminder",
        _ => "Task",
    };

    ctx.renderer.line(
        MessageStyle::Info,
        &format!(
            "Scheduled task {} ({})",
            record.definition.id, record.definition.name
        ),
    )?;
    ctx.renderer.line(
        MessageStyle::Info,
        &format!("Kind: {}", record.definition.action.kind_label()),
    )?;
    ctx.renderer.line(
        MessageStyle::Info,
        &format!(
            "Schedule: {}",
            record.definition.schedule.human_description()
        ),
    )?;
    ctx.renderer.line(
        MessageStyle::Info,
        &format!("Next run: {}", format_task_time(record.runtime.next_run_at)),
    )?;
    ctx.renderer.line(
        MessageStyle::Info,
        &format!("Last run: {}", format_task_time(record.runtime.last_run_at)),
    )?;
    ctx.renderer.line(
        MessageStyle::Info,
        &format!("Status: {}", record_status_label(&record)),
    )?;
    if let Some(workspace) = record.definition.workspace.as_ref() {
        ctx.renderer.line(
            MessageStyle::Info,
            &format!("Workspace: {}", workspace.display()),
        )?;
    }
    ctx.renderer.line(
        MessageStyle::Info,
        &format!("{action_label}: {}", record.definition.action.summary()),
    )?;
    if record_is_overdue(&record) {
        ctx.renderer
            .line(MessageStyle::Warning, DURABLE_SCHEDULER_RUNTIME_HINT)?;
    }
    Ok(())
}

fn render_task_summary(
    ctx: &mut SlashCommandContext<'_>,
    task: &vtcode_core::scheduler::ScheduledTaskSummary,
) -> Result<()> {
    let next_run = task
        .next_run_at
        .map(|value| {
            value
                .with_timezone(&Local)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        })
        .unwrap_or_else(|| "none".to_string());
    let status = summary_status_label(task);
    ctx.renderer.line(
        MessageStyle::Info,
        &format!(
            "{}  {}  {}  next={}  status={}",
            task.id, task.name, task.schedule, next_run, status
        ),
    )?;
    Ok(())
}

fn task_modal_item(
    task: &vtcode_core::scheduler::ScheduledTaskSummary,
    delete_on_select: bool,
) -> InlineListItem {
    let next_run = task
        .next_run_at
        .map(|value| {
            value
                .with_timezone(&Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_else(|| "none".to_string());
    let status = summary_status_label(task);
    let selection = if delete_on_select {
        InlineListSelection::ConfigAction(format!("{}{}", SCHEDULE_TASK_DELETE_PREFIX, task.id))
    } else {
        InlineListSelection::ConfigAction(format!("{}{}", SCHEDULE_TASK_SHOW_PREFIX, task.id))
    };

    InlineListItem {
        title: task.name.clone(),
        subtitle: Some(format!(
            "{} • next {} • {}",
            task.schedule, next_run, status
        )),
        badge: Some(task.action_kind.clone()),
        indent: 0,
        selection: Some(selection),
        search_value: Some(format!(
            "{} {} {}",
            task.name, task.schedule, task.action_kind
        )),
    }
}

async fn prompt_schedule_create_input(
    ctx: &mut SlashCommandContext<'_>,
) -> Result<Option<ScheduleCreateInput>> {
    if !ctx.renderer.supports_inline_ui() {
        ctx.renderer.line(
            MessageStyle::Info,
            "Interactive create is available in the TUI only. Use `/schedule create --prompt ... --every ...` here.",
        )?;
        return Ok(None);
    }

    let Some(task_kind) = pick_task_kind(ctx).await? else {
        return Ok(None);
    };

    let (prompt, reminder) = match task_kind {
        InteractiveTaskKind::Prompt => (
            prompt_required_text(
                ctx,
                "Create scheduled task",
                "Enter the prompt VT Code should run on schedule.",
                "Prompt:",
                "check the deployment",
            )
            .await?,
            None,
        ),
        InteractiveTaskKind::Reminder => (
            None,
            prompt_required_text(
                ctx,
                "Create reminder",
                "Enter the reminder text VT Code should show locally.",
                "Reminder:",
                "push the release branch",
            )
            .await?,
        ),
    };
    if prompt.is_none() && reminder.is_none() {
        return Ok(None);
    }

    let Some(schedule_kind) = pick_schedule_kind(ctx).await? else {
        return Ok(None);
    };

    let (every, cron, at) = match schedule_kind {
        InteractiveScheduleKind::Every => (
            prompt_required_text(
                ctx,
                "Schedule cadence",
                "Enter a fixed interval like 10m, 2h, or 1d.",
                "Interval:",
                "10m",
            )
            .await?,
            None,
            None,
        ),
        InteractiveScheduleKind::Cron => (
            None,
            prompt_required_text(
                ctx,
                "Schedule cadence",
                "Enter a five-field cron expression in local time.",
                "Cron expression:",
                "0 9 * * 1-5",
            )
            .await?,
            None,
        ),
        InteractiveScheduleKind::At => (
            None,
            None,
            prompt_required_text(
                ctx,
                "Schedule time",
                "Enter a local one-shot time such as 15:00 or 2026-03-29 15:00.",
                "Time:",
                "15:00",
            )
            .await?,
        ),
    };
    if every.is_none() && cron.is_none() && at.is_none() {
        return Ok(None);
    }

    let name = prompt_optional_text(
        ctx,
        "Optional label",
        "Optionally give this task a short label.",
        "Label:",
        "deploy check",
    )
    .await?
    .and_then(trimmed_optional);

    let workspace = if matches!(task_kind, InteractiveTaskKind::Prompt) {
        prompt_optional_text(
            ctx,
            "Workspace",
            "Optionally override the workspace path. Leave blank to use the current session workspace.",
            "Workspace path:",
            &ctx.config.workspace.display().to_string(),
        )
        .await?
        .and_then(trimmed_optional)
        .map(PathBuf::from)
    } else {
        None
    };

    Ok(Some(ScheduleCreateInput {
        name,
        prompt,
        reminder,
        every,
        cron,
        at,
        workspace,
    }))
}

async fn pick_task_kind(ctx: &mut SlashCommandContext<'_>) -> Result<Option<InteractiveTaskKind>> {
    if !super::ui::ensure_selection_ui_available(ctx, "creating a scheduled task")? {
        return Ok(None);
    }

    ctx.handle.show_list_modal(
        "Create scheduled task".to_string(),
        vec!["Choose what VT Code should schedule.".to_string()],
        vec![
            InlineListItem {
                title: "Prompt task".to_string(),
                subtitle: Some("Run a fresh `vtcode exec` prompt on schedule.".to_string()),
                badge: Some("Recommended".to_string()),
                indent: 0,
                selection: Some(InlineListSelection::ConfigAction(
                    "schedule.kind.prompt".to_string(),
                )),
                search_value: Some("prompt exec task".to_string()),
            },
            InlineListItem {
                title: "Reminder".to_string(),
                subtitle: Some("Show a local reminder without invoking the model.".to_string()),
                badge: None,
                indent: 0,
                selection: Some(InlineListSelection::ConfigAction(
                    "schedule.kind.reminder".to_string(),
                )),
                search_value: Some("reminder local notification".to_string()),
            },
        ],
        Some(InlineListSelection::ConfigAction(
            "schedule.kind.prompt".to_string(),
        )),
        Some(InlineListSearchConfig {
            label: "Search types".to_string(),
            placeholder: Some("prompt or reminder".to_string()),
        }),
    );

    let Some(selection) = super::ui::wait_for_list_modal_selection(ctx).await else {
        return Ok(None);
    };
    let InlineListSelection::ConfigAction(action) = selection else {
        return Ok(None);
    };

    Ok(match action.as_str() {
        "schedule.kind.prompt" => Some(InteractiveTaskKind::Prompt),
        "schedule.kind.reminder" => Some(InteractiveTaskKind::Reminder),
        _ => None,
    })
}

async fn pick_schedule_kind(
    ctx: &mut SlashCommandContext<'_>,
) -> Result<Option<InteractiveScheduleKind>> {
    if !super::ui::ensure_selection_ui_available(ctx, "choosing a schedule")? {
        return Ok(None);
    }

    ctx.handle.show_list_modal(
        "Schedule type".to_string(),
        vec!["Choose how this task should run.".to_string()],
        vec![
            InlineListItem {
                title: "Fixed interval".to_string(),
                subtitle: Some("Repeat on a cadence like 10m or 2h.".to_string()),
                badge: Some("Recommended".to_string()),
                indent: 0,
                selection: Some(InlineListSelection::ConfigAction(
                    "schedule.when.every".to_string(),
                )),
                search_value: Some("interval every fixed".to_string()),
            },
            InlineListItem {
                title: "Cron expression".to_string(),
                subtitle: Some("Use a five-field cron expression in local time.".to_string()),
                badge: None,
                indent: 0,
                selection: Some(InlineListSelection::ConfigAction(
                    "schedule.when.cron".to_string(),
                )),
                search_value: Some("cron expression".to_string()),
            },
            InlineListItem {
                title: "One-shot time".to_string(),
                subtitle: Some("Run once at a local time.".to_string()),
                badge: None,
                indent: 0,
                selection: Some(InlineListSelection::ConfigAction(
                    "schedule.when.at".to_string(),
                )),
                search_value: Some("once at time".to_string()),
            },
        ],
        Some(InlineListSelection::ConfigAction(
            "schedule.when.every".to_string(),
        )),
        Some(InlineListSearchConfig {
            label: "Search schedule types".to_string(),
            placeholder: Some("interval, cron, once".to_string()),
        }),
    );

    let Some(selection) = super::ui::wait_for_list_modal_selection(ctx).await else {
        return Ok(None);
    };
    let InlineListSelection::ConfigAction(action) = selection else {
        return Ok(None);
    };

    Ok(match action.as_str() {
        "schedule.when.every" => Some(InteractiveScheduleKind::Every),
        "schedule.when.cron" => Some(InteractiveScheduleKind::Cron),
        "schedule.when.at" => Some(InteractiveScheduleKind::At),
        _ => None,
    })
}

async fn prompt_required_text(
    ctx: &mut SlashCommandContext<'_>,
    title: &str,
    question: &str,
    freeform_label: &str,
    placeholder: &str,
) -> Result<Option<String>> {
    prompt_text(ctx, title, question, freeform_label, placeholder, false).await
}

async fn prompt_optional_text(
    ctx: &mut SlashCommandContext<'_>,
    title: &str,
    question: &str,
    freeform_label: &str,
    placeholder: &str,
) -> Result<Option<String>> {
    prompt_text(ctx, title, question, freeform_label, placeholder, true).await
}

async fn prompt_text(
    ctx: &mut SlashCommandContext<'_>,
    title: &str,
    question: &str,
    freeform_label: &str,
    placeholder: &str,
    allow_empty: bool,
) -> Result<Option<String>> {
    let step = WizardStep {
        title: "Input".to_string(),
        question: question.to_string(),
        items: vec![InlineListItem {
            title: "Submit".to_string(),
            subtitle: Some(
                "Press Enter to accept the placeholder, or Tab to type a custom value.".to_string(),
            ),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::RequestUserInputAnswer {
                question_id: SCHEDULE_INPUT_ID.to_string(),
                selected: vec![],
                other: Some(String::new()),
            }),
            search_value: Some("submit input".to_string()),
        }],
        completed: false,
        answer: None,
        allow_freeform: true,
        freeform_label: Some(freeform_label.to_string()),
        freeform_placeholder: Some(placeholder.to_string()),
    };

    let outcome = show_wizard_modal_and_wait(
        ctx.handle,
        ctx.session,
        title.to_string(),
        vec![step],
        0,
        None,
        WizardModalMode::MultiStep,
        ctx.ctrl_c_state,
        ctx.ctrl_c_notify,
    )
    .await?;

    let value = match outcome {
        WizardModalOutcome::Submitted(selections) => {
            selections
                .into_iter()
                .find_map(|selection| match selection {
                    InlineListSelection::RequestUserInputAnswer {
                        question_id,
                        selected,
                        other,
                    } if question_id == SCHEDULE_INPUT_ID => {
                        other.or_else(|| selected.first().cloned())
                    }
                    _ => None,
                })
        }
        WizardModalOutcome::Cancelled { .. } => None,
    };

    let resolved = resolve_prompt_submission(value, placeholder, allow_empty);
    if resolved.is_none() {
        ctx.renderer
            .line(MessageStyle::Info, "Input was empty. Nothing executed.")?;
    }
    Ok(resolved)
}

fn format_task_time(value: Option<chrono::DateTime<chrono::Utc>>) -> String {
    value
        .map(|value| {
            value
                .with_timezone(&Local)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        })
        .unwrap_or_else(|| "none".to_string())
}

fn trimmed_optional(value: String) -> Option<String> {
    let trimmed = value.trim().to_string();
    (!trimmed.is_empty()).then_some(trimmed)
}

fn record_is_overdue(record: &vtcode_core::scheduler::ScheduledTaskRecord) -> bool {
    durable_task_is_overdue(
        record.runtime.next_run_at,
        record.runtime.last_run_at,
        record.runtime.last_status.is_some(),
        Utc::now(),
    )
}

fn record_status_label(record: &vtcode_core::scheduler::ScheduledTaskRecord) -> String {
    if record_is_overdue(record) {
        return "overdue".to_string();
    }
    record
        .runtime
        .last_status
        .as_ref()
        .map(ToString::to_string)
        .unwrap_or_else(|| "never_run".to_string())
}

fn summary_is_overdue(task: &vtcode_core::scheduler::ScheduledTaskSummary) -> bool {
    durable_task_is_overdue(
        task.next_run_at,
        task.last_run_at,
        task.last_status.is_some(),
        Utc::now(),
    )
}

fn summary_status_label(task: &vtcode_core::scheduler::ScheduledTaskSummary) -> String {
    if summary_is_overdue(task) {
        return "overdue".to_string();
    }
    task.last_status
        .clone()
        .unwrap_or_else(|| "never_run".to_string())
}

fn resolve_prompt_submission(
    value: Option<String>,
    placeholder: &str,
    allow_empty: bool,
) -> Option<String> {
    let value = value?;
    let trimmed = value.trim();
    if !trimmed.is_empty() {
        return Some(trimmed.to_string());
    }

    let placeholder = placeholder.trim();
    if !placeholder.is_empty() {
        return Some(placeholder.to_string());
    }

    allow_empty.then_some(value)
}

#[cfg(test)]
mod tests {
    use super::resolve_prompt_submission;

    #[test]
    fn schedule_prompt_submission_accepts_placeholder_on_empty_enter() {
        assert_eq!(
            resolve_prompt_submission(Some(String::new()), "10m", false),
            Some("10m".to_string())
        );
    }

    #[test]
    fn schedule_prompt_submission_prefers_explicit_text() {
        assert_eq!(
            resolve_prompt_submission(Some("  every 2h  ".to_string()), "10m", false),
            Some("every 2h".to_string())
        );
    }

    #[test]
    fn schedule_prompt_submission_keeps_optional_blank_without_placeholder() {
        assert_eq!(
            resolve_prompt_submission(Some(String::new()), "", true),
            Some(String::new())
        );
    }

    #[test]
    fn schedule_prompt_submission_rejects_required_blank_without_placeholder() {
        assert_eq!(
            resolve_prompt_submission(Some(String::new()), "", false),
            None
        );
    }
}
