//! Searchable, typed edits for the CLI configuration shared with Desktop.

use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ConfigValueKind {
    String,
    Integer,
    Float,
    Array,
    Object,
}

#[derive(Clone, Debug)]
pub(super) struct ConfigEditor {
    path: String,
    kind: ConfigValueKind,
    target: protocol::ConfigTarget,
    expected_layer_version: Option<String>,
}

impl ConfigValueKind {
    fn for_value(value: &Value) -> Option<Self> {
        match value {
            Value::String(_) => Some(Self::String),
            Value::Number(number) if number.is_i64() || number.is_u64() => Some(Self::Integer),
            Value::Number(_) => Some(Self::Float),
            Value::Array(_) => Some(Self::Array),
            Value::Object(_) => Some(Self::Object),
            Value::Null | Value::Bool(_) => None,
        }
    }

    fn editor_value(self, value: &Value) -> String {
        match self {
            Self::String => value.as_str().unwrap_or_default().to_owned(),
            Self::Integer | Self::Float => value.to_string(),
            Self::Array | Self::Object => serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string()),
        }
    }

    fn parse(self, text: &str) -> Result<Value, String> {
        match self {
            Self::String => Ok(Value::String(text.to_owned())),
            Self::Integer => {
                let value = serde_json::from_str::<Value>(text).map_err(|error| error.to_string())?;
                if value.as_number().is_some_and(|number| number.is_i64() || number.is_u64()) {
                    Ok(value)
                } else {
                    Err("Enter a whole number.".into())
                }
            }
            Self::Float => {
                let value = serde_json::from_str::<Value>(text).map_err(|error| error.to_string())?;
                if value.as_number().is_some_and(serde_json::Number::is_f64) {
                    Ok(value)
                } else {
                    Err("Enter a decimal number.".into())
                }
            }
            Self::Array | Self::Object => {
                let value = serde_json::from_str::<Value>(text).map_err(|error| error.to_string())?;
                let matches = match self {
                    Self::Array => value.is_array(),
                    Self::Object => value.is_object(),
                    _ => false,
                };
                if matches {
                    Ok(value)
                } else {
                    Err(match self {
                        Self::Array => "Enter a JSON array.".into(),
                        Self::Object => "Enter a JSON object.".into(),
                        _ => "Unsupported value.".into(),
                    })
                }
            }
        }
    }
}

impl DesktopView {
    pub(super) fn render_cli_configuration(&self, cx: &Context<Self>) -> AnyElement {
        let snapshot = self.cli_config_settings.as_ref();
        let count = snapshot.map_or(0, |snapshot| snapshot.settings.len());
        let mut group = GroupBox::new().flex_shrink_0().fill().title("CLI configuration");
        group = group.child(div().text_sm().child(match snapshot {
            Some(_) => format!("{count} settings · values follow the active CLI config layers"),
            None if self.workspace.is_some() => "Loading shared CLI settings…".into(),
            None => "Choose a workspace to browse CLI settings.".into(),
        }));

        let target_buttons = [
            (protocol::ConfigTarget::User, "User"),
            (protocol::ConfigTarget::Workspace, "Workspace"),
            (protocol::ConfigTarget::Project, "Project"),
        ]
        .into_iter()
        .fold(div().flex().flex_wrap().items_center().gap_2(), |row, (target, label)| {
            let selected = self.config_target == target;
            row.child(
                desktop_button(match target {
                    protocol::ConfigTarget::User => "config-target-user",
                    protocol::ConfigTarget::Workspace => "config-target-workspace",
                    protocol::ConfigTarget::Project => "config-target-project",
                })
                .label(label)
                .accessibility_label(format!(
                    "Write CLI settings to {label} configuration{}",
                    if selected { " (selected)" } else { "" }
                ))
                .when(selected, |button| button.primary())
                .when(!selected, |button| button.ghost())
                .small()
                .desktop_disabled(!self.can_edit_cli_settings())
                .on_click(cx.listener(move |view, _, _, cx| view.set_cli_config_target(target, cx))),
            )
        });

        group = group.child(target_buttons).child(
            desktop_button("browse-cli-config")
                .label("Browse all settings…")
                .accessibility_label("Search and edit CLI configuration settings")
                .ghost()
                .desktop_disabled(snapshot.is_none() || !self.can_edit_cli_settings())
                .on_click(cx.listener(|view, _, window, cx| view.open_cli_config_palette(window, cx))),
        );

        let mut note = "Saving uses VT Code's validated configuration writer and reloads the local CLI service. Higher-precedence settings can still override the selected layer.".to_owned();
        if let Some(snapshot) = snapshot {
            if snapshot.omitted_sensitive > 0 {
                note.push_str(" Credential-bearing fields are excluded; manage credentials with `vtcode auth` or the configured environment variable.");
            }
            if snapshot.omitted_large > 0 || snapshot.omitted_limit > 0 {
                note.push_str(" Some unusually large or numerous settings are omitted from this browser.");
            }
        }
        group = group.footer(note);
        accessible_group_box("cli-configuration-group", "CLI configuration", group)
    }

