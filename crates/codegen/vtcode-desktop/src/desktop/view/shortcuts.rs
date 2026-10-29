//! Configurable desktop commands. Text editing and Kit focus bindings retain their own contexts.
use super::*;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::kbd::Kbd;
use std::collections::{BTreeMap, HashSet};

/// Stable shortcut-command identity.
///
/// Ids persist in preferences (`Preferences::shortcuts`), so never rename an
/// existing id: [`COMMANDS`] pins the full id table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CommandId {
    Commands,
    ConversationSearch,
    NewSession,
    CloseSession,
    Workspace,
    Composer,
    Sidebar,
    Inspector,
    ZenMode,
    Cancel,
    Dismiss,
    Settings,
    SettingsSearch,
    Model,
    Permissions,
    AgentMode,
    AddContext,
    StatusBar,
    ReloadCliSettings,
    SyncCliTheme,
    Utilities,
    WorkspaceFiles,
    WorkspaceReview,
    RefreshWorkspace,
    NavigateBack,
    NavigateForward,
    NextReviewChange,
    PreviousReviewChange,
    NextReviewFile,
    PreviousReviewFile,
}

impl CommandId {
    const ALL: [Self; 30] = [
        Self::Commands,
        Self::ConversationSearch,
        Self::NewSession,
        Self::CloseSession,
        Self::Workspace,
        Self::Composer,
        Self::Sidebar,
        Self::Inspector,
        Self::ZenMode,
        Self::Cancel,
        Self::Dismiss,
        Self::Settings,
        Self::SettingsSearch,
        Self::Model,
        Self::Permissions,
        Self::AgentMode,
        Self::AddContext,
        Self::StatusBar,
        Self::ReloadCliSettings,
        Self::SyncCliTheme,
        Self::Utilities,
        Self::WorkspaceFiles,
        Self::WorkspaceReview,
        Self::RefreshWorkspace,
        Self::NavigateBack,
        Self::NavigateForward,
        Self::NextReviewChange,
        Self::PreviousReviewChange,
        Self::NextReviewFile,
        Self::PreviousReviewFile,
    ];

    const fn as_str(self) -> &'static str {
        match self {
            Self::Commands => "commands",
            Self::ConversationSearch => "conversation_search",
            Self::NewSession => "new_session",
            Self::CloseSession => "close_session",
            Self::Workspace => "workspace",
            Self::Composer => "composer",
            Self::Sidebar => "sidebar",
            Self::Inspector => "inspector",
            Self::ZenMode => "zen_mode",
            Self::Cancel => "cancel",
            Self::Dismiss => "dismiss",
            Self::Settings => "settings",
            Self::SettingsSearch => "settings_search",
            Self::Model => "model",
            Self::Permissions => "permissions",
            Self::AgentMode => "agent_mode",
            Self::AddContext => "add_context",
            Self::StatusBar => "status_bar",
            Self::ReloadCliSettings => "reload_cli_settings",
            Self::SyncCliTheme => "sync_cli_theme",
            Self::Utilities => "utilities",
            Self::WorkspaceFiles => "workspace_files",
            Self::WorkspaceReview => "workspace_review",
            Self::RefreshWorkspace => "refresh_workspace",
            Self::NavigateBack => "navigate_back",
            Self::NavigateForward => "navigate_forward",
            Self::NextReviewChange => "next_review_change",
            Self::PreviousReviewChange => "previous_review_change",
            Self::NextReviewFile => "next_review_file",
            Self::PreviousReviewFile => "previous_review_file",
        }
    }

    fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|known| id == known.as_str())
    }
}

/// One configurable desktop command: stable id, settings label, default binding.
#[derive(Clone, Copy, Debug)]
struct ShortcutCommand {
    id: CommandId,
    label: &'static str,
    default_binding: &'static str,
}

