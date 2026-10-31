use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};

use chrono::Timelike;
use gpui_kit::assets::IconName;
use gpui_kit::base::{
    Disableable, FocusTrapElement, FocusableExt, InteractiveElementExt, Selectable, StyledExt, TextViewStyle,
};
use gpui_kit::component::TitleBar;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::attachment::{
    Attachment, AttachmentActions, AttachmentContent, AttachmentDescription, AttachmentMedia, AttachmentTitle,
};
use gpui_kit::component::bubble::{Bubble, BubbleContent, BubbleVariant};
use gpui_kit::component::button::{Button, ButtonVariants, Toggle, ToggleVariants};
use gpui_kit::component::chart::BarChart;
use gpui_kit::component::command::{Command, CommandGroup, CommandItem, CommandState};
use gpui_kit::component::description_list::DescriptionList;
use gpui_kit::component::dialog::{DialogDescription, DialogHeader, DialogTitle};
use gpui_kit::component::diff::{Diff, DiffMode, DiffState};
use gpui_kit::component::empty::{
    Empty as EmptyState, EmptyContent, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle,
};
use gpui_kit::component::group_box::{GroupBox, GroupBoxVariants};
use gpui_kit::component::input::{
    Copy, Cut, Enter, IndentInline, Input, InputEvent, InputState, MoveDown, MoveUp, Paste, Position, Redo, SelectAll,
    Textarea, TextareaState, Undo,
};
use gpui_kit::component::input::{InputGroup, InputGroupAddon, InputGroupAddonAlignment};
use gpui_kit::component::list::ListItem;
use gpui_kit::component::marker::{Marker, MarkerContent};
use gpui_kit::component::menu::DropdownMenu;
use gpui_kit::component::message::{Message, MessageAlignment, MessageContent, MessageFooter};
use gpui_kit::component::message_scroller::{MessageScroller, MessageScrollerState};
use gpui_kit::component::plot::shape::BarAlignment;
use gpui_kit::component::popover::Popover;
use gpui_kit::component::radio::Radio;
use gpui_kit::component::resizable::{HandleEdge, resize_handle};
use gpui_kit::component::scroll::{ScrollableElement, ScrollableMask, Scrollbar};
use gpui_kit::component::select::{Select, SelectEvent};
use gpui_kit::component::shimmer::ShimmerText;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::status_bar::StatusBar;
use gpui_kit::component::stepper::{Stepper, StepperItem};
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::tag::Tag;
use gpui_kit::component::theme::{ActiveTheme, Theme, ThemeMode};
use gpui_kit::component::toolbar::{Toolbar, ToolbarGroup};
use gpui_kit::component::tree::{Tree, TreeEvent, TreeItem, TreeState};
use gpui_kit::component::{Icon, Sizable, VirtualListScrollHandle, v_virtual_list};
use gpui_kit::prelude::*;
use gpui_kit::{
    Anchor, Animation, AnimationExt, AnyElement, App, Axis, Context, CursorStyle, DragMoveEvent, Entity, FocusHandle,
    Focusable, KeyBinding, KeyDownEvent, KeyUpEvent, Menu, MenuItem, MouseButton, MouseDownEvent, MouseUpEvent,
    NavigationDirection, OsAction, PathPromptOptions, Pixels, Render, ScrollHandle, Size, Subscription, SystemMenuType,
    Task, TestSupportExt, Window, WindowOptions, actions, anchored, deferred, div, point, px, size,
};
use serde_json::{Value, json};
use tokio::sync::mpsc;
use vtcode_desktop_protocol as protocol;
use vtcode_desktop_protocol::{
    ConfigurationSnapshot, InteractionChoice, InteractionRequest, InteractionResponse, InteractionStep,
    McpAppReadResult, SessionHistoryMessage, SessionHistoryPage, SessionLoadResult, SessionOutputCursor,
    SessionOutputPage, SessionSummary, ThreadEventNotification,
};

