//! Searchable, typed edits for the CLI configuration shared with Desktop.

use super::*;
use gpui_kit::component::IndexPath;
use gpui_kit::component::searchable_list::SearchableListItem;
use gpui_kit::component::select::{SearchableVec, SelectState};

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
    pub(super) path: String,
    pub(super) kind: ConfigValueKind,
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
    pub(super) fn config_search_query(&self, cx: &App) -> String {
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

    pub(super) fn submit_cli_config_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

    pub(super) fn save_cli_config_value(&mut self, path: String, value: Value, cx: &mut Context<Self>) {
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
pub(super) fn config_group_name(setting: &protocol::ConfigSetting) -> &str {
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
pub(super) fn config_search_terms(query: &str) -> Vec<String> {
    query.split_whitespace().map(|term| term.to_lowercase()).collect()
}

/// Whether a setting matches every search term across its path, label,
/// value, group, and documentation, mirroring the CLI's exact-terms filter.
pub(super) fn setting_matches_query(setting: &protocol::ConfigSetting, terms: &[String]) -> bool {
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

pub(super) fn value_summary(value: &Value) -> String {
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
