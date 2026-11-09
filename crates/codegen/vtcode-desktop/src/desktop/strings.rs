//! Shared string vocabulary for the desktop client.
//!
//! Every literal here is used from at least two production sites, or belongs
//! to a protocol/bundle contract table whose keys must stay spelled as one
//! (a typo breaks the service silently). Other single-use literals stay at
//! their use site; test fixtures keep their own literals as independent
//! oracles.

/// JSON-RPC envelope and protocol param keys.
///
/// Read/write keys are shared by the view and the transport; single-side keys
/// face the backend's typed params. A typo anywhere breaks request handling
/// silently, so all sides reference this table. Wire-format test fixtures
/// keep literals.
pub(super) mod keys {
    pub const JSONRPC: &str = "jsonrpc";
    pub const JSONRPC_VERSION: &str = "2.0";
    pub const ID: &str = "id";
    pub const METHOD: &str = "method";
    pub const PARAMS: &str = "params";
    pub const RESULT: &str = "result";
    pub const ERROR: &str = "error";
    pub const CODE: &str = "code";
    pub const MESSAGE: &str = "message";
    pub const SESSION_ID: &str = "sessionId";
    pub const REQUEST_ID: &str = "requestId";
    pub const TURN_ID: &str = "turnId";
    pub const INTERACTION_ID: &str = "interactionId";
    pub const CANCELLED: &str = "cancelled";
    pub const PROMPT: &str = "prompt";
    pub const ATTACHMENTS: &str = "attachments";
    pub const BATCHABLE: &str = "batchable";
    pub const ENTRY_ID: &str = "entryId";
    pub const CLEAR_QUEUE: &str = "clearQueue";
    pub const BEFORE_INDEX: &str = "beforeIndex";
    pub const LIMIT: &str = "limit";
    pub const ITEM_ID: &str = "itemId";
    pub const OFFSET: &str = "offset";
    pub const URI: &str = "uri";
    pub const STATUS: &str = "status";
    pub const SCOPE: &str = "scope";
    pub const INCLUDE_UNTRACKED: &str = "includeUntracked";
}

/// App identity strings shown in menus, window chrome, notifications, and
/// transcript titles.
pub(super) mod app {
    pub const NAME: &str = "VT Code";
}

/// GPUI key-context names shared by bindings, key-context scopes, and
/// tooltip actions. A typo detaches shortcuts silently.
pub(super) mod key_context {
    pub const DESKTOP: &str = "VTCodeDesktop";
    pub const SETTINGS: &str = "VTCodeSettings";
    pub const DIFF_REVIEW: &str = "VTCodeDiffReview";
    pub const INSPECTOR: &str = "VTCodeInspector";
    pub const COMPOSER: &str = "VTCodeComposer";
    pub const TRANSCRIPT: &str = "VTCodeTranscript";
    pub const SESSIONS: &str = "VTCodeSessions";
    pub const THREAD_TABS: &str = "VTCodeThreadTabs";
    pub const ACTIVITY: &str = "VTCodeActivity";
}

/// macOS app-bundle layout fragments shared by backend resolution and
/// editor detection.
pub(super) mod bundle {
    pub const MACOS_DIR: &str = "MacOS";
    pub const CONTENTS_DIR: &str = "Contents";
    pub const RESOURCES_DIR: &str = "Resources";
    pub const APP_EXTENSION: &str = "app";
    pub const INFO_PLIST: &str = "Info.plist";
    pub const HELPERS_VTCODE: &str = "Helpers/vtcode";
}