use super::files::{self, FileEntry, FilePreview};
use super::inspector::{self, ActivityCounts, ActivityRow, ActivityState, ChangesProjection, InspectorTab};
use super::markdown::{markdown_style, render_markdown, render_markdown_with_style};
use super::preferences;
use super::strings::{app, bundle, display, i18n, key_context, keys};
use super::themes;
use super::transcript::{
    DiffCard, DiffFile, DiffFileKind, NoticeSeverity, Transcript, TranscriptItem, TranscriptRole, status as tool_status,
};
use super::transport::{
    BackendCommand, BackendEvent, BackendEventSender, BackendHandle, UI_EVENT_CAPACITY, launch_backend,
    request_fits_frame,
};
use navigation_history::{NavigationHistory, NavigationLocation, PendingNavigation};
use overlays::Route;

mod appearance;
mod backend_events;
mod bootstrap;
mod browser;
mod chrome;
mod components;
mod composer_actions;
mod composer_controls;
mod composer_render;
mod config_search;
mod configuration;
mod configuration_render;
mod curated;
mod files_panel;
mod inspector_changes;
mod inspector_panels;
mod interaction_render;
mod licenses;
mod mcp_app;
mod menu_bar;
mod navigation;
mod navigation_history;
mod overlays;
mod palette;
mod palette_render;
mod root_render;
mod runtime_items;
mod secret_store;
mod sessions;
mod settings;
mod shared_preferences;
mod shortcuts;
mod shortcuts_render;
mod storage;
mod submit;
mod tabs;
mod transcript_render;
mod typography;
mod updates;
mod utilities;
mod webmcp;
mod workspace;
mod workspace_review;
use super::localization::{UiLanguageOption, UiLanguageSelectState};
#[cfg(test)]
use backend_events::*;
pub(crate) use bootstrap::run;
#[cfg(test)]
use bootstrap::{init_keybindings, init_native_menus, open_or_activate_main_window};
use chrome::*;
use components::*;
use inspector_changes::*;
use mcp_app::{McpAppBodyKey, PendingMcpAppRead};
use palette::*;
use runtime_items::*;
use sessions::*;
use typography::FontTarget;
use workspace_review::WorkspaceReviewState;

const MAX_UI_EVENTS_PER_TICK: usize = 64;
const CONTENT_WIDTH: f32 = 720.;
const ZEN_CONTENT_WIDTH: f32 = 600.;
const DEFAULT_UI_FONT_SIZE: f32 = 16.;
const WELCOME_COPY_COUNT: usize = 3;

const ACTIVITY_ROW_HEIGHT: f32 = 72.;
const SESSION_ROW_HEIGHT: f32 = 36.;
const CHANGE_ROW_HEIGHT: f32 = 28.;
/// Palette panel id; the focus trap targets the same element.
const COMMAND_PALETTE_ELEMENT_ID: &str = "command-palette";
/// The single zen toggle slot: one title-bar trailing id shared by the
/// enter/exit states across modes, so the button never moves when toggled.
const ZEN_MODE_TOGGLE_ELEMENT_ID: &str = "zen-mode-toggle";

