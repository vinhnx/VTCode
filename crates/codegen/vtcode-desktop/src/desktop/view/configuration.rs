//! Searchable, typed edits for the CLI configuration shared with Desktop.

use super::*;
use gpui_kit::component::IndexPath;
use gpui_kit::component::searchable_list::SearchableListItem;
use gpui_kit::component::select::{SearchableVec, Select, SelectState};

#[derive(Clone, Debug)]
pub(super) struct ConfigOption {
    value: String,
}

impl SearchableListItem for ConfigOption {
    type Value = String;

    fn title(&self) -> gpui_kit::SharedString {
        if self.value.is_empty() {
            "Use CLI default".into()
        } else {
            self.value.clone().into()
        }
    }

    fn value(&self) -> &Self::Value {
        &self.value
    }
}

pub(super) type ConfigOptionSelectState = SelectState<SearchableVec<ConfigOption>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ConfigValueKind {
    Boolean,
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
    options: Vec<String>,
    pub(super) option_select: Option<Entity<ConfigOptionSelectState>>,
}

impl ConfigValueKind {
    fn for_value(value: &Value) -> Option<Self> {
        match value {
            Value::Bool(_) => Some(Self::Boolean),
            Value::String(_) => Some(Self::String),
            Value::Number(number) if number.is_i64() || number.is_u64() => Some(Self::Integer),
            Value::Number(_) => Some(Self::Float),
            Value::Array(_) => Some(Self::Array),
            Value::Object(_) => Some(Self::Object),
            Value::Null => None,
        }
    }

    fn for_setting(setting: &protocol::ConfigSetting) -> Option<Self> {
        Self::for_value(&setting.value).or_else(|| {
            if !setting.options.is_empty() {
                return Some(Self::String);
            }
            let type_name = setting.type_name.as_deref()?.to_ascii_lowercase();
            let has_type = |types: &[&str]| {
                type_name
                    .split(|character: char| !character.is_ascii_alphanumeric())
                    .any(|token| types.contains(&token))
            };
            if has_type(&["bool", "boolean"]) {
                Some(Self::Boolean)
            } else if has_type(&[
                "int", "integer", "i8", "i16", "i32", "i64", "i128", "u8", "u16", "u32", "u64", "u128", "isize",
                "usize",
            ]) {
                Some(Self::Integer)
            } else if has_type(&["float", "number", "f32", "f64"]) {
                Some(Self::Float)
            } else if has_type(&["array", "vec", "list"]) {
                Some(Self::Array)
            } else if has_type(&["table", "object", "map", "hashmap", "btreemap"]) {
                Some(Self::Object)
            } else if has_type(&["string", "str", "path", "pathbuf", "enum"]) {
                Some(Self::String)
            } else {
                None
            }
        })
    }

