use std::path::Path;
use std::time::Duration;

use anyhow::Result;
use vtcode_commons::resolve_editor_target;
use vtcode_core::config::EditorToolConfig;
use vtcode_core::config::loader::ConfigManager;
use vtcode_core::tools::terminal_app::{EditorLaunchConfig, TerminalAppLauncher};
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_tui::app::{
    InlineHandle, InlineListItem, InlineListSelection, WizardModalMode, WizardStep,
};

use vtcode_core::hooks::SessionEndReason;

use super::{SlashCommandContext, SlashCommandControl};
use crate::agent::runloop::unified::external_url_guard::{
    ExternalUrlGuardContext, ExternalUrlOpenOutcome, request_external_url_open,
};
use crate::agent::runloop::unified::palettes::{
    ActivePalette, refresh_runtime_config_from_manager,
};
use crate::agent::runloop::unified::settings_interactive::{
    create_settings_palette_state, resolve_settings_view_path, show_settings_palette,
};
use crate::agent::runloop::unified::wizard_modal::{
    WizardModalOutcome, show_wizard_modal_and_wait,
};

const EXTERNAL_APP_EVENT_LOOP_SETTLE_DELAY: Duration = Duration::from_millis(50);
const DOCS_URL: &str = "https://deepwiki.com/vinhnx/vtcode";
const EXTERNAL_EDITOR_TITLE: &str = "External Editor";
const FILE_OPENER_SETTINGS_PATH: &str = "file_opener";
const EDITOR_ENABLED_ID: &str = "tools_editor_enabled";
const EDITOR_PRESET_ID: &str = "tools_editor_preset";
const EDITOR_SUSPEND_ID: &str = "tools_editor_suspend";
const EDITOR_FOLLOW_UP_ID: &str = "tools_editor_follow_up";
const EDITOR_CUSTOM_COMMAND_ID: &str = "tools_editor_custom_command";
const WORKFLOW_ENABLED: &str = "enabled";
const WORKFLOW_DISABLED: &str = "disabled";
const WORKFLOW_DONE: &str = "done";
const WORKFLOW_FILE_OPENER: &str = "file_opener";

pub(crate) async fn handle_new_session(
    ctx: SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    ctx.renderer
        .line(MessageStyle::Info, "Starting new session...")?;
    Ok(SlashCommandControl::BreakWithReason(
        SessionEndReason::NewSession,
    ))
}

pub(crate) async fn handle_open_docs(ctx: SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    match request_external_url_open(
        ExternalUrlGuardContext::new(ctx.handle, ctx.session, ctx.ctrl_c_state, ctx.ctrl_c_notify),
        DOCS_URL,
    )
    .await?
    {
        ExternalUrlOpenOutcome::Opened => {
            ctx.renderer.line(
                MessageStyle::Info,
                &format!("Opening documentation in browser: {}", DOCS_URL),
            )?;
        }
        ExternalUrlOpenOutcome::OpenFailed(err) => {
            ctx.renderer.line(
                MessageStyle::Error,
                &format!("Failed to open browser: {}", err),
            )?;
            ctx.renderer
                .line(MessageStyle::Info, &format!("Please visit: {}", DOCS_URL))?;
        }
        ExternalUrlOpenOutcome::Cancelled => {
            ctx.renderer
                .line(MessageStyle::Info, "Cancelled opening documentation link.")?;
        }
        ExternalUrlOpenOutcome::Exit => {
            return Ok(SlashCommandControl::BreakWithReason(SessionEndReason::Exit));
        }
        ExternalUrlOpenOutcome::Unsupported => {
            ctx.renderer.line(
                MessageStyle::Error,
                "Blocked unsupported documentation link target.",
            )?;
        }
    }
    ctx.renderer.line_if_not_empty(MessageStyle::Output)?;
    Ok(SlashCommandControl::Continue)
}

pub(crate) async fn handle_launch_editor(
    ctx: SlashCommandContext<'_>,
    file: Option<String>,
) -> Result<SlashCommandControl> {
    let mut ctx = ctx;
    launch_editor_from_context(&mut ctx, file).await
}