struct WelcomeCopy {
    action: String,
    workspace_action: String,
    call_to_action: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WelcomePeriod {
    Morning,
    Afternoon,
    Evening,
}

fn welcome_period_for_hour(hour: u32) -> WelcomePeriod {
    match hour % 24 {
        5..=11 => WelcomePeriod::Morning,
        12..=16 => WelcomePeriod::Afternoon,
        _ => WelcomePeriod::Evening,
    }
}

fn welcome_greeting(period: WelcomePeriod) -> String {
    match period {
        WelcomePeriod::Morning => rust_i18n::t!("app.welcome.greeting.morning"),
        WelcomePeriod::Afternoon => rust_i18n::t!("app.welcome.greeting.afternoon"),
        WelcomePeriod::Evening => rust_i18n::t!("app.welcome.greeting.evening"),
    }
    .to_string()
}

fn welcome_copy(index: usize) -> WelcomeCopy {
    match index {
        1 => WelcomeCopy {
            action: rust_i18n::t!("app.welcome.action.improve").to_string(),
            workspace_action: rust_i18n::t!("app.welcome.action.improve_in_workspace").to_string(),
            call_to_action: rust_i18n::t!("app.welcome.cta.open_project").to_string(),
        },
        2 => WelcomeCopy {
            action: rust_i18n::t!("app.welcome.action.explore").to_string(),
            workspace_action: rust_i18n::t!("app.welcome.action.explore_in_workspace").to_string(),
            call_to_action: rust_i18n::t!("app.welcome.cta.select_folder").to_string(),
        },
        _ => WelcomeCopy {
            action: rust_i18n::t!("app.welcome.action.build").to_string(),
            workspace_action: rust_i18n::t!("app.welcome.action.build_in_workspace").to_string(),
            call_to_action: rust_i18n::t!("app.welcome.cta.choose_workspace").to_string(),
        },
    }
}

fn welcome_prompt(greeting: &str, action: &str, workspace_action: &str, workspace_name: Option<&str>) -> String {
    match workspace_name {
        Some(name) => format!("{greeting}. {workspace_action} {name}?"),
        None => format!("{greeting}. {action}?"),
    }
}

fn service_reconnect_guidance(has_retained_submission: bool) -> String {
    if has_retained_submission {
        rust_i18n::t!("app.service.reconnect_with_draft").to_string()
    } else {
        rust_i18n::t!("app.service.reconnect_plain").to_string()
    }
}

/// Leading sentence shared by both credential-guidance variants. The full
/// sentences stay intact below (one localizable unit each); only this prefix
/// is shared so [`is_provider_auth_guidance`] cannot drift from the generator.
const PROVIDER_AUTH_GUIDANCE_PREFIX: &str = "No provider credentials are configured.";

/// Whether an error banner shows credential guidance rather than an
/// unrelated failure. A prefix match avoids rebuilding the guidance string
/// just to compare; no other error uses this prefix.
fn is_provider_auth_guidance(error: &str) -> bool {
    error.starts_with(PROVIDER_AUTH_GUIDANCE_PREFIX)
}

/// Guidance shown while the service is connected but the active provider has
/// no credentials. API-key providers offer an in-app Settings form (no reload
/// needed after saving); login-flow providers still go through the Terminal.
fn provider_auth_guidance(configuration: Option<&ConfigurationSnapshot>) -> String {
    let setup = configuration
        .map(|snapshot| snapshot.credential_setup_command.trim())
        .filter(|command| !command.is_empty())
        .unwrap_or("vtcode auth");
    if configuration.and_then(|snapshot| snapshot.secret_store.as_ref()).is_some() {
        format!(
            "{PROVIDER_AUTH_GUIDANCE_PREFIX} Save your API key in Settings → VT Code → Provider to reconnect. Terminal alternative: run `{setup}`, then choose Reload CLI settings in Settings → VT Code."
        )
    } else {
        format!(
            "{PROVIDER_AUTH_GUIDANCE_PREFIX} In Terminal, run `{setup}`, then choose Reload CLI settings in Settings → VT Code to reconnect."
        )
    }
}

fn scale_row_height_for_ui_font(row_height: f32, font_size: Pixels) -> f32 {
    row_height * font_size.as_f32() / DEFAULT_UI_FONT_SIZE
}

/// Pane-width drag bounds, shared with the preferences clamp.
const MIN_PANE_WIDTH: f32 = preferences::MIN_PANE_WIDTH;
const MAX_PANE_WIDTH: f32 = preferences::MAX_PANE_WIDTH;

/// Window toolbar height. Left-side navigation content starts below this
/// line so it clears the macOS traffic lights while the sidebar surface
/// itself still spans the full window height.
const TOOLBAR_HEIGHT: f32 = 48.;

actions!(
    desktop,
    [
        OpenCommandPalette,
        OpenConversationSearch,
        OpenSettingsSearch,
        OpenModelPicker,
        OpenAddContext,
        OpenPermissions,
        OpenModePicker,
        OpenSelectedFile,
        NewSession,
        CloseSession,
        ChooseWorkspace,
        FocusComposer,
        OpenUtilities,
        OpenWorkspaceFiles,
        OpenWorkspaceReview,
        OpenThreadBrowser,
        RefreshWorkspace,
        NavigateBack,
        NavigateForward,
        RouteBack,
        ToggleInspector,
        ToggleSidebar,
        ToggleZenMode,
        ToggleStatusBar,
        ReloadCliSettings,
        SyncCliTheme,
        NextInspectorTab,
        PreviousInspectorTab,
        FirstInspectorTab,
        LastInspectorTab,
        NextSettingsTab,
        PreviousSettingsTab,
        FirstSettingsTab,
        LastSettingsTab,
        NextReviewChange,
        PreviousReviewChange,
        NextReviewFile,
        PreviousReviewFile,
        CancelRun,
        DismissOverlay,
        OpenSettings,
        CloseWindow,
        MinimizeWindow,
        ZoomWindow,
        ShowMainWindow,
        QuitApplication,
        HideApplication,
        HideOtherApplications,
        ShowAllApplications
    ]
);

/// A recorded external-editor launch request, captured only under test interception.
#[cfg(test)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ExternalLaunchCall {
    pub rel_path: String,
    pub editor: files::ExternalEditor,
}

