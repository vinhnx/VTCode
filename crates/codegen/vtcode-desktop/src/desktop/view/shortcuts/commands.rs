//! The desktop command table: one entry per configurable command.
//!
//! Ids persist in preferences, so the table is the single source of truth
//! for which commands exist and what their default bindings are.

use super::*;

pub(in crate::desktop::view) const COMMANDS: [ShortcutCommand; 36] = [
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
        default_binding: key::ESCAPE,
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
    ShortcutCommand {
        id: CommandId::BrowseThreads,
        // Reuses the translated browser label; no second copy to drift.
        label_key: i18n::BROWSER_BROWSE_THREADS,
        description_key: "app.shortcuts.browse_threads_description",
        default_binding: "cmd-t",
    },
    ShortcutCommand {
        id: CommandId::GoHome,
        label_key: "app.shortcuts.go_home_label",
        description_key: "app.shortcuts.go_home_description",
        default_binding: "cmd-shift-h",
    },
];