pub(crate) async fn launch_editor_from_context(
    ctx: &mut SlashCommandContext<'_>,
    file: Option<String>,
) -> Result<SlashCommandControl> {
    let launcher = TerminalAppLauncher::new(ctx.config.workspace.clone());
    let editor_config = ctx
        .vt_cfg
        .as_ref()
        .map(|config| config.tools.editor.clone())
        .unwrap_or_default();
    if !editor_config.enabled {
        ctx.renderer.line(
            MessageStyle::Warning,
            "External editor is disabled (`tools.editor.enabled = false`).",
        )?;
        ctx.renderer.line_if_not_empty(MessageStyle::Output)?;
        return Ok(SlashCommandControl::Continue);
    }

    let file_target = match file.as_deref() {
        Some(value) => match resolve_editor_target(value, &ctx.config.workspace) {
            Some(target) => Some(target),
            None => {
                ctx.renderer.line(
                    MessageStyle::Error,
                    &format!("Invalid file target for `/edit`: {value}"),
                )?;
                ctx.renderer.line_if_not_empty(MessageStyle::Output)?;
                return Ok(SlashCommandControl::Continue);
            }
        },
        None => None,
    };

    let opening_existing_file = file_target.is_some();
    let wait_for_editor = should_wait_for_editor(opening_existing_file, &editor_config);
    let is_transient_open = opening_existing_file && !wait_for_editor;

    if !is_transient_open {
        ctx.renderer.line(
            MessageStyle::Info,
            if opening_existing_file {
                "Launching editor, with the existing file, close the tab or editor to continue on the VT Code session..."
            } else {
                "Launching editor with current input..."
            },
        )?;
    }

    let launch_config = launch_config_from_settings(&editor_config, wait_for_editor);

    let launch_result = run_with_event_loop_suspended(
        ctx.handle,
        editor_config.suspend_tui && wait_for_editor,
        || launcher.launch_editor_target_with_config(file_target, launch_config),
    )
    .await;

    if launch_result.is_ok() && is_transient_open {
        ctx.handle.force_redraw();
        return Ok(SlashCommandControl::Continue);
    }

    let (message_style, message) = match launch_result {
        Ok(Some(edited_content)) => {
            ctx.handle.set_input(edited_content);
            (
                MessageStyle::Info,
                "Editor closed. Input updated with edited content.".to_owned(),
            )
        }
        Ok(None) => (MessageStyle::Info, "Editor closed.".to_owned()),
        Err(err) => (
            MessageStyle::Error,
            format!("Failed to launch editor: {}", err),
        ),
    };

    ctx.handle.force_redraw();
    ctx.renderer.line(message_style, &message)?;
    ctx.renderer.line_if_not_empty(MessageStyle::Output)?;
    Ok(SlashCommandControl::Continue)
}

pub(crate) async fn handle_configure_editor(
    ctx: &mut SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    let editor_config = ctx
        .vt_cfg
        .as_ref()
        .map(|config| config.tools.editor.clone())
        .unwrap_or_default();
    let current_preset = EditorPreset::from_saved(&editor_config.preferred_editor);

    let steps = build_editor_config_steps(&editor_config, current_preset);
    let outcome = show_wizard_modal_and_wait(
        ctx.handle,
        ctx.session,
        EXTERNAL_EDITOR_TITLE.to_string(),
        steps,
        0,
        None,
        WizardModalMode::MultiStep,
        ctx.ctrl_c_state,
        ctx.ctrl_c_notify,
    )
    .await?;

    let WizardModalOutcome::Submitted(selections) = outcome else {
        return Ok(SlashCommandControl::Continue);
    };
    let Some(mut choices) = parse_editor_workflow_answers(&selections) else {
        return Ok(SlashCommandControl::Continue);
    };

    if choices.preset == EditorPreset::Custom {
        let custom_command = prompt_for_custom_editor_command(ctx, &editor_config).await?;
        let Some(custom_command) = custom_command else {
            return Ok(SlashCommandControl::Continue);
        };
        choices.custom_command = Some(custom_command);
    }

    let open_file_opener_settings = choices.open_file_opener_settings;
    persist_editor_workflow_choices(&ctx.config.workspace, choices)?;
    refresh_runtime_config_from_manager(
        ctx.renderer,
        ctx.handle,
        ctx.config,
        ctx.vt_cfg,
        ctx.provider_client.as_ref(),
        ctx.session_bootstrap,
        ctx.full_auto,
    )
    .await?;

    ctx.renderer
        .line(MessageStyle::Info, "Saved external editor settings.")?;
    ctx.renderer.line(
        MessageStyle::Output,
        "Use `/config file_opener` if you also want terminal hyperlinks to target a specific editor URI scheme.",
    )?;
    ctx.renderer.line_if_not_empty(MessageStyle::Output)?;

    if open_file_opener_settings {
        let workspace_path = ctx.config.workspace.clone();
        let vt_snapshot = ctx.vt_cfg.clone();
        let mut settings_state = create_settings_palette_state(&workspace_path, &vt_snapshot)?;
        settings_state.view_path = Some(resolve_settings_view_path(FILE_OPENER_SETTINGS_PATH));
        if show_settings_palette(ctx.renderer, &settings_state, None)? {
            *ctx.palette_state = Some(ActivePalette::Settings {
                state: Box::new(settings_state),
                esc_armed: false,
            });
        }
    }

    Ok(SlashCommandControl::Continue)
}