/// Test-only interception of external-editor launches so GUI tests never shell
/// out to `/usr/bin/open`. The completion path stays shared with production.
#[cfg(test)]
#[derive(Debug, Default)]
pub(super) struct TestLaunchIntercept {
    pub calls: Vec<ExternalLaunchCall>,
    /// Simulated failure for every intercepted launch; `None` means success.
    pub fail_with: Option<String>,
}

struct DesktopView {
    focus_handle: FocusHandle,
    _focus_lost_subscription: Subscription,
    _window_bounds_subscription: Subscription,
    _welcome_activation_subscription: Subscription,
    welcome_period: WelcomePeriod,
    welcome_copy_index: usize,
    workspace: Option<PathBuf>,
    pending_workspace: Option<PathBuf>,
    navigation_history: NavigationHistory,
    pending_navigation: Option<PendingNavigation>,
    workspace_selection_pending: bool,
    workspace_selection_task: Option<Task<()>>,
    workspace_picker_open: bool,
    workspace_picker_task: Option<Task<()>>,
    backend: Option<BackendHandle>,
    event_rx: Receiver<BackendEvent>,
    event_tx: BackendEventSender,
    event_drain_task: Option<Task<()>>,
    sessions: Vec<SessionSummary>,
    selected_session_id: Option<String>,
    /// Open-thread tabs in first-open order; selection stays in
    /// `selected_session_id`. Tabs are window-local shortcuts — closing one
    /// never touches server-side session state.
    open_tabs: Vec<String>,
    active_primary_agent_display: Option<ActivePrimaryAgentDisplay>,
    transcript: Transcript,
    message_scroller: Entity<MessageScrollerState>,
    /// Restored history rows; mutate only via the `*_restored_activity`
    /// helpers so `restored_activity_revision` stays in sync for the
    /// inspector projection caches.
    restored_activity: Vec<TranscriptItem>,
    restored_activity_revision: u64,
    activity_projection_cache: Option<inspector::ActivityProjectionCache>,
    output_view_cache: Option<inspector::OutputViewCache>,
    /// Bounded tails for expanded transcript tool rows, keyed by row id and
    /// valid only at the stored transcript revision. `output_tail` scans up
    /// to 128 KB, so expanded rows resolve through `resolve_tool_tail`
    /// instead of recomputing per frame.
    tool_tail_cache: HashMap<String, inspector::ToolTailCacheEntry>,
    /// Markdown style for transcript rows, rebuilt once per `render_transcript`
    /// call so visible rows clone instead of rebuilding it each. Rows are only
    /// reachable through `render_transcript`, so the field is always fresh
    /// where it is read; the fallback covers hypothetical direct row renders.
    transcript_markdown_style: Option<TextViewStyle>,
    selected_output_id: Option<String>,
    restored_review_history_incomplete: bool,
    restored_activity_history_incomplete: bool,
    active_turn_id: Option<String>,
    finished_turn_id: Option<String>,
    cancel_requested: bool,
    cancel_request_sent: bool,
    running: bool,
    /// Whether the active turn has produced transcript content. Meaningful
    /// only while `running`: reset when a turn begins (submit, `TurnStarted`),
    /// set by content events. Stale `true` after a turn ends is unobserved
    /// because the pending row also requires `running`.
    turn_content_seen: bool,
    status: String,
    ui_progress_active: bool,
    /// Loading delegated/matrix tasks from the latest `matrix.updated`
    /// snapshot. Feeds the shared loading model (`background`), mirroring the
    /// CLI's local-agent loading count; reset on session switch and disconnect.
    background_activity_count: usize,
    error: Option<String>,
    configuration: Option<ConfigurationSnapshot>,
    pending_permission_choice: Option<protocol::PermissionMode>,
    service_info: Option<protocol::InitializeResult>,
    interaction: Option<InteractionRequest>,
    interaction_focus: FocusHandle,
    pending_interaction_response: Option<String>,
    interaction_choice_ids: Vec<Option<String>>,
    interaction_answer_texts: Vec<Option<String>>,
    interaction_step_index: usize,
    composer: Entity<TextareaState>,
    _composer_subscription: Subscription,
    ui_language_select: Entity<UiLanguageSelectState>,
    _ui_language_subscription: Subscription,
    file_destination_select: Entity<preferences::FileDestinationSelectState>,
    _file_destination_subscription: Subscription,
    turn_completion_select: Entity<preferences::TurnCompletionModeSelectState>,
    _turn_completion_subscription: Subscription,
    question_answer: Entity<TextareaState>,
    theme_mode: ThemeMode,
    files: Vec<FileEntry>,
    file_directory_paths: Arc<HashSet<String>>,
    file_tree: Entity<TreeState>,
    expanded_file_directories: HashSet<String>,
    _file_tree_subscription: Subscription,
    files_task: Option<Task<()>>,
    files_scan_cancel: Option<Arc<AtomicBool>>,
    preview_task: Option<Task<()>>,
    attachments_task: Option<Task<()>>,
    external_launch_task: Option<Task<()>>,
    file_preview: Option<FilePreview>,
    preview_editor_error: Option<String>,
    #[cfg(test)]
    test_launch_intercept: Option<TestLaunchIntercept>,
    preview_focus: FocusHandle,
    preview_return_focus: Option<FocusHandle>,
    palette_return_focus: Option<FocusHandle>,
    palette_focus: FocusHandle,
    sidebar_resize_focus: FocusHandle,
    inspector_resize_focus: FocusHandle,
    inspector_open: bool,
    inspector_tab: InspectorTab,
    inspector_focus: FocusHandle,
    inspector_tabs_scroll: ScrollHandle,
    changes: ChangesProjection,
    selected_change: Option<String>,
    change_viewer: Option<Entity<DiffState>>,
    change_viewer_dirty: bool,
    route: Route,
    workspace_review: Option<WorkspaceReviewState>,
    review_focus: FocusHandle,
    browser_focus: FocusHandle,
    git_branch: Option<String>,
    palette: Option<Entity<CommandState>>,
    palette_kind: PaletteKind,
    palette_query: gpui_kit::SharedString,
    effort_slider: Option<Entity<gpui_kit::component::slider::SliderState>>,
    contrast_slider: Entity<gpui_kit::component::slider::SliderState>,
    contrast_focus: FocusHandle,
    _contrast_slider_subscription: Subscription,
    effort_focus: FocusHandle,
    _effort_slider_subscriptions: Vec<Subscription>,
    composer_resources: Vec<protocol::ComposerResource>,
    slash_commands: Vec<protocol::SlashCommandInfo>,
    resources_loading: bool,
    resources_loaded: bool,
    palette_groups: PaletteGroups,
    config_page_open: bool,
    config_page_group: Option<String>,
    theme_scroll: ScrollHandle,
    settings_scroll: ScrollHandle,
    settings_nav_scroll: ScrollHandle,
    config_rail_scroll: ScrollHandle,
    utilities_scroll: ScrollHandle,
    output_scroll: ScrollHandle,
    review_scroll: ScrollHandle,
    session_scroll: VirtualListScrollHandle,
    activity_scroll: VirtualListScrollHandle,
    change_list_scroll: VirtualListScrollHandle,
    browser_scroll: VirtualListScrollHandle,
    session_row_sizes: FixedRowSizeCache,
    activity_row_sizes: FixedRowSizeCache,
    change_row_sizes: FixedRowSizeCache,
    browser_row_sizes: FixedRowSizeCache,
    settings_focus: FocusHandle,
    settings_navigation_focus: FocusHandle,
    settings_tab: settings::SettingsTab,
    storage: storage::StorageState,
    webmcp_status: Option<protocol::WebmcpBridgeStatus>,
    webmcp_pairing: Option<protocol::WebmcpPairResult>,
    webmcp_origin_input: Entity<TextareaState>,
    webmcp_request_pending: Option<&'static str>,
    webmcp_status_dirty: bool,
    webmcp_notice: Option<webmcp::WebmcpNotice>,
    update_status: Option<protocol::UpdateStatusSnapshot>,
    update_request_pending: bool,
    cli_config_settings: Option<protocol::ConfigSettingsSnapshot>,
    /// Memoized catalog grouping; refreshed on every snapshot/query change.
    /// `refresh_config_catalog` is the only writer besides tests.
    config_catalog_cache: Option<configuration::ConfigCatalogCache>,
    /// Bumped on each catalog snapshot so the cache cannot go stale.
    config_catalog_epoch: u64,
    /// Precomputed search haystacks, rebuilt only when the snapshot epoch
    /// changes so per-keystroke filtering allocates nothing.
    config_search_corpus: Option<config_search::ConfigSearchCorpus>,
    /// Rows rendered for the selected group; the "Show more" control grows it.
    config_visible_row_limit: usize,
    config_target: protocol::ConfigTarget,
    config_search_input: Entity<InputState>,
    _config_search_subscription: Subscription,
    config_value_input: Entity<TextareaState>,
    config_editor: Option<configuration::ConfigEditor>,
    config_editor_focus: FocusHandle,
    config_editor_return_focus: Option<FocusHandle>,
    config_editor_error: Option<String>,
    config_write_pending: bool,
    config_write_notice: Option<String>,
    secret_input: Entity<InputState>,
    secret_editor_error: Option<String>,
    secret_save_pending: bool,
    models: Vec<protocol::ModelInfo>,
    attachments: Vec<protocol::TurnAttachment>,
    attachments_picker_open: bool,
    pending_submission: Option<SubmissionDraft>,
    pending_first_submission: bool,
    first_submission: Option<SubmissionDraft>,
    session_request: Option<&'static str>,
    pending_new_thread: bool,
    session_load_task: Option<Task<()>>,
    session_load_generation: u64,
    history_start_index: usize,
    history_has_earlier: bool,
    history_page_request_id: Option<String>,
    history_page_limit: usize,
    history_page_task: Option<Task<()>>,
    history_page_generation: u64,
    history_page_loading: bool,
    history_page_error: Option<String>,
    restored_output_cursors: HashMap<String, SessionOutputCursor>,
    restored_output_pages: HashMap<String, Vec<gpui_kit::SharedString>>,
    output_page_pending: Option<PendingOutputPage>,
    output_page_task: Option<Task<()>>,
    output_page_error: Option<(String, String)>,
    mcp_app_pending: HashMap<String, PendingMcpAppRead>,
    mcp_app_task: Option<Task<()>>,
    mcp_app_bodies: HashMap<McpAppBodyKey, McpAppReadResult>,
    mcp_app_errors: HashMap<McpAppBodyKey, gpui_kit::SharedString>,
    retained_submission: Option<SubmissionDraft>,
    last_usage: Option<vtcode_exec_events::Usage>,
    theme_id: String,
    preferences: preferences::Preferences,
    system_motion_baseline: bool,
    default_code_font: gpui_kit::SharedString,
    sidebar_open: bool,
    navigation_peek: bool,
    hovered_message: Option<String>,
    zen_mode: bool,
    shortcut_draft: std::collections::BTreeMap<String, String>,
    shortcut_search_input: Entity<InputState>,
    _shortcut_search_subscription: Subscription,
    shortcut_capture: Option<shortcuts::CommandId>,
    shortcut_capture_focus: FocusHandle,
    shortcut_message: Option<String>,
    shortcuts_editing: bool,
    sidebar_width: f32,
    inspector_width: f32,
    resizing: Option<PaneResize>,
}