const COMMANDS: [ShortcutCommand; 30] = [
    ShortcutCommand {
        id: CommandId::Commands,
        label: "Search commands",
        default_binding: "cmd-k",
    },
    ShortcutCommand {
        id: CommandId::ConversationSearch,
        label: "Find in conversation",
        default_binding: "cmd-shift-f",
    },
    ShortcutCommand {
        id: CommandId::NewSession,
        label: "New thread",
        default_binding: "cmd-n",
    },
    ShortcutCommand {
        id: CommandId::CloseSession,
        label: "Close thread",
        default_binding: "cmd-shift-w",
    },
    ShortcutCommand {
        id: CommandId::Workspace,
        label: "Choose workspace",
        default_binding: "cmd-o",
    },
    ShortcutCommand {
        id: CommandId::Composer,
        label: "Focus composer",
        default_binding: "cmd-l",
    },
    ShortcutCommand {
        id: CommandId::Sidebar,
        label: "Toggle navigation",
        default_binding: "cmd-b",
    },
    ShortcutCommand {
        id: CommandId::Inspector,
        label: "Toggle inspector",
        default_binding: "cmd-shift-i",
    },
    ShortcutCommand {
        id: CommandId::ZenMode,
        label: "Toggle Zen mode",
        default_binding: "ctrl-cmd-z",
    },
    ShortcutCommand {
        id: CommandId::Cancel,
        label: "Cancel run",
        default_binding: "cmd-.",
    },
    ShortcutCommand {
        id: CommandId::Dismiss,
        label: "Dismiss overlay",
        default_binding: "escape",
    },
    ShortcutCommand {
        id: CommandId::Settings,
        label: "Settings",
        default_binding: "cmd-,",
    },
    ShortcutCommand {
        id: CommandId::SettingsSearch,
        label: "Search settings and CLI config",
        default_binding: "cmd-f",
    },
    ShortcutCommand {
        id: CommandId::Model,
        label: "Choose model",
        default_binding: "cmd-shift-m",
    },
    ShortcutCommand {
        id: CommandId::Permissions,
        label: "Command permissions",
        default_binding: "cmd-shift-p",
    },
    ShortcutCommand {
        id: CommandId::AgentMode,
        label: "Choose agent mode",
        default_binding: "cmd-shift-b",
    },
    ShortcutCommand {
        id: CommandId::AddContext,
        label: "Add files and more @",
        default_binding: "cmd-shift-a",
    },
    ShortcutCommand {
        id: CommandId::StatusBar,
        label: "Toggle status bar",
        default_binding: "cmd-shift-s",
    },
    ShortcutCommand {
        id: CommandId::ReloadCliSettings,
        label: "Reload CLI settings",
        default_binding: "cmd-shift-r",
    },
    ShortcutCommand {
        id: CommandId::SyncCliTheme,
        label: "Sync current theme to CLI",
        default_binding: "cmd-shift-t",
    },
    ShortcutCommand {
        id: CommandId::Utilities,
        label: "Open workspace utilities",
        default_binding: "cmd-shift-u",
    },
    ShortcutCommand {
        id: CommandId::WorkspaceFiles,
        label: "Open workspace files",
        default_binding: "cmd-shift-e",
    },
    ShortcutCommand {
        id: CommandId::WorkspaceReview,
        label: "Review working tree",
        default_binding: "cmd-shift-g",
    },
    ShortcutCommand {
        id: CommandId::RefreshWorkspace,
        label: "Refresh current workspace view",
        default_binding: "cmd-r",
    },
    ShortcutCommand {
        id: CommandId::NavigateBack,
        label: "Navigate back",
        default_binding: "cmd-[",
    },
    ShortcutCommand {
        id: CommandId::NavigateForward,
        label: "Navigate forward",
        default_binding: "cmd-]",
    },
    ShortcutCommand {
        id: CommandId::NextReviewChange,
        label: "Next review change",
        default_binding: "f7",
    },
    ShortcutCommand {
        id: CommandId::PreviousReviewChange,
        label: "Previous review change",
        default_binding: "shift-f7",
    },
    ShortcutCommand {
        id: CommandId::NextReviewFile,
        label: "Next changed file",
        default_binding: "alt-f7",
    },
    ShortcutCommand {
        id: CommandId::PreviousReviewFile,
        label: "Previous changed file",
        default_binding: "alt-shift-f7",
    },
];

