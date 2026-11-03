//! Configurable desktop commands. Text editing and Kit focus bindings retain their own contexts.
use super::*;
use gpui_kit::component::kbd::Kbd;
use std::collections::{BTreeMap, HashSet};

/// Stable shortcut-command identity.
///
/// Ids persist in preferences (`Preferences::shortcuts`), so never rename an
/// existing id: [`COMMANDS`] pins the full id table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CommandId {
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
    BackgroundOperation,
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
    QueueBatched,
    PromptHistory,
    EditQueue,
}

impl CommandId {
    pub(super) const ALL: [Self; 34] = [
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
        Self::BackgroundOperation,
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
        Self::QueueBatched,
        Self::PromptHistory,
        Self::EditQueue,
    ];

    pub(super) const fn as_str(self) -> &'static str {
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
            Self::BackgroundOperation => "background_operation",
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
            Self::QueueBatched => "queue_batched",
            Self::PromptHistory => "prompt_history",
            Self::EditQueue => "edit_queue",
        }
    }

    pub(super) fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|known| id == known.as_str())
    }
}

/// One configurable desktop command: stable id, locale keys for the settings
/// label and description, and the default binding. Copy resolves at the render
/// call site so the const table stays locale-independent.
#[derive(Clone, Copy, Debug)]
pub(super) struct ShortcutCommand {
    pub(super) id: CommandId,
    pub(super) label_key: &'static str,
    pub(super) description_key: &'static str,
    pub(super) default_binding: &'static str,
}

impl ShortcutCommand {
    /// Settings label in the active locale.
    pub(super) fn label(&self) -> String {
        let key = self.label_key;
        rust_i18n::t!(key).to_string()
    }

    /// Settings description in the active locale.
    pub(super) fn description(&self) -> String {
        let key = self.description_key;
        rust_i18n::t!(key).to_string()
    }

    /// Settings label pinned to one locale, for tests that must not observe
    /// the process-global locale while other tests switch languages.
    #[cfg(test)]
    pub(super) fn label_in(&self, locale: &str) -> String {
        let key = self.label_key;
        rust_i18n::t!(key, locale = locale).to_string()
    }

    /// Settings description pinned to one locale (see [`Self::label_in`]).
    #[cfg(test)]
    pub(super) fn description_in(&self, locale: &str) -> String {
        let key = self.description_key;
        rust_i18n::t!(key, locale = locale).to_string()
    }
}

