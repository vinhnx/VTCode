//! Configurable desktop commands. Text editing and Kit focus bindings retain their own contexts.
use super::*;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::kbd::Kbd;
use std::collections::{BTreeMap, HashSet};

const COMMANDS: [(&str, &str, &str); 21] = [
    ("commands", "Search commands", "cmd-k"),
    ("new_session", "New thread", "cmd-n"),
    ("close_session", "Close thread", "cmd-shift-w"),
    ("workspace", "Choose workspace", "cmd-o"),
    ("composer", "Focus composer", "cmd-l"),
    ("sidebar", "Toggle navigation", "cmd-b"),
    ("inspector", "Toggle inspector", "cmd-shift-i"),
    ("cancel", "Cancel run", "cmd-."),
    ("dismiss", "Dismiss overlay", "escape"),
    ("settings", "Settings", "cmd-,"),
    ("model", "Choose model", "cmd-shift-m"),
    ("permissions", "Command permissions", "cmd-shift-p"),
    ("agent_mode", "Choose agent mode", "cmd-shift-b"),
    ("add_context", "Add files and more @", "cmd-shift-a"),
    ("status_bar", "Toggle status bar", "cmd-shift-s"),
    ("reload_cli_settings", "Reload CLI settings", "cmd-shift-r"),
    ("sync_cli_theme", "Sync current theme to CLI", "cmd-shift-t"),
    ("utilities", "Open workspace utilities", "cmd-shift-u"),
    ("workspace_files", "Open workspace files", "cmd-shift-e"),
    ("workspace_review", "Review working tree", "cmd-shift-g"),
    ("refresh_workspace", "Refresh current workspace view", "cmd-r"),
];

fn action(id: &str) -> Box<dyn gpui_kit::Action> {
    match id {
        "commands" => Box::new(OpenCommandPalette),
        "new_session" => Box::new(NewSession),
        "close_session" => Box::new(CloseSession),
        "workspace" => Box::new(ChooseWorkspace),
        "composer" => Box::new(FocusComposer),
        "sidebar" => Box::new(ToggleSidebar),
        "inspector" => Box::new(ToggleInspector),
        "cancel" => Box::new(CancelRun),
        "dismiss" => Box::new(DismissOverlay),
        "model" => Box::new(OpenModelPicker),
        "permissions" => Box::new(OpenPermissions),
        "agent_mode" => Box::new(OpenModePicker),
        "add_context" => Box::new(OpenAddContext),
        "status_bar" => Box::new(ToggleStatusBar),
        "reload_cli_settings" => Box::new(ReloadCliSettings),
        "sync_cli_theme" => Box::new(SyncCliTheme),
        "utilities" => Box::new(OpenUtilities),
        "workspace_files" => Box::new(OpenWorkspaceFiles),
        "workspace_review" => Box::new(OpenWorkspaceReview),
        "refresh_workspace" => Box::new(RefreshWorkspace),
        _ => Box::new(OpenSettings),
    }
}

pub(super) fn bindings(overrides: &BTreeMap<String, String>) -> Result<Vec<KeyBinding>, String> {
    if overrides.keys().any(|key| !COMMANDS.iter().any(|(id, _, _)| id == key)) {
        return Err("Unknown shortcut action".into());
    }
    let mut seen = HashSet::new();
    COMMANDS
        .iter()
        .map(|(id, label, default)| {
            let source = overrides.get(*id).map(String::as_str).unwrap_or(default);
            if source.is_empty() {
                return Ok(None);
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
            if !(*id == "dismiss" && source == "escape")
                && !source.contains("cmd-")
                && !source.contains("ctrl-")
                && !source.contains("alt-")
            {
                return Err(format!("{label}: include cmd, ctrl, or alt so typing remains available"));
            }
            if !seen.insert(normalized) {
                return Err(format!("{label}: shortcut is already assigned"));
            }
            let context =
                gpui_kit::KeyBindingContextPredicate::parse("VTCodeDesktop").map_err(|error| error.to_string())?;
            KeyBinding::load(source, action(id), Some(context.into()), false, None, &gpui_kit::DummyKeyboardMapper)
                .map(Some)
                .map_err(|error| format!("{label}: invalid shortcut: {error}"))
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|bindings| bindings.into_iter().flatten().collect())
}

pub(super) fn inputs(
    overrides: &BTreeMap<String, String>,
    window: &mut Window,
    cx: &mut Context<DesktopView>,
) -> Vec<Entity<InputState>> {
    COMMANDS
        .iter()
        .map(|(id, _, default)| {
            cx.new(|cx| {
                InputState::new(window, cx).default_value(overrides.get(*id).map(String::as_str).unwrap_or(default))
            })
        })
        .collect()
}

impl DesktopView {
    pub(super) fn shortcut_values(&self, cx: &App) -> BTreeMap<String, String> {
        COMMANDS
            .iter()
            .zip(&self.shortcut_inputs)
            .map(|((id, _, _), input)| (id.to_string(), input.read(cx).value().to_string()))
            .collect()
    }

    pub(super) fn render_shortcuts(&self, cx: &Context<Self>) -> AnyElement {
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

        let mut group = GroupBox::new()
            .flex_shrink_0()
            .fill()
            .title("Keyboard shortcuts")
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
            .children(COMMANDS.iter().zip(self.shortcut_inputs.iter()).map(|((id, label, _), input)| {
                let value = input.read(cx).value().to_string();
                let value_element = if self.shortcuts_editing {
                    div()
                        .child(
                            Input::new(input)
                                .w(px(220.))
                                .max_w_full()
                                .id(format!("shortcut-input-{id}"))
                                .accessibility_id(format!("shortcut-{id}"))
                                .aria_label(format!("Shortcut for {label}")),
                        )
                        .into_any_element()
                } else {
                    shortcut_badge(id, label, &value)
                };
                div()
                    .id(format!("shortcut-row-{id}"))
                    .test_support()
                    .role(gpui_kit::Role::Group)
                    .aria_label(format!("{label}, shortcut {}", shortcut_accessibility_value(&value)))
                    .w_full()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .py_1()
                    .child(*label)
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

fn shortcut_badge(id: &str, label: &str, value: &str) -> AnyElement {
    let badge = if value.is_empty() {
        div().text_xs().child("Not set")
    } else if let Ok(keystroke) = gpui_kit::Keystroke::parse(value) {
        div().child(Kbd::new(keystroke).outline())
    } else {
        div().text_xs().child("Invalid shortcut")
    };
    div()
        .id(format!("shortcut-badge-{id}"))
        .test_support()
        .role(gpui_kit::Role::Label)
        .aria_label(format!("Shortcut for {label}: {}", shortcut_accessibility_value(value)))
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
        assert_eq!(bindings(&BTreeMap::new()).unwrap().len(), 21);
        assert!(bindings(&BTreeMap::from([("commands".into(), "ctrl-shift-k".into())])).is_ok());
        assert_eq!(bindings(&BTreeMap::from([("model".into(), String::new())])).unwrap().len(), 20);
        assert!(bindings(&BTreeMap::from([("dismiss".into(), "x".into())])).is_err());
        for (action, key) in [
            ("commands", "cmd-n"),
            ("commands", "cmd-q"),
            ("commands", "k"),
            ("unknown", "ctrl-k"),
        ] {
            assert!(bindings(&BTreeMap::from([(action.into(), key.into())])).is_err(), "{action}: {key}");
        }
    }
}