    pub(super) fn can_edit_cli_settings(&self) -> bool {
        self.workspace.is_some()
            && self.cli_config_settings.is_some()
            && !self.config_write_pending
            && self.can_reload_cli_settings()
    }

    pub(super) fn set_cli_config_target(&mut self, target: protocol::ConfigTarget, cx: &mut Context<Self>) {
        if self.can_edit_cli_settings() {
            self.config_target = target;
            cx.notify();
        }
    }

    pub(super) fn open_cli_config_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_edit_cli_settings() {
            return;
        }
        let mut groups = std::collections::BTreeMap::<String, Vec<PaletteCommand>>::new();
        if let Some(snapshot) = self.cli_config_settings.as_ref() {
            for setting in &snapshot.settings {
                let mut entry = command(setting.path.clone(), PaletteAction::EditConfigSetting(setting.path.clone()));
                entry.description = Some(format!(
                    "{}{}{}",
                    value_summary(&setting.value),
                    setting
                        .origin
                        .as_deref()
                        .map_or(String::new(), |origin| format!(" · from {origin}")),
                    setting.value.as_bool().map_or_else(String::new, |value| {
                        if value {
                            " · click to turn off".into()
                        } else {
                            " · click to turn on".into()
                        }
                    })
                ));
                entry.keywords = vec![value_summary(&setting.value)];
                let group = setting.path.split('.').next().unwrap_or("Settings").to_owned();
                groups.entry(group).or_default().push(entry);
            }
        }
        self.show_palette(PaletteKind::Commands, groups.into_iter().collect(), window, cx);
    }

    pub(super) fn open_cli_config_editor(&mut self, path: String, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_edit_cli_settings() {
            return;
        }
        if !self.settings_open {
            self.open_settings(window, cx);
        }
        let Some(setting) = self
            .cli_config_settings
            .as_ref()
            .and_then(|snapshot| snapshot.settings.iter().find(|setting| setting.path == path))
        else {
            return;
        };

        if let Some(value) = setting.value.as_bool() {
            self.save_cli_config_value(path, Value::Bool(!value), cx);
            return;
        }
        let Some(kind) = ConfigValueKind::for_value(&setting.value) else {
            return;
        };

        let current_value = setting.value.clone();
        let text = kind.editor_value(&current_value);
        self.config_value_input
            .update(cx, |input, cx| input.set_value(text, window, cx));
        self.config_editor_error = None;
        self.config_editor = Some(ConfigEditor {
            path,
            kind,
            target: self.config_target,
            expected_layer_version: self.target_version(self.config_target),
        });
        self.config_editor_return_focus = Some(self.settings_focus.clone());
        cx.on_next_frame(window, |view, window, cx| view.config_editor_focus.focus(window, cx));
        cx.notify();
    }

    pub(super) fn render_cli_config_editor(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(editor) = self.config_editor.as_ref() else {
            return div().into_any_element();
        };
        let colors = cx.theme().colors;
        let format_hint = match editor.kind {
            ConfigValueKind::String => "Text",
            ConfigValueKind::Integer => "Whole number",
            ConfigValueKind::Float => "Decimal number",
            ConfigValueKind::Array => "JSON array",
            ConfigValueKind::Object => "JSON object",
        };
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .p_6()
            .bg(colors.background.opacity(0.86))
            .child(
                div()
                    .id("cli-config-editor")
                    .test_support()
                    .role(gpui_kit::Role::Dialog)
                    .aria_label(format!("Edit CLI setting {}", editor.path))
                    .w(px(620.))
                    .max_w_full()
                    .max_h_full()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .p_5()
                    .rounded_xl()
                    .border_1()
                    .border_color(colors.border)
                    .bg(colors.background)
                    .shadow_lg()
                    .track_focus(&self.config_editor_focus)
                    .child(
                        DialogHeader::new()
                            .child(DialogTitle::new().child("Edit CLI setting"))
                            .child(DialogDescription::new().child(editor.path.clone())),
                    )
                    .child(div().text_xs().text_color(colors.muted_foreground).child(format_hint))
                    .child(
                        Textarea::new(&self.config_value_input)
                            .aria_label(format!("Value for {}", editor.path))
                            .cursor_text()
                            .w_full(),
                    )
                    .when_some(self.config_editor_error.as_ref(), |dialog, error| {
                        dialog.child(div().text_sm().text_color(colors.danger).child(error.clone()))
                    })
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap_2()
                            .child(
                                desktop_button("cancel-cli-config-edit").label("Cancel").ghost().on_click(
                                    cx.listener(|view, _, window, cx| view.close_cli_config_editor(window, cx)),
                                ),
                            )
                            .child(
                                desktop_button("save-cli-config-edit")
                                    .label(if self.config_write_pending {
                                        "Saving…"
                                    } else {
                                        "Save and reload"
                                    })
                                    .primary()
                                    .desktop_disabled(self.config_write_pending || !self.can_edit_cli_settings())
                                    .on_click(
                                        cx.listener(|view, _, window, cx| view.submit_cli_config_edit(window, cx)),
                                    ),
                            ),
                    )
                    .focus_trap("cli-config-editor-trap", &self.config_editor_focus),
            )
            .into_any_element()
    }

    fn submit_cli_config_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.config_write_pending {
            return;
        }
        let Some(editor) = self.config_editor.as_ref() else {
            return;
        };
        let raw = self.config_value_input.read(cx).value().to_string();
        match editor.kind.parse(&raw) {
            Ok(value) => self.save_cli_config_value_at(
                editor.path.clone(),
                value,
                editor.target,
                editor.expected_layer_version.clone(),
                cx,
            ),
            Err(error) => {
                self.config_editor_error = Some(error);
                self.config_editor_focus.focus(window, cx);
                cx.notify();
            }
        }
    }

    fn save_cli_config_value(&mut self, path: String, value: Value, cx: &mut Context<Self>) {
        let target = self.config_target;
        let expected_layer_version = self.target_version(target);
        self.save_cli_config_value_at(path, value, target, expected_layer_version, cx);
    }

    fn save_cli_config_value_at(
        &mut self,
        path: String,
        value: Value,
        target: protocol::ConfigTarget,
        expected_layer_version: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if !self.can_edit_cli_settings() {
            return;
        }
        let params = protocol::ConfigSettingsWriteParams { target, path, value, expected_layer_version };
        if self.request(protocol::method::CONFIG_SETTINGS_WRITE, json!(params)) {
            self.config_write_pending = true;
            self.session_request = Some(protocol::method::CONFIG_SETTINGS_WRITE);
            self.config_editor_error = None;
            self.status = "Saving CLI settings…".into();
            self.error = None;
        } else {
            self.config_editor_error = Some("The local CLI settings request queue is unavailable.".into());
        }
        cx.notify();
    }

    fn target_version(&self, target: protocol::ConfigTarget) -> Option<String> {
        self.cli_config_settings
            .as_ref()?
            .target_versions
            .iter()
            .find(|version| version.target == target)
            .and_then(|version| version.version.clone())
    }

    pub(super) fn close_cli_config_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.config_write_pending {
            return;
        }
        self.config_editor = None;
        self.config_editor_error = None;
        if let Some(focus) = self.config_editor_return_focus.take() {
            focus.focus(window, cx);
        }
        cx.notify();
    }

    pub(super) fn handle_cli_config_write_response(&mut self, result: Value, cx: &mut Context<Self>) {
        match serde_json::from_value::<protocol::ConfigSettingsWriteResult>(result) {
            Ok(result) => {
                self.config_write_pending = false;
                self.config_editor = None;
                self.config_editor_error = None;
                self.config_write_notice = Some(if result.overridden {
                    format!(
                        "Saved {} to {} configuration. A higher-precedence layer still controls the active value.",
                        result.path,
                        target_label(result.target)
                    )
                } else {
                    format!("Saved {} to {} configuration.", result.path, target_label(result.target))
                });
                self.reload_cli_settings(cx);
            }
            Err(_) => {
                self.config_write_pending = false;
                self.config_editor_error = Some("The CLI returned an invalid settings response.".into());
                self.error = self.config_editor_error.clone();
                cx.notify();
            }
        }
    }

    pub(super) fn handle_cli_config_read_response(&mut self, result: Value) {
        match serde_json::from_value::<protocol::ConfigSettingsSnapshot>(result) {
            Ok(snapshot) => {
                self.cli_config_settings = Some(snapshot);
                if let Some(notice) = self.config_write_notice.take() {
                    self.status = format!("{notice} Settings reloaded for the next session.");
                }
            }
            Err(_) => {
                self.cli_config_settings = None;
                self.error = Some("The CLI returned an invalid settings catalog.".into());
            }
        }
    }
}

