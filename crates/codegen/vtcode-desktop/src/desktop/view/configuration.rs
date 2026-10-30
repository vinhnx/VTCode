//! Searchable, typed edits for the CLI configuration shared with Desktop.

use super::config_search::{ConfigSearchCorpus, config_search_terms, haystack_matches_terms, setting_matches_query};
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
        self.refresh_config_catalog(cx);
        cx.notify();
    }

    /// Rebuild the memoized catalog grouping for the current search query and
    /// reset row paging to the first page. Called on every snapshot/query
    /// change: read responses, keystrokes via the search subscription, and
    /// programmatic query sets. The search corpus is rebuilt only when the
    /// snapshot epoch changed, so keystrokes filter without allocation.
    pub(super) fn refresh_config_catalog(&mut self, cx: &App) {
        let query = self.config_search_query(cx);
        let epoch = self.config_catalog_epoch;
        let corpus_fresh = self.cli_config_settings.as_ref().is_some_and(|snapshot| {
            self.config_search_corpus
                .as_ref()
                .is_some_and(|corpus| corpus.is_fresh(snapshot, epoch))
        });
        if !corpus_fresh {
            self.config_search_corpus = self
                .cli_config_settings
                .as_ref()
                .map(|snapshot| ConfigSearchCorpus::build(snapshot, epoch));
        }
        self.config_catalog_cache = match (self.cli_config_settings.as_ref(), self.config_search_corpus.as_ref()) {
            (Some(snapshot), Some(corpus)) => Some(build_config_catalog_cache(snapshot, corpus, &query, epoch)),
            _ => None,
        };
        self.config_visible_row_limit = CONFIG_GROUP_PAGE_SIZE;
    }

    /// Resolve a `/config <target>` deep link against the loaded catalog.
    /// Exact setting paths narrow the search to that setting, exact group
    /// names select the group, and anything else becomes a search query so
    /// the page shows the closest matches with a clear path back.
    pub(super) fn resolve_config_deep_link(&mut self, target: &str, window: &mut Window, cx: &mut Context<Self>) {
        if target.trim().eq_ignore_ascii_case(curated::CURATED_GROUP_NAME) {
            self.config_page_group = Some(curated::CURATED_GROUP_NAME.to_string());
            self.set_config_search_query("", window, cx);
            return;
        }
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

    /// The layer file can be opened whenever a backend is attached outside a
    /// settings write or reload.
    pub(super) fn can_open_config_file(&self) -> bool {
        self.backend.is_some()
            && self.workspace.is_some()
            && self.pending_workspace.is_none()
            && !self.config_write_pending
    }

    /// Open the selected write-target layer file in the platform default
    /// editor. The service resolves the path itself, so no filesystem path
    /// crosses the protocol boundary.
    pub(super) fn open_config_target_file(&mut self, cx: &mut Context<Self>) {
        if !self.can_open_config_file() {
            return;
        }
        let params = protocol::ConfigOpenFileParams { target: self.config_target };
        if self.request(protocol::method::CONFIG_OPEN_FILE, json!(params)) {
            self.status = "Opening vtcode.toml…".into();
            self.error = None;
        } else {
            self.error = Some(rust_i18n::t!("app.errors.request_queue_unavailable").to_string());
        }
        cx.notify();
    }

    /// Opening can create a never-written layer file, so refresh the catalog
    /// versions that guard the next write.
    pub(super) fn handle_config_open_file_response(&mut self, cx: &mut Context<Self>) {
        self.status = "Opened vtcode.toml in the default editor.".into();
        if !self.request(protocol::method::CONFIG_SETTINGS_READ, json!({})) {
            self.error = Some(rust_i18n::t!("app.errors.request_queue_unavailable").to_string());
        }
        cx.notify();
    }

    /// Flip network egress between allow-all and allowlist-only. The toggle
    /// is only offered for an enabled workspace-write sandbox.
    pub(super) fn set_network_access(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.save_cli_config_value(
            curated::NETWORK_POLICY_PATH.to_string(),
            curated::network_toggle_value(enabled),
            cx,
        );
    }

    /// Show or hide one reasoning effort level in model controls. The stored
    /// preference stays in canonical order no matter which switch moved.
    pub(super) fn set_available_effort(&mut self, level: &str, enabled: bool, cx: &mut Context<Self>) {
        let mut visible = self.preferences.available_efforts();
        if enabled {
            if !visible.iter().any(|entry| entry == level) {
                visible.push(level.to_string());
            }
        } else {
            visible.retain(|entry| entry != level);
        }
        let allowed = vtcode_commons::reasoning::ReasoningEffortLevel::allowed_values();
        visible.sort_by_key(|entry| allowed.iter().position(|known| known == entry).unwrap_or(usize::MAX));
        visible.dedup();
        self.preferences.available_efforts = Some(visible);
        self.preferences.save();
        cx.notify();
    }

    /// Effort levels currently shown in model controls: the saved selection
    /// intersected with route support, plus the active effort when supported.
    pub(super) fn visible_effort_levels(&self) -> Vec<String> {
        let Some(options) = self.session_options() else {
            return Vec::new();
        };
        curated::visible_effort_levels(
            &options.supported_efforts,
            &self.preferences.available_efforts(),
            &options.reasoning_effort,
        )
    }

    pub(super) fn approval_policy_label(&self) -> String {
        self.session_options().map_or_else(
            || "Unavailable".to_string(),
            |options| composer_actions::permission_mode_label(options.permission_mode).to_string(),
        )
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

    pub(super) fn handle_cli_config_read_response(&mut self, result: Value, cx: &App) {
        match serde_json::from_value::<protocol::ConfigSettingsSnapshot>(result) {
            Ok(snapshot) => {
                self.cli_config_settings = Some(snapshot);
                self.config_catalog_epoch = self.config_catalog_epoch.wrapping_add(1);
                // Keep the reader's page across reloads (post-write refresh
                // included); new queries and group switches reset via refresh.
                let limit = self.config_visible_row_limit;
                self.refresh_config_catalog(cx);
                self.config_visible_row_limit = limit.max(CONFIG_GROUP_PAGE_SIZE);
                if let Some(notice) = self.config_write_notice.take() {
                    self.status = format!("{notice} Settings reloaded for the next session.");
                }
            }
            Err(_) => {
                self.cli_config_settings = None;
                self.config_catalog_cache = None;
                self.config_search_corpus = None;
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

/// Catalog rows rendered per group before the "Show more" control. The
/// catalog holds ~900 settings, so an unbounded group would build thousands
/// of elements per render; paging keeps every frame under a fixed budget.
/// Twenty rows cover roughly two viewports while keeping frames cheap.
pub(super) const CONFIG_GROUP_PAGE_SIZE: usize = 20;

/// One filtered catalog group. Snapshot indices keep the cache small and let
/// render borrow current values without cloning settings.
#[derive(Clone, Debug, Default)]
pub(super) struct ConfigCatalogGroup {
    pub(super) name: String,
    pub(super) description: Option<String>,
    pub(super) indices: Vec<usize>,
}

/// Memoized catalog grouping for one snapshot epoch plus search query.
/// Rebuilt by `refresh_config_catalog` on every snapshot/query change so
/// render skips the O(settings) scan on unrelated updates (hover, focus,
/// target switches, editor open/close).
#[derive(Clone, Debug, Default)]
pub(super) struct ConfigCatalogCache {
    pub(super) query: String,
    pub(super) epoch: u64,
    pub(super) groups: Vec<ConfigCatalogGroup>,
    pub(super) total: usize,
    pub(super) editable_total: usize,
    pub(super) visible_count: usize,
}

impl ConfigCatalogCache {
    pub(super) fn is_fresh(&self, query: &str, epoch: u64) -> bool {
        self.epoch == epoch && self.query == query
    }

    pub(super) fn group(&self, name: &str) -> Option<&ConfigCatalogGroup> {
        self.groups.iter().find(|group| group.name == name)
    }
}

/// Pure grouping pass: filter by query, bucket by group name in catalog
/// order, and precompute the header counts. Shared by the cache refresh and
/// render's cold-start fallback so there is a single grouping code path.
/// The corpus must be fresh for `snapshot`/`epoch`; a stale entry falls back
/// to a direct match so grouping stays total.
pub(super) fn build_config_catalog_cache(
    snapshot: &protocol::ConfigSettingsSnapshot,
    corpus: &ConfigSearchCorpus,
    query: &str,
    epoch: u64,
) -> ConfigCatalogCache {
    debug_assert!(corpus.is_fresh(snapshot, epoch), "config search corpus must match the snapshot it filters");
    let terms = config_search_terms(query);
    let mut grouped = std::collections::BTreeMap::<&str, Vec<usize>>::new();
    for (index, setting) in snapshot.settings.iter().enumerate() {
        let matched = match corpus.haystacks.get(index) {
            Some(haystack) => terms.is_empty() || haystack_matches_terms(haystack, &terms),
            None => setting_matches_query(setting, &terms),
        };
        if matched {
            grouped.entry(config_group_name(setting)).or_default().push(index);
        }
    }
    let mut groups = Vec::with_capacity(grouped.len());
    let mut visible_count = 0;
    for (name, indices) in grouped {
        visible_count += indices.len();
        let description = indices
            .iter()
            .filter_map(|index| snapshot.settings.get(*index))
            .find_map(|setting| setting.group_description.clone());
        groups.push(ConfigCatalogGroup { name: name.to_owned(), description, indices });
    }
    let editable_total = snapshot.settings.iter().filter(|setting| setting.editable).count();
    ConfigCatalogCache {
        query: query.to_owned(),
        epoch,
        groups,
        total: snapshot.settings.len(),
        editable_total,
        visible_count,
    }
}

/// Rail icon per catalog group. Unknown groups (CLI additions, test fixtures)
/// fall back to a neutral glyph so the rail never breaks.
pub(super) fn config_group_icon(group: &str) -> IconName {
    if group == curated::CURATED_GROUP_NAME {
        return IconName::House;
    }
    match group {
        "Model & Provider" => IconName::Bot,
        "Agent & Automation" => IconName::RefreshCw,
        "Approvals & Security" => IconName::ShieldCheck,
        "Tools & Integrations" => IconName::Wrench,
        "Context & Memory" => IconName::FileText,
        "Interface & Terminal" => IconName::Terminal,
        "Performance & Diagnostics" => IconName::Activity,
        "Advanced settings" => IconName::Settings2,
        _ => IconName::Ellipsis,
    }
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
}