fn file_tree_items(entries: &[FileEntry], parent: Option<&str>, expanded: &HashSet<String>) -> Vec<TreeItem> {
    // Group once by parent directory so each level below walks only its own
    // children instead of re-scanning every entry; pushes preserve entry
    // order within each level, and output order never depends on map order.
    let mut by_parent: HashMap<Option<&str>, Vec<&FileEntry>> = HashMap::new();
    for entry in entries {
        by_parent
            .entry(entry.rel_path.rsplit_once('/').map(|(dir, _)| dir))
            .or_default()
            .push(entry);
    }
    file_tree_items_grouped(&by_parent, parent, expanded)
}

fn file_tree_items_grouped(
    by_parent: &HashMap<Option<&str>, Vec<&FileEntry>>,
    parent: Option<&str>,
    expanded: &HashSet<String>,
) -> Vec<TreeItem> {
    by_parent.get(&parent).map_or_else(Vec::new, |children| {
        children
            .iter()
            .map(|entry| {
                let nested = if entry.is_dir {
                    file_tree_items_grouped(by_parent, Some(&entry.rel_path), expanded)
                } else {
                    Vec::new()
                };
                TreeItem::new(entry.rel_path.clone(), entry.rel_path.clone())
                    .children(nested)
                    .expanded(expanded.contains(&entry.rel_path))
            })
            .collect()
    })
}

