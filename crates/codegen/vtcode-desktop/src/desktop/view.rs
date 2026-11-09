use std::collections::{HashMap, HashSet};
// Re-exported through `use super::*`: several view modules hash element IDs
// and need these traits in scope for method resolution.
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

use chrono::Timelike;
use gpui_kit::assets::IconName;
use gpui_kit::base::{
    Disableable, FocusTrapElement, FocusableExt, InteractiveElementExt, ResizeHandleContext, ResizeHandleRenderer,
    ResizeHandleState, StyledExt, TextViewStyle,
};
use gpui_kit::component::TitleBar;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::attachment::{
    Attachment, AttachmentActions, AttachmentContent, AttachmentDescription, AttachmentMedia, AttachmentTitle,
};
use gpui_kit::component::bubble::{Bubble, BubbleContent, BubbleVariant};
use gpui_kit::component::button::{Button, ButtonRounded, ButtonVariants, Toggle, ToggleVariants};
use gpui_kit::component::command::{Command, CommandGroup, CommandItem, CommandState};
use gpui_kit::component::description_list::DescriptionList;
use gpui_kit::component::dialog::{DialogDescription, DialogHeader, DialogTitle};
use gpui_kit::component::diff::{Diff, DiffMode, DiffState};
use gpui_kit::component::empty::{
    Empty as EmptyState, EmptyDescription, EmptyHeader, EmptyMedia, EmptyMediaVariant, EmptyTitle,
};
use gpui_kit::component::group_box::{GroupBox, GroupBoxVariants};
use gpui_kit::component::input::{
    Copy, Cut, DeleteToBeginningOfLine, DeleteToEndOfLine, DeleteToNextWordEnd, DeleteToPreviousWordStart, Editor,
    EditorState, Enter, IndentInline, Input, InputEvent, InputState, MoveDown, MoveLeft, MoveRight, MoveToEnd, MoveUp,
    OutdentInline, Paste, Position, Redo, SelectAll, Textarea, TextareaState, Undo,
};
use gpui_kit::component::input::{InputGroup, InputGroupAddon, InputGroupAddonAlignment};
use gpui_kit::component::list::ListItem;
use gpui_kit::component::marker::{Marker, MarkerContent, MarkerIcon, MarkerLoadingStyle, MarkerVariant};
use gpui_kit::component::menu::DropdownMenu;
use gpui_kit::component::message::{Message, MessageAlignment, MessageContent, MessageFooter};
use gpui_kit::component::message_scroller::{MessageScroller, MessageScrollerState};
use gpui_kit::component::popover::Popover;
use gpui_kit::component::radio::Radio;
use gpui_kit::component::resizable::{HandleEdge, resize_handle, resize_handle_appearance};
use gpui_kit::component::scroll::{ScrollableElement, ScrollableMask, Scrollbar};
use gpui_kit::component::select::{Select, SelectEvent};
use gpui_kit::component::shimmer::{ShimmerStyle, ShimmerText};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::status_bar::StatusBar;
use gpui_kit::component::stepper::{Stepper, StepperItem};
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::tag::Tag;
use gpui_kit::component::theme::{ActiveTheme, Theme, ThemeMode};
use gpui_kit::component::toolbar::{Toolbar, ToolbarGroup};
use gpui_kit::component::tree::{Tree, TreeEvent, TreeItem, TreeState};
use gpui_kit::component::{Icon, Selectable, Sizable, VirtualListScrollHandle, v_virtual_list};
use gpui_kit::prelude::*;
use gpui_kit::{
    Anchor, Animation, AnimationExt, AnyElement, App, Axis, Context, CursorStyle, DragMoveEvent, Entity, ExternalPaths,
    FocusHandle, Focusable, KeyBinding, KeyDownEvent, KeyUpEvent, Menu, MenuItem, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, NavigationDirection, NoAction, OsAction, PathPromptOptions, Pixels, Render,
    ScrollEvent, ScrollHandle, Size, Subscription, SystemMenuType, Task, TestSupportExt, Window, actions, anchored,
    deferred, div, img, point, px, size,
};
use serde_json::{Value, json};
use tokio::sync::mpsc;
use vtcode_commons::history_scope::HistoryScope;
use vtcode_commons::ui_protocol::AppIconPreference;
use vtcode_desktop_protocol as protocol;
use vtcode_desktop_protocol::{
    ConfigurationSnapshot, InteractionChoice, InteractionRequest, InteractionResponse, InteractionStep,
    McpAppReadResult, SessionHistoryMessage, SessionHistoryPage, SessionLoadResult, SessionOutputCursor,
    SessionOutputPage, SessionSummary, ThreadEventNotification,
};