pub(super) const COMMANDS: [ShortcutCommand; 34] = [
    ShortcutCommand {
        id: CommandId::Commands,
        label_key: "app.shortcuts.commands_label",
        description_key: "app.shortcuts.commands_description",
        default_binding: "cmd-k",
    },
    ShortcutCommand {
        id: CommandId::ConversationSearch,
        label_key: "app.shortcuts.conversation_search_label",
        description_key: "app.shortcuts.conversation_search_description",
        default_binding: "cmd-shift-f",
    },
    ShortcutCommand {
        id: CommandId::NewSession,
        label_key: "app.shortcuts.new_session_label",
        description_key: "app.shortcuts.new_session_description",
        default_binding: "cmd-n",
    },
    ShortcutCommand {
        id: CommandId::CloseSession,
        label_key: "app.shortcuts.close_session_label",
        description_key: "app.shortcuts.close_session_description",
        default_binding: "cmd-shift-w",
    },
    ShortcutCommand {
        id: CommandId::Workspace,
        label_key: "app.shortcuts.workspace_label",
        description_key: "app.shortcuts.workspace_description",
        default_binding: "cmd-o",
    },
    ShortcutCommand {
        id: CommandId::Composer,
        label_key: "app.shortcuts.composer_label",
        description_key: "app.shortcuts.composer_description",
        default_binding: "cmd-l",
    },
    ShortcutCommand {
        id: CommandId::Sidebar,
        label_key: "app.shortcuts.sidebar_label",
        description_key: "app.shortcuts.sidebar_description",
        default_binding: "cmd-b",
    },
    ShortcutCommand {
        id: CommandId::Inspector,
        label_key: "app.shortcuts.inspector_label",
        description_key: "app.shortcuts.inspector_description",
        default_binding: "cmd-shift-i",
    },
    ShortcutCommand {
        id: CommandId::ZenMode,
        label_key: "app.shortcuts.zen_mode_label",
        description_key: "app.shortcuts.zen_mode_description",
        default_binding: "ctrl-cmd-z",
    },
    ShortcutCommand {
        id: CommandId::Cancel,
        label_key: "app.shortcuts.cancel_label",
        description_key: "app.shortcuts.cancel_description",
        default_binding: "cmd-.",
    },
    ShortcutCommand {
        id: CommandId::BackgroundOperation,
        label_key: "app.shortcuts.background_operation_label",
        description_key: "app.shortcuts.background_operation_description",
        default_binding: "cmd-shift-d",
    },
    ShortcutCommand {
        id: CommandId::Dismiss,
        label_key: "app.shortcuts.dismiss_label",
        description_key: "app.shortcuts.dismiss_description",
        default_binding: "escape",
    },
    ShortcutCommand {
        id: CommandId::Settings,
        label_key: "app.shortcuts.settings_label",
        description_key: "app.shortcuts.settings_description",
        default_binding: "cmd-,",
    },
    ShortcutCommand {
        id: CommandId::SettingsSearch,
        label_key: "app.shortcuts.settings_search_label",
        description_key: "app.shortcuts.settings_search_description",
        default_binding: "cmd-f",
    },
    ShortcutCommand {
        id: CommandId::Model,
        label_key: "app.shortcuts.model_label",
        description_key: "app.shortcuts.model_description",
        default_binding: "cmd-shift-m",
    },
    ShortcutCommand {
        id: CommandId::Permissions,
        label_key: "app.shortcuts.permissions_label",
        description_key: "app.shortcuts.permissions_description",
        default_binding: "cmd-shift-p",
    },
    ShortcutCommand {
        id: CommandId::AgentMode,
        label_key: "app.shortcuts.agent_mode_label",
        description_key: "app.shortcuts.agent_mode_description",
        default_binding: "cmd-shift-b",
    },
    ShortcutCommand {
        id: CommandId::AddContext,
        label_key: "app.shortcuts.add_context_label",
        description_key: "app.shortcuts.add_context_description",
        default_binding: "cmd-shift-a",
    },
    ShortcutCommand {
        id: CommandId::StatusBar,
        label_key: "app.shortcuts.status_bar_label",
        description_key: "app.shortcuts.status_bar_description",
        default_binding: "cmd-shift-s",
    },
    ShortcutCommand {
        id: CommandId::ReloadCliSettings,
        label_key: "app.shortcuts.reload_cli_settings_label",
        description_key: "app.shortcuts.reload_cli_settings_description",
        default_binding: "cmd-shift-r",
    },
    ShortcutCommand {
        id: CommandId::SyncCliTheme,
        label_key: "app.shortcuts.sync_cli_theme_label",
        description_key: "app.shortcuts.sync_cli_theme_description",
        default_binding: "cmd-shift-t",
    },
    ShortcutCommand {
        id: CommandId::Utilities,
        label_key: "app.shortcuts.utilities_label",
        description_key: "app.shortcuts.utilities_description",
        default_binding: "cmd-shift-u",
    },
    ShortcutCommand {
        id: CommandId::WorkspaceFiles,
        label_key: "app.shortcuts.workspace_files_label",
        description_key: "app.shortcuts.workspace_files_description",
        default_binding: "cmd-shift-e",
    },
    ShortcutCommand {
        id: CommandId::WorkspaceReview,
        label_key: "app.shortcuts.workspace_review_label",
        description_key: "app.shortcuts.workspace_review_description",
        default_binding: "cmd-shift-g",
    },
    ShortcutCommand {
        id: CommandId::RefreshWorkspace,
        label_key: "app.shortcuts.refresh_workspace_label",
        description_key: "app.shortcuts.refresh_workspace_description",
        default_binding: "cmd-r",
    },
    ShortcutCommand {
        id: CommandId::NavigateBack,
        label_key: "app.shortcuts.navigate_back_label",
        description_key: "app.shortcuts.navigate_back_description",
        default_binding: "cmd-[",
    },
    ShortcutCommand {
        id: CommandId::NavigateForward,
        label_key: "app.shortcuts.navigate_forward_label",
        description_key: "app.shortcuts.navigate_forward_description",
        default_binding: "cmd-]",
    },
    ShortcutCommand {
        id: CommandId::NextReviewChange,
        label_key: "app.shortcuts.next_review_change_label",
        description_key: "app.shortcuts.next_review_change_description",
        default_binding: "f7",
    },
    ShortcutCommand {
        id: CommandId::PreviousReviewChange,
        label_key: "app.shortcuts.previous_review_change_label",
        description_key: "app.shortcuts.previous_review_change_description",
        default_binding: "shift-f7",
    },
    ShortcutCommand {
        id: CommandId::NextReviewFile,
        label_key: "app.shortcuts.next_review_file_label",
        description_key: "app.shortcuts.next_review_file_description",
        default_binding: "alt-f7",
    },
    ShortcutCommand {
        id: CommandId::PreviousReviewFile,
        label_key: "app.shortcuts.previous_review_file_label",
        description_key: "app.shortcuts.previous_review_file_description",
        default_binding: "alt-shift-f7",
    },
    ShortcutCommand {
        id: CommandId::QueueBatched,
        label_key: "app.shortcuts.queue_batched_label",
        description_key: "app.shortcuts.queue_batched_description",
        default_binding: "cmd-enter",
    },
    ShortcutCommand {
        id: CommandId::PromptHistory,
        label_key: "app.shortcuts.prompt_history_label",
        description_key: "app.shortcuts.prompt_history_description",
        default_binding: "ctrl-r",
    },
    ShortcutCommand {
        id: CommandId::EditQueue,
        label_key: "app.shortcuts.edit_queue_label",
        description_key: "app.shortcuts.edit_queue_description",
        default_binding: "alt-up",
    },
];

