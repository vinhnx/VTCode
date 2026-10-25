//! Configurable desktop commands. Text editing and Kit focus bindings retain their own contexts.
use super::*;
use gpui_kit::component::input::{Input, InputState};
use std::collections::{BTreeMap, HashSet};

const COMMANDS: [(&str, &str, &str); 17] = [
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
        GroupBox::new().flex_shrink_0().outline().title("Keyboard shortcuts")
            .child(div().text_xs().text_color(cx.theme().colors.muted_foreground)
                .child("Use cmd, ctrl, alt, or shift with a key (for example cmd-shift-k). Leave a field empty to unbind it. Saved changes apply when the app restarts. Tab and Shift+Tab reach controls; Enter or Space activates buttons."))
            .children(COMMANDS.iter().zip(self.shortcut_inputs.iter()).map(|((id, label, _), input)| {
                div().w_full().flex().flex_wrap().items_center().justify_between().gap_2().py_1()
                    .child(*label)
                    .child(Input::new(input).w(px(180.)).max_w_full().accessibility_id(format!("shortcut-{id}")).aria_label(format!("Shortcut for {label}")))
            }))
            .children(self.shortcut_message.as_ref().map(|message| div().id("shortcut-status").role(gpui_kit::Role::Status).aria_label(message.clone()).text_sm().child(message.clone())))
            .child(div().flex().flex_wrap().gap_2()
                .child(desktop_button("save-shortcuts").label("Save shortcuts").ghost().on_click(cx.listener(|view, _, _, cx| {
                    let candidate = view.shortcut_values(cx);
                    match bindings(&candidate) {
                        Ok(_) => {
                            let mut preferences = view.preferences.clone();
                            preferences.shortcuts = candidate;
                            match preferences.try_save() {
                                Ok(()) => { view.preferences = preferences; view.shortcut_message = Some("Shortcuts saved. Restart VT Code Desktop to apply.".into()); }
                                Err(error) => view.shortcut_message = Some(error),
                            }
                        }
                        Err(error) => view.shortcut_message = Some(error),
                    }
                    cx.notify();
                })))
                .child(desktop_button("reset-shortcuts").label("Reset shortcuts").ghost().on_click(cx.listener(|view, _, window, cx| {
                    let mut preferences = view.preferences.clone();
                    preferences.shortcuts.clear();
                    if let Err(error) = preferences.try_save() {
                        view.shortcut_message = Some(error);
                        cx.notify();
                        return;
                    }
                    view.preferences = preferences;
                    for ((_, _, default), input) in COMMANDS.iter().zip(&view.shortcut_inputs) {
                        input.update(cx, |input, cx| input.set_value(*default, window, cx));
                    }
                    view.shortcut_message = Some("Defaults saved. Restart VT Code Desktop to apply.".into());
                    cx.notify();
                }))))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_conflicting_reserved_typing_and_unknown_shortcuts() {
        assert_eq!(bindings(&BTreeMap::new()).unwrap().len(), 17);
        assert!(bindings(&BTreeMap::from([("commands".into(), "ctrl-shift-k".into())])).is_ok());
        assert_eq!(bindings(&BTreeMap::from([("model".into(), String::new())])).unwrap().len(), 16);
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