fn find_file_tree_item<'a>(items: &'a [TreeItem], path: &str) -> Option<&'a TreeItem> {
    items.iter().find_map(|item| {
        if item.id.as_ref() == path {
            Some(item)
        } else {
            find_file_tree_item(&item.children, path)
        }
    })
}

fn send_request(commands: &mpsc::Sender<BackendCommand>, method: &str, params: Value) -> bool {
    commands
        .try_send(BackendCommand::Request { method: method.to_string(), params })
        .is_ok()
}

fn valid_slash_command_catalog(commands: &[protocol::SlashCommandInfo]) -> bool {
    commands.len() <= 256
        && commands.iter().all(|command| {
            !command.name.is_empty()
                && command.name.len() <= 80
                && command
                    .name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
                && command.description.chars().count() <= 1_000
                && !command.description.chars().any(char::is_control)
        })
}

fn format_context_window(tokens: u64) -> String {
    if tokens >= 1_000_000 {
        format_compact_count(tokens, 1_000_000, "M")
    } else if tokens >= 1_000 {
        format_compact_count(tokens, 1_000, "K")
    } else {
        tokens.to_string()
    }
}

fn format_compact_count(value: u64, divisor: u64, suffix: &str) -> String {
    let whole = value / divisor;
    let hundredths = (value % divisor) * 100 / divisor;
    if hundredths == 0 {
        format!("{whole}{suffix}")
    } else if hundredths.is_multiple_of(10) {
        format!("{whole}.{}{suffix}", hundredths / 10)
    } else {
        format!("{whole}.{hundredths:02}{suffix}")
    }
}