/// Localization keys referenced from two or more files.
///
/// Call sites pass the const to `rust_i18n::t!`, which accepts any string
/// expression, so a key can only be mistyped here. A miss resolves to the
/// key itself at runtime, which the test below turns into a failure naming
/// the key. Single-file keys stay at their use site; per-namespace locale
/// tests keep their own literals as independent oracles. New keys go here
/// and in the test table together, in the same order.
pub(super) mod i18n {
    pub const BROWSER_BROWSE_THREADS: &str = "app.browser.browse_threads";
    pub const COMPOSER_ADD_CONTEXT: &str = "app.composer.add_context";
    pub const COMPOSER_ATTACHMENTS_LIMIT: &str = "app.composer.attachments_limit";
    pub const COMPOSER_CHOOSE_MODEL: &str = "app.composer.choose_model";
    pub const COMPOSER_INPUT_PLACEHOLDER: &str = "app.composer.input_placeholder";
    pub const COMPOSER_MESSAGE_NOT_SUBMITTED: &str = "app.composer.message_not_submitted";
    pub const COMPOSER_PERMISSION_ASK: &str = "app.composer.permission_ask";
    pub const COMPOSER_PERMISSION_AUTO: &str = "app.composer.permission_auto";
    pub const COMPOSER_PERMISSION_FULL: &str = "app.composer.permission_full";
    pub const COMPOSER_REASONING_EFFORT: &str = "app.composer.reasoning_effort";
    pub const COMPOSER_SENDING_DECISION: &str = "app.composer.sending_decision";
    pub const CONFIGURATION_CANCEL: &str = "app.configuration.cancel";
    pub const CONFIGURATION_DEFAULT_HINT: &str = "app.configuration.default_hint";
    pub const CONFIGURATION_ORIGIN_HINT: &str = "app.configuration.origin_hint";
    pub const CONFIGURATION_UNAVAILABLE: &str = "app.configuration.unavailable";
    pub const CONFIGURATION_UNSET: &str = "app.configuration.unset";
    pub const ERRORS_UNKNOWN_DESTINATION: &str = "app.errors.unknown_destination";
    pub const FILE_PREVIEW_COPY: &str = "app.file_preview.copy";
    pub const FILES_REFRESH_TOOLTIP: &str = "app.files.refresh_tooltip";
    pub const INSPECTOR_EMPTY_ACTIVITY_TITLE: &str = "app.inspector.empty_activity_title";
    pub const LICENSES_TITLE: &str = "app.licenses.title";
    pub const LICENSES_VIEW: &str = "app.licenses.view";
    pub const NAVIGATION_CLOSE_THREAD: &str = "app.navigation.close_thread";
    pub const NAVIGATION_NEW_THREAD: &str = "app.navigation.new_thread";
    pub const NAVIGATION_SEARCH_COMMANDS: &str = "app.navigation.search_commands";
    pub const NAVIGATION_TOGGLE_INSPECTOR: &str = "app.navigation.toggle_inspector";
    pub const NAVIGATION_TOGGLE_NAVIGATION: &str = "app.navigation.toggle_navigation";
    pub const NAVIGATION_WORKSPACE_UTILITIES: &str = "app.navigation.workspace_utilities";
    pub const PALETTE_COMMAND_CHOOSE_MODEL: &str = "app.palette.command.choose_model";
    pub const PALETTE_COMMAND_GO_TO: &str = "app.palette.command.go_to";
    pub const PALETTE_COMMAND_SWITCH_WORKSPACE: &str = "app.palette.command.switch_workspace";
    pub const PALETTE_COMMAND_TOGGLE_INSPECTOR: &str = "app.palette.command.toggle_inspector";
    pub const PALETTE_HISTORY_LOAD: &str = "app.palette.history.load";
    pub const PALETTE_HISTORY_LOADING: &str = "app.palette.history.loading";
    pub const PALETTE_SECTIONS_VTCODE_COMMANDS: &str = "app.palette.sections.vtcode_commands";
    pub const PALETTE_TITLE_CONFIGURATION: &str = "app.palette.title.configuration";
    pub const PALETTE_TITLE_GO_TO: &str = "app.palette.title.go_to";
    pub const PERMISSIONS_CONFIRM_ACKNOWLEDGE: &str = "app.permissions.confirm_full_acknowledge";
    pub const PERMISSIONS_CONFIRM_FILES_BODY: &str = "app.permissions.confirm_full_files_body";
    pub const PERMISSIONS_CONFIRM_FILES_TITLE: &str = "app.permissions.confirm_full_files_title";
    pub const PERMISSIONS_CONFIRM_NETWORK_BODY: &str = "app.permissions.confirm_full_network_body";
    pub const PERMISSIONS_CONFIRM_NETWORK_TITLE: &str = "app.permissions.confirm_full_network_title";
    pub const PERMISSIONS_CONFIRM_RISK: &str = "app.permissions.confirm_full_risk";
    pub const PERMISSIONS_CONFIRM_TERMINAL_BODY: &str = "app.permissions.confirm_full_terminal_body";
    pub const PERMISSIONS_CONFIRM_TERMINAL_TITLE: &str = "app.permissions.confirm_full_terminal_title";
    pub const QUEUE_FULL: &str = "app.workspace.queue_full";
    pub const REQUEST_QUEUE_UNAVAILABLE: &str = "app.errors.request_queue_unavailable";
    pub const SESSION_FALLBACK_TITLE: &str = "app.session.fallback_title";
    pub const SESSION_PREPARING_NEW_THREAD: &str = "app.session.preparing_new_thread";
    pub const SESSION_RUNNING_THREAD_LABEL: &str = "app.session.running_thread_label";
    pub const SESSION_THREAD_READY: &str = "app.session.thread_ready";
    pub const SETTINGS_CONNECTIONS_TERMINAL: &str = "app.settings.connections.terminal";
    pub const SETTINGS_SEARCH_TITLE: &str = "app.settings.search.title";
    pub const SETTINGS_TITLE: &str = "app.settings.title";
    pub const SHORTCUTS_SEARCH_ARIA: &str = "app.shortcuts.search_aria";
    pub const TRANSCRIPT_PLAN_TITLE: &str = "app.transcript.plan_title";
    pub const TRANSCRIPT_YOU: &str = "app.transcript.you";
    pub const WORKSPACE_CHOOSE: &str = "app.workspace.choose";
    pub const WORKSPACE_CURRENT_LABEL: &str = "app.workspace.current_label";
    pub const WORKSPACE_REVIEW_SPLIT: &str = "app.workspace_review.split";
    pub const WORKSPACE_REVIEW_TITLE: &str = "app.workspace_review.title";
    pub const WORKSPACE_REVIEW_UNIFIED: &str = "app.workspace_review.unified";
    pub const WORKSPACE_SECTION: &str = "app.workspace.section";
}