    fn editor_value(self, value: &Value) -> String {
        match self {
            Self::Boolean => value.as_bool().unwrap_or(false).to_string(),
            Self::String => value.as_str().unwrap_or_default().to_owned(),
            Self::Integer | Self::Float => value.to_string(),
            Self::Array | Self::Object => serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string()),
        }
    }

    fn parse(self, text: &str) -> Result<Value, String> {
        match self {
            Self::Boolean => {
                let value = serde_json::from_str::<Value>(text).map_err(|error| error.to_string())?;
                if value.is_boolean() {
                    Ok(value)
                } else {
                    Err("Enter true or false.".into())
                }
            }
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
    pub(super) fn interrupt_cli_config_write(&mut self) {
        if self.config_write_pending {
            self.config_write_pending = false;
            let message = "The service disconnected while saving. The outcome is unknown; reload CLI settings before trying again.";
            self.config_editor_error = Some(message.into());
            self.error = Some(match self.error.take() {
                Some(error) => format!("{error}\n\n{message}"),
                None => message.into(),
            });
        }
    }
    pub(super) fn render_cli_config_page(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let mut groups = std::collections::BTreeMap::<String, Vec<&protocol::ConfigSetting>>::new();
        if let Some(snapshot) = self.cli_config_settings.as_ref() {
            for setting in &snapshot.settings {
                groups
                    .entry(setting.group.clone().unwrap_or_else(|| "Advanced settings".into()))
                    .or_default()
                    .push(setting);
            }
        }
        let selected = self
            .config_page_group
            .as_ref()
            .filter(|group| groups.contains_key(*group))
            .or_else(|| groups.keys().next())
            .cloned();
        let mut categories = div()
            .id("config-categories")
            .w_full()
            .flex_shrink_0()
            .overflow_x_scroll()
            .flex()
            .gap_2();
        for (name, settings) in &groups {
            let name = name.clone();
            let active = selected.as_ref() == Some(&name);
            let id = format!("config-category-{name}");
            let label = format!("{} ({})", name, settings.len());
            categories = categories.child(
                desktop_button(id)
                    .label(label)
                    .small()
                    .when(active, |button| button.secondary())
                    .when(!active, |button| button.ghost())
                    .on_click(cx.listener(move |view, _, _, cx| {
                        view.config_page_group = Some(name.clone());
                        cx.notify();
                    })),
            );
        }
        let mut entries = div().flex().flex_col().gap_2();
        if let Some(name) = selected.as_ref() {
            if let Some(settings) = groups.get(name) {
                entries =
                    entries.child(div().text_lg().font_weight(gpui_kit::FontWeight::SEMIBOLD).child(name.clone()));
                if let Some(description) = settings.iter().find_map(|setting| setting.group_description.as_ref()) {
                    entries =
                        entries.child(div().text_sm().text_color(colors.muted_foreground).child(description.clone()));
                }
                for setting in settings {
                    let path = setting.path.clone();
                    let value = value_summary(&setting.value);
                    let label = setting.label.as_deref().unwrap_or(&setting.path).to_owned();
                    let details = format!(
                        "{} · {}{}",
                        setting.path,
                        value,
                        setting.origin.as_deref().map_or(String::new(), |origin| format!(" · {origin}"))
                    );
                    let editable = setting.editable && self.can_edit_cli_settings();
                    let mut row = div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .p_3()
                        .rounded_lg()
                        .bg(colors.secondary)
                        .child(div().font_weight(gpui_kit::FontWeight::MEDIUM).child(label))
                        .child(div().text_sm().text_color(colors.muted_foreground).child(details));
                    if let Some(description) = setting.description.as_ref() {
                        row = row.child(div().text_sm().child(description.clone()));
                    }
                    if setting.editable {
                        row = row.child(
                            desktop_button(format!("config-edit-{path}"))
                                .label("Edit")
                                .ghost()
                                .small()
                                .desktop_disabled(!editable)
                                .on_click(cx.listener(move |view, _, window, cx| {
                                    view.open_cli_config_editor(path.clone(), window, cx)
                                })),
                        );
                    } else {
                        row = row.child(div().text_xs().text_color(colors.muted_foreground).child("Reference only"));
                    }
                    entries = entries.child(row);
                }
            }
        }
        let panel_height =
            (window.viewport_size().height - cx.theme().sheet.margin_top - cx.theme().font_size * 2.5 - px(8.))
                .max(px(0.));
        let mut panel = div()
            .id("cli-config-page")
            .test_support()
            .role(gpui_kit::Role::Dialog)
            .aria_label("VT Code configuration")
            .track_focus(&self.settings_focus)
            .relative()
            .w_full()
            .h(panel_height)
            .min_h_0()
            .overflow_hidden()
            .flex()
            .flex_col()
            .gap_3()
            .p_6()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .id("cli-config-page-content")
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .lock_scroll_axis()
                    .test_support()
                    .track_scroll(&self.settings_scroll)
                    .overflow_y_scroll()
                    .vertical_scrollbar(&self.settings_scroll)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_3()
                            .child(self.render_cli_configuration(cx))
                            .child(categories)
                            .child(entries),
                    ),
            );
        if self.config_editor.is_some() {
            panel = panel.child(self.render_cli_config_editor(cx));
        }
        panel.into_any_element()
    }

    pub(super) fn render_cli_configuration(&self, cx: &Context<Self>) -> AnyElement {
        let snapshot = self.cli_config_settings.as_ref();
        let count = snapshot.map_or(0, |snapshot| snapshot.settings.len());
        let editable_count =
            snapshot.map_or(0, |snapshot| snapshot.settings.iter().filter(|setting| setting.editable).count());
        let mut group = GroupBox::new()
            .flex_shrink_0()
            .fill()
            .title(group_box_title("CLI configuration", &cx.theme().colors));
        group = group.child(div().text_sm().child(match snapshot {
            Some(_) => format!("{editable_count} editable settings across {count} CLI config fields · values follow the active config layers"),
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
            let read_only = snapshot.settings.iter().filter(|setting| !setting.editable).count();
            if read_only > 0 {
                note.push_str(&format!(
                    " {read_only} documented schema entries are reference-only until they have a concrete value."
                ));
            }
        }
        group = group.footer(note);
        accessible_group_box("cli-configuration-group", "CLI configuration", group)
    }

    pub(super) fn can_edit_cli_settings(&self) -> bool {
        self.workspace.is_some()
            && self.backend.is_some()
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
        let groups = self.cli_config_settings.as_ref().map(config_palette_groups).unwrap_or_default();
        self.show_palette(PaletteKind::ConfigSettings, groups, window, cx);
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

        if !setting.editable {
            self.error = Some(format!("{} is a schema reference and is not directly editable.", setting.path));
            cx.notify();
            return;
        }

        let Some(kind) = ConfigValueKind::for_setting(setting) else {
            return;
        };
        if kind == ConfigValueKind::Boolean {
            self.save_cli_config_value(path, Value::Bool(!setting.value.as_bool().unwrap_or(false)), cx);
            return;
        }

        let current_value = if setting.value.is_null() {
            setting
                .default_value
                .as_deref()
                .and_then(|default| serde_json::from_str::<Value>(default).ok())
                .unwrap_or_else(|| Value::String(String::new()))
        } else {
            setting.value.clone()
        };
        let text = kind.editor_value(&current_value);
        let options = setting.options.clone();
        let option_select = (kind == ConfigValueKind::String && !options.is_empty()).then(|| {
            let items = options.iter().cloned().map(|value| ConfigOption { value }).collect::<Vec<_>>();
            let selected = items.iter().position(|option| option.value == text).map(IndexPath::new);
            let items = SearchableVec::new(items);
            cx.new(|cx| SelectState::new(items, selected, window, cx).searchable(options.len() > 8))
        });
        if option_select.is_none() {
            self.config_value_input
                .update(cx, |input, cx| input.set_value(text, window, cx));
        }
        self.config_editor_error = None;
        self.config_editor = Some(ConfigEditor {
            path,
            kind,
            target: self.config_target,
            expected_layer_version: self.target_version(self.config_target),
            options,
            option_select,
        });
        self.config_editor_return_focus = Some(self.settings_focus.clone());
        cx.on_next_frame(window, |view, window, cx| {
            let option_select = view.config_editor.as_ref().and_then(|editor| editor.option_select.clone());
            if let Some(option_select) = option_select {
                option_select.update(cx, |state, cx| state.focus(window, cx));
            } else {
                view.config_value_input.read(cx).focus_handle(cx).focus(window, cx);
            }
        });
        cx.notify();
    }

    pub(super) fn render_cli_config_editor(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(editor) = self.config_editor.as_ref() else {
            return div().into_any_element();
        };
        let colors = cx.theme().colors;
        let format_hint = match editor.kind {
            ConfigValueKind::Boolean => "Boolean",
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
                    .bg(colors.secondary)
                    .shadow_lg()
                    .track_focus(&self.config_editor_focus)
                    .child(
                        DialogHeader::new()
                            .child(DialogTitle::new().child("Edit CLI setting"))
                            .child(DialogDescription::new().child(editor.path.clone())),
                    )
                    .child(div().text_xs().text_color(colors.muted_foreground).child(
                        if editor.option_select.is_some() {
                            "Choose one of the documented values.".to_string()
                        } else {
                            format_hint.to_string()
                        },
                    ))
                    .when_some(editor.option_select.as_ref(), |dialog, state| {
                        dialog.child(
                            Select::new(state)
                                .id("cli-config-option-select")
                                .placeholder("Choose a documented value")
                                .accessibility_label(format!("Value for {}", editor.path))
                                .menu_max_h(px(320.))
                                .w_full(),
                        )
                    })
                    .when(editor.option_select.is_none(), |dialog| {
                        dialog.child(
                            Textarea::new(&self.config_value_input)
                                .aria_label(format!("Value for {}", editor.path))
                                .cursor_text()
                                .bg(colors.popover)
                                .rounded_md()
                                .px_3()
                                .py_2()
                                .w_full(),
                        )
                    })
                    .when_some(self.config_editor_error.as_ref(), |dialog, error| {
                        dialog.child(
                            div()
                                .id("cli-config-save-error")
                                .test_support()
                                .role(gpui_kit::Role::Alert)
                                .aria_label(error.clone())
                                .text_sm()
                                .text_color(colors.danger)
                                .child(error.clone()),
                        )
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
        let raw = if let Some(state) = self.config_editor.as_ref().and_then(|editor| editor.option_select.as_ref()) {
            let Some(value) = state.read(cx).selected_value().cloned() else {
                self.config_editor_error = Some("Choose a documented value before saving.".into());
                let state = state.clone();
                state.update(cx, |state, cx| state.focus(window, cx));
                cx.notify();
                return;
            };
            value
        } else {
            self.config_value_input.read(cx).value().to_string()
        };
        match editor.parse(&raw) {
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
            self.config_editor_error = Some(rust_i18n::t!("app.errors.request_queue_unavailable").to_string());
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

impl ConfigEditor {
    fn parse(&self, text: &str) -> Result<Value, String> {
        let value = self.kind.parse(text)?;
        if !self.options.is_empty() {
            let Some(selected) = value.as_str() else {
                return Err("Choose one of the documented string options.".into());
            };
            if !self.options.iter().any(|option| option == selected) {
                return Err(format!("Choose one of the documented values: {}.", self.options.join(", ")));
            }
        }
        Ok(value)
    }
}

fn target_label(target: protocol::ConfigTarget) -> &'static str {
    match target {
        protocol::ConfigTarget::User => "user",
        protocol::ConfigTarget::Workspace => "workspace",
        protocol::ConfigTarget::Project => "project",
    }
}

pub(super) fn config_palette_groups(snapshot: &protocol::ConfigSettingsSnapshot) -> Vec<(String, Vec<PaletteCommand>)> {
    let mut groups = std::collections::BTreeMap::<String, Vec<PaletteCommand>>::new();
    for setting in &snapshot.settings {
        if !setting.editable {
            continue;
        }
        let label = setting.label.as_deref().unwrap_or(&setting.path);
        let mut entry = command(label.to_string(), PaletteAction::EditConfigSetting(setting.path.clone()));
        let summary = if setting.value.is_null() {
            "<unset>".to_string()
        } else {
            value_summary(&setting.value)
        };
        let origin = setting
            .origin
            .as_deref()
            .map_or(String::new(), |origin| format!(" · from {origin}"));
        let action_hint = setting.value.as_bool().map_or_else(String::new, |value| {
            if value {
                " · click to turn off".into()
            } else {
                " · click to turn on".into()
            }
        });
        let description = setting.description.as_deref().unwrap_or_default();
        let default = setting
            .default_value
            .as_deref()
            .map_or(String::new(), |value| format!(" · default {value}"));
        let options = if setting.options.is_empty() {
            String::new()
        } else {
            format!(" · options {}", setting.options.join(", "))
        };
        entry.description = Some(format!("{summary}{origin}{action_hint} · {description}{default}{options}"));
        entry.keywords = vec![setting.path.clone(), summary];
        entry.keywords.extend(setting.options.iter().cloned());
        if let Some(origin) = setting.origin.as_ref() {
            entry.keywords.push(origin.clone());
        }
        if let Some(default) = setting.default_value.as_ref() {
            entry.keywords.push(default.clone());
        }
        if let Some(description) = setting.description.as_ref() {
            entry.keywords.push(description.clone());
        }
        let group = setting.group.clone().unwrap_or_else(|| "Advanced settings".into());
        groups.entry(group).or_default().push(entry);
    }
    groups.into_iter().collect()
}

fn value_summary(value: &Value) -> String {
    let rendered = match value {
        Value::Null => "<unset>".to_string(),
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
    use super::{ConfigValueKind, config_palette_groups};
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

    #[test]
    fn config_editor_enforces_the_shared_cli_options() {
        let editor = super::ConfigEditor {
            path: "provider.openai.service_tier".into(),
            kind: ConfigValueKind::String,
            target: super::protocol::ConfigTarget::User,
            expected_layer_version: None,
            options: vec!["auto".into(), "fast".into()],
            option_select: None,
        };
        assert_eq!(editor.parse("fast"), Ok(json!("fast")));
        assert!(editor.parse("ultrafast").is_err());
    }

    #[test]
    fn unset_documented_fields_keep_the_cli_type_and_read_only_schema_is_not_editable() {
        let optional_string = super::protocol::ConfigSetting {
            path: "provider.openai.service_tier".into(),
            value: json!(null),
            origin: None,
            label: Some("Service tier".into()),
            type_name: Some("Option<String>".into()),
            default_value: Some("\"auto\"".into()),
            description: Some("Choose a service tier.".into()),
            options: vec!["auto".into(), "fast".into()],
            group: Some("Model & Provider".into()),
            group_description: None,
            editable: true,
        };
        assert_eq!(ConfigValueKind::for_setting(&optional_string), Some(ConfigValueKind::String));
        let optional_enum = super::protocol::ConfigSetting {
            type_name: Some("ReasoningEffortLevel | null".into()),
            options: vec!["low".into(), "high".into()],
            ..optional_string.clone()
        };
        assert_eq!(ConfigValueKind::for_setting(&optional_enum), Some(ConfigValueKind::String));
        let unknown_struct = super::protocol::ConfigSetting {
            type_name: Some("PointConfig".into()),
            options: Vec::new(),
            ..optional_string.clone()
        };
        assert_eq!(ConfigValueKind::for_setting(&unknown_struct), None);

        let read_only = super::protocol::ConfigSetting {
            path: "mcp.providers.*.command".into(),
            editable: false,
            ..optional_string.clone()
        };
        let snapshot = super::protocol::ConfigSettingsSnapshot {
            settings: vec![optional_string, read_only],
            target_versions: Vec::new(),
            omitted_sensitive: 0,
            omitted_large: 0,
            omitted_limit: 0,
        };
        let groups = config_palette_groups(&snapshot);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].0, "Model & Provider");
        assert_eq!(groups[0].1.len(), 1);
        assert_eq!(groups[0].1[0].label, "Service tier");
    }
}