pub(super) fn action(id: CommandId) -> Box<dyn gpui_kit::Action> {
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
        CommandId::BackgroundOperation => Box::new(BackgroundOperation),
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
        CommandId::QueueBatched => Box::new(QueueBatched),
        CommandId::PromptHistory => Box::new(OpenPromptHistory),
        CommandId::EditQueue => Box::new(TakeLatestQueued),
    }
}

pub(super) fn bindings(overrides: &BTreeMap<String, String>, cx: &App) -> Result<Vec<KeyBinding>, String> {
    if overrides.keys().any(|key| CommandId::from_id(key).is_none()) {
        return Err(rust_i18n::t!("app.shortcuts.unknown_action").to_string());
    }
    // Validation normalizes against the host OS so headed and headless runs
    // agree; loading resolves through the live keyboard mapper below.
    let os = host_operating_system();
    let mut seen = HashSet::new();
    COMMANDS
        .iter()
        .map(|command| {
            let source = overrides
                .get(command.id.as_str())
                .map(String::as_str)
                .unwrap_or(command.default_binding);
            let label = command.label();
            if source.is_empty() {
                return Ok(Vec::new());
            }
            if source.trim().is_empty() || source.split_whitespace().count() != 1 {
                return Err(rust_i18n::t!("app.shortcuts.enter_one_combo", label = label.as_str()).to_string());
            }
            let key = gpui_kit::Keystroke::parse_with_os(source, os).map_err(|error| {
                rust_i18n::t!("app.shortcuts.invalid_combo", label = label.as_str(), error = error.to_string())
                    .to_string()
            })?;
            let normalized = key.unparse(os);
            if [
                "cmd-q",
                "cmd-w",
                "cmd-m",
                "cmd-h",
                "alt-cmd-h",
                "cmd-shift-n",
                "ctrl-cmd-f",
                "cmd-c",
                "cmd-x",
                "cmd-v",
                "cmd-a",
                "cmd-z",
                "cmd-shift-z",
            ]
            .contains(&normalized.as_str())
                || bootstrap::FIXED_RESERVED_CHORDS.contains(&normalized.as_str())
            {
                return Err(rust_i18n::t!("app.shortcuts.reserved_shortcut", label = label.as_str()).to_string());
            }
            if !(command.id == CommandId::Dismiss && source == "escape")
                && !source.contains("cmd-")
                && !source.contains("ctrl-")
                && !source.contains("alt-")
                && !source.contains("secondary-")
                && !is_function_key(&normalized)
            {
                return Err(rust_i18n::t!("app.shortcuts.include_modifier", label = label.as_str()).to_string());
            }
            if !seen.insert(normalized) {
                return Err(rust_i18n::t!("app.shortcuts.already_assigned", label = label.as_str()).to_string());
            }
            let context = gpui_kit::KeyBindingContextPredicate::parse(context_for(command.id))
                .map_err(|error| error.to_string())?;
            let mut bindings = vec![
                KeyBinding::load(source, action(command.id), Some(context.into()), false, None, cx).map_err(
                    |error| {
                        rust_i18n::t!(
                            "app.shortcuts.invalid_shortcut",
                            label = label.as_str(),
                            error = error.to_string()
                        )
                        .to_string()
                    },
                )?,
            ];
            if command.id == CommandId::SettingsSearch || command.id == CommandId::Settings {
                let context = gpui_kit::KeyBindingContextPredicate::parse(key_context::SETTINGS)
                    .map_err(|error| error.to_string())?;
                bindings.push(
                    KeyBinding::load(source, action(command.id), Some(context.into()), false, None, cx).map_err(
                        |error| {
                            rust_i18n::t!(
                                "app.shortcuts.invalid_shortcut",
                                label = label.as_str(),
                                error = error.to_string()
                            )
                            .to_string()
                        },
                    )?,
                );
            }
            Ok(bindings)
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|bindings| bindings.into_iter().flatten().collect())
}

/// `NoAction` shadows for chords a saved table retires. GPUI keymaps only
/// append, so rebinding after a save would otherwise leave removed or rebound
/// chords firing their stale action until restart. Callers bind these shadows
/// before the fresh table: same-chord conflicts resolve toward the latest
/// binding, so shared chords take the new action while dropped chords stay
/// dead. A table startup rejected was never bound (defaults were), so retired
/// chords diff against defaults then; chords that no longer parse are skipped
/// because they were never bound either.
pub(super) fn stale_shadows(
    old_overrides: &BTreeMap<String, String>,
    new_overrides: &BTreeMap<String, String>,
    cx: &App,
) -> Vec<KeyBinding> {
    let defaults = BTreeMap::new();
    let old = if bindings(old_overrides, cx).is_ok() {
        old_overrides
    } else {
        &defaults
    };
    let mut shadows = Vec::new();
    for command in COMMANDS.iter() {
        let retired = staged_value(old, command);
        if retired.is_empty() || retired == staged_value(new_overrides, command) {
            continue;
        }
        // Mirrors `bindings()`: Settings and SettingsSearch bind twice, once
        // per context, so both stale chords need a shadow. Keep in sync.
        let mut contexts = vec![context_for(command.id)];
        if command.id == CommandId::Settings || command.id == CommandId::SettingsSearch {
            contexts.push(key_context::SETTINGS);
        }
        shadows.extend(contexts.into_iter().filter_map(|context| {
            let predicate = gpui_kit::KeyBindingContextPredicate::parse(context).ok()?;
            KeyBinding::load(retired, Box::new(NoAction {}), Some(predicate.into()), false, None, cx).ok()
        }));
    }
    shadows
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

/// Full staged table: every command id maps to its override or default.
/// Empty values unbind the command. The draft always carries all ids so
/// staged edits validate and persist without re-merging defaults.
pub(super) fn draft_from(overrides: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    COMMANDS
        .iter()
        .map(|command| (command.id.as_str().to_string(), staged_value(overrides, command).to_string()))
        .collect()
}

/// Staged binding for one command: the override when present, else the default.
pub(super) fn staged_value<'a>(overrides: &'a BTreeMap<String, String>, command: &ShortcutCommand) -> &'a str {
    overrides
        .get(command.id.as_str())
        .map(String::as_str)
        .unwrap_or(command.default_binding)
}