fn action(id: CommandId) -> Box<dyn gpui_kit::Action> {
    match id {
        CommandId::Commands => Box::new(OpenCommandPalette),
        CommandId::ConversationSearch => Box::new(OpenConversationSearch),
        CommandId::NewSession => Box::new(NewSession),
        CommandId::CloseSession => Box::new(CloseSession),
        CommandId::Workspace => Box::new(ChooseWorkspace),
        CommandId::Composer => Box::new(FocusComposer),
        CommandId::Sidebar => Box::new(ToggleSidebar),
        CommandId::Inspector => Box::new(ToggleInspector),
        CommandId::ZenMode => Box::new(ToggleZenMode),
        CommandId::Cancel => Box::new(CancelRun),
        CommandId::Dismiss => Box::new(DismissOverlay),
        CommandId::Settings => Box::new(OpenSettings),
        CommandId::SettingsSearch => Box::new(OpenSettingsSearch),
        CommandId::Model => Box::new(OpenModelPicker),
        CommandId::Permissions => Box::new(OpenPermissions),
        CommandId::AgentMode => Box::new(OpenModePicker),
        CommandId::AddContext => Box::new(OpenAddContext),
        CommandId::StatusBar => Box::new(ToggleStatusBar),
        CommandId::ReloadCliSettings => Box::new(ReloadCliSettings),
        CommandId::SyncCliTheme => Box::new(SyncCliTheme),
        CommandId::Utilities => Box::new(OpenUtilities),
        CommandId::WorkspaceFiles => Box::new(OpenWorkspaceFiles),
        CommandId::WorkspaceReview => Box::new(OpenWorkspaceReview),
        CommandId::RefreshWorkspace => Box::new(RefreshWorkspace),
        CommandId::NavigateBack => Box::new(NavigateBack),
        CommandId::NavigateForward => Box::new(NavigateForward),
        CommandId::NextReviewChange => Box::new(NextReviewChange),
        CommandId::PreviousReviewChange => Box::new(PreviousReviewChange),
        CommandId::NextReviewFile => Box::new(NextReviewFile),
        CommandId::PreviousReviewFile => Box::new(PreviousReviewFile),
    }
}

pub(super) fn bindings(overrides: &BTreeMap<String, String>) -> Result<Vec<KeyBinding>, String> {
    if overrides.keys().any(|key| CommandId::from_id(key).is_none()) {
        return Err("Unknown shortcut action".into());
    }
    let mut seen = HashSet::new();
    COMMANDS
        .iter()
        .map(|command| {
            let source = overrides
                .get(command.id.as_str())
                .map(String::as_str)
                .unwrap_or(command.default_binding);
            let label = command.label;
            if source.is_empty() {
                return Ok(Vec::new());
            }
            if source.trim().is_empty() || source.split_whitespace().count() != 1 {
                return Err(format!("{label}: enter one key combination"));
            }
            let key = gpui_kit::Keystroke::parse(source)
                .map_err(|error| format!("{label}: invalid key combination: {error}"))?;
            let normalized = key.unparse();
            if [
                "cmd-q",
                "cmd-w",
                "cmd-m",
                "cmd-h",
                "alt-cmd-h",
                "cmd-shift-n",
                "cmd-c",
                "cmd-x",
                "cmd-v",
                "cmd-a",
                "cmd-z",
                "cmd-shift-z",
            ]
            .contains(&normalized.as_str())
            {
                return Err(format!("{label}: reserved macOS shortcut"));
            }
            if !(command.id == CommandId::Dismiss && source == "escape")
                && !source.contains("cmd-")
                && !source.contains("ctrl-")
                && !source.contains("alt-")
                && !is_function_key(&normalized)
            {
                return Err(format!("{label}: include cmd, ctrl, alt, or use a function key"));
            }
            if !seen.insert(normalized) {
                return Err(format!("{label}: shortcut is already assigned"));
            }
            let context = gpui_kit::KeyBindingContextPredicate::parse(context_for(command.id))
                .map_err(|error| error.to_string())?;
            let mut bindings = vec![
                KeyBinding::load(
                    source,
                    action(command.id),
                    Some(context.into()),
                    false,
                    None,
                    &gpui_kit::DummyKeyboardMapper,
                )
                .map_err(|error| format!("{label}: invalid shortcut: {error}"))?,
            ];
            if command.id == CommandId::SettingsSearch {
                let context = gpui_kit::KeyBindingContextPredicate::parse(key_context::SETTINGS)
                    .map_err(|error| error.to_string())?;
                bindings.push(
                    KeyBinding::load(
                        source,
                        action(command.id),
                        Some(context.into()),
                        false,
                        None,
                        &gpui_kit::DummyKeyboardMapper,
                    )
                    .map_err(|error| format!("{label}: invalid shortcut: {error}"))?,
                );
            }
            Ok(bindings)
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|bindings| bindings.into_iter().flatten().collect())
}

fn is_function_key(shortcut: &str) -> bool {
    shortcut
        .rsplit('-')
        .next()
        .and_then(|key| key.strip_prefix('f'))
        .and_then(|number| number.parse::<u8>().ok())
        .is_some_and(|number| (1..=24).contains(&number))
}

fn context_for(id: CommandId) -> &'static str {
    if matches!(
        id,
        CommandId::NextReviewChange
            | CommandId::PreviousReviewChange
            | CommandId::NextReviewFile
            | CommandId::PreviousReviewFile
    ) {
        key_context::DIFF_REVIEW
    } else {
        key_context::DESKTOP
    }
}