pub(crate) async fn handle_launch_git(ctx: SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    use vtcode_core::tools::terminal_app::TerminalAppLauncher;

    let launcher = TerminalAppLauncher::new(ctx.config.workspace.clone());

    ctx.renderer
        .line(MessageStyle::Info, "Launching git interface (lazygit)...")?;

    let (message_style, message) =
        match run_with_event_loop_suspended(ctx.handle, true, || launcher.launch_git_interface())
            .await
        {
            Ok(()) => (MessageStyle::Info, "Git interface closed.".to_owned()),
            Err(err) => (
                MessageStyle::Error,
                format!("Failed to launch git interface: {}", err),
            ),
        };

    ctx.handle.force_redraw();
    ctx.renderer.line(message_style, &message)?;
    ctx.renderer.line_if_not_empty(MessageStyle::Output)?;
    Ok(SlashCommandControl::Continue)
}

pub(crate) async fn run_with_event_loop_suspended<T, F>(
    handle: &InlineHandle,
    suspend_tui: bool,
    launch: F,
) -> T
where
    F: FnOnce() -> T,
{
    if suspend_tui {
        handle.suspend_event_loop();
        tokio::time::sleep(EXTERNAL_APP_EVENT_LOOP_SETTLE_DELAY).await;
        handle.clear_input_queue();
    }

    let result = launch();

    if suspend_tui {
        handle.clear_input_queue();
        handle.resume_event_loop();
    }

    result
}

fn should_wait_for_editor(opening_existing_file: bool, editor_config: &EditorToolConfig) -> bool {
    if !opening_existing_file {
        return true;
    }

    editor_config.suspend_tui && editor_likely_requires_terminal(editor_config)
}

fn editor_likely_requires_terminal(editor_config: &EditorToolConfig) -> bool {
    editor_command_from_settings(editor_config)
        .as_deref()
        .is_some_and(editor_command_requires_terminal)
}