/// Whether a command matches every search term across its label, description,
/// and staged binding, mirroring the CLI config exact-terms filter.
pub(super) fn shortcut_matches_query(command: &ShortcutCommand, staged: &str, terms: &[String]) -> bool {
    if terms.is_empty() {
        return true;
    }
    let haystack =
        format!("{} {} {staged} {}", command.label(), command.description(), command.default_binding).to_lowercase();
    terms.iter().all(|term| haystack.contains(term))
}

/// Host OS for keystroke parsing/formatting in contexts without an `App`.
/// Matches `cx.platform_info().os` on native targets; unknown platforms fall
/// back to `Unknown`, where `secondary-` resolves to ctrl.
pub(super) fn host_operating_system() -> gpui_kit::OperatingSystem {
    match std::env::consts::OS {
        "macos" => gpui_kit::OperatingSystem::Mac,
        "windows" => gpui_kit::OperatingSystem::Windows,
        "linux" => gpui_kit::OperatingSystem::Linux,
        "freebsd" => gpui_kit::OperatingSystem::FreeBsd,
        "ios" => gpui_kit::OperatingSystem::Ios,
        "android" => gpui_kit::OperatingSystem::Android,
        _ => gpui_kit::OperatingSystem::Unknown,
    }
}

/// Bare-modifier key names reported by GPUI key events. Capture ignores them
/// and keeps waiting for the completed chord instead of flashing a rejection.
pub(super) fn is_modifier_key(key: &str) -> bool {
    matches!(
        key,
        "shift" | "control" | "ctrl" | "alt" | "option" | "cmd" | "command" | "meta" | "function" | "fn" | "platform"
    )
}