/// Display joiner shared by label composition and its inverse.
///
/// `MIDDOT` joins composed labels (`Model · Effort`) and strips them back
/// (`trim_start_matches`); both directions reference this const so they
/// cannot silently diverge. `format!` templates (`"+{additions}"`, `"{} · {}"`)
/// stay inline: stable `format!` requires a string literal, and their
/// structure is already compiler-checked via captured identifiers.
pub(super) mod display {
    /// Middot joiner for composed labels (`Model · Effort`).
    pub const MIDDOT: &str = " · ";
}

#[cfg(test)]
mod tests {
    use super::i18n;

    /// Every shared key must resolve in the shipped English locale, covering
    /// both plain keys and `%{...}` templates (which resolve to their raw
    /// template without args). `rust_i18n::t!` returns the key itself on a
    /// miss, so a typo in the table fails here naming the key. Cross-locale
    /// completeness stays with the per-namespace localization tests.
    #[test]
    fn shared_i18n_keys_resolve_in_english() {
        const KEYS: [&str; 63] = [
            i18n::BROWSER_BROWSE_THREADS,
            i18n::COMPOSER_ADD_CONTEXT,
            i18n::COMPOSER_ATTACHMENTS_LIMIT,
            i18n::COMPOSER_CHOOSE_MODEL,
            i18n::COMPOSER_INPUT_PLACEHOLDER,
            i18n::COMPOSER_MESSAGE_NOT_SUBMITTED,
            i18n::COMPOSER_PERMISSION_ASK,
            i18n::COMPOSER_PERMISSION_AUTO,
            i18n::COMPOSER_PERMISSION_FULL,
            i18n::COMPOSER_REASONING_EFFORT,
            i18n::COMPOSER_SENDING_DECISION,
            i18n::CONFIGURATION_CANCEL,
            i18n::CONFIGURATION_DEFAULT_HINT,
            i18n::CONFIGURATION_ORIGIN_HINT,
            i18n::CONFIGURATION_UNAVAILABLE,
            i18n::CONFIGURATION_UNSET,
            i18n::ERRORS_UNKNOWN_DESTINATION,
            i18n::FILE_PREVIEW_COPY,
            i18n::FILES_REFRESH_TOOLTIP,
            i18n::INSPECTOR_EMPTY_ACTIVITY_TITLE,
            i18n::LICENSES_TITLE,
            i18n::LICENSES_VIEW,
            i18n::NAVIGATION_CLOSE_THREAD,
            i18n::NAVIGATION_NEW_THREAD,
            i18n::NAVIGATION_SEARCH_COMMANDS,
            i18n::NAVIGATION_TOGGLE_INSPECTOR,
            i18n::NAVIGATION_TOGGLE_NAVIGATION,
            i18n::NAVIGATION_WORKSPACE_UTILITIES,
            i18n::PALETTE_COMMAND_CHOOSE_MODEL,
            i18n::PALETTE_COMMAND_GO_TO,
            i18n::PALETTE_COMMAND_SWITCH_WORKSPACE,
            i18n::PALETTE_COMMAND_TOGGLE_INSPECTOR,
            i18n::PALETTE_HISTORY_LOAD,
            i18n::PALETTE_HISTORY_LOADING,
            i18n::PALETTE_SECTIONS_VTCODE_COMMANDS,
            i18n::PALETTE_TITLE_CONFIGURATION,
            i18n::PALETTE_TITLE_GO_TO,
            i18n::PERMISSIONS_CONFIRM_ACKNOWLEDGE,
            i18n::PERMISSIONS_CONFIRM_FILES_BODY,
            i18n::PERMISSIONS_CONFIRM_FILES_TITLE,
            i18n::PERMISSIONS_CONFIRM_NETWORK_BODY,
            i18n::PERMISSIONS_CONFIRM_NETWORK_TITLE,
            i18n::PERMISSIONS_CONFIRM_RISK,
            i18n::PERMISSIONS_CONFIRM_TERMINAL_BODY,
            i18n::PERMISSIONS_CONFIRM_TERMINAL_TITLE,
            i18n::QUEUE_FULL,
            i18n::REQUEST_QUEUE_UNAVAILABLE,
            i18n::SESSION_FALLBACK_TITLE,
            i18n::SESSION_PREPARING_NEW_THREAD,
            i18n::SESSION_RUNNING_THREAD_LABEL,
            i18n::SESSION_THREAD_READY,
            i18n::SETTINGS_CONNECTIONS_TERMINAL,
            i18n::SETTINGS_SEARCH_TITLE,
            i18n::SETTINGS_TITLE,
            i18n::SHORTCUTS_SEARCH_ARIA,
            i18n::TRANSCRIPT_PLAN_TITLE,
            i18n::TRANSCRIPT_YOU,
            i18n::WORKSPACE_CHOOSE,
            i18n::WORKSPACE_CURRENT_LABEL,
            i18n::WORKSPACE_REVIEW_SPLIT,
            i18n::WORKSPACE_REVIEW_TITLE,
            i18n::WORKSPACE_REVIEW_UNIFIED,
            i18n::WORKSPACE_SECTION,
        ];
        for key in KEYS {
            let text = rust_i18n::t!(key, locale = "en").to_string();
            assert!(!text.is_empty(), "{key}");
            assert_ne!(text, key, "missing English translation for {key}");
        }
    }
}
