//! Configurable desktop commands. Text editing and Kit focus bindings retain their own contexts.
use super::*;
use gpui_kit::component::kbd::Kbd;
use std::collections::{BTreeMap, HashSet};

mod commands;

pub(super) use commands::COMMANDS;

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
    BrowseThreads,
    GoHome,
}

impl CommandId {
    pub(super) const ALL: [Self; 36] = [
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
        Self::BrowseThreads,
        Self::GoHome,
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
            Self::BrowseThreads => "browse_threads",
            Self::GoHome => "go_home",
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

/// Static table entry for one command id. Every id has a row by construction;
/// the pinned table test in `shortcuts::tests` keeps that invariant honest, so
/// callers can use the first row only as an unreachable fallback instead of
/// panicking on a lookup that cannot fail.
pub(super) fn lookup(id: CommandId) -> &'static ShortcutCommand {
    let fallback = match &COMMANDS {
        [first, ..] => first,
    };
    COMMANDS.iter().find(|command| command.id == id).unwrap_or(fallback)
}

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
        CommandId::BrowseThreads => Box::new(OpenThreadBrowser),
        CommandId::GoHome => Box::new(GoHome),
    }
}

/// Fixed chords from the composer's inner `Input mode=editor` scope that are
/// also configurable defaults. GPUI ranks the deeper context first, so the
/// fixed binding owns each chord while the composer has focus even after its
/// owner is rebound. They cannot join [`bootstrap::FIXED_RESERVED_CHORDS`]
/// because that list rejects the defaults themselves; validation instead pins
/// each chord to its owner. Keep in sync with the three rows scoped to
/// `Input && mode == editor` in [`bootstrap::init_keybindings`].
pub(super) const EDITOR_CONTEXT_CHORDS: [(&str, CommandId); 3] = [
    ("cmd-.", CommandId::Cancel),
    ("cmd-[", CommandId::NavigateBack),
    ("cmd-]", CommandId::NavigateForward),
];

/// Normalized chord for a saved source, or `None` when empty/unparsable.
/// Keeps override/default collision checks identical to binding validation.
fn normalized_chord(source: &str) -> Option<String> {
    if source.is_empty() {
        return None;
    }
    let os = host_operating_system();
    gpui_kit::Keystroke::parse_with_os(source, os).ok().map(|key| key.unparse(os))
}

/// A default newly introduced in a later release can collide with a chord a
/// user already assigned to another command. Their override wins: leave the
/// new command unbound instead of rejecting the whole table and silently
/// reverting every customization on launch.
fn suppressed_default(overrides: &BTreeMap<String, String>, command: &ShortcutCommand) -> bool {
    if overrides.contains_key(command.id.as_str()) {
        return false;
    }
    let Some(default) = normalized_chord(command.default_binding) else {
        return false;
    };
    overrides
        .iter()
        .any(|(id, source)| id != command.id.as_str() && normalized_chord(source).is_some_and(|chord| chord == default))
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
            let source = staged_value(overrides, command);
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
            // A deeper composer binding would silently consume the chord
            // whenever the composer has focus, so these stay with the command
            // whose default they are (see [`EDITOR_CONTEXT_CHORDS`]).
            let editor_owner = EDITOR_CONTEXT_CHORDS
                .iter()
                .find(|(chord, _)| *chord == normalized.as_str())
                .map(|(_, owner)| *owner);
            if editor_owner.is_some_and(|owner| owner != command.id) {
                return Err(rust_i18n::t!("app.shortcuts.reserved_shortcut", label = label.as_str()).to_string());
            }
            if !(command.id == CommandId::Dismiss && source == key::ESCAPE)
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

/// Staged binding for one command: the override when present, else the
/// default, unless that default collides with a user override (see
/// [`suppressed_default`]).
pub(super) fn staged_value<'a>(overrides: &'a BTreeMap<String, String>, command: &ShortcutCommand) -> &'a str {
    if suppressed_default(overrides, command) {
        return "";
    }
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
mod tests;
