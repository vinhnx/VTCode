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
    /// Current configuration search text, trimmed. The Kit input owns the
    /// editing state; render reads it directly so there is a single source.
    fn config_search_query(&self, cx: &App) -> String {
        self.config_search_input.read(cx).value().trim().to_string()
    }

    pub(super) fn set_config_search_query(&mut self, query: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.config_search_input
            .update(cx, |input, cx| input.set_value(query, window, cx));
        cx.notify();
    }

    /// Resolve a `/config <target>` deep link against the loaded catalog.
    /// Exact setting paths narrow the search to that setting, exact group
    /// names select the group, and anything else becomes a search query so
    /// the page shows the closest matches with a clear path back.
    pub(super) fn resolve_config_deep_link(&mut self, target: &str, window: &mut Window, cx: &mut Context<Self>) {
        let settings = self
            .cli_config_settings
            .as_ref()
            .map(|snapshot| snapshot.settings.as_slice())
            .unwrap_or(&[]);
        match resolve_config_target(settings, target) {
            ConfigTargetResolution::Setting { group, path } => {
                self.config_page_group = Some(group);
                self.set_config_search_query(&path, window, cx);
            }
            ConfigTargetResolution::Group { name } => {
                self.config_page_group = Some(name);
                self.set_config_search_query("", window, cx);
            }
            ConfigTargetResolution::Search { query } => {
                self.set_config_search_query(&query, window, cx);
            }
        }
    }

    pub(super) fn render_cli_config_page(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let query = self.config_search_query(cx);
        let terms = config_search_terms(&query);
        let mut groups = std::collections::BTreeMap::<String, Vec<&protocol::ConfigSetting>>::new();
        if let Some(snapshot) = self.cli_config_settings.as_ref() {
            for setting in &snapshot.settings {
                if setting_matches_query(setting, &terms) {
                    groups.entry(config_group_name(setting).to_owned()).or_default().push(setting);
                }
            }
        }
        let (total, editable_total) = self.cli_config_settings.as_ref().map_or((0, 0), |snapshot| {
            (snapshot.settings.len(), snapshot.settings.iter().filter(|setting| setting.editable).count())
        });
        let visible_count: usize = groups.values().map(Vec::len).sum();
        let selected = self
            .config_page_group
            .as_ref()
            .filter(|group| groups.contains_key(*group))
            .or_else(|| groups.keys().next())
            .cloned();
        let counts = if terms.is_empty() {
            format!("{editable_total} editable of {total} settings")
        } else {
            format!("{visible_count} of {total} match \"{query}\"")
        };

        let mut header = div()
            .id("cli-config-header")
            .test_support()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        Input::new(&self.config_search_input)
                            .id("config-search")
                            .flex_1()
                            .min_w_0()
                            .small()
                            .aria_label("Search configuration settings")
                            .prefix(Icon::new(IconName::Search).size_4())
                            .cleanable(true),
                    )
                    .child(
                        desktop_button("browse-cli-config")
                            .label("Browse all settings…")
                            .accessibility_label("Search and edit CLI configuration settings")
                            .ghost()
                            .small()
                            .desktop_disabled(self.cli_config_settings.is_none() || !self.can_edit_cli_settings())
                            .on_click(cx.listener(|view, _, window, cx| view.open_cli_config_palette(window, cx))),
                    )
                    .child(
                        desktop_button("reload-cli-config")
                            .label("Reload")
                            .ghost()
                            .small()
                            .tooltip_with_action("Reload CLI settings", &ReloadCliSettings, Some("VTCodeDesktop"))
                            .desktop_disabled(!self.can_reload_cli_settings())
                            .dispatches_desktop_action(ReloadCliSettings, self.focus_handle.clone()),
                    ),
            );
        let targets = [
            (protocol::ConfigTarget::User, "config-target-user", "User"),
            (protocol::ConfigTarget::Workspace, "config-target-workspace", "Workspace"),
            (protocol::ConfigTarget::Project, "config-target-project", "Project"),
        ]
        .into_iter()
        .fold(div().flex().flex_wrap().items_center().gap_2().flex_shrink_0(), |row, (target, id, label)| {
            let selected = self.config_target == target;
            row.child(
                desktop_button(id)
                    .label(label)
                    .accessibility_label(format!(
                        "Write CLI settings to {label} configuration{}",
                        if selected { " (selected)" } else { "" }
                    ))
                    .toggled(selected)
                    .when(selected, |button| button.bg(colors.secondary).text_color(colors.foreground))
                    .when(!selected, |button| button.ghost())
                    .small()
                    .desktop_disabled(!self.can_edit_cli_settings())
                    .on_click(cx.listener(move |view, _, _, cx| view.set_cli_config_target(target, cx))),
            )
        });
        header = header.child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_2()
                .child(section_label("Write to", &colors))
                .child(targets)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_right()
                        .child(row_description(counts, &colors)),
                ),
        );
        if let Some(notice) = self.config_write_notice.as_ref() {
            header = header.child(
                div()
                    .id("config-write-notice")
                    .test_support()
                    .role(gpui_kit::Role::Status)
                    .text_sm()
                    .text_color(colors.foreground)
                    .child(notice.clone()),
            );
        }

        let body = match (self.cli_config_settings.as_ref(), selected.as_ref()) {
            (None, _) if self.workspace.is_none() => self.render_config_content_frame(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .child(empty_state(
                        "Choose a workspace",
                        "Choose a workspace to browse CLI configuration settings.",
                        IconName::Folder,
                    )),
            ),
            (None, _) => self.render_config_content_frame(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap_3()
                    .child(
                        div()
                            .text_sm()
                            .text_color(colors.muted_foreground)
                            .child("Loading shared CLI settings…"),
                    )
                    .child(skeleton_line("config-loading-line-one", px(320.), cx))
                    .child(skeleton_line("config-loading-line-two", px(240.), cx)),
            ),
            (Some(_), None) => self.render_config_content_frame(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .flex()
                            .flex_col()
                            .items_center()
                            .justify_center()
                            .gap_3()
                            .child(empty_state(
                                "No matching settings",
                                "Try a different search or browse another group.",
                                IconName::Search,
                            ))
                            .child(
                                desktop_button("clear-config-search")
                                    .label("Clear search")
                                    .ghost()
                                    .small()
                                    .on_click(
                                        cx.listener(|view, _, window, cx| view.set_config_search_query("", window, cx)),
                                    ),
                            ),
                    )
                    .child(self.render_config_page_footer(cx)),
            ),
            (Some(_), Some(name)) => {
                let mut rail = div()
                    .id("config-rail")
                    .lock_scroll_axis()
                    .test_support()
                    .role(gpui_kit::Role::Navigation)
                    .aria_label("Configuration groups")
                    .w_56()
                    .flex_shrink_0()
                    .min_h_0()
                    .track_scroll(&self.config_rail_scroll)
                    .overflow_y_scroll()
                    .vertical_scrollbar(&self.config_rail_scroll)
                    .flex()
                    .flex_col()
                    .gap_1();
                for (group, settings) in &groups {
                    let group = group.clone();
                    let active = Some(&group) == selected.as_ref();
                    rail = rail.child(
                        desktop_button(format!("config-category-{group}"))
                            .label(format!("{group} ({})", settings.len()))
                            .small()
                            .w_full()
                            .justify_start()
                            .toggled(active)
                            .when(active, |button| button.bg(colors.secondary).text_color(colors.foreground))
                            .when(!active, |button| button.ghost())
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.config_page_group = Some(group.clone());
                                cx.notify();
                            })),
                    );
                }
                let mut list = div().flex().flex_col();
                if let Some(settings) = groups.get(name) {
                    for (index, setting) in settings.iter().enumerate() {
                        if index > 0 {
                            list = list.child(row_divider(&colors));
                        }
                        list = list.child(self.render_config_setting_row(setting, cx));
                    }
                }
                let description = groups
                    .get(name)
                    .and_then(|settings| settings.iter().find_map(|setting| setting.group_description.as_ref()));
                let mut group = div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(div().text_lg().font_weight(gpui_kit::FontWeight::SEMIBOLD).child(name.clone()));
                if let Some(description) = description {
                    group = group.child(div().text_sm().text_color(colors.muted_foreground).child(description.clone()));
                }
                group = group.child(list);
                group = group.child(self.render_config_page_footer(cx));
                let content = self.render_config_content_frame(group);
                div()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .flex()
                    .gap_4()
                    .child(rail)
                    .child(content)
                    .into_any_element()
            }
        };

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
            .gap_4()
            .p_6()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(header)
            .child(body);
        if self.config_editor.is_some() {
            panel = panel.child(self.render_cli_config_editor(cx));
        }
        panel.into_any_element()
    }

    /// Shared scroll frame for the configuration content column: one
    /// scrollable region keeps the header fixed while the catalog, empty
    /// states, and trailing notes share the remaining height.
    fn render_config_content_frame(&self, child: impl IntoElement) -> AnyElement {
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
            .pr_2()
            .flex()
            .flex_col()
            .child(child)
            .into_any_element()
    }

    /// One value-first setting row: title and live value on stable lanes,
    /// path and provenance beneath, description last. Boolean rows toggle
    /// in place; every other editable row opens the typed editor dialog.
    fn render_config_setting_row(&self, setting: &protocol::ConfigSetting, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let label = setting.label.as_deref().unwrap_or(&setting.path).to_owned();
        let can_edit = self.can_edit_cli_settings();
        let mut header = div()
            .flex()
            .items_center()
            .gap_2()
            .child(div().flex_1().min_w_0().truncate().child(row_title(label.clone(), &colors)));
        if setting.value.is_null() {
            header = header.child(
                div()
                    .flex_shrink_0()
                    .text_sm()
                    .text_color(colors.muted_foreground)
                    .child("<unset>".to_string()),
            );
        } else {
            header = header.child(
                div()
                    .flex_shrink_0()
                    .min_w_0()
                    .truncate()
                    .text_sm()
                    .font_semibold()
                    .text_color(colors.foreground)
                    .child(value_summary(&setting.value)),
            );
        }
        if setting.value.is_boolean() && setting.editable {
            let path = setting.path.clone();
            let enabled = setting.value.as_bool().unwrap_or(false);
            header = header.child(
                gpui_kit::component::switch::Switch::new(format!("config-toggle-{path}"))
                    .small()
                    .checked(enabled)
                    .disabled(!can_edit)
                    .accessibility_label(label.clone())
                    .tooltip(if enabled {
                        "Turn off".to_string()
                    } else {
                        "Turn on".to_string()
                    })
                    .on_click(cx.listener(move |view, enabled, _, cx| {
                        view.save_cli_config_value(path.clone(), Value::Bool(*enabled), cx);
                    })),
            );
        } else if setting.editable {
            let path = setting.path.clone();
            header = header.child(
                desktop_button(format!("config-edit-{path}"))
                    .label("Edit")
                    .accessibility_label(format!("Edit {}", setting.path))
                    .ghost()
                    .small()
                    .desktop_disabled(!can_edit)
                    .on_click(
                        cx.listener(move |view, _, window, cx| view.open_cli_config_editor(path.clone(), window, cx)),
                    ),
            );
        } else {
            header = header.child(Tag::secondary().outline().small().child("Reference only"));
        }
        let mut meta = setting.path.clone();
        if let Some(origin) = setting.origin.as_deref() {
            meta.push_str(&format!(" · from {origin}"));
        }
        if let Some(default) = setting.default_value.as_deref() {
            meta.push_str(&format!(" · default {default}"));
        }
        let mut row = div()
            .flex()
            .flex_col()
            .gap_1()
            .py_2()
            .child(header)
            .child(div().text_xs().text_color(colors.muted_foreground).child(meta));
        if let Some(description) = setting.description.as_ref() {
            row = row.child(row_description(description.clone(), &colors));
        }
        row.into_any_element()
    }

    /// Writer, omission, and reset notes trailing the configuration list.
    /// Reset stays a terminal command: the desktop protocol exposes no
    /// destructive layer reset, so the page documents the CLI instead.
    fn render_config_page_footer(&self, cx: &Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let mut notes = vec![
            "Saving uses VT Code's validated configuration writer and reloads the local CLI service. Higher-precedence settings can still override the selected layer."
                .to_string(),
        ];
        if let Some(snapshot) = self.cli_config_settings.as_ref() {
            if snapshot.omitted_sensitive > 0 {
                notes.push(
                    "Credential-bearing fields are excluded; manage credentials with `vtcode auth` or the configured environment variable."
                        .to_string(),
                );
            }
            if snapshot.omitted_large > 0 || snapshot.omitted_limit > 0 {
                notes.push("Some unusually large or numerous settings are omitted from this browser.".to_string());
            }
            let read_only = snapshot.settings.iter().filter(|setting| !setting.editable).count();
            if read_only > 0 {
                notes.push(format!(
                    "{read_only} documented schema entries are reference-only until they have a concrete value."
                ));
            }
        }
        let mut footer = div()
            .id("cli-config-footer")
            .test_support()
            .w_full()
            .flex()
            .flex_col()
            .gap_1()
            .child(row_divider(&colors));
        for note in notes {
            footer = footer.child(div().text_xs().text_color(colors.muted_foreground).child(note));
        }
        footer = footer.child(
            div()
                .id("config-reset-hint")
                .test_support()
                .text_xs()
                .text_color(colors.muted_foreground)
                .child("Reset a layer from the terminal with `vtcode config reset` (`--global` for user, `--project` for project)."),
        );
        footer.into_any_element()
    }

    pub(super) fn render_cli_configuration(&self, cx: &Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
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
                .toggled(selected)
                .when(selected, |button| button.bg(colors.secondary).text_color(colors.foreground))
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

/// Curated CLI `/config` group, defaulting to the shared advanced bucket.
fn config_group_name(setting: &protocol::ConfigSetting) -> &str {
    setting.group.as_deref().unwrap_or("Advanced settings")
}

/// How a `/config <target>` deep link resolved against the loaded catalog.
#[derive(Clone, Debug, PartialEq, Eq)]
enum ConfigTargetResolution {
    /// An exact setting path: narrow the page to it within its group.
    Setting { group: String, path: String },
    /// An exact group name (case-insensitive): select the group.
    Group { name: String },
    /// Anything else: filter the catalog by it as a search query.
    Search { query: String },
}

fn resolve_config_target(settings: &[protocol::ConfigSetting], target: &str) -> ConfigTargetResolution {
    let target = target.trim();
    if let Some(setting) = settings.iter().find(|setting| setting.path == target) {
        return ConfigTargetResolution::Setting {
            group: config_group_name(setting).to_owned(),
            path: setting.path.clone(),
        };
    }
    if let Some(name) = settings
        .iter()
        .map(config_group_name)
        .find(|name| name.eq_ignore_ascii_case(target))
    {
        return ConfigTargetResolution::Group { name: name.to_owned() };
    }
    ConfigTargetResolution::Search { query: target.to_owned() }
}

/// Lowercased whitespace-separated search terms; every term must match.
/// Unicode lowering mirrors the CLI's `normalize_query`.
fn config_search_terms(query: &str) -> Vec<String> {
    query.split_whitespace().map(|term| term.to_lowercase()).collect()
}

/// Whether a setting matches every search term across its path, label,
/// value, group, and documentation, mirroring the CLI's exact-terms filter.
fn setting_matches_query(setting: &protocol::ConfigSetting, terms: &[String]) -> bool {
    if terms.is_empty() {
        return true;
    }
    let mut haystack = String::new();
    haystack.push_str(&setting.path);
    haystack.push(' ');
    if let Some(label) = setting.label.as_deref() {
        haystack.push_str(label);
        haystack.push(' ');
    }
    haystack.push_str(&value_summary(&setting.value));
    haystack.push(' ');
    haystack.push_str(config_group_name(setting));
    haystack.push(' ');
    if let Some(description) = setting.description.as_deref() {
        haystack.push_str(description);
        haystack.push(' ');
    }
    if let Some(origin) = setting.origin.as_deref() {
        haystack.push_str(origin);
        haystack.push(' ');
    }
    if let Some(default) = setting.default_value.as_deref() {
        haystack.push_str(default);
        haystack.push(' ');
    }
    for option in &setting.options {
        haystack.push_str(option);
        haystack.push(' ');
    }
    let haystack = haystack.to_lowercase();
    terms.iter().all(|term| haystack.contains(term))
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
        let group = config_group_name(setting).to_owned();
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
    use super::{
        ConfigTargetResolution, ConfigValueKind, config_palette_groups, config_search_terms, resolve_config_target,
        setting_matches_query,
    };
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

    fn deep_link_fixture() -> Vec<super::protocol::ConfigSetting> {
        vec![
            super::protocol::ConfigSetting {
                path: "agent.default_model".into(),
                value: json!("model-a"),
                origin: Some("workspace".into()),
                label: Some("Default model".into()),
                type_name: Some("string".into()),
                default_value: None,
                description: Some("Model used for new conversations.".into()),
                options: Vec::new(),
                group: Some("Model & Provider".into()),
                group_description: None,
                editable: true,
            },
            super::protocol::ConfigSetting {
                path: "provider.openai.service_tier".into(),
                value: json!("auto"),
                origin: Some("user".into()),
                label: Some("Service tier".into()),
                type_name: Some("string".into()),
                default_value: Some("\"auto\"".into()),
                description: Some("Choose the request service tier.".into()),
                options: vec!["auto".into(), "fast".into()],
                group: Some("Tools".into()),
                group_description: None,
                editable: true,
            },
        ]
    }

    #[test]
    fn config_deep_links_prefer_exact_paths_then_groups_then_search() {
        let settings = deep_link_fixture();
        assert_eq!(
            resolve_config_target(&settings, "agent.default_model"),
            ConfigTargetResolution::Setting {
                group: "Model & Provider".into(),
                path: "agent.default_model".into(),
            }
        );
        assert_eq!(
            resolve_config_target(&settings, "  tools  "),
            ConfigTargetResolution::Group { name: "Tools".into() }
        );
        // Paths are case-sensitive; groups are not. Near-misses fall
        // through to search instead of selecting the wrong destination.
        assert_eq!(
            resolve_config_target(&settings, "Agent.Default_Model"),
            ConfigTargetResolution::Search { query: "Agent.Default_Model".into() }
        );
        assert_eq!(resolve_config_target(&settings, "tool"), ConfigTargetResolution::Search { query: "tool".into() });
        assert_eq!(
            resolve_config_target(&settings, "agent.retries"),
            ConfigTargetResolution::Search { query: "agent.retries".into() }
        );
        assert_eq!(
            resolve_config_target(&[], "agent.default_model"),
            ConfigTargetResolution::Search { query: "agent.default_model".into() }
        );
    }

    #[test]
    fn config_search_requires_every_term_across_documented_fields() {
        let settings = deep_link_fixture();
        let model = &settings[0];
        let tier = &settings[1];
        assert!(setting_matches_query(model, &config_search_terms("")));
        for (setting, term) in [
            (model, "agent.default_model"),
            (model, "Default model"),
            (model, "model-a"),
            (model, "conversations"),
            (model, "workspace"),
            (model, "Model & Provider"),
            (tier, "fast"),
            (tier, "service tier"),
            (tier, "\"auto\""),
        ] {
            assert!(
                setting_matches_query(setting, &config_search_terms(term)),
                "{term:?} must match its documented field"
            );
        }
        // Every term must match: the tier row carries both "service" and
        // "fast", while the model row carries neither "tier" term.
        assert!(setting_matches_query(tier, &config_search_terms("service fast")));
        assert!(!setting_matches_query(model, &config_search_terms("model zebra")));
        assert!(!setting_matches_query(model, &config_search_terms("service")));
        assert!(setting_matches_query(model, &config_search_terms("MODEL")));
    }

    #[test]
    fn config_search_folds_unicode_case_without_stripping_accents() {
        let accented = super::protocol::ConfigSetting {
            path: "agent.break_mode".into(),
            value: json!(true),
            origin: None,
            label: Some("Café mode".into()),
            type_name: Some("bool".into()),
            default_value: None,
            description: None,
            options: Vec::new(),
            group: None,
            group_description: None,
            editable: true,
        };
        assert!(setting_matches_query(&accented, &config_search_terms("CAFÉ")));
        assert!(setting_matches_query(&accented, &config_search_terms("café")));
        assert!(
            !setting_matches_query(&accented, &config_search_terms("CAFE")),
            "folding is case-only; accents still distinguish terms"
        );
    }
}