pub(super) fn inputs(
    overrides: &BTreeMap<String, String>,
    window: &mut Window,
    cx: &mut Context<DesktopView>,
) -> Vec<Entity<InputState>> {
    COMMANDS
        .iter()
        .map(|command| {
            cx.new(|cx| {
                InputState::new(window, cx).default_value(
                    overrides
                        .get(command.id.as_str())
                        .map(String::as_str)
                        .unwrap_or(command.default_binding),
                )
            })
        })
        .collect()
}

impl DesktopView {
    pub(super) fn shortcut_values(&self, cx: &App) -> BTreeMap<String, String> {
        COMMANDS
            .iter()
            .zip(&self.shortcut_inputs)
            .map(|(command, input)| (command.id.as_str().to_string(), input.read(cx).value().to_string()))
            .collect()
    }

    pub(super) fn render_shortcuts(&self, window: &Window, cx: &Context<Self>) -> AnyElement {
        let description = if self.shortcuts_editing {
            "Change a binding or leave it empty to unbind it. Saved changes apply after restarting VT Code Desktop."
        } else {
            "Current desktop bindings. Select Edit shortcuts to customize them."
        };
        let cli_shortcuts_available = self
            .configuration
            .as_ref()
            .and_then(|configuration| configuration.shared_ui.as_ref())
            .is_some();
        let import_button = desktop_button("import-cli-shortcuts")
            .label("Import from CLI")
            .ghost()
            .desktop_disabled(!cli_shortcuts_available)
            .on_click(cx.listener(|view, _, window, cx| view.import_cli_shortcuts(window, cx)));
        let mut actions = div().flex().flex_wrap().gap_2();
        if self.shortcuts_editing {
            actions = actions
                .child(desktop_button("save-shortcuts").label("Save").primary().on_click(cx.listener(
                    |view, _, window, cx| {
                        let candidate = view.shortcut_values(cx);
                        match bindings(&candidate) {
                            Ok(_) => {
                                let mut preferences = view.preferences.clone();
                                preferences.shortcuts = candidate;
                                match preferences.try_save() {
                                    Ok(()) => {
                                        view.preferences = preferences;
                                        view.shortcuts_editing = false;
                                        view.settings_navigation_focus.focus(window, cx);
                                        view.shortcut_message =
                                            Some("Shortcuts saved. Restart VT Code Desktop to apply.".into());
                                    }
                                    Err(error) => view.shortcut_message = Some(error),
                                }
                            }
                            Err(error) => view.shortcut_message = Some(error),
                        }
                        cx.notify();
                    },
                )))
                .child(
                    desktop_button("cancel-shortcuts")
                        .label("Cancel")
                        .ghost()
                        .on_click(cx.listener(|view, _, window, cx| view.cancel_shortcut_edit(window, cx))),
                )
                .child(
                    desktop_button("reset-shortcuts")
                        .label("Reset defaults")
                        .ghost()
                        .on_click(cx.listener(|view, _, window, cx| view.reset_shortcuts(window, cx))),
                );
        } else {
            actions = actions.child(
                desktop_button("edit-shortcuts")
                    .label("Edit shortcuts")
                    .ghost()
                    .on_click(cx.listener(|view, _, window, cx| view.begin_shortcut_edit(window, cx))),
            );
        }
        actions = actions.child(import_button);

        let mut group = settings_group_box("Keyboard shortcuts", &cx.theme().colors)
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().colors.muted_foreground)
                    .child(description),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(div().font_medium().child("Desktop bindings"))
                    .child(actions),
            )
            .children(COMMANDS.iter().zip(self.shortcut_inputs.iter()).map(|(command, input)| {
                let value = input.read(cx).value().to_string();
                let current_binding = active_binding(command.id, window);
                let value_element = if self.shortcuts_editing {
                    div()
                        .child(
                            Input::new(input)
                                .w(px(220.))
                                .max_w_full()
                                .cursor_text()
                                .bg(cx.theme().colors.popover)
                                .id(format!("shortcut-input-{}", command.id.as_str()))
                                .accessibility_id(format!("shortcut-{}", command.id.as_str()))
                                .aria_label(format!("Shortcut for {}", command.label)),
                        )
                        .into_any_element()
                } else {
                    shortcut_badge(command.id, command.label, current_binding.as_ref())
                };
                let accessible_shortcut = if self.shortcuts_editing {
                    shortcut_accessibility_value(&value)
                } else {
                    current_binding.as_ref().map(Kbd::format).unwrap_or_else(|| "not set".into())
                };
                div()
                    .id(format!("shortcut-row-{}", command.id.as_str()))
                    .test_support()
                    .role(gpui_kit::Role::Group)
                    .aria_label(format!("{}, shortcut {accessible_shortcut}", command.label))
                    .w_full()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .py_1()
                    .child(command.label)
                    .child(value_element)
            }))
            .children(self.shortcut_message.as_ref().map(|message| {
                div()
                    .id("shortcut-status")
                    .role(gpui_kit::Role::Status)
                    .aria_label(message.clone())
                    .text_sm()
                    .child(message.clone())
            }));

        if self.shortcuts_editing {
            group = group.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().colors.muted_foreground)
                    .child("Use cmd, ctrl, or alt with a key. Tab and Shift+Tab move between controls."),
            );
        }
        accessible_group_box("keyboard-shortcuts-group", "Keyboard shortcuts", group)
    }

    fn begin_shortcut_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.shortcut_inputs = inputs(&self.preferences.shortcuts, window, cx);
        self.shortcut_message = None;
        self.shortcuts_editing = true;
        if let Some(input) = self.shortcut_inputs.first().cloned() {
            input.update(cx, |input, cx| input.focus(window, cx));
        }
        cx.notify();
    }

    fn cancel_shortcut_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.shortcut_inputs = inputs(&self.preferences.shortcuts, window, cx);
        self.shortcut_message = None;
        self.shortcuts_editing = false;
        self.settings_navigation_focus.focus(window, cx);
        cx.notify();
    }

    fn reset_shortcuts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut preferences = self.preferences.clone();
        preferences.shortcuts.clear();
        if let Err(error) = preferences.try_save() {
            self.shortcut_message = Some(error);
            cx.notify();
            return;
        }
        self.preferences = preferences;
        self.shortcut_inputs = inputs(&BTreeMap::new(), window, cx);
        self.shortcut_message = Some("Defaults saved. Restart VT Code Desktop to apply.".into());
        self.shortcuts_editing = false;
        self.settings_navigation_focus.focus(window, cx);
        cx.notify();
    }
}