use super::app_icon;
use super::build_info;
use super::diff_preferences;
use super::files::{self, FileEntry, FilePreview};
use super::inspector::{self, ActivityCounts, ActivityRow, ActivityState, ChangesProjection, InspectorTab};
use super::markdown::{
    MarkdownViewHeight, has_viewable_plan_markdown, markdown_style, prepare_markdown, render_markdown,
    render_markdown_view, render_markdown_with_style, render_output_text,
};
use super::preferences;
use super::strings::{app, bundle, display, i18n, key_context, keys};
use super::themes;
use super::transcript::{
    DiffCard, DiffFile, DiffFileKind, EditTotals, NoticeSeverity, ToolCategory, ToolCategoryCounts, Transcript,
    TranscriptItem, TranscriptRole, UserAttachment, activity_details_summary, activity_output_text, activity_payload,
    activity_text, describe_activity_verbs, harness_is_user_visible, status as tool_status, tool_row_gist,
};
use super::transport::{
    BackendCommand, BackendEvent, BackendEventSender, BackendHandle, UI_EVENT_CAPACITY, handoff_vtcode_binary,
    launch_backend, request_fits_frame,
};
use cli_handoff::CliHandoff;
use file_rename::FileRenameState;
use full_access_confirm::FullAccessConfirmState;
use ids::{opaque_path_element_id, uuid_like_id};
use navigation_history::{
    ArmOutcome, NavigationHistory, NavigationLocation, PendingNavigation, SWIPE_COMMIT_DELAY, SwipeArming,
    SwipeScrollAccumulator, is_vertical_dominant_scroll, natural_scroll_enabled, swipe_direction_for_dx,
};
use overlays::{DiscardIntent, Route, RouteHistory, dismissable_route_page};

mod about;
mod app_icon_settings;
mod appearance;
mod appearance_theme_picker;
mod appearance_themes;
mod attachment_cards;
mod attachment_picker;
mod backend_events;
mod bootstrap;
mod browser;
mod chrome;
mod cli_handoff;
mod components;
mod composer_actions;
mod composer_attachments;
mod composer_controls;
mod composer_history;
mod composer_render;
mod config_search;
mod configuration;
mod configuration_render;
mod context_format;
mod curated;
mod deeplinks;
mod exec_controls;
mod file_preview;
mod file_rename;
mod files_panel;
mod full_access_confirm;
mod glass_buttons;
mod ids;
mod inspector_changes;
mod inspector_panels;
mod interaction_render;
mod launch_bounds;
mod licenses;
mod login_items;
mod mcp_app;
mod menu_bar;
mod navigation;
mod navigation_history;
mod overlays;
mod palette;
mod palette_render;
mod queue;
mod root_render;
mod running_confirm;
mod runtime_items;
mod secret_store;
mod session_nav;
mod session_options;
mod sessions;
mod settings;
mod settings_accessibility;
mod settings_connections;
mod settings_diff;
mod settings_kit;
mod settings_notifications;
mod settings_vtcode;
mod shared_preferences;
mod shortcuts;
mod shortcuts_render;
mod storage;
mod submit;
mod swipe_navigation_overlay;
mod tabs;
mod transcript_activity;
mod transcript_diff;
mod transcript_nav;
mod transcript_notice;
mod transcript_render;
mod turn_undo;
mod typography;
mod updates;
mod utilities;
mod webmcp;
mod welcome_copy;
mod workspace;
mod workspace_review;
use super::localization::{UiLanguageOption, UiLanguageSelectState};
#[cfg(test)]
use backend_events::*;
pub(crate) use bootstrap::run;
#[cfg(test)]
use bootstrap::{dispatch_handoff_links, init_keybindings, open_or_activate_main_window};
use chrome::*;
use components::*;
use glass_buttons::*;
use inspector_changes::*;
use launch_bounds::{desktop_window_options, main_screen_visible_bounds};
use mcp_app::{McpAppBodyKey, PendingMcpAppRead};
use palette::*;
use queue::*;
use running_confirm::{RunningSwitch, RunningSwitchIntent};
use runtime_items::*;
use sessions::*;
use transcript_nav::*;
use typography::FontTarget;
use welcome_copy::*;
use workspace_review::WorkspaceReviewState;