fn editor_command_from_settings(editor_config: &EditorToolConfig) -> Option<String> {
    if !editor_config.preferred_editor.trim().is_empty() {
        return Some(editor_config.preferred_editor.trim().to_string());
    }

    ["VISUAL", "EDITOR"]
        .into_iter()
        .find_map(|key| std::env::var(key).ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn editor_command_requires_terminal(command: &str) -> bool {
    let Some(program) = shell_words::split(command)
        .ok()
        .and_then(|tokens| tokens.first().cloned())
    else {
        return false;
    };
    let normalized = Path::new(&program)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(&program)
        .to_ascii_lowercase();

    matches!(
        normalized.as_str(),
        "vi" | "vim" | "nvim" | "nano" | "emacs" | "pico" | "hx" | "helix"
    )
}

fn launch_config_from_settings(
    editor_config: &EditorToolConfig,
    wait_for_editor: bool,
) -> EditorLaunchConfig {
    EditorLaunchConfig {
        preferred_editor: (!editor_config.preferred_editor.trim().is_empty())
            .then(|| editor_config.preferred_editor.clone()),
        wait_for_editor,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EditorPreset {
    Auto,
    Vscode,
    Zed,
    Neovim,
    Vim,
    SublimeText,
    Custom,
}

const EDITOR_PRESET_CHOICES: [EditorPreset; 7] = [
    EditorPreset::Auto,
    EditorPreset::Vscode,
    EditorPreset::Zed,
    EditorPreset::Neovim,
    EditorPreset::Vim,
    EditorPreset::SublimeText,
    EditorPreset::Custom,
];

impl EditorPreset {
    fn from_saved(preferred_editor: &str) -> Self {
        let trimmed = preferred_editor.trim();
        if trimmed.is_empty() {
            return Self::Auto;
        }

        let program = trimmed.split_whitespace().next().unwrap_or_default();
        let program = Path::new(program)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(trimmed)
            .to_ascii_lowercase();

        match program.as_str() {
            "code" | "code-insiders" => Self::Vscode,
            "zed" => Self::Zed,
            "nvim" => Self::Neovim,
            "vim" | "vi" => Self::Vim,
            "subl" => Self::SublimeText,
            _ => Self::Custom,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto (VS Code first)",
            Self::Vscode => "VS Code",
            Self::Zed => "Zed",
            Self::Neovim => "Neovim",
            Self::Vim => "Vim",
            Self::SublimeText => "Sublime Text",
            Self::Custom => "Custom",
        }
    }

    fn value(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Vscode => "vscode",
            Self::Zed => "zed",
            Self::Neovim => "neovim",
            Self::Vim => "vim",
            Self::SublimeText => "sublime_text",
            Self::Custom => "custom",
        }
    }

    fn from_value(value: &str) -> Option<Self> {
        match value {
            "auto" => Some(Self::Auto),
            "vscode" => Some(Self::Vscode),
            "zed" => Some(Self::Zed),
            "neovim" => Some(Self::Neovim),
            "vim" => Some(Self::Vim),
            "sublime_text" => Some(Self::SublimeText),
            "custom" => Some(Self::Custom),
            _ => None,
        }
    }

    fn default_command(self) -> Option<&'static str> {
        match self {
            Self::Auto | Self::Custom => None,
            Self::Vscode => Some("code --wait"),
            Self::Zed => Some("zed"),
            Self::Neovim => Some("nvim"),
            Self::Vim => Some("vim"),
            Self::SublimeText => Some("subl -w"),
        }
    }

    fn picker_subtitle(self) -> String {
        match self {
            Self::Auto => {
                "Use env vars when set; otherwise probe installed editors with VS Code first."
                    .to_string()
            }
            Self::Custom => "Enter a raw editor command in the next step.".to_string(),
            _ => format!(
                "Save `{}` as the preferred editor command.",
                self.default_command().unwrap_or_default()
            ),
        }
    }

    fn preferred_editor(self, custom_command: Option<String>) -> String {
        match self {
            Self::Auto => String::new(),
            Self::Vscode | Self::Zed | Self::Neovim | Self::Vim | Self::SublimeText => {
                self.default_command().unwrap_or_default().to_string()
            }
            Self::Custom => custom_command.unwrap_or_default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EditorWorkflowChoices {
    enabled: bool,
    preset: EditorPreset,
    custom_command: Option<String>,
    suspend_tui: bool,
    open_file_opener_settings: bool,
}

fn build_editor_config_steps(
    editor_config: &EditorToolConfig,
    current_preset: EditorPreset,
) -> Vec<WizardStep> {
    vec![
        WizardStep {
            title: "Enable".to_string(),
            question: format!(
                "External editor launch is currently {}.",
                if editor_config.enabled {
                    "enabled"
                } else {
                    "disabled"
                }
            ),
            items: vec![
                request_choice_item(
                    EDITOR_ENABLED_ID,
                    "Enabled",
                    "Allow `/edit` and single-click file links to open in the configured editor.",
                    WORKFLOW_ENABLED,
                ),
                request_choice_item(
                    EDITOR_ENABLED_ID,
                    "Disabled",
                    "Keep editor settings saved, but block external editor launching.",
                    WORKFLOW_DISABLED,
                ),
            ],
            completed: false,
            answer: Some(InlineListSelection::RequestUserInputAnswer {
                question_id: EDITOR_ENABLED_ID.to_string(),
                selected: vec![if editor_config.enabled {
                    WORKFLOW_ENABLED.to_string()
                } else {
                    WORKFLOW_DISABLED.to_string()
                }],
                other: None,
            }),
            allow_freeform: false,
            freeform_label: None,
            freeform_placeholder: None,
            freeform_default: None,
        },
        WizardStep {
            title: "Preset".to_string(),
            question: format!("Current editor preset: {}.", current_preset.label()),
            items: EDITOR_PRESET_CHOICES
                .into_iter()
                .map(preset_choice_item)
                .collect(),
            completed: false,
            answer: Some(InlineListSelection::RequestUserInputAnswer {
                question_id: EDITOR_PRESET_ID.to_string(),
                selected: vec![current_preset.value().to_string()],
                other: None,
            }),
            allow_freeform: false,
            freeform_label: None,
            freeform_placeholder: None,
            freeform_default: None,
        },
        WizardStep {
            title: "Suspend TUI".to_string(),
            question: format!(
                "Suspending the event loop is currently {}.",
                if editor_config.suspend_tui {
                    "on"
                } else {
                    "off"
                }
            ),
            items: vec![
                request_choice_item(
                    EDITOR_SUSPEND_ID,
                    "Suspend TUI",
                    "Pause the TUI event loop while the external editor is active.",
                    WORKFLOW_ENABLED,
                ),
                request_choice_item(
                    EDITOR_SUSPEND_ID,
                    "Keep TUI live",
                    "Do not suspend the event loop while the editor is open.",
                    WORKFLOW_DISABLED,
                ),
            ],
            completed: false,
            answer: Some(InlineListSelection::RequestUserInputAnswer {
                question_id: EDITOR_SUSPEND_ID.to_string(),
                selected: vec![if editor_config.suspend_tui {
                    WORKFLOW_ENABLED.to_string()
                } else {
                    WORKFLOW_DISABLED.to_string()
                }],
                other: None,
            }),
            allow_freeform: false,
            freeform_label: None,
            freeform_placeholder: None,
            freeform_default: None,
        },
        WizardStep {
            title: "Next".to_string(),
            question: "Do you also want to jump to terminal hyperlink settings after saving?"
                .to_string(),
            items: vec![
                request_choice_item(
                    EDITOR_FOLLOW_UP_ID,
                    "Done",
                    "Save these editor settings and return to the session.",
                    WORKFLOW_DONE,
                ),
                request_choice_item(
                    EDITOR_FOLLOW_UP_ID,
                    "Open File Opener",
                    "Save these settings, then open `/config file_opener` for terminal hyperlink behavior.",
                    WORKFLOW_FILE_OPENER,
                ),
            ],
            completed: false,
            answer: Some(InlineListSelection::RequestUserInputAnswer {
                question_id: EDITOR_FOLLOW_UP_ID.to_string(),
                selected: vec![WORKFLOW_DONE.to_string()],
                other: None,
            }),
            allow_freeform: false,
            freeform_label: None,
            freeform_placeholder: None,
            freeform_default: None,
        },
    ]
}

async fn prompt_for_custom_editor_command(
    ctx: &mut SlashCommandContext<'_>,
    editor_config: &EditorToolConfig,
) -> Result<Option<String>> {
    let placeholder = if editor_config.preferred_editor.trim().is_empty() {
        EditorPreset::Vscode
            .default_command()
            .unwrap_or_default()
            .to_string()
    } else {
        editor_config.preferred_editor.clone()
    };
    let step = build_custom_editor_command_step(placeholder);

    let outcome = show_wizard_modal_and_wait(
        ctx.handle,
        ctx.session,
        EXTERNAL_EDITOR_TITLE.to_string(),
        vec![step],
        0,
        None,
        WizardModalMode::MultiStep,
        ctx.ctrl_c_state,
        ctx.ctrl_c_notify,
    )
    .await?;

    let (value, submitted) = match outcome {
        WizardModalOutcome::Submitted(selections) => (
            selections
                .into_iter()
                .find_map(|selection| match selection {
                    InlineListSelection::RequestUserInputAnswer {
                        question_id,
                        selected,
                        other,
                    } if question_id == EDITOR_CUSTOM_COMMAND_ID => other
                        .or_else(|| selected.first().cloned())
                        .map(|value| value.trim().to_string())
                        .filter(|value| !value.is_empty()),
                    _ => None,
                }),
            true,
        ),
        WizardModalOutcome::Cancelled { .. } => (None, false),
    };

    if submitted && value.is_none() {
        ctx.renderer.line(
            MessageStyle::Warning,
            "Custom editor command was not saved because the command was empty.",
        )?;
    }

    Ok(value)
}

fn build_custom_editor_command_step(placeholder: String) -> WizardStep {
    WizardStep {
        title: "Custom command".to_string(),
        question: "Enter the raw editor command. Include any flags you want VT Code to keep using."
            .to_string(),
        items: vec![InlineListItem {
            title: "Save command".to_string(),
            subtitle: Some("Press Tab to type the command, then Enter to save.".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::RequestUserInputAnswer {
                question_id: EDITOR_CUSTOM_COMMAND_ID.to_string(),
                selected: vec![],
                other: Some(String::new()),
            }),
            search_value: Some("editor command custom raw command".to_string()),
        }],
        completed: false,
        answer: None,
        allow_freeform: true,
        freeform_label: Some("Editor command".to_string()),
        freeform_placeholder: Some(placeholder.clone()),
        freeform_default: Some(placeholder),
    }
}

fn parse_editor_workflow_answers(
    selections: &[InlineListSelection],
) -> Option<EditorWorkflowChoices> {
    let enabled = request_answer_value(selections, EDITOR_ENABLED_ID)?;
    let preset = EditorPreset::from_value(&request_answer_value(selections, EDITOR_PRESET_ID)?)?;
    let suspend_tui = request_answer_value(selections, EDITOR_SUSPEND_ID)?;
    let follow_up = request_answer_value(selections, EDITOR_FOLLOW_UP_ID)?;

    Some(EditorWorkflowChoices {
        enabled: enabled == WORKFLOW_ENABLED,
        preset,
        custom_command: None,
        suspend_tui: suspend_tui == WORKFLOW_ENABLED,
        open_file_opener_settings: follow_up == WORKFLOW_FILE_OPENER,
    })
}

fn preset_choice_item(preset: EditorPreset) -> InlineListItem {
    let subtitle = preset.picker_subtitle();
    request_choice_item(EDITOR_PRESET_ID, preset.label(), &subtitle, preset.value())
}

fn request_answer_value(selections: &[InlineListSelection], question_id: &str) -> Option<String> {
    selections.iter().find_map(|selection| match selection {
        InlineListSelection::RequestUserInputAnswer {
            question_id: current_id,
            selected,
            other,
        } if current_id == question_id => other
            .as_ref()
            .filter(|value| !value.trim().is_empty())
            .cloned()
            .or_else(|| selected.first().cloned()),
        _ => None,
    })
}

fn request_choice_item(
    question_id: &str,
    title: &str,
    subtitle: &str,
    value: &str,
) -> InlineListItem {
    InlineListItem {
        title: title.to_string(),
        subtitle: Some(subtitle.to_string()),
        badge: None,
        indent: 0,
        selection: Some(InlineListSelection::RequestUserInputAnswer {
            question_id: question_id.to_string(),
            selected: vec![value.to_string()],
            other: None,
        }),
        search_value: Some(format!("{title} {subtitle} {value}")),
    }
}

fn persist_editor_workflow_choices(
    workspace: &std::path::Path,
    choices: EditorWorkflowChoices,
) -> Result<()> {
    let mut manager = ConfigManager::load_from_workspace(workspace)?;
    let mut config = manager.config().clone();
    config.tools.editor.enabled = choices.enabled;
    config.tools.editor.preferred_editor = choices.preset.preferred_editor(choices.custom_command);
    config.tools.editor.suspend_tui = choices.suspend_tui;
    manager.save_config(&config)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn editor_preset_detects_known_commands() {
        assert_eq!(EditorPreset::from_saved(""), EditorPreset::Auto);
        assert_eq!(
            EditorPreset::from_saved("code --wait"),
            EditorPreset::Vscode
        );
        assert_eq!(EditorPreset::from_saved("zed"), EditorPreset::Zed);
        assert_eq!(EditorPreset::from_saved("nvim"), EditorPreset::Neovim);
        assert_eq!(EditorPreset::from_saved("vim"), EditorPreset::Vim);
        assert_eq!(
            EditorPreset::from_saved("subl -w"),
            EditorPreset::SublimeText
        );
        assert_eq!(
            EditorPreset::from_saved("/opt/custom/editor --flag"),
            EditorPreset::Custom
        );
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

        let manager =
            ConfigManager::load_from_workspace(workspace.path()).expect("load saved config");
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
        assert!(!editor_command_requires_terminal(""));
    }

    #[test]
    fn custom_editor_command_step_uses_placeholder_as_default() {
        let step = build_custom_editor_command_step("code --wait".to_string());

        assert_eq!(step.freeform_placeholder.as_deref(), Some("code --wait"));
        assert_eq!(step.freeform_default.as_deref(), Some("code --wait"));
    }
}
