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