fn target_label(target: protocol::ConfigTarget) -> &'static str {
    match target {
        protocol::ConfigTarget::User => "user",
        protocol::ConfigTarget::Workspace => "workspace",
        protocol::ConfigTarget::Project => "project",
    }
}

fn value_summary(value: &Value) -> String {
    let rendered = match value {
        Value::String(value) => {
            let safe = value
                .chars()
                .map(|character| if character.is_control() { ' ' } else { character })
                .collect::<String>();
            format!("\"{safe}\"")
        }
        _ => value.to_string(),
    };
    if rendered.chars().count() <= 96 {
        rendered
    } else {
        let mut shortened = rendered.chars().take(93).collect::<String>();
        shortened.push('…');
        shortened
    }
}

#[cfg(test)]
mod tests {
    use super::ConfigValueKind;
    use serde_json::json;

    #[test]
    fn config_editor_parses_strings_and_typed_json_without_coercion() {
        assert_eq!(ConfigValueKind::String.parse("a \"quoted\" value"), Ok(json!("a \"quoted\" value")));
        assert_eq!(ConfigValueKind::Integer.parse("42"), Ok(json!(42)));
        assert!(ConfigValueKind::Integer.parse("4.2").is_err());
        assert_eq!(ConfigValueKind::Float.parse("4.2"), Ok(json!(4.2)));
        assert_eq!(ConfigValueKind::Array.parse("[\"a\", 2]"), Ok(json!(["a", 2])));
        assert!(ConfigValueKind::Array.parse("{\"a\": 2}").is_err());
        assert_eq!(ConfigValueKind::Object.parse("{\"enabled\": true}"), Ok(json!({"enabled": true})));
    }
}