const MAX_UI_EVENTS_PER_TICK: usize = 64;
/// Normal-mode reading column: ~80-column code at the UI-16/code-14 defaults
/// (768 - 64 title inset - 34 code padding/border is ~79 cols at 0.6 mono).
/// Centered `max_w`, so narrow windows are unaffected.
const CONTENT_WIDTH: f32 = 768.;
/// Zen reading column: trades the 80-column fit for focus (~69 cols at code 14;
/// longer lines wrap, contained). Narrower than normal.
const ZEN_CONTENT_WIDTH: f32 = 680.;
const DEFAULT_UI_FONT_SIZE: f32 = 16.;

const ACTIVITY_ROW_HEIGHT: f32 = 44.;
const SESSION_ROW_HEIGHT: f32 = 36.;
const CHANGE_ROW_HEIGHT: f32 = 28.;
/// Palette panel id; the focus trap targets the same element.
const COMMAND_PALETTE_ELEMENT_ID: &str = "command-palette";
/// The single zen toggle slot: one title-bar trailing id shared by the
/// enter/exit states across modes, so the button never moves when toggled.
const ZEN_MODE_TOGGLE_ELEMENT_ID: &str = "zen-mode-toggle";

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
    if configuration.is_some_and(|snapshot| snapshot.secret_store.is_some()) {
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
///
/// macOS traffic-light anchor (see `desktop_window_options`): the lights sit
/// at (12, 17) with a 12px diameter, so their bottom edge is y = 29.
/// Windowed zen hides the toolbar that otherwise owns this zone, so the
/// zen drag strip owns it instead and every route starts below the lights
/// instead of floating them over its corner.
const TRAFFIC_LIGHT_X: f32 = 12.;
const TRAFFIC_LIGHT_Y: f32 = 17.;
/// Height of the zen drag strip (see `render_zen_drag_strip`), which doubles
/// as the traffic-light clearance when the toolbar is hidden (windowed
/// zen): the lights end at y = 29, so 40px clears them with an 11px gap.
/// Fixed pixels: this matches a platform window inset, not the rem scale.
/// Fullscreen hides the lights and cannot move, so no strip renders there.
const ZEN_DRAG_STRIP_HEIGHT: f32 = 40.;

/// Compact layout breakpoint: below this viewport width the sidebar yields
/// to its rail on every route, workspace included: at 600px a full sidebar
/// would starve the main column to ~274px and hide bar buttons underneath
/// it. The preference is untouched, so the sidebar returns on widen.
const NARROW_PANEL_BREAKPOINT: f32 = 900.;
/// Main-column width that keeps the title-bar details (workspace suffix and
/// speed tag): below it both collapse. Measured worst cases against the
/// main column (not the viewport): hidden needs ~448px and settles at 552
/// on a 600px window with the rail; shown needs ~684px (capped label plus
/// widest trailing set). The flex layout is correct end to end; these
/// bounds exist because the bar's fixed controls cannot shrink below that.
const MAIN_COLUMN_DETAILS_BREAKPOINT: f32 = 760.;
const WORKSPACE_LABEL_MAX_WIDTH: f32 = 160.;
/// Navigation rail width, shared with render_navigation.
const NAVIGATION_RAIL_WIDTH: f32 = 48.;
/// Upper bound for the sidebar resize handle beside expanded navigation.
/// Generous on purpose; the breakpoint margins absorb the slack.
const SIDEBAR_RESIZE_SLOP: f32 = 12.;
const TOOLBAR_HEIGHT: f32 = 48.;

/// Whether panels collapse for a narrow viewport. Shared by root layout
/// (sidebar rail) and chrome (title-bar budgets) so both agree on the
/// sidebar footprint for a given window.
fn compact_viewport(window: &Window) -> bool {
    window.viewport_size().width < px(NARROW_PANEL_BREAKPOINT)
}

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
        RenameSelectedFile,
        DeleteSelectedFile,
        NewSession,
        CloseSession,
        OpenCliHandoff,
        ChooseWorkspace,
        FocusComposer,
        OpenUtilities,
        OpenWorkspaceFiles,
        OpenWorkspaceReview,
        OpenThreadBrowser,
        RefreshWorkspace,
        NavigateBack,
        NavigateForward,
        ToggleInspector,
        ToggleSidebar,
        ToggleZenMode,
        ToggleStatusBar,
        ToggleFullScreen,
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
        BackgroundOperation,
        QueueBatched,
        OpenPromptHistory,
        CycleHistoryScope,
        CancelPromptHistory,
        TakeLatestQueued,
        HistoryPrevious,
        HistoryNext,
        TranscriptPrevious,
        TranscriptNext,
        TranscriptFirst,
        TranscriptLast,
        SessionPrevious,
        SessionNext,
        SessionFirst,
        SessionLast,
        OpenSelectedSession,
        NextThreadTab,
        PreviousThreadTab,
        FirstThreadTab,
        LastThreadTab,
        OpenSelectedThreadTab,
        ActivityPrevious,
        ActivityNext,
        ActivityFirst,
        ActivityLast,
        DeleteHighlightedSession,
        DismissOverlay,
        SavePreviewSource,
        OpenSettings,
        CloseWindow,
        MinimizeWindow,
        ZoomWindow,
        ShowMainWindow,
        QuitApplication,
        HideApplication,
        HideOtherApplications,
        ShowAllApplications,
        AboutApplication,
        OpenHelp
    ]
);

