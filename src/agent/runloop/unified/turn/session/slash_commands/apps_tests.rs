use std::fs;

use super::*;
use tempfile::tempdir;

#[test]
fn editor_preset_detects_known_commands() {
    assert_eq!(EditorPreset::from_saved(""), EditorPreset::Auto);
    assert_eq!(EditorPreset::from_saved("code --wait"), EditorPreset::Vscode);
    assert_eq!(EditorPreset::from_saved("zed"), EditorPreset::Zed);
    assert_eq!(EditorPreset::from_saved("nvim"), EditorPreset::Neovim);
    assert_eq!(EditorPreset::from_saved("vim"), EditorPreset::Vim);
    assert_eq!(EditorPreset::from_saved("subl -w"), EditorPreset::SublimeText);
    assert_eq!(EditorPreset::from_saved("/opt/custom/editor --flag"), EditorPreset::Custom);
}

#[test]
fn parse_editor_workflow_answers_extracts_choices() {
    let selections = vec![
        InlineListSelection::RequestUserInputAnswer {
            question_id: EDITOR_ENABLED_ID.to_string(),
            selected: vec!["enabled".to_string()],
            other: None,
        },
        InlineListSelection::RequestUserInputAnswer {
            question_id: EDITOR_PRESET_ID.to_string(),
            selected: vec!["custom".to_string()],
            other: None,
        },
        InlineListSelection::RequestUserInputAnswer {
            question_id: EDITOR_SUSPEND_ID.to_string(),
            selected: vec!["disabled".to_string()],
            other: None,
        },
        InlineListSelection::RequestUserInputAnswer {
            question_id: EDITOR_FOLLOW_UP_ID.to_string(),
            selected: vec!["file_opener".to_string()],
            other: None,
        },
    ];

    assert_eq!(
        parse_editor_workflow_answers(&selections),
        Some(EditorWorkflowChoices {
            enabled: true,
            preset: EditorPreset::Custom,
            custom_command: None,
            suspend_tui: false,
            open_file_opener_settings: true,
        })
    );
}

#[test]
fn persist_editor_workflow_choices_writes_preferred_editor_fields() {
    let workspace = tempdir().expect("workspace");
    fs::write(workspace.path().join("vtcode.toml"), "[workspace]\nuse_root_config = true\n")
        .expect("write isolated workspace config");

    persist_editor_workflow_choices(
        workspace.path(),
        EditorWorkflowChoices {
            enabled: true,
            preset: EditorPreset::Vscode,
            custom_command: None,
            suspend_tui: true,
            open_file_opener_settings: false,
        },
    )
    .expect("persist editor workflow choices");

    let manager = ConfigManager::load_from_workspace(workspace.path()).expect("load saved config");
    let config = manager.config();
    assert!(config.tools.editor.enabled);
    assert_eq!(config.tools.editor.preferred_editor, "code --wait");
    assert!(config.tools.editor.suspend_tui);
}

#[test]
fn should_wait_for_editor_only_blocks_live_file_opens_for_terminal_editors() {
    let mut config = EditorToolConfig {
        suspend_tui: true,
        preferred_editor: "nvim".to_string(),
        ..EditorToolConfig::default()
    };

    assert!(should_wait_for_editor(false, &config));
    assert!(should_wait_for_editor(true, &config));

    config.preferred_editor = "code --wait".to_string();
    assert!(!should_wait_for_editor(true, &config));

    config.suspend_tui = false;
    config.preferred_editor = "nvim".to_string();
    assert!(!should_wait_for_editor(true, &config));
}

#[test]
fn editor_command_requires_terminal_detects_known_terminal_editors() {
    assert!(editor_command_requires_terminal("nvim"));
    assert!(editor_command_requires_terminal("/usr/bin/vim"));
    assert!(editor_command_requires_terminal("helix"));
    assert!(!editor_command_requires_terminal("code --wait"));
    assert!(!editor_command_requires_terminal("zed"));
    // An empty command falls back to $VISUAL/$EDITOR and PATH detection, so
    // its result is environment-dependent and cannot be asserted here.
}

#[test]
fn custom_editor_command_step_uses_placeholder_as_default() {
    let step = build_custom_editor_command_step("code --wait".to_string());

    assert_eq!(step.freeform_placeholder.as_deref(), Some("code --wait"));
    assert_eq!(step.freeform_default.as_deref(), Some("code --wait"));
}