fn workspace_picker_options() -> PathPromptOptions {
    PathPromptOptions {
        files: false,
        directories: true,
        multiple: false,
        prompt: Some(rust_i18n::t!(i18n::WORKSPACE_CHOOSE).to_string().into()),
    }
}

/// Approximate context utilization after a turn: every token the provider
/// processed, as a share of the active model's context window. Returns
/// `None` while either side is unknown, so the status bar hides the field
/// rather than inventing a number.
fn context_usage_percent(usage: &vtcode_exec_events::Usage, context_window: u64) -> Option<u8> {
    if context_window == 0 {
        return None;
    }
    let tokens = usage
        .input_tokens
        .saturating_add(usage.cached_input_tokens)
        .saturating_add(usage.cache_creation_tokens)
        .saturating_add(usage.output_tokens);
    let percent = (tokens.saturating_mul(100)).checked_div(context_window)?;
    u8::try_from(percent).ok().or(Some(100))
}

fn uuid_like_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT_ID: AtomicU64 = AtomicU64::new(1);
    NEXT_ID.fetch_add(1, Ordering::Relaxed).to_string()
}

fn desktop_window_options() -> WindowOptions {
    let mut options = TitleBar::window_options();
    if let Some(titlebar) = options.titlebar.as_mut() {
        titlebar.traffic_light_position = Some(point(px(12.), px(17.)));
    }
    // Smallest geometry that keeps every surface non-degenerate: 600x560 is
    // the smallest headless-tested size with all suites green, while 400px
    // tall provably starves flex content (the config header plus rail alone
    // exceed the row). The platform enforces this; headless tests may still
    // open smaller windows to stress overflow paths.
    options.window_min_size = Some(size(px(600.), px(560.)));
    options
}

fn chrome_background(colors: &gpui_kit::component::theme::ThemeColor) -> gpui_kit::Hsla {
    themes::navigation_surfaces(colors.background, colors.secondary, colors.foreground).rail
}

/// Keep local paths out of element IDs while preserving path-based identity across sorting.
fn opaque_path_element_id(prefix: &str, path: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut hasher);
    format!("{prefix}-{:016x}", hasher.finish())
}

#[cfg(test)]
mod tests;