fn active_binding(id: CommandId, window: &Window) -> Option<gpui_kit::Keystroke> {
    let context = gpui_kit::KeyContext::parse(context_for(id)).ok()?;
    let action = action(id);
    window
        .highest_precedence_binding_for_action_in_context(action.as_ref(), context)?
        .keystrokes()
        .first()
        .map(|keystroke| gpui_kit::AsKeystroke::as_keystroke(keystroke).clone())
}

fn shortcut_badge(id: CommandId, label: &str, binding: Option<&gpui_kit::Keystroke>) -> AnyElement {
    let badge = match binding {
        Some(keystroke) => div().child(Kbd::new(keystroke.clone()).outline()),
        None => div().text_xs().child("Not set"),
    };
    div()
        .id(format!("shortcut-badge-{}", id.as_str()))
        .test_support()
        .role(gpui_kit::Role::Label)
        .aria_label(format!("Shortcut for {label}: {}", binding.map(Kbd::format).unwrap_or_else(|| "not set".into())))
        .child(badge)
        .into_any_element()
}

fn shortcut_accessibility_value(value: &str) -> String {
    if value.is_empty() {
        return "not set".into();
    }

    gpui_kit::Keystroke::parse(value)
        .map(|keystroke| Kbd::format(&keystroke))
        .unwrap_or_else(|_| "invalid shortcut".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accessible_shortcut_labels_use_the_platform_formatted_key() {
        let keystroke = gpui_kit::Keystroke::parse("cmd-shift-p").unwrap();
        assert_eq!(shortcut_accessibility_value("cmd-shift-p"), Kbd::format(&keystroke));
        assert_eq!(shortcut_accessibility_value(""), "not set");
        assert_eq!(shortcut_accessibility_value("a-b"), "invalid shortcut");
    }

    #[test]
    fn rejects_conflicting_reserved_typing_and_unknown_shortcuts() {
        assert_eq!(bindings(&BTreeMap::new()).unwrap().len(), 31);
        assert!(bindings(&BTreeMap::from([("commands".into(), "ctrl-shift-k".into())])).is_ok());
        assert!(bindings(&BTreeMap::from([("settings_search".into(), "ctrl-alt-f".into())])).is_ok());
        assert!(bindings(&BTreeMap::from([("zen_mode".into(), "ctrl-alt-z".into())])).is_ok());
        assert_eq!(bindings(&BTreeMap::from([("model".into(), String::new())])).unwrap().len(), 30);
        assert_eq!(
            bindings(&BTreeMap::from([("settings_search".into(), String::new())]))
                .unwrap()
                .len(),
            29
        );
        assert!(bindings(&BTreeMap::from([("dismiss".into(), "x".into())])).is_err());
        for (action, key) in [
            ("commands", "cmd-n"),
            ("commands", "cmd-q"),
            ("commands", "k"),
            ("zen_mode", "cmd-shift-z"),
            ("unknown", "ctrl-k"),
        ] {
            assert!(bindings(&BTreeMap::from([(action.into(), key.into())])).is_err(), "{action}: {key}");
        }
    }

    #[test]
    fn review_navigation_accepts_configurable_function_key_bindings() {
        let configured = BTreeMap::from([
            ("next_review_change".into(), "f8".into()),
            ("previous_review_change".into(), "shift-f8".into()),
            ("next_review_file".into(), "alt-f9".into()),
            ("previous_review_file".into(), "alt-shift-f9".into()),
        ]);
        assert!(bindings(&configured).is_ok());
        assert_eq!(context_for(CommandId::NextReviewChange), "VTCodeDiffReview");
        assert_eq!(context_for(CommandId::PreviousReviewFile), "VTCodeDiffReview");
        assert_eq!(context_for(CommandId::NavigateBack), "VTCodeDesktop");
        assert!(bindings(&BTreeMap::from([("next_review_change".into(), "f25".into())])).is_err());
        assert!(
            bindings(&BTreeMap::from([
                ("next_review_change".into(), "f8".into()),
                ("previous_review_change".into(), "f8".into()),
            ]))
            .is_err()
        );
    }

    #[test]
    fn command_ids_round_trip_and_pin_the_persisted_table() {
        // Ids persist in preferences: this literal table pins the contract so
        // a rename breaks loudly instead of orphaning saved shortcuts.
        let pinned = [
            "commands",
            "conversation_search",
            "new_session",
            "close_session",
            "workspace",
            "composer",
            "sidebar",
            "inspector",
            "zen_mode",
            "cancel",
            "dismiss",
            "settings",
            "settings_search",
            "model",
            "permissions",
            "agent_mode",
            "add_context",
            "status_bar",
            "reload_cli_settings",
            "sync_cli_theme",
            "utilities",
            "workspace_files",
            "workspace_review",
            "refresh_workspace",
            "navigate_back",
            "navigate_forward",
            "next_review_change",
            "previous_review_change",
            "next_review_file",
            "previous_review_file",
        ];
        assert_eq!(CommandId::ALL.map(CommandId::as_str), pinned);
        assert_eq!(COMMANDS.map(|command| command.id), CommandId::ALL);
        for id in CommandId::ALL {
            assert_eq!(CommandId::from_id(id.as_str()), Some(id));
        }
        assert_eq!(CommandId::from_id("unknown"), None);
        assert_eq!(CommandId::from_id(""), None);
        assert_eq!(CommandId::from_id("Commands"), None);
    }
}