pub(super) fn shortcut_accessibility_value(value: &str) -> String {
    if value.is_empty() {
        return rust_i18n::t!("app.shortcuts.a11y_not_set").to_string();
    }

    gpui_kit::Keystroke::parse_with_os(value, host_operating_system())
        .map(|keystroke| Kbd::format(&keystroke))
        .unwrap_or_else(|_| rust_i18n::t!("app.shortcuts.a11y_invalid").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::TestAppContext;

    #[test]
    fn accessible_shortcut_labels_use_the_platform_formatted_key() {
        let keystroke = gpui_kit::Keystroke::parse("cmd-shift-p").unwrap();
        assert_eq!(shortcut_accessibility_value("cmd-shift-p"), Kbd::format(&keystroke));
        assert_eq!(shortcut_accessibility_value(""), "not set");
        assert_eq!(shortcut_accessibility_value("a-b"), "invalid shortcut");
    }

    #[gpui_kit::test]
    fn rejects_conflicting_reserved_typing_and_unknown_shortcuts(cx: &mut TestAppContext) {
        cx.update(|cx| {
            assert_eq!(bindings(&BTreeMap::new(), cx).unwrap().len(), 36);
            assert!(bindings(&BTreeMap::from([("commands".into(), "ctrl-shift-k".into())]), cx).is_ok());
            assert!(bindings(&BTreeMap::from([("settings_search".into(), "ctrl-alt-f".into())]), cx).is_ok());
            assert!(bindings(&BTreeMap::from([("zen_mode".into(), "ctrl-alt-z".into())]), cx).is_ok());
            assert_eq!(bindings(&BTreeMap::from([("model".into(), String::new())]), cx).unwrap().len(), 35);
            assert_eq!(
                bindings(&BTreeMap::from([("settings_search".into(), String::new())]), cx)
                    .unwrap()
                    .len(),
                34
            );
            assert_eq!(
                bindings(&BTreeMap::from([("settings".into(), String::new())]), cx)
                    .unwrap()
                    .len(),
                34,
                "unbinding Settings removes both its desktop and in-settings bindings"
            );
            assert!(bindings(&BTreeMap::from([("dismiss".into(), "x".into())]), cx).is_err());
            for (action, key) in [
                ("commands", "cmd-n"),
                ("commands", "cmd-q"),
                ("commands", "k"),
                ("zen_mode", "cmd-shift-z"),
                ("unknown", "ctrl-k"),
            ] {
                assert!(bindings(&BTreeMap::from([(action.into(), key.into())]), cx).is_err(), "{action}: {key}");
            }
        });
    }

    #[gpui_kit::test]
    fn rejects_overrides_colliding_with_fixed_chords(cx: &mut TestAppContext) {
        cx.update(|cx| {
            // Every modified fixed-table chord must be rejected: the deeper fixed
            // binding would shadow the rebind (e.g. `ctrl-m` opening the model
            // picker instead of the rebound command in the composer). CLI import
            // funnels through the same `bindings()` gate, so it is covered too.
            for key in bootstrap::FIXED_RESERVED_CHORDS {
                assert!(
                    bindings(&BTreeMap::from([("model".into(), (*key).into())]), cx).is_err(),
                    "{key} must be reserved"
                );
            }
            // Near-miss neighbors stay accepted (asymmetric sides).
            assert!(bindings(&BTreeMap::from([("model".into(), "ctrl-shift-m".into())]), cx).is_ok());
            assert!(bindings(&BTreeMap::from([("commands".into(), "ctrl-alt-p".into())]), cx).is_ok());
            assert!(bindings(&BTreeMap::from([("settings".into(), "cmd-shift-]".into())]), cx).is_ok());
        });
    }

    #[gpui_kit::test]
    fn review_navigation_accepts_configurable_function_key_bindings(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let configured = BTreeMap::from([
                ("next_review_change".into(), "f8".into()),
                ("previous_review_change".into(), "shift-f8".into()),
                ("next_review_file".into(), "alt-f9".into()),
                ("previous_review_file".into(), "alt-shift-f9".into()),
            ]);
            assert!(bindings(&configured, cx).is_ok());
            assert_eq!(context_for(CommandId::NextReviewChange), "VTCodeDiffReview");
            assert_eq!(context_for(CommandId::PreviousReviewFile), "VTCodeDiffReview");
            assert_eq!(context_for(CommandId::NavigateBack), "VTCodeDesktop");
            assert!(bindings(&BTreeMap::from([("next_review_change".into(), "f25".into())]), cx).is_err());
            assert!(
                bindings(
                    &BTreeMap::from([
                        ("next_review_change".into(), "f8".into()),
                        ("previous_review_change".into(), "f8".into()),
                    ]),
                    cx
                )
                .is_err()
            );
        });
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
            "background_operation",
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
            "queue_batched",
            "prompt_history",
            "edit_queue",
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

    #[test]
    fn commands_carry_unique_labels_descriptions_and_parsable_defaults() {
        use std::collections::HashSet;

        let mut labels = HashSet::new();
        for command in COMMANDS {
            let label = command.label_in("en");
            let description = command.description_in("en");
            assert!(!label.is_empty(), "{:?} needs a label", command.id);
            assert!(!description.is_empty(), "{:?} needs a description", command.id);
            assert!(labels.insert(label.clone()), "duplicate label: {label}");
            assert!(
                gpui_kit::Keystroke::parse(command.default_binding).is_ok(),
                "{:?} default must parse: {}",
                command.id,
                command.default_binding
            );
        }
    }

    #[test]
    fn draft_from_fills_defaults_and_keeps_overrides() {
        let draft = draft_from(&BTreeMap::new());
        assert_eq!(draft.len(), COMMANDS.len());
        assert_eq!(draft.get("commands").map(String::as_str), Some("cmd-k"));
        assert_eq!(draft.get("previous_review_file").map(String::as_str), Some("alt-shift-f7"));

        let draft = draft_from(&BTreeMap::from([
            ("commands".into(), "ctrl-shift-k".into()),
            ("model".into(), String::new()),
        ]));
        assert_eq!(draft.len(), COMMANDS.len());
        assert_eq!(draft.get("commands").map(String::as_str), Some("ctrl-shift-k"));
        assert_eq!(draft.get("model").map(String::as_str), Some(""));
        assert_eq!(draft.get("zen_mode").map(String::as_str), Some("ctrl-cmd-z"));

        let zen = COMMANDS.iter().find(|command| command.id == CommandId::ZenMode).unwrap();
        assert_eq!(staged_value(&BTreeMap::new(), zen), "ctrl-cmd-z");
        let overrides = BTreeMap::from([("zen_mode".into(), "ctrl-alt-z".into())]);
        assert_eq!(staged_value(&overrides, zen), "ctrl-alt-z");
    }

    #[test]
    fn shortcut_search_matches_label_description_and_binding() {
        use super::super::config_search::config_search_terms;

        let zen = COMMANDS.iter().find(|command| command.id == CommandId::ZenMode).unwrap();
        let commands = COMMANDS.iter().find(|command| command.id == CommandId::Commands).unwrap();
        let model = COMMANDS.iter().find(|command| command.id == CommandId::Model).unwrap();

        assert!(shortcut_matches_query(zen, "ctrl-cmd-z", &[]));
        // Label-only term: "zen" hits Zen mode, not the palette command.
        let terms = config_search_terms("zen");
        assert!(shortcut_matches_query(zen, "ctrl-cmd-z", &terms));
        assert!(!shortcut_matches_query(commands, "cmd-k", &terms));
        // Description-only term: "palette" hits Search commands, not Choose model.
        let terms = config_search_terms("palette");
        assert!(shortcut_matches_query(commands, "cmd-k", &terms));
        assert!(!shortcut_matches_query(model, "cmd-shift-m", &terms));
        // Every term must match: "toggle zen" hits Zen mode, "toggle palette" hits nothing.
        let terms = config_search_terms("toggle zen");
        assert!(shortcut_matches_query(zen, "ctrl-cmd-z", &terms));
        assert!(!shortcut_matches_query(commands, "cmd-k", &terms));
        let terms = config_search_terms("toggle palette");
        assert!(!shortcut_matches_query(zen, "ctrl-cmd-z", &terms));
        assert!(!shortcut_matches_query(commands, "cmd-k", &terms));
        // Case-insensitive, and staged bindings match by their stored text.
        let terms = config_search_terms("ZEN");
        assert!(shortcut_matches_query(zen, "ctrl-cmd-z", &terms));
        let terms = config_search_terms("ctrl-shift-k");
        assert!(shortcut_matches_query(commands, "ctrl-shift-k", &terms));
        assert!(!shortcut_matches_query(commands, "cmd-k", &terms));
    }

    #[test]
    fn capture_ignores_bare_modifiers_but_keeps_real_keys() {
        for key in [
            "shift", "control", "ctrl", "alt", "option", "cmd", "command", "meta", "function", "fn", "platform",
        ] {
            assert!(is_modifier_key(key), "{key} must not commit a capture");
        }
        for key in ["a", ";", "escape", "enter", "space", "f7", "f12", "[", ",", "."] {
            assert!(!is_modifier_key(key), "{key} must commit a capture attempt");
        }
    }

    #[test]
    fn captured_chords_round_trip_through_parse() {
        let os = host_operating_system();
        for chord in ["cmd-shift-k", "ctrl-cmd-z", "escape", "f7", "alt-shift-f7", "cmd-["] {
            let once = gpui_kit::Keystroke::parse_with_os(chord, os).unwrap().unparse(os);
            let twice = gpui_kit::Keystroke::parse_with_os(&once, os).unwrap().unparse(os);
            assert_eq!(twice, once, "{chord} must survive a capture round-trip");
        }
    }

    #[gpui_kit::test]
    fn secondary_alias_resolves_to_the_host_modifier(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let loaded = bindings(&BTreeMap::from([("model".into(), "secondary-s".into())]), cx)
                .expect("secondary- resolves on the host OS");
            assert_eq!(loaded.len(), 36, "full table builds with the secondary override");
            #[cfg(target_os = "macos")]
            let expected = "cmd-s";
            #[cfg(not(target_os = "macos"))]
            let expected = "ctrl-s";
            assert_eq!(
                gpui_kit::Keystroke::parse_with_os("secondary-s", host_operating_system())
                    .unwrap()
                    .unparse(host_operating_system()),
                expected
            );
        });
    }

    use super::super::tests::{assert_chord_dead, assert_chord_resolves_once_to, assert_chord_resolves_to};

    /// Mirror a save reinstall through the live keymap: old table, shadows,
    /// then the fresh table, exactly as `bootstrap::reinstall_shortcuts` binds.
    fn reinstall(old: &BTreeMap<String, String>, new: &BTreeMap<String, String>, cx: &mut App) {
        cx.bind_keys(bindings(old, cx).expect("old table builds"));
        cx.bind_keys(stale_shadows(old, new, cx));
        cx.bind_keys(bindings(new, cx).expect("new table builds"));
    }

    #[gpui_kit::test]
    fn reinstall_retires_rebound_chord_and_keeps_new_live(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let old = BTreeMap::from([("model".into(), "ctrl-shift-m".into())]);
            let new = BTreeMap::from([("model".into(), "ctrl-alt-m".into())]);
            reinstall(&old, &new, cx);
            assert_chord_dead(cx, "ctrl-shift-m", key_context::DESKTOP);
            assert_chord_resolves_once_to(cx, "ctrl-alt-m", key_context::DESKTOP, &OpenModelPicker);
            assert_chord_resolves_to(cx, "cmd-k", key_context::DESKTOP, &OpenCommandPalette);
        });
    }

    #[gpui_kit::test]
    fn reinstall_retires_unbound_chord_without_touching_neighbors(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let old = BTreeMap::from([("model".into(), "ctrl-shift-m".into())]);
            let new = BTreeMap::from([("model".into(), String::new())]);
            reinstall(&old, &new, cx);
            assert_chord_dead(cx, "ctrl-shift-m", key_context::DESKTOP);
            assert_chord_resolves_to(cx, "cmd-k", key_context::DESKTOP, &OpenCommandPalette);
            assert_chord_resolves_to(cx, "cmd-,", key_context::DESKTOP, &OpenSettings);
        });
    }

    #[gpui_kit::test]
    fn unchanged_tables_emit_no_shadows(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let sparse = BTreeMap::from([("model".into(), "ctrl-shift-m".into())]);
            assert!(stale_shadows(&sparse, &sparse, cx).is_empty());
            // The staged draft is a full table while disk holds sparse
            // overrides: identical effective chords still mean no shadows.
            let full = draft_from(&sparse);
            assert!(stale_shadows(&sparse, &full, cx).is_empty());
            assert!(stale_shadows(&BTreeMap::new(), &BTreeMap::new(), cx).is_empty());
        });
    }

    #[gpui_kit::test]
    fn invalid_old_table_retires_against_startup_defaults(cx: &mut TestAppContext) {
        cx.update(|cx| {
            // Startup rejects the garbage table and binds defaults instead,
            // so the reinstall must retire the default chord, not the garbage.
            let old = BTreeMap::from([("model".into(), "a-b".into())]);
            let new = BTreeMap::from([("model".into(), "ctrl-shift-m".into())]);
            assert!(bindings(&old, cx).is_err(), "fixture must be invalid");
            cx.bind_keys(bindings(&BTreeMap::new(), cx).expect("defaults build"));
            cx.bind_keys(stale_shadows(&old, &new, cx));
            cx.bind_keys(bindings(&new, cx).expect("new table builds"));
            assert_chord_dead(cx, "cmd-shift-m", key_context::DESKTOP);
            assert_chord_resolves_once_to(cx, "ctrl-shift-m", key_context::DESKTOP, &OpenModelPicker);
        });
    }

    #[gpui_kit::test]
    fn settings_rebind_shadows_both_contexts(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let old = BTreeMap::new();
            let new = BTreeMap::from([("settings".into(), "ctrl-,".into())]);
            let shadows = stale_shadows(&old, &new, cx);
            assert_eq!(shadows.len(), 2, "the dual-context binding retires twice");
            reinstall(&old, &new, cx);
            for context in [key_context::DESKTOP, key_context::SETTINGS] {
                assert_chord_dead(cx, "cmd-,", context);
                assert_chord_resolves_once_to(cx, "ctrl-,", context, &OpenSettings);
            }
            // The untouched twin keeps its own chord live in both contexts.
            for context in [key_context::DESKTOP, key_context::SETTINGS] {
                assert_chord_resolves_to(cx, "cmd-f", context, &OpenSettingsSearch);
            }
        });
    }

    #[gpui_kit::test]
    fn retiring_the_full_table_shadows_every_binding(cx: &mut TestAppContext) {
        cx.update(|cx| {
            // Structural pin: retiring everything must shadow every binding,
            // including dual-context commands. If `bindings()` gains a
            // context, this fails until `stale_shadows` mirrors it.
            let unbind_all: BTreeMap<String, String> = COMMANDS
                .iter()
                .map(|command| (command.id.as_str().to_string(), String::new()))
                .collect();
            let defaults = bindings(&BTreeMap::new(), cx).expect("defaults build");
            let shadows = stale_shadows(&BTreeMap::new(), &unbind_all, cx);
            assert_eq!(shadows.len(), defaults.len(), "one shadow per bound chord");
            cx.bind_keys(defaults);
            cx.bind_keys(shadows);
            for command in COMMANDS.iter() {
                assert_chord_dead(cx, command.default_binding, context_for(command.id));
            }
        });
    }
}