/// A recorded external-editor launch request, captured only under test interception.
#[cfg(test)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ExternalLaunchCall {
    pub rel_path: String,
    pub editor: files::ExternalEditor,
}

/// A recorded CLI handoff script launch: the exact script content that would
/// have been written and opened, so GUI tests assert routing without spawning
/// `/usr/bin/open`.
#[cfg(test)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct HandoffScriptCall {
    pub content: String,
    pub editor: files::ExternalEditor,
}

/// Test-only interception of external-editor launches so GUI tests never shell
/// out to `/usr/bin/open`. The completion path stays shared with production.
#[cfg(test)]
#[derive(Debug, Default)]
pub(super) struct TestLaunchIntercept {
    pub calls: Vec<ExternalLaunchCall>,
    pub handoff_scripts: Vec<HandoffScriptCall>,
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
    swipe_scroll: SwipeScrollAccumulator,
    swipe_arming: SwipeArming,
    workspace_selection_pending: bool,
    workspace_selection_task: Option<Task<()>>,
    workspace_picker_open: bool,
    workspace_picker_task: Option<Task<()>>,
    backend: Option<BackendHandle>,
    event_rx: Receiver<BackendEvent>,
    event_tx: BackendEventSender,
    event_drain_task: Option<Task<()>>,
    sessions: Vec<SessionSummary>,
    /// Limit of the last `session/list` request.
    session_list_limit: usize,
    /// Whether the last page filled exactly below the maximum, so older
    /// threads may exist beyond it. Set only where a page lands
    /// (`apply_session_list`, `prepare_workspace`); local creates/deletes
    /// shift loaded and total counts equally, so they preserve it.
    session_list_has_older: bool,
    /// Whether any `session/list` page has landed for the current backend.
    /// Pending navigation must not judge session membership before this is
    /// set; reset beside `sessions` in `prepare_workspace`.
    session_list_received: bool,
    selected_session_id: Option<String>,
    /// Open-thread tabs in first-open order; selection stays in
    /// `selected_session_id`. Tabs are window-local shortcuts — closing one
    /// never touches server-side session state.
    open_tabs: Vec<String>,
    /// Tracked focus for the tab strip. Left/Right move the shared
    /// [`Self::keyboard_focused_session`] highlight in first-open order
    /// while this handle is focused.
    tab_strip_focus: FocusHandle,
    /// Scroll handle for the tab strip. Folder tabs keep their natural
    /// width, so keyboard moves reveal the highlighted tab explicitly.
    tab_strip_scroll: ScrollHandle,
    active_primary_agent_display: Option<ActivePrimaryAgentDisplay>,
    transcript: Transcript,
    message_scroller: Entity<MessageScrollerState>,
    /// Whether the transcript scroller has completed a layout pass. List
    /// readers (`logical_scroll_top`) are unsettled before the first layout,
    /// so overlay visibility must wait for it. Reset with the transcript.
    transcript_layout_settled: bool,
    /// Snapshot of conversation-search matches for overlay cycling, seeded
    /// by search jumps and cleared on session switch or dismiss.
    filter_matches: Option<FilterMatches>,
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
    restored_timeline: Arc<Vec<vtcode_exec_events::VersionedThreadEvent>>,
    active_turn_id: Option<String>,
    finished_turn_id: Option<String>,
    /// Per-turn file-change undo flight state, keyed by canonical turn id.
    /// Absent means idle; cleared on session switch (see `turn_undo`).
    turn_undos: HashMap<String, turn_undo::TurnUndoState>,
    cancel_requested: bool,
    cancel_request_sent: bool,
    /// Turn id named by the last `turn/cancel` send, so a late error cannot
    /// reconcile a newer turn that started since. Overwritten on every send;
    /// self-invalidating as `active_turn_id` moves on.
    cancel_sent_turn_id: Option<String>,
    /// Last Escape with nothing to dismiss (CLI `Esc Esc` parity): a second
    /// press clears the composer draft only when the draft is unchanged since
    /// arming, so typing between the presses disarms the clear.
    last_escape_press: Option<(Instant, gpui_kit::SharedString)>,
    /// Inline history-recall cursor (CLI Up/Down/Ctrl+P/N parity): index into
    /// [`DesktopView::prompt_history_entries`], `None` while editing live.
    composer_history_index: Option<usize>,
    /// Live draft stashed when recall starts; restored when traversing back.
    composer_history_stash: gpui_kit::SharedString,
    /// Pre-search draft stashed when reverse search opens; restored on cancel.
    history_filter_stash: gpui_kit::SharedString,
    transcript_focus: FocusHandle,
    /// Keyboard highlight for Up/Down transcript navigation (message id).
    /// Reveals the same row actions as hover; Esc leaves it as position memory.
    keyboard_focused_message: Option<String>,
    /// Transient search/deeplink tint; the generation guards stale expiry (see `TranscriptFlash`).
    transcript_flash: Option<TranscriptFlash>,
    transcript_flash_generation: u64,
    session_focus: FocusHandle,
    /// Keyboard highlight for Up/Down threads-panel navigation (session id).
    /// Distinct from the loaded selection; Enter loads it, clicks anchor it.
    keyboard_focused_session: Option<String>,
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
    /// Latest `tracker.updated` checklist snapshot. Feeds the Tasks inspector
    /// tab and the inline transcript task block; reset on session switch and
    /// disconnect so another conversation cannot inherit it.
    tracker_snapshot: Option<vtcode_exec_events::tracker::TrackerSnapshot>,
    error: Option<String>,
    configuration: Option<ConfigurationSnapshot>,
    pending_permission_choice: Option<protocol::PermissionMode>,
    full_access_confirm: Option<FullAccessConfirmState>,
    full_access_focus: FocusHandle,
    running_confirm: Option<RunningSwitch>,
    running_confirm_focus: FocusHandle,
    armed_running_switch: Option<RunningSwitch>,
    about_open: bool,
    about_focus: FocusHandle,
    license_viewer: Option<licenses::LicenseViewerState>,
    license_viewer_focus: FocusHandle,
    license_return_focus: Option<FocusHandle>,
    license_open_task: Option<Task<()>>,
    changelog_open: bool,
    changelog_mode: updates::ChangelogMode,
    changelog_focus: FocusHandle,
    changelog_return_focus: Option<FocusHandle>,
    changelog_scroll: ScrollHandle,
    release_notes_banner_visible_now: bool,
    dismissed_release_notes_version: Option<String>,
    service_info: Option<protocol::InitializeResult>,
    interaction: Option<InteractionRequest>,
    interaction_focus: FocusHandle,
    pending_interaction_response: Option<String>,
    /// A plan-edit hotkey is in flight: the service owns the editor and will
    /// re-show the approval, so the panel stays locked on the editing copy
    /// until the follow-up request arrives.
    interaction_edit_pending: bool,
    interaction_choice_ids: Vec<Option<String>>,
    interaction_answer_texts: Vec<Option<String>>,
    interaction_step_index: usize,
    /// Mouse selection for immediate (non-wizard) choice panels. `None` means
    /// the service default (selected row, else first row). Clicking a card
    /// moves this; digits still commit immediately and Confirm submits it.
    interaction_immediate_choice_id: Option<String>,
    /// Whether the approval panel shows the full plan proposal. Reset on
    /// every new interaction and when the interaction clears.
    interaction_plan_expanded: bool,
    composer: Entity<TextareaState>,
    _composer_subscription: Subscription,
    ui_language_select: Entity<UiLanguageSelectState>,
    _ui_language_subscription: Subscription,
    file_destination_select: Entity<preferences::FileDestinationSelectState>,
    _file_destination_subscription: Subscription,
    cli_terminal_select: Entity<preferences::CliTerminalSelectState>,
    _cli_terminal_subscription: Subscription,
    turn_completion_select: Entity<preferences::TurnCompletionModeSelectState>,
    _turn_completion_subscription: Subscription,
    question_answer: Entity<TextareaState>,
    theme_mode: ThemeMode,
    files: Vec<FileEntry>,
    /// Whether the last scan hit a snapshot bound; renders the truncation
    /// notice under the file tree. Reset alongside `files` on every scan.
    files_truncated: bool,
    file_directory_paths: Arc<HashSet<String>>,
    file_tree: Entity<TreeState>,
    /// One-shot selection for the next tree rebuild (rename repoints the
    /// selection at the new path, since the old one no longer exists).
    /// Consumed by `rebuild_file_tree`.
    pending_tree_select: Option<String>,
    expanded_file_directories: HashSet<String>,
    _file_tree_subscription: Subscription,
    file_filter_input: Entity<InputState>,
    _file_filter_subscription: Subscription,
    /// Ranked hit count for the current filter query (`None` in browse mode).
    file_search_hit_count: Option<usize>,
    files_task: Option<Task<()>>,
    files_scan_cancel: Option<Arc<AtomicBool>>,
    preview_task: Option<Task<()>>,
    attachments_task: Option<Task<()>>,
    external_launch_task: Option<Task<()>>,
    file_preview: Option<FilePreview>,
    /// Highlighted markdown source for the preview toggle; the toggle swaps
    /// arms, never this state, so scroll, caret, and drafts survive.
    file_preview_source: Entity<EditorState>,
    _preview_source_subscription: Subscription,
    preview_editor_error: Option<String>,
    #[cfg(test)]
    test_launch_intercept: Option<TestLaunchIntercept>,
    /// Test-only substitution for the user's real `~/.Trash`: when set, the
    /// post-confirm delete moves the entry into this scratch directory
    /// instead of calling `NSFileManager`. Left `None` in production.
    #[cfg(test)]
    test_trash_sink: Option<PathBuf>,
    preview_focus: FocusHandle,
    preview_return_focus: Option<FocusHandle>,
    file_rename: Option<FileRenameState>,
    rename_focus: FocusHandle,
    rename_return_focus: Option<FocusHandle>,
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
    /// Kind filter for the Changes list; `None` shows every file.
    change_kind_filter: Option<DiffFileKind>,
    change_viewer: Option<Entity<DiffState>>,
    change_viewer_dirty: bool,
    route_history: RouteHistory,
    workspace_review: Option<WorkspaceReviewState>,
    review_focus: FocusHandle,
    browser_focus: FocusHandle,
    /// Tracked focus for the thread-browser session list. Arrows move the
    /// shared [`Self::keyboard_focused_session`] highlight in browser-row
    /// order while this handle is focused; `browser_focus` keeps the panel.
    browser_list_focus: FocusHandle,
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
    /// History-palette search scope; kept across opens like the CLI picker.
    history_scope: HistoryScope,
    /// Last cross-thread recent prompts from `session/recent-prompts`, merged
    /// under the live thread in the history palette so empty threads still
    /// offer history search. Refreshed on every palette open (async).
    recent_prompts: Vec<protocol::RecentPrompt>,
    /// Scope the cache was fetched for; responses echo their scope so
    /// out-of-order arrivals after a scope change never mislabel rows.
    recent_prompts_scope: Option<HistoryScope>,
    /// A recent-prompts request is in flight; guards against stacking one per
    /// keypress while the palette stays open.
    recent_prompts_loading: bool,
    palette_groups: PaletteGroups,
    config_page_open: bool,
    config_page_group: Option<String>,
    theme_scroll_light: ScrollHandle,
    theme_scroll_dark: ScrollHandle,
    theme_editor: appearance_themes::CustomThemeState,
    settings_scroll: ScrollHandle,
    settings_nav_scroll: ScrollHandle,
    config_rail_scroll: ScrollHandle,
    utilities_scroll: ScrollHandle,
    output_scroll: ScrollHandle,
    output_filter_input: Entity<InputState>,
    _output_filter_subscription: Subscription,
    /// Last rendered Output tail; bounded tails can change without growing.
    output_tail_seen: Option<gpui_kit::SharedString>,
    /// Selected output id at the last Output tab render; a change resets the
    /// scroll offset to the top because a new capture is a new document.
    output_follow_selection: Option<String>,
    review_scroll: ScrollHandle,
    session_scroll: VirtualListScrollHandle,
    activity_scroll: VirtualListScrollHandle,
    /// Status filter for the Activity tab; `None` shows every row.
    activity_filter: Option<ActivityState>,
    /// Filter value at the last Activity render; a change resets the list to
    /// the top because a filtered list is a new document.
    activity_follow_filter: Option<ActivityState>,
    /// Head row id at the last Activity render; a change means new entries
    /// arrived at the top while the reader may have scrolled away.
    activity_seen_head: Option<String>,
    /// Tracked focus for the Activity list. Arrows move
    /// [`Self::keyboard_focused_activity`] in visible-row order; row action
    /// buttons stay tab stops (transcript pattern — no Enter binding, so no
    /// focus/button ambiguity).
    activity_focus: FocusHandle,
    /// Keyboard highlight over Activity rows (row id). Navigation and
    /// screen-reader position only; actions stay on the row buttons.
    keyboard_focused_activity: Option<String>,
    change_list_scroll: VirtualListScrollHandle,
    browser_scroll: VirtualListScrollHandle,
    session_row_sizes: FixedRowSizeCache,
    activity_row_sizes: FixedRowSizeCache,
    tasks_row_sizes: FixedRowSizeCache,
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
    /// Live install state, updated by `update/installProgress`. Kept beside
    /// the cached snapshot rather than inside it: progress is ephemeral and
    /// must survive a status refresh that carries no install field.
    update_install_state: protocol::UpdateInstallState,
    cli_config_settings: Option<Rc<protocol::ConfigSettingsSnapshot>>,
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
    /// Decoded-ready thumbnails for image attachments, keyed by attach path.
    attachment_thumbnails: HashMap<String, Arc<gpui_kit::Image>>,
    attachments_picker_open: bool,
    attachment_picker: Option<attachment_picker::AttachmentPickerState>,
    attachment_picker_focus: FocusHandle,
    /// Mirrors `window.is_fullscreen()` for the native menu labels.
    menus_show_exit_full_screen: bool,
    picker_tree: Entity<TreeState>,
    expanded_picker_directories: HashSet<String>,
    _picker_tree_subscription: Subscription,
    picker_filter_input: Entity<InputState>,
    _picker_filter_subscription: Subscription,
    /// Ranked hit count for the current picker query (`None` in browse mode).
    picker_search_hit_count: Option<usize>,
    pending_submission: Option<SubmissionDraft>,
    pending_first_submission: bool,
    first_submission: Option<SubmissionDraft>,
    /// Prompts queued behind the running turn, oldest first. Entries are
    /// added optimistically on submit and reconciled against authoritative
    /// `turn/queue-status` snapshots; see `view/queue.rs`.
    queued_submissions: Vec<QueuedSubmission>,
    /// Authoritative queue depth from the latest status snapshot.
    queue_depth: usize,
    /// Display previews from the latest status snapshot (backstop when local
    /// entries are missing; normally the panel renders `queued_submissions`).
    queue_previews: Vec<String>,
    /// Seconds since each entry was queued, oldest first, from the latest
    /// status snapshot (empty when the service omits them).
    queue_ages_secs: Vec<u64>,
    /// Total oldest queued prompts the service discarded via the soft cap.
    queue_dropped_total: usize,
    /// Whether a take-latest request is in flight.
    take_latest_pending: bool,
    /// Take-latest text awaiting composer restore (applied with a window by
    /// `process_drained_event`).
    pending_composer_restore: Option<String>,
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
    cli_handoff: Option<CliHandoff>,
    last_usage: Option<vtcode_exec_events::Usage>,
    theme_id: String,
    preferences: preferences::Preferences,
    /// Cached login-item status; refreshed on gestures, never per render
    /// (the native ServiceManagement read is synchronous XPC).
    login_item_status: login_items::LoginItemStatus,
    system_motion_baseline: bool,
    default_code_font: gpui_kit::SharedString,
    sidebar_open: bool,
    navigation_peek: bool,
    hovered_message: Option<String>,
    zen_mode: bool,
    zen_drag_armed: bool,
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

fn workspace_picker_options() -> PathPromptOptions {
    PathPromptOptions {
        files: false,
        directories: true,
        multiple: false,
        prompt: Some(rust_i18n::t!(i18n::WORKSPACE_CHOOSE).to_string().into()),
    }
}

fn chrome_background(colors: &gpui_kit::component::theme::ThemeColor) -> gpui_kit::Hsla {
    themes::navigation_surfaces(colors.background, colors.secondary, colors.foreground).rail
}

#[cfg(test)]
mod tests;
