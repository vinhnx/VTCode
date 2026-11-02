use super::*;
use crate::hooks::WorkspaceLifecycleHooks;

fn layer(source: ConfigLayerSource, content: &str) -> ConfigLayerEntry {
    ConfigLayerEntry::new(source, toml::from_str(content).expect("valid test toml"))
}

fn collect(stack: &ConfigLayerStack) -> WorkspaceLifecycleHooks {
    let (effective_toml, _) = stack.effective_config_with_origins();
    let config: VTCodeConfig = effective_toml.try_into().expect("effective config parses");
    ConfigManager::collect_workspace_lifecycle_hooks(stack, &config.hooks)
}

const WS_SESSION_START: &str = r#"
[[hooks.lifecycle.session_start]]
matcher = "startup"

[[hooks.lifecycle.session_start.hooks]]
command = "echo workspace-session"
timeout_seconds = 30
"#;

const USER_SESSION_END: &str = r#"
[[hooks.lifecycle.session_end]]
[[hooks.lifecycle.session_end.hooks]]
command = "echo user-session-end"
"#;

#[test]
fn workspace_layer_hooks_are_collected_user_hooks_are_not() {
    let mut stack = ConfigLayerStack::default();
    stack.push(layer(ConfigLayerSource::User { file: "/home/u/.vtcode/vtcode.toml".into() }, USER_SESSION_END));
    stack.push(layer(ConfigLayerSource::Workspace { file: "/ws/vtcode.toml".into() }, WS_SESSION_START));

    let collected = collect(&stack);

    assert_eq!(collected.commands.len(), 1, "only workspace hooks should be collected");
    let command = &collected.commands[0];
    assert_eq!(command.event, "session_start");
    assert_eq!(command.command, "echo workspace-session");
    assert_eq!(command.matcher.as_deref(), Some("startup"));
    assert_eq!(command.timeout_seconds, Some(30));
    assert!(!collected.is_empty());
}

#[test]
fn user_only_hooks_are_not_workspace_controlled() {
    let mut stack = ConfigLayerStack::default();
    stack.push(layer(ConfigLayerSource::User { file: "/home/u/.vtcode/vtcode.toml".into() }, USER_SESSION_END));

    let collected = collect(&stack);

    assert!(collected.is_empty());
}

#[test]
fn deprecated_task_completion_aliases_fold_into_stop() {
    let mut stack = ConfigLayerStack::default();
    stack.push(layer(
        ConfigLayerSource::Workspace { file: "/ws/vtcode.toml".into() },
        r#"
[[hooks.lifecycle.task_completion]]
[[hooks.lifecycle.task_completion.hooks]]
command = "echo ws-task-completion"
"#,
    ));

    let collected = collect(&stack);

    assert_eq!(collected.commands.len(), 1);
    assert_eq!(collected.commands[0].event, "stop");
    assert_eq!(collected.commands[0].command, "echo ws-task-completion");
}

#[test]
fn empty_workspace_hook_arrays_do_not_flag_content() {
    let mut stack = ConfigLayerStack::default();
    stack.push(layer(
        ConfigLayerSource::Workspace { file: "/ws/vtcode.toml".into() },
        "[hooks.lifecycle]\nsession_start = []\n",
    ));

    let collected = collect(&stack);

    assert!(collected.is_empty(), "an empty workspace hook array is not executable content");
}

#[test]
fn load_from_file_populates_workspace_lifecycle_hooks() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let config_path = temp_dir.path().join("vtcode.toml");
    fs::write(&config_path, WS_SESSION_START).expect("write workspace config");

    let manager = ConfigManager::load_from_file(&config_path).expect("load config");
    let workspace_hooks = manager
        .config()
        .workspace_lifecycle_hooks
        .as_ref()
        .expect("workspace hooks populated at load");

    assert!(!workspace_hooks.is_empty());
    assert_eq!(workspace_hooks.commands.len(), 1);
    assert_eq!(workspace_hooks.commands[0].command, "echo workspace-session");
}
