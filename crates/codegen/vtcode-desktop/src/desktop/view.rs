use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};

use chrono::Timelike;
use gpui_kit::assets::IconName;
use gpui_kit::base::{Disableable, FocusTrapElement, FocusableExt, InteractiveElementExt, Selectable, StyledExt};
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
    Copy, Cut, Enter, IndentInline, InputEvent, MoveDown, MoveUp, Paste, Position, Redo, SelectAll, Textarea,
    TextareaState, Undo,
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
use gpui_kit::component::skeleton::Skeleton;
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
    OsAction, PathPromptOptions, Pixels, Render, ScrollHandle, Size, Subscription, SystemMenuType, Task,
    TestSupportExt, Window, WindowOptions, actions, anchored, deferred, div, point, px, size,
};
use serde_json::{Value, json};
use tokio::sync::mpsc;
use vtcode_desktop_protocol as protocol;
use vtcode_desktop_protocol::{
    ConfigurationSnapshot, InteractionChoice, InteractionRequest, InteractionResponse, InteractionStep,
    SessionHistoryMessage, SessionHistoryPage, SessionLoadResult, SessionOutputCursor, SessionOutputPage,
    SessionSummary, ThreadEventNotification,
};

use super::files::{self, FileEntry, FilePreview};
use super::inspector::{self, ActivityCounts, ActivityRow, ActivityState, ChangesProjection, InspectorTab};
use super::markdown::render_markdown;
use super::preferences;
use super::themes;
use super::transcript::{DiffCard, DiffFile, DiffFileKind, Transcript, TranscriptItem};
use super::transport::{
    BackendCommand, BackendEvent, BackendEventSender, BackendHandle, UI_EVENT_CAPACITY, launch_backend,
    request_fits_frame,
};
use navigation_history::{NavigationHistory, NavigationLocation, PendingNavigation};

mod composer_controls;
mod configuration;
mod navigation;
mod navigation_history;
mod settings;
mod shared_preferences;
mod shortcuts;
mod typography;
mod utilities;
mod webmcp;
mod workspace_review;
use typography::FontTarget;
use workspace_review::WorkspaceReviewState;

const MAX_UI_EVENTS_PER_TICK: usize = 64;
const CONTENT_WIDTH: f32 = 720.;
const DEFAULT_UI_FONT_SIZE: f32 = 16.;
const WELCOME_COPY_COUNT: usize = 3;
const PARTIAL_REVIEW_HISTORY_MESSAGE: &str =
    "Saved change history is partial. Some historical patches are unavailable.";
const PARTIAL_ACTIVITY_HISTORY_MESSAGE: &str =
    "Saved tool history is partial. Some output may be missing or shortened.";
const ACTIVITY_ROW_HEIGHT: f32 = 72.;
const SESSION_ROW_HEIGHT: f32 = 36.;

struct WelcomeCopy {
    action: String,
    workspace_action: String,
    call_to_action: String,
}

struct FileScanCancellationGuard(Arc<AtomicBool>);

impl Drop for FileScanCancellationGuard {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
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

fn service_reconnect_guidance(has_retained_submission: bool) -> &'static str {
    if has_retained_submission {
        "Restore or dismiss the retained message, then choose Reload CLI settings in Settings → VT Code to reconnect."
    } else {
        "Choose Reload CLI settings in Settings → VT Code to reconnect."
    }
}

fn scale_row_height_for_ui_font(row_height: f32, font_size: Pixels) -> f32 {
    row_height * font_size.as_f32() / DEFAULT_UI_FONT_SIZE
}

/// Reuse fixed virtual-list geometry until its row count or height changes.
#[derive(Default)]
struct FixedRowSizeCache {
    row_height_bits: u32,
    sizes: Rc<Vec<Size<Pixels>>>,
}

impl FixedRowSizeCache {
    fn get(&mut self, row_count: usize, row_height: f32) -> Rc<Vec<Size<Pixels>>> {
        let row_height_bits = row_height.to_bits();
        if self.sizes.len() != row_count || self.row_height_bits != row_height_bits {
            self.sizes = Rc::new(vec![size(px(0.), px(row_height)); row_count]);
            self.row_height_bits = row_height_bits;
        }
        Rc::clone(&self.sizes)
    }
}

fn accessible_group_box(id: &'static str, label: impl Into<gpui_kit::SharedString>, group: GroupBox) -> AnyElement {
    div()
        .id(id)
        .test_support()
        .role(gpui_kit::Role::Group)
        .aria_label(label)
        .w_full()
        .min_w_0()
        .flex_shrink_0()
        .child(group.id(format!("{id}-surface")))
        .into_any_element()
}

fn skeleton_line(id: &'static str, width: Pixels, cx: &App) -> AnyElement {
    let placeholder = if cx.reduce_motion() {
        div()
            .w(width)
            .max_w_full()
            .h_4()
            .rounded_md()
            .bg(cx.theme().skeleton)
            .into_any_element()
    } else {
        Skeleton::new().w(width).max_w_full().h_4().rounded_md().into_any_element()
    };

    div()
        .id(id)
        .test_support()
        .w(width)
        .max_w_full()
        .h_4()
        .child(placeholder)
        .into_any_element()
}

#[derive(Clone, Copy)]
struct ActivityChartDatum {
    label: &'static str,
    count: usize,
    state: ActivityState,
}

/// Pane-width drag bounds, shared with the preferences clamp.
const MIN_PANE_WIDTH: f32 = preferences::MIN_PANE_WIDTH;
const MAX_PANE_WIDTH: f32 = preferences::MAX_PANE_WIDTH;

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
        RefreshWorkspace,
        NavigateBack,
        NavigateForward,
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

fn init_keybindings(cx: &mut App, preferences: &preferences::Preferences) {
    match shortcuts::bindings(&preferences.shortcuts) {
        Ok(bindings) => cx.bind_keys(bindings),
        Err(error) => {
            eprintln!("Desktop shortcuts: {error}; using defaults");
            if let Ok(bindings) = shortcuts::bindings(&Default::default()) {
                cx.bind_keys(bindings);
            }
        }
    }
    cx.bind_keys([
        KeyBinding::new("cmd-w", CloseWindow, Some("VTCodeDesktop")),
        KeyBinding::new("cmd-m", MinimizeWindow, Some("VTCodeDesktop")),
        KeyBinding::new("cmd-shift-n", ShowMainWindow, None),
        KeyBinding::new("cmd-q", QuitApplication, None),
        KeyBinding::new("cmd-h", HideApplication, None),
        KeyBinding::new("alt-cmd-h", HideOtherApplications, None),
        KeyBinding::new("right", NextInspectorTab, Some("VTCodeInspector")),
        KeyBinding::new("left", PreviousInspectorTab, Some("VTCodeInspector")),
        KeyBinding::new("home", FirstInspectorTab, Some("VTCodeInspector")),
        KeyBinding::new("end", LastInspectorTab, Some("VTCodeInspector")),
        KeyBinding::new("right", NextSettingsTab, Some("VTCodeSettings")),
        KeyBinding::new("left", PreviousSettingsTab, Some("VTCodeSettings")),
        KeyBinding::new("home", FirstSettingsTab, Some("VTCodeSettings")),
        KeyBinding::new("end", LastSettingsTab, Some("VTCodeSettings")),
        KeyBinding::new("enter", OpenSelectedFile, Some(gpui_kit::base::tree_key_context())),
    ]);
}

fn init_native_menus(cx: &mut App) {
    let _app = cx.on_action(|_: &QuitApplication, cx| cx.quit());
    let _app = cx.on_action(|_: &HideApplication, cx| cx.hide());
    let _app = cx.on_action(|_: &HideOtherApplications, cx| cx.hide_other_apps());
    let _app = cx.on_action(|_: &ShowAllApplications, cx| cx.unhide_other_apps());
    let _app = cx.on_action(|_: &ShowMainWindow, cx| show_main_window(cx));
    cx.set_menus([
        Menu::new("VT Code").items([
            MenuItem::action("Settings…", OpenSettings),
            MenuItem::separator(),
            MenuItem::os_submenu("Services", SystemMenuType::Services),
            MenuItem::separator(),
            MenuItem::action("Hide VT Code", HideApplication),
            MenuItem::action("Hide Others", HideOtherApplications),
            MenuItem::action("Show All", ShowAllApplications),
            MenuItem::separator(),
            MenuItem::action("Quit VT Code", QuitApplication),
        ]),
        Menu::new("File").items([
            MenuItem::action("New Session", NewSession),
            MenuItem::action("Open Workspace…", ChooseWorkspace),
            MenuItem::action("Close Session", CloseSession),
            MenuItem::action("Choose Model…", OpenModelPicker),
        ]),
        Menu::new("Edit").items([
            MenuItem::os_action("Undo", Undo, OsAction::Undo),
            MenuItem::os_action("Redo", Redo, OsAction::Redo),
            MenuItem::separator(),
            MenuItem::os_action("Cut", Cut, OsAction::Cut),
            MenuItem::os_action("Copy", Copy, OsAction::Copy),
            MenuItem::os_action("Paste", Paste, OsAction::Paste),
            MenuItem::os_action("Select All", SelectAll, OsAction::SelectAll),
        ]),
        Menu::new("View").items([
            MenuItem::action("Command Palette…", OpenCommandPalette),
            MenuItem::action("Find in Conversation…", OpenConversationSearch),
            MenuItem::action("Focus Composer", FocusComposer),
            MenuItem::action("Toggle Inspector", ToggleInspector),
            MenuItem::action("Toggle Zen Mode", ToggleZenMode),
            MenuItem::separator(),
            MenuItem::action("Cancel Run", CancelRun),
        ]),
        Menu::new("Window").items([
            MenuItem::action("Show VT Code", ShowMainWindow),
            MenuItem::action("Minimize", MinimizeWindow),
            MenuItem::action("Zoom", ZoomWindow),
            MenuItem::action("Close Window", CloseWindow),
        ]),
    ]);
}

/// One executable entry in the command palette.
#[derive(Debug)]
struct PaletteCommand {
    label: String,
    description: Option<String>,
    keywords: Vec<String>,
    shared_keywords: Vec<gpui_kit::SharedString>,
    action: PaletteAction,
}

/// Immutable entries shared by the owning palette and its rendered callbacks.
type PaletteGroups = Vec<(String, Vec<Rc<PaletteCommand>>)>;

fn share_palette_commands(groups: Vec<(String, Vec<PaletteCommand>)>) -> PaletteGroups {
    groups
        .into_iter()
        .map(|(label, entries)| (label, entries.into_iter().map(Rc::new).collect()))
        .collect()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PaletteKind {
    Commands,
    ConversationSearch,
    ConfigSettings,
    Models,
    ModelOptions,
    AddContext,
    FileMentions,
    SlashCommands,
    PromptHistory,
    Permissions,
    Modes,
    Fonts(FontTarget),
    SettingsSearch,
    ReviewFiles,
}

/// What confirming a palette entry does. Executed through existing methods so
/// palette and toolbar stay behaviorally identical.
#[derive(Clone, Debug)]
enum PaletteAction {
    NewSession,
    CloseSession,
    OpenConversationSearch { session_id: String },
    JumpToTranscriptItem { session_id: String, item_id: String },
    ShowActivityOutput { session_id: String, item_id: String },
    ChooseModel,
    ChooseWorkspace,
    RefreshSessions,
    RefreshFiles,
    ToggleInspector,
    ToggleTheme,
    SelectFont { target: FontTarget, family: Option<String> },
    SelectSettingsTab(settings::SettingsTab),
    EditConfigSetting(String),
    LoadSession(String),
    PreviewFile(String),
    SelectModel { provider: String, model: String },
    SelectSessionOptions(protocol::SessionOptionsSelectParams),
    ToggleZenMode,
    AttachFiles,
    AttachFile(String),
    InsertFileMention(String),
    CompleteSlashCommand(String),
    InsertHistoryPrompt(String),
    InsertDraft(String),
    SelectWorkspaceReviewFile(String),
}

pub(crate) fn run() {
    let preferences = preferences::Preferences::load();
    super::localization::initialize(preferences.ui_language());
    let application = gpui_kit::application().with_assets(super::assets::DesktopAssets);
    let _application = application.on_reopen(show_main_window);
    application.run(move |cx: &mut App| {
        gpui_kit::init(cx);
        super::notifications::initialize(cx);
        init_keybindings(cx, &preferences);
        init_native_menus(cx);
        if let Err(error) = open_or_activate_main_window(cx, Some(preferences)) {
            vtcode_commons::diagnostics::stderr(format_args!("failed to open VT Code Desktop window: {error}"));
        }
    });
}

fn show_main_window(cx: &mut App) {
    if let Err(error) = open_or_activate_main_window(cx, None) {
        vtcode_commons::diagnostics::stderr(format_args!("failed to open VT Code Desktop window: {error}"));
    }
}

/// Reopening must preserve an existing window's draft, session, and backend.
fn open_or_activate_main_window(
    cx: &mut App,
    preferences: Option<preferences::Preferences>,
) -> anyhow::Result<gpui_kit::AnyWindowHandle> {
    // A dock/Finder reopen should return to the user's focused workspace when
    // multiple windows exist; keep the first-window fallback for the current
    // single-window lifecycle when the platform has no active window.
    if let Some(window) = cx.active_window().or_else(|| cx.windows().first().copied()) {
        window.update(cx, |_, window, _| window.activate_window())?;
        cx.activate(true);
        return Ok(window);
    }
    // Saved preferences decide the starting theme; Ciapre is the default.
    let preferences = preferences.unwrap_or_else(preferences::Preferences::load);
    let start_theme = preferences
        .theme
        .clone()
        .unwrap_or_else(|| themes::DEFAULT_THEME_ID.to_string());
    let (window, view) = gpui_kit::open_window(desktop_window_options(), cx, move |window, cx| {
        window.set_window_title("VT Code");
        let theme_mode = themes::apply_by_id(&start_theme, Some(window), cx);
        let view = cx.new(|cx| DesktopView::new_with_preferences(window, cx, theme_mode, preferences));
        view.update(cx, |view, cx| view.load_preferences(window, cx));
        view
    })?;
    // Reopen the most recent workspace when it still exists.
    let last_workspace = view.read(cx).preferences.recent_workspaces.first().map(PathBuf::from);
    if let Some(path) = last_workspace {
        window.update(cx, |_, _, cx| view.update(cx, |view, cx| view.select_workspace_on_startup(path, cx)))?;
    }
    view.update(cx, |view, cx| view.start_event_drain(cx));
    cx.activate(true);
    Ok(window)
}

/// Retain the exact submitted content until the service accepts it.
struct SubmissionDraft {
    prompt: String,
    attachments: Vec<protocol::TurnAttachment>,
    echo_id: String,
    delivery_uncertain: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PendingOutputPage {
    request_id: String,
    session_id: String,
    item_id: String,
    offset: u64,
}

fn valid_session_output_page(
    page: &SessionOutputPage,
    pending: &PendingOutputPage,
    cursor: &SessionOutputCursor,
) -> bool {
    let Some(byte_delta) = page.next_offset.checked_sub(page.offset) else {
        return false;
    };
    page.session_id == pending.session_id
        && page.request_id == pending.request_id
        && page.item_id == pending.item_id
        && page.offset == pending.offset
        && page.total_bytes == cursor.total_bytes
        && page.offset == cursor.next_offset
        && page.next_offset > page.offset
        && page.next_offset <= page.total_bytes
        && byte_delta <= protocol::MAX_SESSION_OUTPUT_PAGE_BYTES as u64
        && page.text.len() <= protocol::MAX_SESSION_OUTPUT_PAGE_BYTES
        && page.has_more == (page.next_offset < page.total_bytes)
        && byte_delta == page.text.len() as u64
}

fn append_restored_output_page(
    item: &mut TranscriptItem,
    pages_by_item: &mut HashMap<String, Vec<gpui_kit::SharedString>>,
    page: &mut SessionOutputPage,
) -> bool {
    let Some(current_output) = item.output.as_ref() else {
        return false;
    };
    let current_preview = current_output
        .strip_suffix(protocol::RESTORED_OUTPUT_TRUNCATION_SUFFIX)
        .unwrap_or(current_output.as_ref());
    let pages = pages_by_item
        .entry(item.id.clone())
        .or_insert_with(|| vec![current_preview.into()]);

    let mut next_preview = String::with_capacity(
        current_preview
            .len()
            .saturating_add(page.text.len())
            .saturating_add(protocol::RESTORED_OUTPUT_TRUNCATION_SUFFIX.len()),
    );
    next_preview.push_str(current_preview);
    next_preview.push_str(&page.text);
    let mut next_preview = inspector::output_tail(&next_preview.into()).to_string();
    if page.has_more {
        next_preview.push_str(protocol::RESTORED_OUTPUT_TRUNCATION_SUFFIX);
    }
    item.output = Some(next_preview.into());
    pages.push(std::mem::take(&mut page.text).into());
    true
}

fn restored_output_for_copy(
    item_id: &str,
    preview: &gpui_kit::SharedString,
    pages_by_item: &HashMap<String, Vec<gpui_kit::SharedString>>,
) -> String {
    let Some(pages) = pages_by_item.get(item_id) else {
        return preview
            .strip_suffix(protocol::RESTORED_OUTPUT_TRUNCATION_SUFFIX)
            .unwrap_or(preview.as_ref())
            .to_owned();
    };
    let capacity = pages.iter().fold(0usize, |length, page| length.saturating_add(page.len()));
    let mut output = String::with_capacity(capacity);
    for page in pages {
        output.push_str(page);
    }
    output
}

struct ActivePrimaryAgentDisplay {
    session_id: String,
    display_name: String,
}

struct DesktopView {
    focus_handle: FocusHandle,
    _focus_lost_subscription: Subscription,
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
    active_primary_agent_display: Option<ActivePrimaryAgentDisplay>,
    transcript: Transcript,
    message_scroller: Entity<MessageScrollerState>,
    restored_activity: Vec<TranscriptItem>,
    selected_output_id: Option<String>,
    restored_review_history_incomplete: bool,
    restored_activity_history_incomplete: bool,
    active_turn_id: Option<String>,
    finished_turn_id: Option<String>,
    cancel_requested: bool,
    cancel_request_sent: bool,
    running: bool,
    status: String,
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
    file_preview: Option<FilePreview>,
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
    review_open: bool,
    workspace_review: Option<WorkspaceReviewState>,
    review_focus: FocusHandle,
    review_return_focus: Option<FocusHandle>,
    git_branch: Option<String>,
    palette: Option<Entity<CommandState>>,
    palette_kind: PaletteKind,
    palette_query: gpui_kit::SharedString,
    effort_slider: Option<Entity<gpui_kit::component::slider::SliderState>>,
    _effort_slider_subscriptions: Vec<Subscription>,
    composer_resources: Vec<protocol::ComposerResource>,
    slash_commands: Vec<protocol::SlashCommandInfo>,
    resources_loading: bool,
    resources_loaded: bool,
    palette_groups: PaletteGroups,
    settings_open: bool,
    theme_scroll: ScrollHandle,
    settings_scroll: ScrollHandle,
    settings_tabs_scroll: ScrollHandle,
    session_scroll: VirtualListScrollHandle,
    activity_scroll: VirtualListScrollHandle,
    session_row_sizes: FixedRowSizeCache,
    activity_row_sizes: FixedRowSizeCache,
    settings_focus: FocusHandle,
    settings_navigation_focus: FocusHandle,
    settings_tab: settings::SettingsTab,
    settings_return_focus: Option<FocusHandle>,
    webmcp_status: Option<protocol::WebmcpBridgeStatus>,
    webmcp_pairing: Option<protocol::WebmcpPairResult>,
    webmcp_origin_input: Entity<TextareaState>,
    webmcp_request_pending: Option<&'static str>,
    webmcp_notice: Option<String>,
    cli_config_settings: Option<protocol::ConfigSettingsSnapshot>,
    config_target: protocol::ConfigTarget,
    config_value_input: Entity<TextareaState>,
    config_editor: Option<configuration::ConfigEditor>,
    config_editor_focus: FocusHandle,
    config_editor_return_focus: Option<FocusHandle>,
    config_editor_error: Option<String>,
    config_write_pending: bool,
    config_write_notice: Option<String>,
    models: Vec<protocol::ModelInfo>,
    attachments: Vec<protocol::TurnAttachment>,
    attachments_picker_open: bool,
    pending_submission: Option<SubmissionDraft>,
    first_submission: Option<SubmissionDraft>,
    session_request: Option<&'static str>,
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
    retained_submission: Option<SubmissionDraft>,
    last_usage: Option<vtcode_exec_events::Usage>,
    theme_id: String,
    preferences: preferences::Preferences,
    system_motion_baseline: bool,
    default_code_font: gpui_kit::SharedString,
    sidebar_open: bool,
    navigation_peek: bool,
    zen_mode: bool,
    shortcut_inputs: Vec<Entity<gpui_kit::component::input::InputState>>,
    shortcut_message: Option<String>,
    shortcuts_editing: bool,
    sidebar_width: f32,
    inspector_width: f32,
    resizing: Option<PaneResize>,
}

/// Which divider is being dragged.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ResizeEdge {
    Sidebar,
    Inspector,
}

#[derive(Clone, Copy)]
struct PaneResize {
    edge: ResizeEdge,
    start_pointer_x_window: Pixels,
    start_width: Pixels,
}

/// The splitter has no drag preview; keeping its entity empty lets the handle
/// provide GPUI's drag lifecycle without covering the panels during resizing.
struct ResizeDragPreview;

impl Render for ResizeDragPreview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size(px(0.))
    }
}

fn file_tree_items(entries: &[FileEntry], parent: Option<&str>, expanded: &HashSet<String>) -> Vec<TreeItem> {
    entries
        .iter()
        .filter(|entry| entry.rel_path.rsplit_once('/').map(|(parent, _)| parent) == parent)
        .map(|entry| {
            let children = if entry.is_dir {
                file_tree_items(entries, Some(&entry.rel_path), expanded)
            } else {
                Vec::new()
            };
            TreeItem::new(entry.rel_path.clone(), entry.rel_path.clone())
                .children(children)
                .expanded(expanded.contains(&entry.rel_path))
        })
        .collect()
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

impl DesktopView {
    #[cfg(test)]
    fn new(window: &mut Window, cx: &mut Context<Self>, theme_mode: ThemeMode) -> Self {
        Self::new_with_preferences(window, cx, theme_mode, preferences::Preferences::load())
    }

    fn new_with_preferences(
        window: &mut Window,
        cx: &mut Context<Self>,
        theme_mode: ThemeMode,
        preferences: preferences::Preferences,
    ) -> Self {
        let (event_tx, event_rx) = BackendEventSender::channel(UI_EVENT_CAPACITY);
        let focus_handle = cx.focus_handle();
        let focus_lost_subscription = cx.on_focus_lost(window, |view, window, cx| {
            let target = window
                .focus_lost_restore_target(cx)
                .unwrap_or_else(|| view.focus_handle.clone());
            target.focus(window, cx);
        });
        focus_handle.focus(window, cx);
        let system_motion_baseline = preferences::system_motion_baseline(cx);
        preferences::apply_reduced_motion(preferences.reduced_motion, system_motion_baseline, cx);
        let shortcut_inputs = shortcuts::inputs(&preferences.shortcuts, window, cx);
        let message_scroller = cx.new(|cx| MessageScrollerState::new(0, cx));
        let file_tree = cx.new(|cx| TreeState::new(cx));
        let file_tree_subscription = cx.subscribe(&file_tree, |view, _, event, cx| {
            match event {
                TreeEvent::Expanded(path) => {
                    let _inserted = view.expanded_file_directories.insert(path.to_string());
                }
                TreeEvent::Collapsed(path) => {
                    let _removed = view.expanded_file_directories.remove(path.as_ref());
                }
            }
            cx.notify();
        });
        let composer = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder("Ask VT Code to make changes…")
                .auto_grow(1, 6)
                .submit_on_enter(true)
        });
        let composer_subscription = cx.subscribe_in(&composer, window, |view, _, event, window, cx| {
            if matches!(event, InputEvent::Change) {
                view.update_composer_suggestions(window, cx);
            }
        });
        Self {
            shortcut_inputs,
            shortcut_message: None,
            shortcuts_editing: false,
            default_code_font: typography::default_code_font(cx),
            focus_handle,
            _focus_lost_subscription: focus_lost_subscription,
            welcome_period: welcome_period_for_hour(chrono::Local::now().hour()),
            welcome_copy_index: rand::random_range(0..WELCOME_COPY_COUNT),
            workspace: None,
            pending_workspace: None,
            navigation_history: NavigationHistory::default(),
            pending_navigation: None,
            workspace_selection_pending: false,
            workspace_selection_task: None,
            workspace_picker_open: false,
            workspace_picker_task: None,
            backend: None,
            event_rx,
            event_tx,
            event_drain_task: None,
            sessions: Vec::new(),
            selected_session_id: None,
            active_primary_agent_display: None,
            transcript: Transcript::default(),
            message_scroller,
            restored_activity: Vec::new(),
            selected_output_id: None,
            restored_review_history_incomplete: false,
            restored_activity_history_incomplete: false,
            active_turn_id: None,
            finished_turn_id: None,
            cancel_requested: false,
            cancel_request_sent: false,
            running: false,
            status: "Choose a workspace to get started".to_string(),
            error: None,
            configuration: None,
            pending_permission_choice: None,
            service_info: None,
            interaction: None,
            interaction_focus: cx.focus_handle(),
            pending_interaction_response: None,
            interaction_choice_ids: Vec::new(),
            interaction_answer_texts: Vec::new(),
            interaction_step_index: 0,
            composer,
            _composer_subscription: composer_subscription,
            question_answer: cx.new(|cx| TextareaState::new(window, cx).auto_grow(1, 4)),
            theme_mode,
            files: Vec::new(),
            file_directory_paths: Arc::new(HashSet::new()),
            file_tree,
            expanded_file_directories: HashSet::new(),
            _file_tree_subscription: file_tree_subscription,
            files_task: None,
            files_scan_cancel: None,
            preview_task: None,
            attachments_task: None,
            file_preview: None,
            preview_focus: cx.focus_handle(),
            preview_return_focus: None,
            palette_return_focus: None,
            palette_focus: cx.focus_handle(),
            sidebar_resize_focus: cx.focus_handle().tab_stop(true),
            inspector_resize_focus: cx.focus_handle().tab_stop(true),
            inspector_open: false,
            inspector_tab: InspectorTab::default(),
            inspector_focus: cx.focus_handle().tab_stop(true),
            inspector_tabs_scroll: ScrollHandle::default(),
            changes: ChangesProjection::default(),
            selected_change: None,
            change_viewer: None,
            change_viewer_dirty: false,
            review_open: false,
            workspace_review: None,
            review_focus: cx.focus_handle().tab_stop(true),
            review_return_focus: None,
            git_branch: None,
            palette: None,
            palette_kind: PaletteKind::Commands,
            palette_query: gpui_kit::SharedString::default(),
            effort_slider: None,
            _effort_slider_subscriptions: Vec::new(),
            composer_resources: Vec::new(),
            slash_commands: Vec::new(),
            resources_loading: false,
            resources_loaded: false,
            palette_groups: Vec::new(),
            settings_open: false,
            theme_scroll: ScrollHandle::default(),
            settings_scroll: ScrollHandle::default(),
            settings_tabs_scroll: ScrollHandle::default(),
            session_scroll: VirtualListScrollHandle::new(),
            activity_scroll: VirtualListScrollHandle::new(),
            session_row_sizes: FixedRowSizeCache::default(),
            activity_row_sizes: FixedRowSizeCache::default(),
            settings_focus: cx.focus_handle().tab_stop(true),
            settings_navigation_focus: cx.focus_handle().tab_stop(true),
            settings_tab: settings::SettingsTab::Appearance,
            settings_return_focus: None,
            webmcp_status: None,
            webmcp_pairing: None,
            webmcp_origin_input: cx.new(|cx| {
                TextareaState::new(window, cx)
                    .auto_grow(1, 2)
                    .placeholder("https://vtcode.vinhnx.chatgpt.site")
            }),
            webmcp_request_pending: None,
            webmcp_notice: None,
            cli_config_settings: None,
            config_target: protocol::ConfigTarget::Workspace,
            config_value_input: cx.new(|cx| TextareaState::new(window, cx).auto_grow(4, 12)),
            config_editor: None,
            config_editor_focus: cx.focus_handle().tab_stop(true),
            config_editor_return_focus: None,
            config_editor_error: None,
            config_write_pending: false,
            config_write_notice: None,
            models: Vec::new(),
            attachments: Vec::new(),
            attachments_picker_open: false,
            pending_submission: None,
            first_submission: None,
            session_request: None,
            session_load_task: None,
            session_load_generation: 0,
            history_start_index: 0,
            history_has_earlier: false,
            history_page_request_id: None,
            history_page_limit: 0,
            history_page_task: None,
            history_page_generation: 0,
            history_page_loading: false,
            history_page_error: None,
            restored_output_cursors: HashMap::new(),
            restored_output_pages: HashMap::new(),
            output_page_pending: None,
            output_page_task: None,
            output_page_error: None,
            retained_submission: None,
            last_usage: None,
            theme_id: themes::DEFAULT_THEME_ID.to_string(),
            preferences,
            system_motion_baseline,
            sidebar_open: false,
            navigation_peek: false,
            zen_mode: false,
            sidebar_width: 270.0,
            inspector_width: 320.0,
            resizing: None,
        }
    }

    /// Apply saved preferences: theme, pane widths, and inspector state.
    fn load_preferences(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let preferences = self.preferences.clone();
        self.preferences.follow_cli_theme = Some(preferences.follow_cli_theme.unwrap_or(preferences.theme.is_none()));
        if let Some(theme) = preferences.theme.as_deref() {
            self.theme_mode = themes::apply_by_id(theme, Some(window), cx);
            self.theme_id = theme.to_string();
        }
        self.apply_typography(cx);
        self.sidebar_width = preferences.sidebar_width();
        self.inspector_width = preferences.inspector_width();
        self.inspector_open = false;
        cx.notify();
    }

    /// Persist the current comfort settings.
    fn save_preferences(&mut self) {
        self.preferences.theme = Some(self.theme_id.clone());
        self.preferences.inspector_open = Some(self.inspector_open);
        self.preferences.set_widths(self.sidebar_width, self.inspector_width);
        self.preferences.save();
    }

    /// Remember a workspace as recently used.
    fn remember_workspace(&mut self, workspace: &Path) {
        self.preferences.touch_workspace(&workspace.display().to_string());
        self.preferences.save();
    }

    /// The saved recent workspaces for the sidebar.
    fn recent_workspaces(&self) -> Vec<String> {
        self.preferences.recent_workspaces.clone()
    }

    fn choose_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.workspace_picker_open {
            return;
        }
        if self.retained_submission.is_some() {
            self.error = Some("Restore or dismiss the unsent message before changing workspaces.".to_string());
            cx.notify();
            return;
        }
        if !self.workspace_switch_idle() {
            self.error = Some(
                "Cancel the active turn or wait for the pending session change before switching workspaces."
                    .to_string(),
            );
            cx.notify();
            return;
        }

        self.workspace_picker_open = true;
        let paths = cx.prompt_for_paths(workspace_picker_options());
        cx.notify();
        self.workspace_picker_task = Some(cx.spawn_in(window, async move |view, cx| {
            let result = paths.await;
            let _updated = view.update_in(cx, |view, window, cx| {
                view.workspace_picker_open = false;
                match result {
                    Ok(Ok(Some(paths))) => {
                        if let Some(path) = paths.into_iter().next() {
                            view.select_workspace_from_ui(path, window, cx);
                        }
                    }
                    Ok(Err(error)) => view.error = Some(format!("Unable to open the workspace picker: {error}")),
                    Ok(Ok(None)) => {}
                    Err(_) => view.error = Some("The workspace picker closed without returning a result.".to_string()),
                }
                cx.notify();
            });
        }));
    }

    fn workspace_switch_idle(&self) -> bool {
        !self.running
            && self.pending_workspace.is_none()
            && !self.workspace_selection_pending
            && self.session_request.is_none()
    }

    fn current_navigation_location(&self) -> Option<NavigationLocation> {
        Some(NavigationLocation {
            workspace: self.workspace.clone()?,
            session_id: self.selected_session_id.clone(),
        })
    }

    fn record_current_navigation(&mut self) {
        if let Some(location) = self.current_navigation_location() {
            self.navigation_history.record(location);
        }
    }

    fn can_navigate_back(&self) -> bool {
        self.navigation_history.back_index().is_some()
            && self.pending_navigation.is_none()
            && self.session_change_allowed(None)
    }

    fn can_navigate_forward(&self) -> bool {
        self.navigation_history.forward_index().is_some()
            && self.pending_navigation.is_none()
            && self.session_change_allowed(None)
    }

    fn navigate_back(&mut self, cx: &mut Context<Self>) {
        if self.can_navigate_back()
            && let Some(index) = self.navigation_history.back_index()
        {
            self.navigate_to_history_index(index, cx);
        }
    }

    fn navigate_forward(&mut self, cx: &mut Context<Self>) {
        if self.can_navigate_forward()
            && let Some(index) = self.navigation_history.forward_index()
        {
            self.navigate_to_history_index(index, cx);
        }
    }

    fn navigate_to_history_index(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(target) = self.navigation_history.location(index).cloned() else {
            return;
        };
        let workspace_switch = self.workspace.as_ref() != Some(&target.workspace);
        if workspace_switch {
            self.pending_navigation = Some(PendingNavigation { target_index: index, workspace_switch: true });
            self.select_workspace(target.workspace.clone(), cx);
            if self.workspace.as_ref() != Some(&target.workspace)
                && self.pending_workspace.as_ref() != Some(&target.workspace)
                && !self.workspace_selection_pending
            {
                self.pending_navigation = None;
            }
            return;
        }

        if target.session_id == self.selected_session_id {
            let _moved = self.navigation_history.navigate_to(index);
            cx.notify();
            return;
        }

        if let Some(session_id) = target.session_id {
            if !self.sessions.iter().any(|session| session.session_id == session_id) {
                self.error = Some("This thread is no longer available in the selected workspace.".into());
                cx.notify();
                return;
            }
            self.pending_navigation = Some(PendingNavigation { target_index: index, workspace_switch: false });
            if !self.request_session_change(protocol::method::SESSION_LOAD, json!({ "sessionId": session_id }), cx) {
                self.pending_navigation = None;
            }
        } else {
            let attachments = std::mem::take(&mut self.attachments);
            self.selected_session_id = None;
            self.reset_session_state();
            self.attachments = attachments;
            let _moved = self.navigation_history.navigate_to(index);
            self.status = "Workspace ready. Start a new thread to continue.".into();
            cx.notify();
        }
    }

    fn continue_pending_workspace_navigation(&mut self, cx: &mut Context<Self>) {
        let Some(pending) = self.pending_navigation.filter(|pending| pending.workspace_switch) else {
            return;
        };
        let Some(target) = self.navigation_history.location(pending.target_index).cloned() else {
            self.fail_pending_navigation();
            return;
        };
        if self.workspace.as_ref() != Some(&target.workspace) {
            self.fail_pending_navigation();
            return;
        }
        let Some(session_id) = target.session_id else {
            self.pending_navigation = None;
            let _moved = self.navigation_history.navigate_to(pending.target_index);
            self.status = "Workspace ready. Start a new thread to continue.".into();
            return;
        };
        if !self.sessions.iter().any(|session| session.session_id == session_id) {
            self.error = Some("This thread is no longer available in the selected workspace.".into());
            self.fail_pending_navigation();
            return;
        }
        if !self.request_session_change(protocol::method::SESSION_LOAD, json!({ "sessionId": session_id }), cx) {
            self.fail_pending_navigation();
        }
    }

    fn fail_pending_navigation(&mut self) {
        let Some(pending) = self.pending_navigation.take() else {
            return;
        };
        if pending.workspace_switch
            && let Some(location) = self.current_navigation_location()
        {
            let _replaced = self.navigation_history.replace_and_navigate_to(pending.target_index, location);
        }
    }

    fn select_workspace_from_ui(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.select_workspace_with_invalid_path_feedback(path, true, cx);
        self.restore_focus_after_review_close(window, cx);
    }

    fn select_workspace(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.select_workspace_with_invalid_path_feedback(path, true, cx);
    }

    fn select_workspace_on_startup(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.select_workspace_with_invalid_path_feedback(path, false, cx);
    }

    fn select_workspace_with_invalid_path_feedback(
        &mut self,
        path: PathBuf,
        show_invalid_path_error: bool,
        cx: &mut Context<Self>,
    ) {
        // A turn may have started while the asynchronous picker was open.
        if self.retained_submission.is_some() {
            self.error = Some("Restore or dismiss the unsent message before changing workspaces.".to_string());
            cx.notify();
            return;
        }
        if !self.workspace_switch_idle() {
            self.error = Some(
                "Cancel the active turn or wait for the pending session change before switching workspaces."
                    .to_string(),
            );
            cx.notify();
            return;
        }
        self.workspace_selection_pending = true;
        self.error = None;
        self.workspace_selection_task = Some(cx.spawn(async move |view, cx| {
            let canonical = cx
                .background_spawn(async move { files::canonicalize_workspace_folder(&path) })
                .await;
            let _updated = view.update(cx, |view, cx| {
                view.workspace_selection_pending = false;
                match canonical {
                    Some(canonical) => view.select_canonical_workspace(canonical, cx),
                    None => {
                        view.fail_pending_navigation();
                        if show_invalid_path_error {
                            view.error = Some("Choose an existing workspace folder.".to_string());
                        }
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn select_canonical_workspace(&mut self, canonical: PathBuf, cx: &mut Context<Self>) {
        // Revalidate because a turn or session transition may start while the path is checked.
        if self.retained_submission.is_some() {
            self.fail_pending_navigation();
            self.error = Some("Restore or dismiss the unsent message before changing workspaces.".to_string());
            return;
        }
        if !self.workspace_switch_idle() {
            self.fail_pending_navigation();
            self.error = Some(
                "Cancel the active turn or wait for the pending session change before switching workspaces."
                    .to_string(),
            );
            return;
        }
        if self.workspace.as_ref() == Some(&canonical) {
            self.continue_pending_workspace_navigation(cx);
            return;
        }
        if let Some(backend) = self.backend.as_ref() {
            self.pending_workspace = Some(canonical);
            self.status = "Shutting down the current workspace service…".to_string();
            if backend.commands.try_send(BackendCommand::Stop).is_err() {
                self.pending_workspace = None;
                self.fail_pending_navigation();
                self.error = Some("The local service request queue is full. Try again.".to_string());
            }
            cx.notify();
            return;
        }
        self.start_workspace(canonical, cx);
    }

    fn start_workspace(&mut self, workspace: PathBuf, cx: &mut Context<Self>) {
        self.prepare_workspace(&workspace, cx);
        match launch_backend(workspace, self.event_tx.clone()) {
            Ok(backend) => self.initialize_backend(backend),
            Err(error) => self.report_service_launch_failure(error),
        }
        cx.notify();
    }

    fn report_service_launch_failure(&mut self, error: String) {
        self.fail_pending_navigation();
        self.status = "Could not connect to the local VT Code service".to_string();
        self.error = Some(format!("{error}\n\n{}", service_reconnect_guidance(self.retained_submission.is_some())));
    }

    fn prepare_workspace(&mut self, workspace: &Path, cx: &mut Context<Self>) {
        // A same-workspace restart reloads CLI configuration without losing draft sources.
        let same_workspace = self.workspace.as_deref() == Some(workspace);
        self.reset_webmcp_state(None);
        let draft_sources = same_workspace.then(|| std::mem::take(&mut self.attachments));
        self.workspace = Some(workspace.to_path_buf());
        self.remember_workspace(workspace);
        self.pending_workspace = None;
        self.sessions.clear();
        self.selected_session_id = None;
        self.reset_session_state();
        if self.pending_navigation.is_none() {
            self.record_current_navigation();
        }
        if let Some(sources) = draft_sources {
            self.attachments = sources;
        }
        self.configuration = None;
        self.cli_config_settings = None;
        self.config_editor = None;
        self.config_editor_error = None;
        self.config_write_pending = false;
        if !same_workspace {
            self.config_target = protocol::ConfigTarget::Workspace;
            self.config_write_notice = None;
        }
        self.service_info = None;
        self.pending_permission_choice = None;
        self.composer_resources.clear();
        self.slash_commands.clear();
        self.resources_loading = false;
        self.resources_loaded = false;
        self.active_turn_id = None;
        self.cancel_requested = false;
        self.cancel_request_sent = false;
        self.running = false;
        self.error = None;
        self.status = "Starting local VT Code service…".to_string();
        if !same_workspace {
            self.files.clear();
            self.file_directory_paths = Arc::new(HashSet::new());
            self.expanded_file_directories.clear();
            self.file_tree.update(cx, |tree, cx| tree.set_items(Vec::new(), cx));
        }
        self.git_branch = None;
        self.preview_task = None;
        self.refresh_files(cx);
        self.file_preview = None;
        self.models.clear();
    }

    fn initialize_backend(&mut self, backend: BackendHandle) {
        let initialize = protocol::InitializeParams {
            protocol: protocol::ProtocolRange {
                minimum: protocol::PROTOCOL_VERSION,
                maximum: protocol::PROTOCOL_VERSION,
            },
            client_name: "VT Code Desktop".to_string(),
            client_version: env!("CARGO_PKG_VERSION").to_string(),
        };
        if send_request(&backend.commands, protocol::method::INITIALIZE, json!(initialize)) {
            self.backend = Some(backend);
            // Reuse the session transition gate until the handshake is validated.
            self.session_request = Some(protocol::method::INITIALIZE);
        } else {
            self.report_service_launch_failure("The local service request queue is unavailable.".to_string());
        }
    }

    fn start_event_drain(&mut self, cx: &mut Context<Self>) {
        if self.event_drain_task.is_some() {
            return;
        }

        let event_tx = self.event_tx.clone();
        self.event_drain_task = Some(cx.spawn(async move |weak_view, cx| {
            loop {
                event_tx.notified().await;
                match weak_view.update_in(cx, |view, window, cx| view.drain_events(window, cx)) {
                    Ok(batch_full) if batch_full => event_tx.wake(),
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
        }));
    }

    fn session_change_idle(&self) -> bool {
        self.workspace_switch_idle() && self.backend.is_some() && self.interaction.is_none()
    }

    fn session_change_allowed(&self, reload_session_id: Option<&str>) -> bool {
        self.session_change_idle()
            && (self.retained_submission.is_none()
                || reload_session_id.is_some_and(|id| self.selected_session_id.as_deref() == Some(id)))
    }

    fn request_session_change(&mut self, method: &'static str, params: Value, cx: &mut Context<Self>) -> bool {
        let reload_session_id = (method == protocol::method::SESSION_LOAD)
            .then(|| params.get("sessionId").and_then(Value::as_str))
            .flatten();
        if !self.session_change_allowed(reload_session_id) {
            if self.retained_submission.is_some() && self.session_change_idle() {
                self.error =
                    Some("Restore or dismiss the unsent message before changing sessions or models.".to_string());
                cx.notify();
            }
            return false;
        }
        self.clear_error();
        let sent = self.request(method, params);
        if sent {
            self.session_request = Some(method);
            self.status = "Updating session…".to_string();
        } else {
            self.error = Some("The local service request queue is full. Try again.".to_string());
        }
        cx.notify();
        sent
    }

    fn create_session(&mut self, cx: &mut Context<Self>) {
        let _sent = self.request_session_change(protocol::method::SESSION_CREATE, json!({ "title": null }), cx);
    }

    fn load_session(&mut self, session_id: String, cx: &mut Context<Self>) {
        let _sent = self.request_session_change(protocol::method::SESSION_LOAD, json!({ "sessionId": session_id }), cx);
    }

    fn request_earlier_history(&mut self, cx: &mut Context<Self>) {
        let Some(session_id) = self.selected_session_id.clone() else {
            return;
        };
        if !self.history_has_earlier || self.running || self.history_page_loading {
            return;
        }
        let limit = self
            .transcript
            .remaining_capacity()
            .min(protocol::MAX_SESSION_HISTORY_PAGE_MESSAGES);
        if limit == 0 {
            return;
        }
        let request_id = uuid_like_id();
        if self.request(
            protocol::method::SESSION_HISTORY,
            json!({
                "sessionId": session_id,
                "requestId": request_id,
                "beforeIndex": self.history_start_index,
                "limit": limit,
            }),
        ) {
            self.history_page_generation = self.history_page_generation.wrapping_add(1);
            self.history_page_request_id = Some(request_id);
            self.history_page_limit = limit;
            self.history_page_loading = true;
            self.history_page_error = None;
        } else {
            self.history_page_error = Some("The local service request queue is full. Try again.".into());
        }
        cx.notify();
    }

    fn fail_history_page(&mut self, error: String, cx: &mut Context<Self>) {
        self.history_page_generation = self.history_page_generation.wrapping_add(1);
        self.history_page_request_id = None;
        self.history_page_limit = 0;
        self.history_page_task = None;
        self.history_page_loading = false;
        self.history_page_error = Some(error);
        cx.notify();
    }

    fn request_more_captured_output(&mut self, item_id: &str, cx: &mut Context<Self>) {
        if self.output_page_pending.is_some() {
            return;
        }
        let Some(session_id) = self.selected_session_id.clone() else {
            return;
        };
        let Some(cursor) = self.restored_output_cursors.get(item_id) else {
            return;
        };
        let pending = PendingOutputPage {
            request_id: uuid_like_id(),
            session_id,
            item_id: item_id.to_string(),
            offset: cursor.next_offset,
        };
        if self.request(
            protocol::method::SESSION_OUTPUT_READ,
            json!({
                "sessionId": pending.session_id,
                "requestId": pending.request_id,
                "itemId": pending.item_id,
                "offset": pending.offset,
            }),
        ) {
            self.output_page_pending = Some(pending);
            self.output_page_error = None;
        } else {
            self.output_page_error =
                Some((item_id.to_string(), "The local service request queue is full. Try again.".into()));
        }
        cx.notify();
    }

    fn fail_output_page(&mut self, cx: &mut Context<Self>) {
        let item_id = self.output_page_pending.take().map(|pending| pending.item_id);
        self.output_page_task = None;
        self.output_page_error =
            item_id.map(|item_id| (item_id, "Could not load more captured output. Try again.".into()));
        cx.notify();
    }

    fn begin_session_output_page(&mut self, result: Value, cx: &mut Context<Self>) {
        let Some(pending) = self.output_page_pending.clone() else {
            return;
        };
        self.output_page_task = Some(cx.spawn(async move |view, cx| {
            let page = cx
                .background_spawn(async move { serde_json::from_value::<SessionOutputPage>(result) })
                .await;
            let _updated = view.update(cx, |view, cx| {
                if view.output_page_pending.as_ref() != Some(&pending) {
                    return;
                }
                if let Some(task) = view.output_page_task.take() {
                    task.detach();
                }
                let Ok(mut page) = page else {
                    view.fail_output_page(cx);
                    return;
                };
                let Some(cursor) = view.restored_output_cursors.get(&pending.item_id).cloned() else {
                    view.fail_output_page(cx);
                    return;
                };
                if view.selected_session_id.as_deref() != Some(pending.session_id.as_str())
                    || !valid_session_output_page(&page, &pending, &cursor)
                {
                    view.fail_output_page(cx);
                    return;
                }

                let Some(item) = view.restored_activity.iter_mut().find(|item| item.id == pending.item_id) else {
                    view.fail_output_page(cx);
                    return;
                };
                let updated_output = append_restored_output_page(item, &mut view.restored_output_pages, &mut page);
                if !updated_output {
                    view.fail_output_page(cx);
                    return;
                }

                view.output_page_pending = None;
                view.output_page_error = None;
                if page.has_more {
                    let _previous = view.restored_output_cursors.insert(
                        pending.item_id,
                        SessionOutputCursor {
                            item_id: page.item_id,
                            next_offset: page.next_offset,
                            total_bytes: page.total_bytes,
                        },
                    );
                } else {
                    let _previous = view.restored_output_cursors.remove(&pending.item_id);
                }
                view.refresh_conversation_search(&pending.session_id);
                cx.notify();
            });
        }));
    }

    /// Decode and prepare bounded historical content away from the UI thread.
    fn begin_session_load(&mut self, result: Value, cx: &mut Context<Self>) {
        if self.session_request != Some(protocol::method::SESSION_LOAD) {
            return;
        }
        self.session_load_generation = self.session_load_generation.wrapping_add(1);
        let generation = self.session_load_generation;
        self.status = "Restoring conversation…".to_string();
        self.session_load_task = Some(cx.spawn(async move |view, cx| {
            let loaded = cx
                .background_spawn(async move {
                    serde_json::from_value::<SessionLoadResult>(result).map(|mut loaded| {
                        let history =
                            prepare_session_history(&loaded.session.session_id, std::mem::take(&mut loaded.messages));
                        (loaded, history)
                    })
                })
                .await;
            let _updated = view.update(cx, |view, cx| {
                if view.session_load_generation != generation
                    || view.session_request != Some(protocol::method::SESSION_LOAD)
                {
                    return;
                }
                if let Some(task) = view.session_load_task.take() {
                    // The completion callback is running inside this task; let it return normally.
                    task.detach();
                }
                view.session_request = None;
                match loaded {
                    Ok((loaded, history)) => view.accept_loaded_session(loaded, history, cx),
                    Err(_) => {
                        view.fail_pending_navigation();
                        view.status = "Session load failed".to_string();
                        view.error = Some("The local service returned an invalid session load result.".into());
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// Decode and prepare one bounded archive-history page off the UI thread.
    fn begin_session_history_page(&mut self, result: Value, cx: &mut Context<Self>) {
        let Some(request_id) = self.history_page_request_id.clone() else {
            return;
        };
        let Some(session_id) = self.selected_session_id.clone() else {
            return;
        };
        let generation = self.history_page_generation;
        let before_index = self.history_start_index;
        let limit = self.history_page_limit;
        self.history_page_task = Some(cx.spawn(async move |view, cx| {
            let page = cx
                .background_spawn(async move {
                    serde_json::from_value::<SessionHistoryPage>(result).map(|mut page| {
                        let valid_indices = page.messages.iter().enumerate().all(|(offset, message)| {
                            page.start_index.checked_add(offset) == message.archive_index
                        });
                        let valid = !page.messages.is_empty()
                            && page.messages.len() <= limit
                            && page.start_index < before_index
                            && valid_indices
                            && page
                                .messages
                                .last()
                                .and_then(|message| message.archive_index)
                                .is_some_and(|index| index < before_index)
                            && page.has_earlier == (page.start_index > 0);
                        valid.then(|| {
                            let history = prepare_session_history(&page.session_id, std::mem::take(&mut page.messages));
                            (page, history)
                        })
                    })
                })
                .await;
            let _updated = view.update(cx, |view, cx| {
                if view.history_page_generation != generation
                    || view.history_page_request_id.as_deref() != Some(request_id.as_str())
                {
                    return;
                }
                if let Some(task) = view.history_page_task.take() {
                    task.detach();
                }
                view.history_page_request_id = None;
                view.history_page_limit = 0;
                view.history_page_loading = false;
                match page {
                    Ok(Some((page, history)))
                        if page.request_id == request_id
                            && page.session_id == session_id
                            && view.selected_session_id.as_deref() == Some(session_id.as_str()) =>
                    {
                        let inserted = view.transcript.prepend_prepared(history);
                        if inserted == 0 {
                            view.history_page_error = Some(
                                "The transcript reached its local message limit. Start a new session to continue browsing.".into(),
                            );
                        } else {
                            view.history_start_index = page.start_index;
                            view.history_has_earlier = page.has_earlier;
                            view.history_page_error = None;
                        }
                    }
                    Ok(_) => {
                        view.history_page_error = Some("The local service returned an invalid history page.".into());
                    }
                    Err(_) => {
                        view.history_page_error = Some("The local service returned an invalid history page.".into());
                    }
                }
                view.refresh_conversation_search(&session_id);
                cx.notify();
            });
        }));
    }

    fn accept_loaded_session(
        &mut self,
        loaded: SessionLoadResult,
        history: Vec<TranscriptItem>,
        cx: &mut Context<Self>,
    ) {
        if loaded.session.session_id.trim().is_empty() {
            self.fail_pending_navigation();
            self.status = "Session load failed".to_string();
            self.error = Some("The local service returned an invalid session load result.".into());
            return;
        }
        if let Some(pending) = self.pending_navigation
            && self
                .navigation_history
                .location(pending.target_index)
                .and_then(|location| location.session_id.as_deref())
                != Some(loaded.session.session_id.as_str())
        {
            self.error = Some("The service returned a different thread than the navigation target.".into());
            self.fail_pending_navigation();
            return;
        }
        if self.retained_submission.is_some()
            && self.selected_session_id.as_deref() != Some(loaded.session.session_id.as_str())
        {
            self.error = Some("The service returned a different session; the unsent message was retained.".to_string());
            return;
        }

        let retained_submission = self.retained_submission.take();
        self.load_history(history);
        self.history_start_index = loaded.history_start_index;
        self.history_has_earlier = loaded.history_has_earlier && loaded.history_start_index > 0;
        self.restored_review_history_incomplete = loaded.review_history_incomplete;
        self.restored_activity_history_incomplete = loaded.activity_history_incomplete;
        let output_item_ids = loaded
            .activity_items
            .iter()
            .filter(|item| matches!(&item.details, vtcode_exec_events::ThreadItemDetails::ToolOutput(_)))
            .map(|item| item.id.as_str())
            .collect::<HashSet<_>>();
        self.restored_output_cursors.clear();
        self.restored_output_pages.clear();
        for cursor in loaded.activity_output_cursors {
            if !cursor.item_id.is_empty()
                && cursor.item_id.len() <= 512
                && cursor.next_offset > 0
                && cursor.next_offset <= protocol::MAX_SESSION_OUTPUT_PAGE_BYTES as u64
                && cursor.next_offset < cursor.total_bytes
                && output_item_ids.contains(cursor.item_id.as_str())
            {
                let _previous = self.restored_output_cursors.insert(cursor.item_id.clone(), cursor);
            }
        }
        self.output_page_pending = None;
        self.output_page_task = None;
        self.output_page_error = None;
        for item in &loaded.review_items {
            if matches!(item.details, vtcode_exec_events::ThreadItemDetails::FileChange(_)) {
                self.update_runtime_item(item);
            }
        }
        for item in &loaded.activity_items {
            use vtcode_exec_events::ThreadItemDetails;
            if !matches!(
                item.details,
                ThreadItemDetails::AgentMessage(_)
                    | ThreadItemDetails::Plan(_)
                    | ThreadItemDetails::Reasoning(_)
                    | ThreadItemDetails::Decision(_)
                    | ThreadItemDetails::FileChange(_)
            ) {
                self.project_runtime_item(item, true);
            }
        }
        self.retained_submission = retained_submission;
        self.selected_session_id = Some(loaded.session.session_id.clone());
        if let Some(pending) = self.pending_navigation.take() {
            let _moved = self.navigation_history.navigate_to(pending.target_index);
        } else {
            self.record_current_navigation();
        }
        self.status = if loaded.review_history_incomplete || loaded.activity_history_incomplete {
            "Session loaded. Recent inspector history is partial or unavailable."
        } else {
            "Session loaded. Submit a prompt to continue."
        }
        .to_string();
        cx.notify();
    }

    fn close_session(&mut self, cx: &mut Context<Self>) {
        let Some(session_id) = self.selected_session_id.clone() else {
            return;
        };
        let _sent =
            self.request_session_change(protocol::method::SESSION_CLOSE, json!({ "sessionId": session_id }), cx);
    }

    /// Pick workspace text files to attach to the next prompt.
    fn attach_files(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        const MAX_ATTACHMENTS: usize = 5;

        if self.attachments_picker_open || self.workspace.is_none() || self.session_request.is_some() {
            return;
        }
        self.attachments_picker_open = true;
        let Some(workspace) = self.workspace.clone() else {
            self.attachments_picker_open = false;
            return;
        };
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Attach files".into()),
        });
        cx.notify();
        let session_id = self.selected_session_id.clone();
        self.attachments_task = Some(cx.spawn(async move |view, cx| {
            let result = picked.await;
            let result = match result {
                Ok(Ok(Some(paths))) => {
                    let root = workspace.clone();
                    // Bound both work and retained data, even for a very large native selection.
                    Ok(cx
                        .background_spawn(async move {
                            let overflow = paths.len() > MAX_ATTACHMENTS;
                            let results = paths
                                .into_iter()
                                .take(MAX_ATTACHMENTS)
                                .map(|path| files::read_attachment(&root, &path))
                                .collect::<Vec<_>>();
                            (results, overflow)
                        })
                        .await)
                }
                Ok(Ok(None)) => Ok((Vec::new(), false)),
                Ok(Err(error)) => Err(format!("Unable to open the file picker: {error}")),
                Err(_) => Err("The file picker was disconnected.".to_string()),
            };
            let _updated = view.update(cx, |view, cx| {
                view.attachments_picker_open = false;
                if view.workspace.as_ref() != Some(&workspace) || view.selected_session_id != session_id {
                    cx.notify();
                    return;
                }
                match result {
                    Err(error) => view.error = Some(error),
                    Ok((results, overflow)) => {
                        if overflow {
                            view.error = Some(format!("At most {MAX_ATTACHMENTS} attachments per prompt."));
                        }
                        for result in results {
                            match result {
                                Ok(attachment) => {
                                    if view.attachments.iter().any(|existing| existing.path == attachment.path) {
                                        continue;
                                    }
                                    if view.attachments.len() >= MAX_ATTACHMENTS {
                                        view.error = Some(format!("At most {MAX_ATTACHMENTS} attachments per prompt."));
                                        break;
                                    }
                                    view.attachments.push(attachment);
                                }
                                Err(error) => view.error = Some(error),
                            }
                        }
                    }
                }
                cx.notify();
            });
        }));
    }

    fn remove_attachment(&mut self, path: String, cx: &mut Context<Self>) {
        self.attachments.retain(|attachment| attachment.path != path);
        cx.notify();
    }

    /// Switch the service to another catalog model. The service restarts its
    /// session; the next prompt starts a fresh one under the new model.
    fn select_model(&mut self, provider: String, model: String, cx: &mut Context<Self>) {
        if self.attachments_picker_open {
            return;
        }
        if self.request_session_change(
            protocol::method::CONFIG_SELECT,
            json!({ "provider": provider, "model": model }),
            cx,
        ) {
            self.status = "Switching model…".to_string();
        }
    }

    fn refresh_files(&mut self, cx: &mut Context<Self>) {
        if let Some(previous) = self.files_scan_cancel.take() {
            previous.store(true, Ordering::Relaxed);
        }
        let Some(workspace) = self.workspace.clone() else {
            return;
        };
        let cancellation = Arc::new(AtomicBool::new(false));
        self.files_scan_cancel = Some(cancellation.clone());
        let worker_cancellation = cancellation.clone();
        self.files_task = Some(cx.spawn(async move |view, cx| {
            let _cancel_on_drop = FileScanCancellationGuard(cancellation.clone());
            let root = workspace.clone();
            let result = cx
                .background_spawn(async move {
                    let entries =
                        files::scan_workspace_cancellable(&root, || worker_cancellation.load(Ordering::Relaxed))?;
                    if worker_cancellation.load(Ordering::Relaxed) {
                        return None;
                    }
                    Some((entries, files::git_branch(&root)))
                })
                .await;
            let _updated = view.update_in(cx, |view, window, cx| {
                let is_current_request = view
                    .files_scan_cancel
                    .as_ref()
                    .is_some_and(|active| Arc::ptr_eq(active, &cancellation));
                if !is_current_request {
                    return;
                }
                view.files_scan_cancel = None;
                if view.workspace.as_ref() != Some(&workspace) {
                    return;
                }
                let Some((entries, branch)) = result else {
                    return;
                };
                view.replace_file_entries(entries, cx);
                view.git_branch = branch;
                view.update_composer_suggestions(window, cx);
                cx.notify();
            });
        }));
    }

    fn refresh_workspace_view(&mut self, cx: &mut Context<Self>) {
        if self.review_open {
            self.refresh_workspace_review(cx);
        } else {
            self.refresh_files(cx);
        }
    }

    fn focus_workspace_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.inspector_open = true;
        self.set_inspector_tab(InspectorTab::Files, cx);

        let view = cx.entity().downgrade();
        window.defer(cx, move |window, cx| {
            let _updated = view.update(cx, |view, cx| {
                if !view.inspector_open || view.inspector_tab != InspectorTab::Files {
                    return;
                }
                view.file_tree.update(cx, |tree, cx| {
                    if tree.selected_index().is_none() && tree.entry(0).is_some() {
                        tree.set_selected_index(Some(0), cx);
                    }
                    tree.focus(window, cx);
                });
            });
        });
    }

    fn replace_file_entries(&mut self, entries: Vec<FileEntry>, cx: &mut Context<Self>) {
        let selected_path = self.file_tree.read(cx).selected_item().map(|item| item.id.to_string());
        let directory_paths: HashSet<_> = entries
            .iter()
            .filter(|entry| entry.is_dir)
            .map(|entry| entry.rel_path.clone())
            .collect();
        self.expanded_file_directories.retain(|path| directory_paths.contains(path));
        let tree_items = file_tree_items(&entries, None, &self.expanded_file_directories);
        let selected_item = selected_path
            .as_deref()
            .and_then(|path| find_file_tree_item(&tree_items, path))
            .cloned();
        self.file_tree.update(cx, |tree, cx| {
            tree.set_items(tree_items, cx);
            tree.set_selected_item(selected_item.as_ref(), cx);
        });
        self.files = entries;
        self.file_directory_paths = Arc::new(directory_paths);
    }

    fn open_selected_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.file_tree.read(cx).selected_item().map(|item| item.id.to_string()) else {
            return;
        };
        if self.files.iter().any(|entry| entry.rel_path == path && !entry.is_dir) {
            self.open_file_preview(path, window, cx);
        }
    }

    fn open_file_preview(&mut self, rel_path: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.review_open {
            self.close_review(window, cx);
        }
        let Some(workspace) = self.workspace.clone() else {
            return;
        };
        if self.file_preview.is_none() {
            self.preview_return_focus = window.focused(cx);
        }
        self.preview_task = Some(cx.spawn_in(window, async move |view, cx| {
            let root = workspace.clone();
            let path = rel_path.clone();
            let preview = cx.background_spawn(async move { files::read_preview(&root, &path) }).await;
            let _updated = view.update_in(cx, |view, window, cx| {
                if view.workspace.as_ref() != Some(&workspace) {
                    return;
                }
                match preview {
                    Some(preview) => {
                        view.file_preview = Some(preview);
                        view.preview_focus.focus(window, cx);
                        view.clear_error();
                    }
                    None => {
                        view.error = Some(format!("“{rel_path}” is outside the workspace, missing, or not UTF-8 text."))
                    }
                }
                cx.notify();
            });
        }));
    }

    fn focus_composer(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.composer.read(cx).focus_handle(cx).focus(window, cx);
    }

    fn activate_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.review_open {
            self.close_review(window, cx);
        }
        if self.settings_open {
            self.close_settings(window, cx);
        }
        if self.palette.is_some() {
            self.close_palette(window, cx);
        }
        if self.file_preview.is_some() {
            self.close_file_preview(window, cx);
        }
        self.focus_composer(window, cx);
        cx.notify();
    }

    fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings_open {
            return;
        }
        if self.review_open {
            self.close_review(window, cx);
        }
        if self.palette.is_some() {
            self.close_palette(window, cx);
        }
        if self.file_preview.is_some() {
            self.close_file_preview(window, cx);
        }
        if !self.settings_open {
            self.settings_return_focus = window.focused(cx);
        }
        self.settings_open = true;
        let settings_view = cx.entity().clone();
        let close_view = settings_view.clone();
        let initial_focus = std::cell::Cell::new(true);
        let settings_navigation_focus = self.settings_navigation_focus.clone();
        window.open_sheet_at(gpui_kit::component::Placement::Right, cx, move |sheet, window, cx| {
            if initial_focus.replace(false) {
                settings_navigation_focus.focus(window, cx);
            }
            // The Kit builder runs again as the window resizes; keep the sheet
            // within the current viewport instead of retaining its opening width.
            let sheet_width = (window.viewport_size().width.min(px(860.)) - px(24.)).max(px(0.));
            let content = settings_view.update(cx, |view, cx| view.render_settings_tabs(window, cx));
            let on_close_view = close_view.clone();
            sheet
                .title(DialogTitle::new().text_lg().child("Settings"))
                .size(sheet_width)
                .p_0()
                .child(content)
                .on_close(move |_, window, cx| {
                    on_close_view.update(cx, |view, cx| view.handle_settings_sheet_close(window, cx));
                })
        });
        cx.notify();
    }

    fn close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if window.has_active_sheet(cx) {
            window.close_sheet(cx);
        }
        self.finish_settings_close(window, cx);
    }

    fn finish_settings_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.settings_open {
            return;
        }
        self.settings_open = false;
        if self.shortcuts_editing {
            self.shortcut_inputs = shortcuts::inputs(&self.preferences.shortcuts, window, cx);
            self.shortcuts_editing = false;
            self.shortcut_message = None;
        }
        if let Some(focus) = self.settings_return_focus.take() {
            focus.focus(window, cx);
        } else {
            self.focus_composer(window, cx);
        }
        cx.notify();
    }

    fn handle_settings_sheet_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.config_editor.is_none() {
            self.finish_settings_close(window, cx);
            return;
        }

        if !self.config_write_pending {
            self.config_editor = None;
            self.config_editor_error = None;
            self.config_editor_return_focus = None;
        }
        let settings_return_focus = self.settings_return_focus.take();
        self.settings_open = false;
        self.open_settings(window, cx);
        self.settings_return_focus = settings_return_focus;
    }

    fn toggle_inspector(&mut self, cx: &mut Context<Self>) {
        if self.workspace.is_some() {
            self.inspector_open = !self.inspector_open;
            self.save_preferences();
            cx.notify();
        }
    }

    fn toggle_zen_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.zen_mode = !self.zen_mode;
        self.navigation_peek = false;
        if self.zen_mode {
            self.activate_composer(window, cx);
        } else {
            self.focus_composer(window, cx);
        }
        cx.notify();
    }

    fn close_file_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.preview_task = None;
        self.file_preview = None;
        if let Some(focus) = self.preview_return_focus.take() {
            focus.focus(window, cx);
        } else {
            self.focus_composer(window, cx);
        }
        cx.notify();
    }

    fn close_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let return_to_settings = matches!(
            self.palette_kind,
            PaletteKind::ConfigSettings | PaletteKind::Fonts(_) | PaletteKind::SettingsSearch
        ) && self.palette.is_some();
        self.clear_palette_state();
        if let Some(focus) = self.palette_return_focus.take() {
            focus.focus(window, cx);
        } else {
            self.focus_composer(window, cx);
        }
        if return_to_settings {
            self.open_settings(window, cx);
        }
        cx.notify();
    }

    fn clear_palette_state(&mut self) {
        self.palette = None;
        self.palette_query = gpui_kit::SharedString::default();
        self._effort_slider_subscriptions.clear();
        self.effort_slider = None;
        self.palette_groups.clear();
    }

    fn dismiss_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.config_editor.is_some() {
            self.close_cli_config_editor(window, cx);
        } else if self.navigation_peek {
            self.navigation_peek = false;
            cx.notify();
        } else if self.settings_open {
            self.close_settings(window, cx);
        } else if self.palette.is_some() {
            self.close_palette(window, cx);
        } else if self.review_open {
            self.close_review(window, cx);
        } else if self.file_preview.is_some() {
            self.close_file_preview(window, cx);
        } else {
            cx.propagate();
        }
    }

    /// Build and open the ⌘K command palette over the current state.
    fn open_command_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette.is_some() && self.palette_kind == PaletteKind::Commands {
            self.close_palette(window, cx);
            return;
        }
        let mut groups: Vec<(String, Vec<PaletteCommand>)> = Vec::new();
        groups.push((
            "Session".to_string(),
            vec![
                command("New session", PaletteAction::NewSession),
                command("Close session", PaletteAction::CloseSession),
                command("Switch workspace…", PaletteAction::ChooseWorkspace),
                command("Refresh recent threads", PaletteAction::RefreshSessions),
                command("Refresh workspace files", PaletteAction::RefreshFiles),
                command("Choose a model…", PaletteAction::ChooseModel),
            ],
        ));
        groups.push((
            "View".to_string(),
            vec![
                command("Toggle inspector panel", PaletteAction::ToggleInspector),
                command("Toggle Zen mode", PaletteAction::ToggleZenMode),
                command(
                    if self.theme_mode == ThemeMode::Light {
                        "Switch to Dark theme"
                    } else {
                        "Switch to Light theme"
                    },
                    PaletteAction::ToggleTheme,
                ),
            ],
        ));
        if let Some(session_id) = self.selected_session_id.clone()
            && self.has_searchable_conversation_content()
        {
            groups.push((
                "Conversation".into(),
                vec![command(
                    "Find in conversation…",
                    PaletteAction::OpenConversationSearch { session_id },
                )],
            ));
        }
        let slash_commands = self
            .slash_commands
            .iter()
            .map(|entry| {
                let mut item =
                    command(format!("/{}", entry.name), PaletteAction::InsertDraft(format!("/{} ", entry.name)));
                item.description = Some(entry.description.clone());
                item.keywords = vec![entry.name.clone(), format!("/{}", entry.name)];
                item
            })
            .collect::<Vec<_>>();
        if !slash_commands.is_empty() {
            groups.push(("VT Code commands".into(), slash_commands));
        }
        let sessions: Vec<PaletteCommand> =
            self.sessions
                .iter()
                .map(|session| {
                    let label = session.title.clone().unwrap_or_else(|| {
                        format!("Session {}", session.session_id.chars().take(8).collect::<String>())
                    });
                    command(format!("Open session: {label}"), PaletteAction::LoadSession(session.session_id.clone()))
                })
                .collect();
        if !sessions.is_empty() {
            groups.push(("Recent sessions".to_string(), sessions));
        }
        let file_commands: Vec<PaletteCommand> = self
            .files
            .iter()
            .filter(|entry| !entry.is_dir)
            .map(|entry| {
                command(format!("Preview file: {}", entry.rel_path), PaletteAction::PreviewFile(entry.rel_path.clone()))
            })
            .collect();
        if !file_commands.is_empty() {
            groups.push(("Files".to_string(), file_commands));
        }

        self.show_palette(PaletteKind::Commands, groups, window, cx);
    }

    fn open_conversation_search(&mut self, session_id: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_session_id.as_deref() != Some(session_id.as_str())
            || !self.has_searchable_conversation_content()
        {
            return;
        }
        if self.palette.is_some() && self.palette_kind == PaletteKind::ConversationSearch {
            self.close_palette(window, cx);
            return;
        }

        self.show_palette(PaletteKind::ConversationSearch, self.conversation_search_groups(&session_id), window, cx);
    }

    fn conversation_search_groups(&self, session_id: &str) -> Vec<(String, Vec<PaletteCommand>)> {
        let messages = self.conversation_search_messages(session_id);
        let outputs = self.conversation_search_outputs(session_id);
        let mut groups = Vec::with_capacity(2);
        if !messages.is_empty() {
            groups.push(("Loaded messages".into(), messages));
        }
        if !outputs.is_empty() {
            groups.push(("Loaded tool output".into(), outputs));
        }
        groups
    }

    fn conversation_search_messages(&self, session_id: &str) -> Vec<PaletteCommand> {
        self.transcript
            .items_newest_first()
            .filter(|item| is_searchable_conversation_role(&item.role) && !item.text.trim().is_empty())
            .map(|item| {
                let speaker = if item.role == "user" {
                    "You"
                } else if item.title.trim().is_empty() && item.role == "assistant" {
                    "VT Code"
                } else if item.title.trim().is_empty() {
                    "Tool activity"
                } else {
                    item.title.as_str()
                };
                let label = format!("{speaker} · {}", transcript_search_snippet(&item.text));
                let mut entry = command(
                    label,
                    PaletteAction::JumpToTranscriptItem {
                        session_id: session_id.to_string(),
                        item_id: item.id.clone(),
                    },
                );
                entry.shared_keywords.extend([item.text.clone(), item.title.clone()]);
                entry
            })
            .collect()
    }

    fn conversation_search_outputs(&self, session_id: &str) -> Vec<PaletteCommand> {
        let mut seen = HashSet::new();
        self.restored_activity
            .iter()
            .chain(self.transcript.items())
            .filter(|item| item.role == "tool" && item.output.as_ref().is_some_and(|output| !output.trim().is_empty()))
            .filter(|item| seen.insert(item.id.as_str()))
            .filter_map(|item| {
                let output = item.output.as_ref()?;
                let searchable_output = inspector::output_tail(output);
                let mut entry = command(
                    format!("Output · {} · {}", item.title, transcript_search_snippet(&searchable_output)),
                    PaletteAction::ShowActivityOutput {
                        session_id: session_id.to_string(),
                        item_id: item.id.clone(),
                    },
                );
                entry.shared_keywords.extend([searchable_output, item.title.clone()]);
                Some(entry)
            })
            .collect()
    }

    fn refresh_conversation_search(&mut self, session_id: &str) {
        if self.palette_kind != PaletteKind::ConversationSearch
            || self.selected_session_id.as_deref() != Some(session_id)
        {
            return;
        }
        self.palette_groups = share_palette_commands(self.conversation_search_groups(session_id));
    }

    fn has_searchable_conversation_content(&self) -> bool {
        self.transcript.items().any(|item| {
            (is_searchable_conversation_role(&item.role) && !item.text.trim().is_empty())
                || (item.role == "tool" && item.output.as_ref().is_some_and(|output| !output.trim().is_empty()))
        }) || self
            .restored_activity
            .iter()
            .any(|item| item.role == "tool" && item.output.as_ref().is_some_and(|output| !output.trim().is_empty()))
    }

    fn open_model_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.session_change_allowed(None) || self.attachments_picker_open {
            return;
        }
        let mut groups = std::collections::BTreeMap::<String, Vec<PaletteCommand>>::new();
        let active_model = self.active_model_fallback();
        for model in self.models.iter().chain(active_model.iter()) {
            let entry = model_palette_command(model, self.model_is_current(&model.provider, &model.id));
            groups.entry(model.provider.clone()).or_default().push(entry);
        }
        let mut groups = groups.into_iter().collect::<Vec<_>>();
        for (_, entries) in &mut groups {
            entries.sort_by_key(|entry| !self.is_current_palette_action(&entry.action));
        }
        groups.sort_by_key(|(_, entries)| !entries.iter().any(|entry| self.is_current_palette_action(&entry.action)));
        self.show_palette(PaletteKind::Models, groups, window, cx);
    }

    fn active_model_fallback(&self) -> Option<protocol::ModelInfo> {
        self.configuration
            .as_ref()
            .filter(|configuration| {
                !self
                    .models
                    .iter()
                    .any(|model| model.provider == configuration.provider && model.id == configuration.model)
            })
            .map(|configuration| protocol::ModelInfo {
                provider: configuration.provider.clone(),
                id: configuration.model.clone(),
                display_name: configuration.model.clone(),
                context_window: configuration.context_window,
                current: true,
                recommended_rank: None,
                capabilities: None,
            })
    }

    fn model_is_current(&self, provider: &str, model: &str) -> bool {
        if let Some(configuration) = self.configuration.as_ref() {
            configuration.provider == provider && configuration.model == model
        } else {
            self.models
                .iter()
                .any(|entry| entry.provider == provider && entry.id == model && entry.current)
        }
    }

    fn is_current_palette_action(&self, action: &PaletteAction) -> bool {
        match action {
            PaletteAction::SelectModel { provider, model } => self.model_is_current(provider, model),
            PaletteAction::SelectSessionOptions(options) => self.session_option_is_current(options),
            PaletteAction::SelectFont { target, family } => {
                let typography = self.preferences.typography_for(self.theme_mode);
                let selected = match target {
                    FontTarget::Ui => typography.ui_font_family,
                    FontTarget::Code => typography.code_font_family,
                };
                family.as_deref() == selected
            }
            PaletteAction::SelectSettingsTab(tab) => self.settings_tab == *tab,
            PaletteAction::SelectWorkspaceReviewFile(path) => self
                .workspace_review
                .as_ref()
                .is_some_and(|review| review.selected_file.as_deref() == Some(path)),
            _ => false,
        }
    }

    fn show_palette(
        &mut self,
        kind: PaletteKind,
        groups: Vec<(String, Vec<PaletteCommand>)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.review_open && kind != PaletteKind::ReviewFiles {
            self.close_review(window, cx);
        }
        if self.palette.is_some() {
            self.close_palette(window, cx);
        }
        if self.settings_open {
            self.close_settings(window, cx);
        }
        self.palette_return_focus = window.focused(cx);
        let state = cx.new(|cx| CommandState::new(window, cx));
        self.palette = Some(state.clone());
        self.palette_kind = kind;
        self.palette_query = gpui_kit::SharedString::default();
        self.palette_groups = share_palette_commands(groups);
        // The palette installs its input state during its first render, so
        // focusing has to wait for that frame.
        cx.on_next_frame(window, |this, window, cx| this.focus_palette_state(window, cx));
        cx.notify();
    }

    fn focus_palette_state(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(
            self.palette_kind,
            PaletteKind::FileMentions | PaletteKind::SlashCommands | PaletteKind::PromptHistory
        ) {
            let has_suggestions = self.palette_groups.first().is_some_and(|(_, entries)| !entries.is_empty());
            if has_suggestions && let Some(state) = self.palette.clone() {
                state.update(cx, |state, cx| {
                    state.set_selected_index(Some(gpui_kit::component::IndexPath::new(0)), window, cx);
                });
            }
            return;
        }
        if let Some(state) = self.palette.clone() {
            let current = (matches!(
                self.palette_kind,
                PaletteKind::Models
                    | PaletteKind::Permissions
                    | PaletteKind::Modes
                    | PaletteKind::Fonts(_)
                    | PaletteKind::SettingsSearch
                    | PaletteKind::ReviewFiles
            ))
            .then(|| {
                self.palette_groups.iter().enumerate().find_map(|(section, (_, entries))| {
                    entries
                        .iter()
                        .position(|entry| self.is_current_palette_action(&entry.action))
                        .map(|row| gpui_kit::component::IndexPath::new(row).section(section))
                })
            })
            .flatten();
            state.update(cx, |state, cx| {
                if let Some(current) = current {
                    state.set_selected_index(Some(current), window, cx);
                }
                state.focus(window, cx);
            });
        }
    }

    fn run_palette_action(&mut self, action: PaletteAction, window: &mut Window, cx: &mut Context<Self>) {
        if !self.palette_action_available(&action) {
            cx.notify();
            return;
        }
        self.close_palette(window, cx);
        match action {
            PaletteAction::NewSession => self.create_session(cx),
            PaletteAction::CloseSession => self.close_session(cx),
            PaletteAction::OpenConversationSearch { session_id } => {
                self.open_conversation_search(session_id, window, cx);
            }
            PaletteAction::JumpToTranscriptItem { session_id, item_id } => {
                if self.selected_session_id.as_deref() != Some(session_id.as_str()) {
                    return;
                }
                if let Some(index) = self.transcript.position_by_id(&item_id) {
                    if self.transcript.get(index).is_some_and(|item| item.role == "tool") {
                        if self.transcript.reveal_tool_item(&item_id).is_none() {
                            return;
                        }
                        cx.notify();
                        let session_id = session_id.clone();
                        cx.on_next_frame(window, move |view, _, cx| {
                            if view.selected_session_id.as_deref() != Some(session_id.as_str()) {
                                return;
                            }
                            if let Some(index) = view.transcript.position_by_id(&item_id) {
                                view.message_scroller.update(cx, |state, cx| {
                                    let _scrolled = state.scroll_to_item(index, cx);
                                });
                            }
                        });
                    } else {
                        self.message_scroller.update(cx, |state, cx| {
                            let _scrolled = state.scroll_to_item(index, cx);
                        });
                    }
                }
            }
            PaletteAction::ShowActivityOutput { session_id, item_id } => {
                if self.selected_session_id.as_deref() == Some(session_id.as_str()) {
                    self.select_activity_output(item_id, window, cx);
                }
            }
            PaletteAction::ChooseModel => self.open_model_palette(window, cx),
            PaletteAction::ChooseWorkspace => self.choose_workspace(window, cx),
            PaletteAction::RefreshSessions => {
                let _ = self.request(protocol::method::SESSION_LIST, json!({}));
            }
            PaletteAction::RefreshFiles => self.refresh_files(cx),
            PaletteAction::ToggleInspector => self.toggle_inspector(cx),
            PaletteAction::ToggleZenMode => self.toggle_zen_mode(window, cx),
            PaletteAction::ToggleTheme => self.toggle_theme(window, cx),
            PaletteAction::SelectFont { target, family } => self.select_font(target, family, cx),
            PaletteAction::SelectSettingsTab(tab) => {
                self.settings_tab = tab;
                self.open_settings(window, cx);
                self.settings_navigation_focus.focus(window, cx);
                cx.notify();
            }
            PaletteAction::EditConfigSetting(path) => self.open_cli_config_editor(path, window, cx),
            PaletteAction::SelectSessionOptions(options) => {
                let _selected = self.select_session_options(options, cx);
            }
            PaletteAction::AttachFiles => self.attach_files(window, cx),
            PaletteAction::AttachFile(path) => self.attach_context_file(path, cx),
            PaletteAction::InsertDraft(text) => {
                self.composer.update(cx, |input, cx| {
                    let current = input.value().to_string();
                    let separator = if current.is_empty() || current.ends_with(char::is_whitespace) {
                        ""
                    } else {
                        " "
                    };
                    input.set_value(format!("{current}{separator}{text} "), window, cx);
                });
                self.focus_composer(window, cx);
            }
            PaletteAction::InsertHistoryPrompt(prompt) => {
                self.composer.update(cx, |input, cx| input.set_value(prompt, window, cx));
                self.focus_composer(window, cx);
            }
            PaletteAction::InsertFileMention(path) => self.complete_composer_trigger(
                composer_controls::ComposerTriggerKind::FileMention,
                match composer_controls::file_mention_token(&path) {
                    Some(mention) => mention,
                    None => return,
                },
                window,
                cx,
            ),
            PaletteAction::CompleteSlashCommand(name) => self.complete_composer_trigger(
                composer_controls::ComposerTriggerKind::SlashCommand,
                format!("/{name}"),
                window,
                cx,
            ),
            PaletteAction::LoadSession(session_id) => self.load_session(session_id, cx),
            PaletteAction::PreviewFile(rel_path) => self.open_file_preview(rel_path, window, cx),
            PaletteAction::SelectWorkspaceReviewFile(path) => self.select_workspace_review_file(path, cx),
            PaletteAction::SelectModel { provider, model } => {
                if !self.model_is_current(&provider, &model) {
                    self.select_model(provider, model, cx);
                }
            }
        }
    }

    fn palette_action_available(&self, action: &PaletteAction) -> bool {
        match action {
            PaletteAction::NewSession | PaletteAction::ChooseModel => self.session_change_allowed(None),
            PaletteAction::OpenConversationSearch { session_id } => {
                self.selected_session_id.as_deref() == Some(session_id.as_str())
                    && self.has_searchable_conversation_content()
            }
            PaletteAction::JumpToTranscriptItem { session_id, item_id } => {
                self.selected_session_id.as_deref() == Some(session_id.as_str())
                    && self.transcript.position_by_id(item_id).is_some_and(|index| {
                        self.transcript
                            .get(index)
                            .is_some_and(|item| is_searchable_conversation_role(&item.role))
                    })
            }
            PaletteAction::ShowActivityOutput { session_id, item_id } => {
                self.selected_session_id.as_deref() == Some(session_id.as_str())
                    && (self
                        .restored_activity
                        .iter()
                        .any(|item| item.id == *item_id && item.role == "tool" && item.output.is_some())
                        || self
                            .transcript
                            .position_by_id(item_id)
                            .and_then(|index| self.transcript.get(index))
                            .is_some_and(|item| item.role == "tool" && item.output.is_some()))
            }
            PaletteAction::SelectSessionOptions(_) => self.can_select_session_options(),
            PaletteAction::AttachFiles | PaletteAction::AttachFile(_) => {
                self.workspace.is_some() && !self.attachments_picker_open && self.session_request.is_none()
            }
            PaletteAction::InsertDraft(_)
            | PaletteAction::InsertFileMention(_)
            | PaletteAction::CompleteSlashCommand(_)
            | PaletteAction::InsertHistoryPrompt(_) => true,
            PaletteAction::CloseSession => self.selected_session_id.is_some() && self.session_change_allowed(None),
            PaletteAction::LoadSession(session_id) => self.session_change_allowed(Some(session_id)),
            PaletteAction::SelectWorkspaceReviewFile(_) => {
                self.review_open && self.workspace_review.as_ref().is_some_and(|review| review.viewer.is_some())
            }
            PaletteAction::ChooseWorkspace => {
                self.retained_submission.is_none() && self.workspace_switch_idle() && !self.workspace_picker_open
            }
            PaletteAction::RefreshSessions => self.backend.is_some(),
            PaletteAction::RefreshFiles | PaletteAction::ToggleInspector | PaletteAction::PreviewFile(_) => {
                self.workspace.is_some()
            }
            PaletteAction::ToggleZenMode
            | PaletteAction::ToggleTheme
            | PaletteAction::SelectFont { .. }
            | PaletteAction::SelectSettingsTab(_) => true,
            PaletteAction::EditConfigSetting(path) => {
                self.can_edit_cli_settings()
                    && self
                        .cli_config_settings
                        .as_ref()
                        .is_some_and(|snapshot| snapshot.settings.iter().any(|setting| setting.path == *path))
            }
            PaletteAction::SelectModel { provider, model } => {
                self.session_change_allowed(None)
                    && (self.model_is_current(provider, model)
                        || self
                            .models
                            .iter()
                            .any(|entry| entry.provider == *provider && entry.id == *model))
            }
        }
    }

    fn submit_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let prompt = self.composer.read(cx).value().to_string();
        let desktop_slash_action = composer_controls::desktop_slash_action(&prompt);
        if desktop_slash_action.as_ref() == Some(&composer_controls::DesktopSlashAction::StopTurn) {
            let _handled =
                self.handle_desktop_slash_action(composer_controls::DesktopSlashAction::StopTurn, window, cx);
            return;
        }
        if !self.session_change_idle() || self.attachments_picker_open || self.retained_submission.is_some() {
            return;
        }
        if prompt.trim().is_empty() {
            return;
        }
        if self.maybe_handle_webmcp_command(&prompt, window, cx) {
            return;
        }
        if let Some(action) = desktop_slash_action {
            let unavailable_message = match &action {
                composer_controls::DesktopSlashAction::FastMode(_) => {
                    "Fast processing is unavailable for the selected model."
                }
                composer_controls::DesktopSlashAction::ModelPicker => {
                    "Model selection is unavailable while the runtime is changing."
                }
                composer_controls::DesktopSlashAction::Modes | composer_controls::DesktopSlashAction::Permissions => {
                    "Runtime options are still loading or unavailable. Try again when they finish loading."
                }
                _ => "This desktop command is currently unavailable. Try again.",
            };
            if !self.handle_desktop_slash_action(action, window, cx) {
                self.error = Some(unavailable_message.into());
                cx.notify();
            }
            return;
        }
        if !self.workspace_permission_ready() {
            self.error = Some(
                "Wait for the saved permission choice to be restored, or choose permissions again before sending."
                    .into(),
            );
            cx.notify();
            return;
        }
        let draft = SubmissionDraft {
            prompt,
            attachments: std::mem::take(&mut self.attachments),
            echo_id: format!("desktop-user-{}", uuid_like_id()),
            delivery_uncertain: false,
        };
        let sent = if self.selected_session_id.is_none() {
            if self.request_session_change(protocol::method::SESSION_CREATE, json!({ "title": null }), cx) {
                self.first_submission = Some(draft);
                self.status = "Creating a session for your message…".to_string();
                true
            } else {
                self.attachments = draft.attachments;
                false
            }
        } else {
            match self.enqueue_submission(draft, cx) {
                Ok(()) => true,
                Err(draft) => {
                    self.attachments = draft.attachments;
                    false
                }
            }
        };
        if sent {
            self.composer.update(cx, |state, cx| state.set_value("", window, cx));
        }
        cx.notify();
    }

    /// The original Send intent can cross session creation without reading a newer composer draft.
    fn enqueue_submission(&mut self, draft: SubmissionDraft, cx: &mut Context<Self>) -> Result<(), SubmissionDraft> {
        let Some(session_id) = self.selected_session_id.clone() else {
            return Err(draft);
        };
        let params = json!({ "sessionId": session_id, "prompt": draft.prompt, "attachments": draft.attachments });
        self.clear_error();
        if !request_fits_frame(protocol::method::TURN_SUBMIT, &params) {
            self.error = Some(
                "This prompt exceeds the 1 MiB request limit after JSON encoding. Shorten it or remove attachments."
                    .to_string(),
            );
            return Err(draft);
        }
        if !self.request(protocol::method::TURN_SUBMIT, params) {
            self.error = Some("The local service request queue is full. Try again.".to_string());
            return Err(draft);
        }
        if let Some(entry) = self
            .sessions
            .iter_mut()
            .find(|session| session.session_id == session_id && session.title.as_deref().is_none_or(str::is_empty))
        {
            entry.title = Some(session_title_from_prompt(&draft.prompt));
        }
        let echo = if draft.attachments.is_empty() {
            draft.prompt.clone()
        } else {
            let paths = draft
                .attachments
                .iter()
                .map(|attachment| attachment.path.as_str())
                .collect::<Vec<_>>();
            format!("{}\n\nAttached: {}", draft.prompt, paths.join(", "))
        };
        self.transcript
            .push(TranscriptItem::new(draft.echo_id.clone(), "user".into(), "You".into(), echo));
        self.pending_submission = Some(draft);
        self.transcript.follow_latest();
        self.running = true;
        self.active_turn_id = None;
        self.cancel_requested = false;
        self.cancel_request_sent = false;
        self.status = "Submitting turn…".to_string();
        cx.notify();
        Ok(())
    }

    fn recover_pending_submission(&mut self, delivery_uncertain: bool) {
        if let Some(mut draft) = self.pending_submission.take() {
            draft.delivery_uncertain = delivery_uncertain;
            if !delivery_uncertain {
                self.transcript.remove(&draft.echo_id);
            }
            self.retained_submission = Some(draft);
        }
    }

    fn restore_submission(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.composer.read(cx).value().is_empty()
            || !self.attachments.is_empty()
            || self.attachments_picker_open
            || self.session_request.is_some()
        {
            return;
        }
        if let Some(draft) = self.retained_submission.take() {
            self.transcript.remove(&draft.echo_id);
            self.composer.update(cx, |state, cx| state.set_value(draft.prompt, window, cx));
            self.attachments = draft.attachments;
            self.clear_error();
            self.status = "Draft restored. Review it before sending again.".to_string();
            cx.notify();
        }
    }

    fn cancel_turn(&mut self, cx: &mut Context<Self>) {
        if !self.running {
            return;
        }
        self.cancel_requested = true;
        self.status = "Cancelling turn and waiting for cleanup…".to_string();
        self.try_send_pending_cancel();
        cx.notify();
    }

    fn try_send_pending_cancel(&mut self) {
        if !self.cancel_requested || self.cancel_request_sent || !self.running {
            return;
        }
        let (Some(session_id), Some(turn_id)) = (self.selected_session_id.clone(), self.active_turn_id.clone()) else {
            self.status = "Cancellation queued; waiting for the service to accept the turn…".to_string();
            return;
        };
        if self.request(protocol::method::TURN_CANCEL, json!({ "sessionId": session_id, "turnId": turn_id })) {
            self.cancel_request_sent = true;
        } else {
            self.error = Some("The cancellation request could not be queued.".to_string());
        }
    }

    fn handle_cancel_state_after_turn_event(&mut self) {
        if !self.running {
            self.cancel_requested = false;
            self.cancel_request_sent = false;
            return;
        }
        self.try_send_pending_cancel();
    }

    fn clear_pending_interaction(&mut self) {
        self.interaction = None;
        self.pending_interaction_response = None;
        self.interaction_choice_ids.clear();
        self.interaction_answer_texts.clear();
        self.interaction_step_index = 0;
    }

    fn submit_interaction(
        &mut self,
        choice_ids: Vec<String>,
        cancelled: bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.pending_interaction_response.is_some() {
            return;
        }
        let Some(interaction) = self.interaction.clone() else {
            return;
        };
        self.store_current_interaction_answer(cx);
        let answer_texts = if cancelled {
            Vec::new()
        } else {
            self.interaction_answer_texts.clone()
        };
        let choice_ids = if cancelled { Vec::new() } else { choice_ids };
        let response = InteractionResponse {
            session_id: interaction.session_id,
            turn_id: interaction.turn_id,
            interaction_id: interaction.interaction_id,
            choice_id: choice_ids.first().cloned(),
            choice_ids,
            text: answer_texts.iter().flatten().next().cloned(),
            answer_texts,
            cancelled,
        };
        let params = json!(response);
        if !request_fits_frame(protocol::method::INTERACTION_RESPOND, &params) {
            self.error =
                Some("These answers exceed the local service request limit. Shorten them before submitting.".into());
            cx.notify();
            return;
        }
        if self.request(protocol::method::INTERACTION_RESPOND, params) {
            self.pending_interaction_response = Some(response.interaction_id);
            self.clear_error();
            self.status = "Sending decision to VT Code…".into();
        } else {
            self.error = Some("The decision could not be sent to the local service.".to_string());
        }
        cx.notify();
    }

    fn choose_interaction_option(
        &mut self,
        step_index: usize,
        choice_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.pending_interaction_response.is_some() {
            return;
        }
        let Some(interaction) = self.interaction.as_ref() else {
            return;
        };
        if interaction.kind == protocol::InteractionKind::Question && !interaction.steps.is_empty() {
            if step_index != self.interaction_step_index
                || !interaction
                    .steps
                    .get(step_index)
                    .is_some_and(|step| step.choices.iter().any(|choice| choice.id == choice_id))
            {
                return;
            }
            if let Some(selected) = self.interaction_choice_ids.get_mut(step_index) {
                *selected = Some(choice_id);
            }
            self.clear_error();
            cx.notify();
            return;
        }
        self.submit_interaction(vec![choice_id], false, window, cx);
    }

    fn store_current_interaction_answer(&mut self, cx: &App) {
        if self.interaction.as_ref().is_none_or(|interaction| interaction.steps.is_empty()) {
            return;
        }
        let answer = self.question_answer.read(cx).value().to_string();
        if let Some(stored) = self.interaction_answer_texts.get_mut(self.interaction_step_index) {
            *stored = (!answer.trim().is_empty()).then_some(answer);
        }
    }

    fn show_interaction_step(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending_interaction_response.is_some()
            || index == self.interaction_step_index
            || self.interaction.as_ref().is_none_or(|interaction| {
                interaction.kind != protocol::InteractionKind::Question || interaction.steps.get(index).is_none()
            })
        {
            return;
        }
        self.store_current_interaction_answer(cx);
        self.interaction_step_index = index;
        let answer = self
            .interaction_answer_texts
            .get(index)
            .and_then(Clone::clone)
            .unwrap_or_default();
        self.question_answer.update(cx, |state, cx| state.set_value(answer, window, cx));
        self.clear_error();
        cx.notify();
    }

    fn advance_interaction_step(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending_interaction_response.is_some() {
            return;
        }
        let Some(interaction) = self.interaction.as_ref() else {
            return;
        };
        let selected = self
            .interaction_choice_ids
            .get(self.interaction_step_index)
            .and_then(Option::as_deref);
        if !interaction
            .steps
            .get(self.interaction_step_index)
            .is_some_and(|step| step.choices.iter().any(|choice| Some(choice.id.as_str()) == selected))
        {
            self.error = Some("Choose an option for this question before continuing.".into());
            cx.notify();
            return;
        }
        let final_step = self.interaction_step_index + 1 >= interaction.steps.len();
        if final_step {
            let selections = self.interaction_choice_ids.iter().filter_map(Clone::clone).collect::<Vec<_>>();
            if selections.len() != interaction.steps.len() {
                self.error = Some("Choose an option for every question before submitting.".to_string());
                cx.notify();
                return;
            }
            self.submit_interaction(selections, false, window, cx);
        } else {
            self.show_interaction_step(self.interaction_step_index + 1, window, cx);
        }
    }

    /// Drain one bounded event batch. A full batch asks the async waiter to schedule
    /// another foreground update without returning to a periodic polling loop.
    fn drain_events(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let mut changed = false;
        let mut processed = 0;
        let mut pending_item_update = None;
        for _ in 0..MAX_UI_EVENTS_PER_TICK {
            match self.event_rx.try_recv() {
                Ok(event) => {
                    processed += 1;
                    changed = true;
                    if pending_item_update
                        .as_ref()
                        .is_some_and(|pending| can_coalesce_item_updates(pending, &event))
                    {
                        // ItemUpdated carries a full snapshot, so intermediate
                        // snapshots for this item need not be rendered in one
                        // UI tick. The service persists every event separately.
                        pending_item_update = Some(event);
                        continue;
                    }
                    if let Some(pending) = pending_item_update.take() {
                        self.process_drained_event(pending, window, cx);
                    }
                    if is_item_update_event(&event) {
                        pending_item_update = Some(event);
                    } else {
                        self.process_drained_event(event, window, cx);
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    if let Some(pending) = pending_item_update.take() {
                        self.process_drained_event(pending, window, cx);
                    }
                    self.cancel_session_load_task();
                    self.backend = None;
                    self.clear_pending_interaction();
                    self.question_answer.update(cx, |state, cx| state.set_value("", window, cx));
                    self.reset_webmcp_state(Some(webmcp::WEBMCP_DISCONNECTED_NOTICE));
                    self.fail_workspace_review("Local service disconnected.".into());
                    self.running = false;
                    self.status = "Local service disconnected".to_string();
                    changed = true;
                    break;
                }
            }
        }
        if let Some(pending) = pending_item_update {
            self.process_drained_event(pending, window, cx);
        }
        if changed {
            cx.notify();
        }
        processed == MAX_UI_EVENTS_PER_TICK
    }

    fn process_drained_event(&mut self, event: BackendEvent, window: &mut Window, cx: &mut Context<Self>) {
        let accepted_decision = matches!(&event,
            BackendEvent::InteractionResponded { identity, error: None }
                if self.pending_interaction_response.as_deref() == Some(identity.interaction_id.as_str()))
            && self.interaction_focus.contains_focused(window, cx);
        let new_interaction = matches!(&event, BackendEvent::Interaction(_));
        let previous_interaction_id = new_interaction
            .then(|| self.interaction.as_ref().map(|interaction| interaction.interaction_id.clone()))
            .flatten();
        self.notify_background_activity(&event, window, cx);
        self.handle_backend_event(event, cx);
        let interaction_replaced = new_interaction
            && previous_interaction_id.as_deref()
                != self.interaction.as_ref().map(|interaction| interaction.interaction_id.as_str());
        if (self.interaction.is_none() || interaction_replaced) && !self.question_answer.read(cx).value().is_empty() {
            self.question_answer.update(cx, |state, cx| state.set_value("", window, cx));
        }
        if accepted_decision
            && self.interaction.is_none()
            && !self.settings_open
            && self.palette.is_none()
            && !self.review_open
        {
            self.focus_composer(window, cx);
        }
        self.restore_focus_after_review_close(window, cx);
    }

    fn notify_background_activity(&self, event: &BackendEvent, window: &mut Window, cx: &mut Context<Self>) {
        if window.is_window_active() {
            return;
        }
        let activity = match event {
            BackendEvent::ThreadEvent(notification) => {
                if self.selected_session_id.as_deref() != Some(notification.session_id.as_str())
                    || !self.running
                    || !notification
                        .turn_id
                        .as_deref()
                        .is_some_and(|turn_id| self.active_turn_id.as_deref() == Some(turn_id))
                    || !self.notification_matches(&notification.session_id, notification.turn_id.as_deref())
                {
                    return;
                }
                match notification.event.event() {
                    vtcode_exec_events::ThreadEvent::TurnCompleted(_) => {
                        Some(super::notifications::BackgroundAgentActivity::Completed)
                    }
                    vtcode_exec_events::ThreadEvent::TurnFailed(_)
                    | vtcode_exec_events::ThreadEvent::TurnBlocked(_) => {
                        Some(super::notifications::BackgroundAgentActivity::NeedsAttention)
                    }
                    _ => None,
                }
            }
            BackendEvent::Interaction(interaction) => {
                let current_turn = interaction.turn_id == "session-start"
                    || (self.running
                        && self.active_turn_id.as_deref() == Some(interaction.turn_id.as_str())
                        && self.notification_matches(&interaction.session_id, Some(&interaction.turn_id)));
                if self.selected_session_id.as_deref() != Some(interaction.session_id.as_str())
                    || !current_turn
                    || self
                        .interaction
                        .as_ref()
                        .is_some_and(|current| current.interaction_id == interaction.interaction_id)
                {
                    return;
                }
                Some(super::notifications::BackgroundAgentActivity::WaitingForDecision)
            }
            _ => None,
        };
        if let Some(activity) = activity {
            super::notifications::background_agent_activity(window, activity, cx);
        }
    }

    fn handle_backend_event(&mut self, event: BackendEvent, cx: &mut Context<Self>) {
        match event {
            BackendEvent::InteractionResponded { identity, error } => {
                if self.pending_interaction_response.as_deref() != Some(identity.interaction_id.as_str())
                    || !self.interaction.as_ref().is_some_and(|interaction| {
                        interaction.interaction_id == identity.interaction_id
                            && interaction.session_id == identity.session_id
                            && interaction.turn_id == identity.turn_id
                    })
                {
                    return;
                }
                self.pending_interaction_response = None;
                if let Some(error) = error {
                    self.error = Some(error);
                    self.status = "The decision was not accepted. Review it and try again.".into();
                } else {
                    self.clear_pending_interaction();
                    self.clear_error();
                    self.status = if identity.cancelled {
                        "Interaction cancelled"
                    } else {
                        "Decision accepted"
                    }
                    .into();
                }
            }
            BackendEvent::Response { method, result, error } => {
                if self.session_request == Some(method.as_str()) && method != protocol::method::SESSION_LOAD {
                    self.session_request = None;
                }
                if let Some(error) = error {
                    if method == protocol::method::SESSION_HISTORY {
                        self.fail_history_page(error, cx);
                        return;
                    }
                    if self.session_request == Some(method.as_str()) {
                        self.session_request = None;
                    }
                    if matches!(
                        method.as_str(),
                        protocol::method::INITIALIZE | protocol::method::SESSION_LIST | protocol::method::SESSION_LOAD
                    ) {
                        self.fail_pending_navigation();
                    }
                    if method == protocol::method::CONFIG_OPTIONS {
                        self.pending_permission_choice = None;
                    }
                    if self.handle_webmcp_error(&method, &error) {
                        cx.notify();
                        return;
                    }
                    if method == protocol::method::COMPOSER_RESOURCES {
                        self.resources_loading = false;
                    }
                    if method == protocol::method::CONFIG_SETTINGS_WRITE {
                        self.config_write_pending = false;
                        self.config_editor_error = Some(error.clone());
                    }
                    if method == protocol::method::WORKSPACE_REVIEW {
                        self.fail_workspace_review(error);
                        return;
                    }
                    if method == protocol::method::INITIALIZE {
                        self.backend = None;
                    }
                    if method == protocol::method::SESSION_CREATE
                        && let Some(draft) = self.first_submission.take()
                    {
                        self.retained_submission = Some(draft);
                    }
                    self.error = Some(error);
                    if method == protocol::method::TURN_SUBMIT {
                        self.recover_pending_submission(false);
                        self.running = false;
                        self.cancel_requested = false;
                        self.cancel_request_sent = false;
                    } else if method == protocol::method::TURN_CANCEL {
                        self.cancel_request_sent = false;
                    }
                    self.status = "Service request failed".to_string();
                    return;
                }
                let result = result.unwrap_or(Value::Null);
                match method.as_str() {
                    protocol::method::WORKSPACE_REVIEW => {
                        match serde_json::from_value::<protocol::WorkspaceReviewResult>(result) {
                            Ok(snapshot) => self.accept_workspace_review(snapshot, cx),
                            Err(_) => self.fail_workspace_review("Invalid Git review response.".into()),
                        }
                    }
                    protocol::method::INITIALIZE => {
                        let initialized = serde_json::from_value::<protocol::InitializeResult>(result);
                        match initialized {
                            Ok(info) if info.protocol == protocol::PROTOCOL_VERSION => {
                                if valid_slash_command_catalog(&info.slash_commands) {
                                    self.slash_commands = info.slash_commands.clone();
                                } else {
                                    self.slash_commands.clear();
                                    self.error =
                                        Some("The local CLI returned an invalid slash command catalog.".into());
                                }
                                self.service_info = Some(info);
                                self.status = "Connected to the local VT Code service".to_string();
                                let mut discovery_queued = true;
                                for method in [
                                    protocol::method::CONFIG_DISCOVER,
                                    protocol::method::CONFIG_SETTINGS_READ,
                                    protocol::method::CONFIG_MODELS,
                                    protocol::method::SESSION_LIST,
                                ] {
                                    if !self.request(method, json!({})) {
                                        self.error =
                                            Some("The local service request queue is unavailable.".to_string());
                                        discovery_queued = false;
                                        break;
                                    }
                                }
                                if discovery_queued {
                                    self.request_webmcp_status();
                                }
                            }
                            _ => {
                                self.service_info = None;
                                self.backend = None;
                                self.fail_pending_navigation();
                                self.error = Some("The local service returned an invalid or incompatible initialization response. Rebuild the desktop app and its bundled helper together.".to_string());
                                self.status = "Could not initialize the local VT Code service".to_string();
                            }
                        }
                    }
                    protocol::method::CONFIG_DISCOVER => {
                        self.configuration = serde_json::from_value::<ConfigurationSnapshot>(result).ok();
                        self.sync_cli_theme(cx);
                        self.restore_workspace_permission(cx);
                        if self
                            .configuration
                            .as_ref()
                            .is_some_and(|configuration| !configuration.authenticated)
                        {
                            self.error = Some(
                                "No provider credentials are configured. In Terminal, run `vtcode auth` or `vtcode login <provider>`, then restart the desktop app.".to_string(),
                            );
                        }
                    }
                    protocol::method::CONFIG_SETTINGS_READ => {
                        self.handle_cli_config_read_response(result);
                    }
                    protocol::method::CONFIG_SETTINGS_WRITE => {
                        self.handle_cli_config_write_response(result, cx);
                    }
                    protocol::method::WEBMCP_STATUS | protocol::method::WEBMCP_PAIR | protocol::method::WEBMCP_STOP => {
                        let _handled = self.handle_webmcp_response(&method, result, cx);
                    }
                    protocol::method::CONFIG_MODELS => {
                        match serde_json::from_value::<Vec<protocol::ModelInfo>>(result) {
                            Ok(models) => self.models = models,
                            Err(_) => self.error = Some("The local service returned an invalid model catalog.".into()),
                        }
                    }
                    protocol::method::COMPOSER_RESOURCES => {
                        self.resources_loading = false;
                        match serde_json::from_value::<Vec<protocol::ComposerResource>>(result) {
                            Ok(resources)
                                if resources.len() <= 192
                                    && resources.iter().all(|resource| {
                                        protocol::ComposerResource::is_valid_name(&resource.name)
                                            && resource.description.chars().count() <= 400
                                    }) =>
                            {
                                self.composer_resources = resources;
                                self.resources_loaded = true;
                            }
                            _ => self.error = Some("Invalid CLI resource catalog.".into()),
                        }
                    }
                    protocol::method::CONFIG_OPTIONS => match serde_json::from_value::<ConfigurationSnapshot>(result) {
                        Ok(snapshot) => {
                            if let Some(choice) = self.pending_permission_choice.take()
                                && snapshot
                                    .session_options
                                    .as_ref()
                                    .is_some_and(|options| options.permission_mode == choice)
                            {
                                self.save_workspace_permission(choice);
                            }
                            let attachments = std::mem::take(&mut self.attachments);
                            self.reset_session_state();
                            self.attachments = attachments;
                            self.selected_session_id = None;
                            self.record_current_navigation();
                            self.configuration = Some(snapshot);
                            self.status = "Runtime options updated. The next message starts a new session.".into();
                        }
                        Err(_) => {
                            self.pending_permission_choice = None;
                            self.backend = None;
                            self.error = Some("Invalid runtime options response. Reconnect before sending.".into());
                        }
                    },
                    protocol::method::CONFIG_THEME => match serde_json::from_value::<ConfigurationSnapshot>(result) {
                        Ok(snapshot) => {
                            let theme = snapshot
                                .shared_ui
                                .as_ref()
                                .map(|preferences| preferences.theme.clone())
                                .filter(|theme| themes::find(theme).is_some());
                            if let Some(theme) = theme {
                                let attachments = std::mem::take(&mut self.attachments);
                                self.reset_session_state();
                                self.attachments = attachments;
                                self.selected_session_id = None;
                                self.record_current_navigation();
                                self.configuration = Some(snapshot);
                                self.preferences.follow_cli_theme = Some(true);
                                self.apply_theme_state(theme, None, cx);
                                self.status = "CLI theme saved. The next message starts a new session.".into();
                            } else {
                                self.backend = None;
                                self.error = Some("The CLI returned an unsupported theme after saving.".into());
                                self.status = "Could not apply CLI theme".into();
                            }
                        }
                        Err(_) => {
                            self.backend = None;
                            self.error = Some("Invalid CLI theme response. Reconnect before sending.".into());
                        }
                    },
                    protocol::method::CONFIG_SELECT => {
                        // The service restarted with the new provider/model and
                        // no selected session; the next prompt starts a fresh
                        // session under it.
                        self.configuration = serde_json::from_value::<ConfigurationSnapshot>(result).ok();
                        self.selected_session_id = None;
                        let attachments = std::mem::take(&mut self.attachments);
                        self.reset_session_state();
                        self.attachments = attachments;
                        self.record_current_navigation();
                        self.models.clear();
                        let _ = self.request(protocol::method::CONFIG_MODELS, json!({}));
                        let model = self
                            .configuration
                            .as_ref()
                            .map(|configuration| configuration.model.clone())
                            .unwrap_or_default();
                        self.status = format!("Switched to {model}. Start a new session to use it.");
                    }
                    protocol::method::SESSION_LIST => {
                        match serde_json::from_value::<Vec<SessionSummary>>(result) {
                            Ok(mut refreshed) => {
                                let mut session_ids = HashSet::with_capacity(refreshed.len());
                                let has_invalid_session_id = refreshed.iter().any(|session| {
                                    session.session_id.trim().is_empty()
                                        || !session_ids.insert(session.session_id.as_str())
                                });
                                if has_invalid_session_id {
                                    self.fail_pending_navigation();
                                    self.error = Some("The local service returned an invalid session list.".into());
                                } else {
                                    // Keep a locally derived title when the archive has
                                    // not persisted its own preview yet.
                                    for session in &mut refreshed {
                                        if session.title.as_deref().is_none_or(str::is_empty) {
                                            session.title = self
                                                .sessions
                                                .iter()
                                                .find(|existing| existing.session_id == session.session_id)
                                                .and_then(|existing| existing.title.clone());
                                        }
                                    }
                                    self.sessions = refreshed;
                                    self.continue_pending_workspace_navigation(cx);
                                }
                            }
                            Err(_) => {
                                self.fail_pending_navigation();
                                self.error = Some("The local service returned an invalid session list.".into());
                            }
                        }
                    }
                    protocol::method::SESSION_CREATE => {
                        let first_submission = self.first_submission.take();
                        if let Some(session) = serde_json::from_value::<SessionSummary>(result)
                            .ok()
                            .filter(|session| !session.session_id.is_empty())
                        {
                            self.selected_session_id = Some(session.session_id.clone());
                            self.sessions.insert(0, session);
                            self.reset_session_state();
                            self.record_current_navigation();
                            self.status = "Session ready. Submit a prompt to begin.".to_string();
                            if let Some(draft) = first_submission
                                && let Err(draft) = self.enqueue_submission(draft, cx)
                            {
                                self.retained_submission = Some(draft);
                                self.status = "Message not submitted".to_string();
                            }
                        } else {
                            self.status = "Session creation failed".to_string();
                            self.retained_submission = first_submission;
                            self.error = Some(
                                "The service returned an invalid session. Your message has not been submitted."
                                    .to_string(),
                            );
                        }
                    }
                    protocol::method::SESSION_CLOSE => {
                        self.selected_session_id = None;
                        self.reset_session_state();
                        self.record_current_navigation();
                        self.status = "Session closed. Start a new session to continue.".to_string();
                    }
                    protocol::method::SESSION_LOAD => self.begin_session_load(result, cx),
                    protocol::method::SESSION_HISTORY => self.begin_session_history_page(result, cx),
                    protocol::method::TURN_SUBMIT => {
                        let accepted =
                            serde_json::from_value::<protocol::TurnAccepted>(result)
                                .ok()
                                .filter(|accepted| {
                                    self.selected_session_id.as_deref() == Some(accepted.session_id.as_str())
                                        && !accepted.turn_id.is_empty()
                                });
                        let Some(accepted) = accepted else {
                            if self.pending_submission.is_some() {
                                self.recover_pending_submission(true);
                                self.running = false;
                                self.status = "Invalid turn acknowledgement".to_string();
                            }
                            self.error = Some("The service returned an invalid turn acknowledgement. Delivery could not be confirmed.".to_string());
                            return;
                        };
                        // Lifecycle events can complete the turn before the request response arrives.
                        if self.pending_submission.is_none() && self.active_turn_id.is_none() {
                            return;
                        }
                        self.pending_submission = None;
                        self.active_turn_id = Some(accepted.turn_id);
                        self.running = true;
                        self.status = "Running".to_string();
                        self.try_send_pending_cancel();
                    }
                    protocol::method::TURN_CANCEL => {
                        self.status = "Cancellation accepted; waiting for cleanup…".to_string();
                    }
                    _ => {}
                }
            }
            BackendEvent::SessionHistoryResponse { request_id, result, error } => {
                if self.history_page_request_id.as_deref() != Some(request_id.as_str()) {
                    return;
                }
                if let Some(error) = error {
                    self.fail_history_page(error, cx);
                } else if let Some(result) = result {
                    self.begin_session_history_page(result, cx);
                } else {
                    self.fail_history_page("The local service returned an empty history response.".into(), cx);
                }
            }
            BackendEvent::SessionOutputResponse { request_id, result, error } => {
                if self.output_page_pending.as_ref().map(|pending| pending.request_id.as_str())
                    != Some(request_id.as_str())
                {
                    return;
                }
                if error.is_some() {
                    self.fail_output_page(cx);
                } else if let Some(result) = result {
                    self.begin_session_output_page(result, cx);
                } else {
                    self.fail_output_page(cx);
                }
            }
            BackendEvent::ThreadEvent(notification) => {
                if !self.notification_matches(&notification.session_id, notification.turn_id.as_deref()) {
                    return;
                }
                self.handle_thread_event(notification);
                self.refresh_selected_change(cx);
                self.handle_cancel_state_after_turn_event();
            }
            BackendEvent::PrimaryAgentChanged(notification) => {
                if self.selected_session_id.as_deref() != Some(notification.session_id.as_str()) {
                    return;
                }
                self.active_primary_agent_display = notification
                    .display_name
                    .filter(|name| !name.trim().is_empty())
                    .map(|display_name| ActivePrimaryAgentDisplay {
                        session_id: notification.session_id,
                        display_name,
                    });
            }
            BackendEvent::Interaction(interaction) => {
                if self.selected_session_id.as_deref() != Some(interaction.session_id.as_str())
                    || (interaction.turn_id != "session-start"
                        && !self.notification_matches(&interaction.session_id, Some(&interaction.turn_id)))
                    || self
                        .interaction
                        .as_ref()
                        .is_some_and(|current| current.interaction_id == interaction.interaction_id)
                {
                    return;
                }
                self.interaction_choice_ids =
                    interaction.steps.iter().map(|step| step.selected_choice_id.clone()).collect();
                if self.interaction_choice_ids.is_empty() {
                    self.interaction_choice_ids = interaction.selected_choice_ids.iter().cloned().map(Some).collect();
                }
                self.interaction_answer_texts = vec![None; interaction.steps.len()];
                self.interaction_step_index = 0;
                self.pending_interaction_response = None;
                self.interaction = Some(interaction);
                self.status = "Waiting for your decision".to_string();
            }
            BackendEvent::TurnStatus(status) => {
                if !status.get("sessionId").and_then(Value::as_str).is_some_and(|session_id| {
                    self.notification_matches(session_id, status.get("turnId").and_then(Value::as_str))
                }) {
                    return;
                }
                if let Some(label) = status.get("status").and_then(Value::as_str) {
                    self.status = label.to_string();
                }
            }
            BackendEvent::Exited { status, authentication_required } => {
                self.cancel_session_load_task();
                self.reset_webmcp_state(Some(webmcp::WEBMCP_DISCONNECTED_NOTICE));
                self.fail_workspace_review("The local service exited before Git review completed.".into());
                self.clear_pending_interaction();
                self.session_request = None;
                if let Some(draft) = self.first_submission.take() {
                    self.retained_submission = Some(draft);
                }
                self.recover_pending_submission(true);
                self.backend = None;
                self.running = false;
                self.active_turn_id = None;
                self.cancel_requested = false;
                self.cancel_request_sent = false;
                if self.pending_workspace.is_none() {
                    self.fail_pending_navigation();
                }
                if let Some(workspace) = self.pending_workspace.take() {
                    self.start_workspace(workspace, cx);
                } else if authentication_required {
                    self.status = "Provider credentials required".to_string();
                    self.error = Some(format!(
                        "VT Code could not authenticate the configured provider. In Terminal, run `vtcode auth` or `vtcode login <provider>`. {}",
                        service_reconnect_guidance(self.retained_submission.is_some())
                    ));
                } else {
                    self.status = match status {
                        Some(code) => format!("Local service exited with code {code}"),
                        None => "Local service exited".to_string(),
                    };
                    let reconnect_error = format!(
                        "The local VT Code service disconnected. {}",
                        service_reconnect_guidance(self.retained_submission.is_some())
                    );
                    self.error = Some(match self.error.take() {
                        Some(error) => format!("{error}\n\n{reconnect_error}"),
                        None => reconnect_error,
                    });
                }
            }
            BackendEvent::LaunchFailed(error) => {
                self.cancel_session_load_task();
                self.reset_webmcp_state(Some(webmcp::WEBMCP_DISCONNECTED_NOTICE));
                self.fail_workspace_review("The local service could not start.".into());
                self.clear_pending_interaction();
                self.session_request = None;
                if let Some(draft) = self.first_submission.take() {
                    self.retained_submission = Some(draft);
                }
                self.recover_pending_submission(true);
                self.report_service_launch_failure(error);
                self.backend = None;
                self.running = false;
                self.cancel_requested = false;
                self.cancel_request_sent = false;
            }
        }
    }

    fn notification_matches(&self, session_id: &str, turn_id: Option<&str>) -> bool {
        if self.selected_session_id.as_deref() != Some(session_id) {
            return false;
        }
        if let Some(turn_id) = turn_id {
            if self.active_turn_id.as_deref().is_some_and(|active| active != turn_id) {
                return false;
            }
            if self.running && self.finished_turn_id.as_deref() == Some(turn_id) {
                return false;
            }
        }
        true
    }

    fn handle_thread_event(&mut self, notification: ThreadEventNotification) {
        use vtcode_exec_events::ThreadEvent;
        let terminal_turn_id = matches!(
            notification.event.event(),
            ThreadEvent::TurnCompleted(_) | ThreadEvent::TurnFailed(_) | ThreadEvent::TurnBlocked(_)
        )
        .then(|| notification.turn_id.clone())
        .flatten();
        match notification.event.event() {
            ThreadEvent::TurnStarted(_) => {
                self.pending_submission = None;
                self.running = true;
                self.active_turn_id = notification.turn_id;
                self.set_selected_session_completed(false);
                self.status = "Running".to_string();
            }
            ThreadEvent::TurnCompleted(event) => {
                self.pending_submission = None;
                self.running = false;
                self.active_turn_id = None;
                self.cancel_requested = false;
                self.cancel_request_sent = false;
                self.status = "Completed".to_string();
                self.last_usage = Some(event.usage.clone());
                self.set_selected_session_completed(true);
            }
            ThreadEvent::TurnFailed(event) => {
                self.finish_unverified_turn(&event.message);
            }
            ThreadEvent::TurnBlocked(event) => {
                self.finish_unverified_turn(&event.message);
            }
            ThreadEvent::PlanDelta(delta) => {
                self.transcript
                    .append_text(delta.item_id.clone(), "assistant", "Plan".into(), &delta.delta);
            }
            ThreadEvent::ItemStarted(event) => self.update_runtime_item(&event.item),
            ThreadEvent::ItemUpdated(event) => self.update_runtime_item(&event.item),
            ThreadEvent::ItemCompleted(event) => self.update_runtime_item(&event.item),
            ThreadEvent::Error(event) => {
                let message = event.message.clone();
                self.transcript.push(TranscriptItem::new(
                    format!("runtime-error-{}", uuid_like_id()),
                    "notice".to_string(),
                    "Runtime error".into(),
                    message,
                ));
            }
            _ => {}
        }
        if let Some(turn_id) = terminal_turn_id {
            self.finished_turn_id = Some(turn_id);
        }
    }

    fn finish_unverified_turn(&mut self, message: &str) {
        self.pending_submission = None;
        self.running = false;
        self.active_turn_id = None;
        self.cancel_requested = false;
        self.cancel_request_sent = false;
        self.status = "Turn ended without verified completion".to_string();
        self.set_selected_session_completed(false);
        self.transcript.push(TranscriptItem::new(
            format!("event-{}", uuid_like_id()),
            "notice".to_string(),
            "Run status".into(),
            message.to_string(),
        ));
    }

    /// Keep the selected sidebar row current without rereading every archive.
    fn set_selected_session_completed(&mut self, completed: bool) {
        let Some(session_id) = self.selected_session_id.as_deref() else {
            return;
        };
        if let Some(session) = self.sessions.iter_mut().find(|session| session.session_id == session_id) {
            session.completed = completed;
        }
    }

    fn update_runtime_item(&mut self, item: &vtcode_exec_events::ThreadItem) {
        self.restored_activity.retain(|restored| restored.id != item.id);
        let _cursor = self.restored_output_cursors.remove(&item.id);
        let _pages = self.restored_output_pages.remove(&item.id);
        if self
            .output_page_pending
            .as_ref()
            .is_some_and(|pending| pending.item_id == item.id)
        {
            self.output_page_pending = None;
            self.output_page_task = None;
            self.output_page_error = None;
        }
        self.project_runtime_item(item, false);
    }

    fn project_runtime_item(&mut self, item: &vtcode_exec_events::ThreadItem, historical: bool) {
        use vtcode_exec_events::ThreadItemDetails;
        let id = item.id.clone();
        match &item.details {
            ThreadItemDetails::AgentMessage(message) => {
                self.append_or_update_item(id, "assistant", "VT Code", &message.text, false);
            }
            ThreadItemDetails::Plan(plan) => {
                self.append_or_update_item(id, "assistant", "Plan", &plan.text, false);
            }
            ThreadItemDetails::Reasoning(reasoning) => {
                self.append_or_update_item(id, "assistant", "Planning update", &reasoning.text, false);
            }
            ThreadItemDetails::Decision(decision) => {
                self.append_or_update_item(id, "assistant", "Decision", &decision.summary, false);
            }
            ThreadItemDetails::FileChange(change) => self.upsert_diff_card(id, change),
            details => {
                let (title, mut status) = describe_activity(details);
                if historical && ActivityState::from_status(&status) == ActivityState::Running {
                    status = "interrupted".to_string();
                }
                let payload = activity_details_payload(details);
                let text = format!("{status}\n\n{payload}");
                // Keep the raw aggregated output for the inspector's Output
                // tab; it updates in place as streaming events arrive.
                let output = match details {
                    ThreadItemDetails::CommandExecution(command) => Some(command.aggregated_output.as_str().into()),
                    ThreadItemDetails::ToolOutput(tool_output) => Some(tool_output.output.as_str().into()),
                    _ => None,
                };
                if historical {
                    let mut item = TranscriptItem::new(id, "tool".to_string(), title.into(), text);
                    item.output = output;
                    self.restored_activity.push(item);
                } else {
                    self.append_or_update_item(id.clone(), "tool", &title, &text, true);
                    if output.is_some() {
                        self.transcript.set_output(&id, output);
                    }
                }
            }
        }
    }

    fn upsert_diff_card(&mut self, id: String, change: &vtcode_exec_events::FileChangeItem) {
        let files = change
            .changes
            .iter()
            .map(|file| DiffFile {
                path: file.path.clone(),
                kind: match file.kind {
                    vtcode_exec_events::PatchChangeKind::Add => DiffFileKind::Add,
                    vtcode_exec_events::PatchChangeKind::Delete => DiffFileKind::Delete,
                    vtcode_exec_events::PatchChangeKind::Update => DiffFileKind::Update,
                },
            })
            .collect();
        let card = DiffCard {
            files,
            additions: change.additions,
            deletions: change.deletions,
            incomplete: change.diff_incomplete.unwrap_or(false),
            failed: change.status == vtcode_exec_events::PatchApplyStatus::Failed,
            patch: change.unified_diff.clone().unwrap_or_default(),
        };
        let mut item = TranscriptItem::new(id, "diff".to_string(), "File changes".into(), card.patch.clone());
        item.diff = Some(card);
        let previous_patch = self
            .selected_change
            .as_deref()
            .and_then(|path| self.changes.patch_for(&self.transcript, path));
        self.transcript.upsert(item);
        // Change-sets are rare; rebuild the inspector projection eagerly so
        // renders never parse patches.
        self.changes = ChangesProjection::rebuild(&self.transcript);
        self.change_viewer_dirty |= self
            .selected_change
            .as_deref()
            .is_some_and(|path| self.changes.patch_for(&self.transcript, path) != previous_patch);
    }

    /// Refresh only when the selected file's accepted patch changed, preserving its mode.
    fn refresh_selected_change(&mut self, cx: &mut Context<Self>) {
        if !std::mem::take(&mut self.change_viewer_dirty) {
            return;
        }
        let file = self.selected_change.as_deref().and_then(|path| self.changes.select(path));
        if let (Some(viewer), Some(file)) = (self.change_viewer.as_ref(), file) {
            viewer.update(cx, |viewer, cx| viewer.set_files(vec![file], cx));
        } else {
            self.selected_change = None;
            self.change_viewer = None;
            if self.workspace_review.is_none() {
                self.review_open = false;
            }
        }
    }

    /// Open one changed file in the inspector's syntax-highlighted diff view.
    fn select_change(&mut self, path: String, cx: &mut Context<Self>) {
        let Some(file) = self.changes.select(&path) else {
            return;
        };
        let mode = self
            .change_viewer
            .as_ref()
            .map(|viewer| viewer.read(cx).mode())
            .unwrap_or(DiffMode::Unified);
        self.selected_change = Some(path);
        self.change_viewer_dirty = false;
        self.change_viewer = Some(cx.new(|cx| DiffState::new(vec![file], cx).with_mode(mode)));
        self.inspector_open = true;
        self.set_inspector_tab(InspectorTab::Changes, cx);
    }

    fn select_activity_output(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.selected_output_id = Some(id);
        self.inspector_open = true;
        self.set_inspector_tab(InspectorTab::Output, cx);
        self.inspector_focus.focus(window, cx);
    }

    fn navigate_change_file(&mut self, next: bool, cx: &mut Context<Self>) {
        let entries = self.changes.entries();
        if entries.is_empty() {
            return;
        }
        let current = self
            .selected_change
            .as_deref()
            .and_then(|path| entries.iter().position(|entry| entry.path == path));
        let index = match current {
            Some(index) if next => (index + 1) % entries.len(),
            Some(0) => entries.len() - 1,
            Some(index) => index - 1,
            None => 0,
        };
        if let Some(entry) = entries.get(index) {
            self.select_change(entry.path.clone(), cx);
        }
    }

    fn open_review(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.change_viewer.is_none() {
            return;
        }
        self.workspace_review = None;
        self.show_review(window, cx);
    }

    fn show_review(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings_open {
            self.close_settings(window, cx);
        }
        if self.palette.is_some() {
            self.close_palette(window, cx);
        }
        if self.file_preview.is_some() {
            self.close_file_preview(window, cx);
        }
        if !self.review_open {
            self.review_return_focus = window.focused(cx);
        }
        self.review_open = true;
        self.review_focus.focus(window, cx);
        cx.notify();
    }

    fn close_review(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.review_open = false;
        self.workspace_review = None;
        if let Some(focus) = self.review_return_focus.take() {
            focus.focus(window, cx);
        } else {
            self.focus_composer(window, cx);
        }
        cx.notify();
    }

    /// Return focus when a state transition removes the focused review surface.
    fn restore_focus_after_review_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.review_open && self.review_focus.contains_focused(window, cx) {
            self.review_return_focus = None;
            self.focus_composer(window, cx);
        }
    }

    fn navigate_review_change(&self, next: bool, cx: &mut Context<Self>) {
        let viewer = match &self.workspace_review {
            Some(review) => review.viewer.as_ref(),
            None => self.change_viewer.as_ref(),
        };
        if let Some(viewer) = viewer {
            viewer.update(cx, |viewer, cx| {
                if next {
                    viewer.next_change(cx);
                } else {
                    viewer.previous_change(cx);
                }
            });
        }
    }

    fn append_or_update_item(&mut self, id: String, role: &str, title: &str, text: &str, expandable: bool) {
        self.transcript.upsert(TranscriptItem::new(
            id,
            if expandable {
                role.to_string()
            } else {
                "assistant".to_string()
            },
            title.into(),
            text.to_string(),
        ));
    }

    fn cancel_session_load_task(&mut self) {
        self.session_load_generation = self.session_load_generation.wrapping_add(1);
        self.session_load_task = None;
        self.history_page_generation = self.history_page_generation.wrapping_add(1);
        self.history_page_request_id = None;
        self.history_page_limit = 0;
        self.history_page_task = None;
        self.history_page_loading = false;
        self.history_page_error = None;
    }

    /// Clear session-owned projections together so another conversation cannot inherit them.
    fn reset_session_state(&mut self) {
        self.cancel_session_load_task();
        self.active_primary_agent_display = None;
        self.transcript.clear();
        self.restored_activity.clear();
        self.history_start_index = 0;
        self.history_has_earlier = false;
        self.restored_output_cursors.clear();
        self.restored_output_pages.clear();
        self.output_page_pending = None;
        self.output_page_task = None;
        self.output_page_error = None;
        self.selected_output_id = None;
        self.activity_scroll = VirtualListScrollHandle::new();
        self.restored_review_history_incomplete = false;
        self.restored_activity_history_incomplete = false;
        self.changes = ChangesProjection::default();
        self.selected_change = None;
        self.change_viewer = None;
        self.change_viewer_dirty = false;
        self.review_open = false;
        self.workspace_review = None;
        self.review_return_focus = None;
        self.last_usage = None;
        self.finished_turn_id = None;
        self.clear_pending_interaction();
        self.active_turn_id = None;
        self.cancel_requested = false;
        self.cancel_request_sent = false;
        self.attachments_task = None;
        self.attachments_picker_open = false;
        self.attachments.clear();
        self.pending_submission = None;
        self.first_submission = None;
        self.session_request = None;
        self.retained_submission = None;
    }

    fn load_history(&mut self, history: Vec<TranscriptItem>) {
        self.reset_session_state();
        self.transcript.replace_prepared(history);
        self.transcript.follow_latest();
    }

    fn request(&self, method: &str, params: Value) -> bool {
        self.backend.as_ref().is_some_and(|backend| {
            backend
                .commands
                .try_send(BackendCommand::Request { method: method.to_string(), params })
                .is_ok()
        })
    }

    fn clear_error(&mut self) {
        self.error = None;
    }

    /// The draggable divider between panes.
    fn render_resize_handle(&self, edge: ResizeEdge, cx: &mut Context<Self>) -> impl IntoElement {
        let (id, placement, label, width) = match edge {
            ResizeEdge::Sidebar => (
                "resize-sidebar",
                if self.preferences.navigation_on_right() {
                    HandleEdge::Leading
                } else {
                    HandleEdge::Trailing
                },
                "Resize navigation",
                self.sidebar_width,
            ),
            ResizeEdge::Inspector => {
                ("resize-inspector", HandleEdge::Leading, "Resize inspector", self.inspector_width)
            }
        };
        let focus = match edge {
            ResizeEdge::Sidebar => &self.sidebar_resize_focus,
            ResizeEdge::Inspector => &self.inspector_resize_focus,
        };
        resize_handle::<ResizeEdge, ResizeDragPreview>(id, Axis::Horizontal)
            .inside(placement)
            .on_drag(edge, |_, _, _, cx| cx.new(|_| ResizeDragPreview))
            .on_drag_move(cx.listener(|this, event: &DragMoveEvent<ResizeEdge>, _, cx| {
                let edge = *event.drag(cx);
                if this.resizing.is_some_and(|drag| drag.edge == edge) {
                    let changed = this.resize_from_drag(edge, event.event.position.x);
                    cx.stop_propagation();
                    if changed {
                        cx.notify();
                    }
                }
            }))
            .track_focus(focus)
            .role(gpui_kit::Role::Splitter)
            .aria_label(label)
            .aria_description("Use Left and Right arrow keys to resize; hold Shift for larger steps.")
            .aria_numeric_value(width as f64)
            .aria_min_numeric_value(MIN_PANE_WIDTH as f64)
            .aria_max_numeric_value(MAX_PANE_WIDTH as f64)
            .focus_visible(|style| style.bg(cx.theme().colors.ring.opacity(0.2)))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                let modifiers = event.keystroke.modifiers;
                if modifiers.control || modifiers.alt || modifiers.platform || modifiers.function {
                    return;
                }
                let direction = match event.keystroke.key.as_str() {
                    "left" => -1.,
                    "right" => 1.,
                    _ => return,
                };
                let step = if modifiers.shift { 50. } else { 10. };
                let current_width = match edge {
                    ResizeEdge::Sidebar => this.sidebar_width,
                    ResizeEdge::Inspector => this.inspector_width,
                };
                let delta = match edge {
                    ResizeEdge::Sidebar if this.preferences.navigation_on_right() => -direction * step,
                    ResizeEdge::Sidebar => direction * step,
                    ResizeEdge::Inspector => -direction * step,
                };
                if this.resize_pane_to(edge, current_width + delta) {
                    this.save_preferences();
                    cx.notify();
                }
                cx.stop_propagation();
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    match edge {
                        ResizeEdge::Sidebar => this.sidebar_resize_focus.focus(window, cx),
                        ResizeEdge::Inspector => this.inspector_resize_focus.focus(window, cx),
                    }
                    this.resizing = Some(PaneResize {
                        edge,
                        start_pointer_x_window: event.position.x,
                        start_width: px(match edge {
                            ResizeEdge::Sidebar => this.sidebar_width,
                            ResizeEdge::Inspector => this.inspector_width,
                        }),
                    });
                    cx.notify();
                }),
            )
            .on_drop(cx.listener(|this, _: &ResizeEdge, _, cx| {
                if this.resizing.take().is_some() {
                    this.save_preferences();
                    cx.notify();
                }
            }))
    }

    fn resize_pane_to(&mut self, edge: ResizeEdge, width: f32) -> bool {
        if !width.is_finite() {
            return false;
        }
        let width = width.clamp(MIN_PANE_WIDTH, MAX_PANE_WIDTH);
        let current = match edge {
            ResizeEdge::Sidebar => &mut self.sidebar_width,
            ResizeEdge::Inspector => &mut self.inspector_width,
        };
        if (*current - width).abs() < f32::EPSILON {
            return false;
        }
        *current = width;
        true
    }

    fn resize_from_drag(&mut self, edge: ResizeEdge, pointer_x_window: Pixels) -> bool {
        let Some(drag) = self.resizing.filter(|drag| drag.edge == edge) else {
            return false;
        };
        let pointer_delta_window = pointer_x_window - drag.start_pointer_x_window;
        let width = match edge {
            ResizeEdge::Sidebar if self.preferences.navigation_on_right() => drag.start_width - pointer_delta_window,
            ResizeEdge::Sidebar => drag.start_width + pointer_delta_window,
            ResizeEdge::Inspector => drag.start_width - pointer_delta_window,
        };
        // Preferences keep pane widths as f32; convert only at that boundary.
        self.resize_pane_to(edge, width.as_f32())
    }

    fn render_sidebar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let mut sidebar = div()
            .id("project-thread-sidebar")
            .test_support()
            .role(gpui_kit::Role::Group)
            .aria_label("Workspace, projects, and recent threads")
            .w(px(self.sidebar_width))
            .min_w(px(MIN_PANE_WIDTH))
            .h_full()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .bg(
                themes::navigation_surfaces(theme.colors.background, theme.colors.secondary, theme.colors.foreground)
                    .panel,
            )
            .rounded_lg()
            .border_1()
            .border_color(theme.colors.border)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(section_label("WORKSPACE", theme.colors.muted_foreground))
                    .child(
                        desktop_button("choose-workspace")
                            .tooltip_with_action("Choose workspace", &ChooseWorkspace, Some("VTCodeDesktop"))
                            .label(if self.workspace_selection_pending {
                                "Checking…"
                            } else if self.workspace.is_some() {
                                "Change"
                            } else {
                                "Open"
                            })
                            .ghost()
                            .desktop_disabled(
                                self.workspace_picker_open
                                    || !self.workspace_switch_idle()
                                    || self.retained_submission.is_some(),
                            )
                            .dispatches_desktop_action(ChooseWorkspace, self.focus_handle.clone()),
                    ),
            );

        sidebar = sidebar.child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(Icon::new(IconName::FolderGit2).text_color(theme.colors.primary))
                .child(
                    div().min_w_0().flex_1().text_sm().font_semibold().truncate().child(
                        self.workspace
                            .as_ref()
                            .and_then(|p| p.file_name())
                            .and_then(|n| n.to_str())
                            .unwrap_or("Choose a project")
                            .to_string(),
                    ),
                ),
        );

        sidebar = sidebar.child(
            div()
                .flex()
                .w_full()
                .gap_2()
                .child(
                    desktop_button("new-session")
                        .tooltip_with_action("New session", &NewSession, Some("VTCodeDesktop"))
                        .label("New thread")
                        .icon(IconName::Plus)
                        .ghost()
                        .flex_1()
                        .desktop_disabled(!self.session_change_allowed(None))
                        .dispatches_desktop_action(NewSession, self.focus_handle.clone()),
                )
                .child(
                    desktop_button("close-session")
                        .tooltip_with_action("Close session", &CloseSession, Some("VTCodeDesktop"))
                        .icon(IconName::X)
                        .accessibility_label("Close thread")
                        .ghost()
                        .desktop_disabled(self.selected_session_id.is_none() || !self.session_change_allowed(None))
                        .dispatches_desktop_action(CloseSession, self.focus_handle.clone()),
                ),
        );

        sidebar = sidebar.child(section_label("PROJECTS", theme.colors.muted_foreground));
        let recent = self.recent_workspaces();
        if !recent.is_empty() {
            sidebar = sidebar.child(div().w_full().flex().flex_col().gap_1().children(recent.iter().map(|path| {
                let workspace_path = path.clone();
                let current = self
                    .workspace
                    .as_ref()
                    .is_some_and(|selected| selected.display().to_string() == *path);
                let label = Path::new(path)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or(path)
                    .to_string();
                div().w_full().child(
                    desktop_button(opaque_path_element_id("workspace", path))
                        .accessibility_label(format!("{label}{}", if current { " (current workspace)" } else { "" }))
                        .child(navigation_label(label, IconName::Folder, current, theme.colors.primary))
                        .ghost()
                        .w_full()
                        .desktop_disabled(!self.workspace_switch_idle() || self.retained_submission.is_some())
                        .when(current, |button| button.bg(theme.colors.secondary).text_color(theme.colors.foreground))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.select_workspace_from_ui(PathBuf::from(workspace_path.clone()), window, cx)
                        })),
                )
            })));
        }

        sidebar = sidebar.child(
            div()
                .mt_2()
                .flex()
                .items_center()
                .justify_between()
                .child(section_label("RECENT THREADS", theme.colors.muted_foreground))
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.colors.muted_foreground)
                        .child(self.sessions.len().to_string()),
                ),
        );
        let session_row_height = scale_row_height_for_ui_font(SESSION_ROW_HEIGHT, cx.theme().font_size);
        let session_sizes = self.session_row_sizes.get(self.sessions.len(), session_row_height);
        let session_primary = theme.colors.primary;
        sidebar = sidebar.child(
            div()
                .id("session-list")
                .relative()
                .flex_1()
                .min_h_0()
                .w_full()
                .overflow_hidden()
                .child(
                    v_virtual_list(
                        cx.entity(),
                        "session-virtual-list",
                        session_sizes,
                        move |view, visible_range, _, cx| {
                            visible_range
                                .filter_map(|index| {
                                    let session = view.sessions.get(index)?;
                                    let session_id = session.session_id.clone();
                                    let selected = view.selected_session_id.as_deref() == Some(&session_id);
                                    let label = session.title.clone().unwrap_or_else(|| {
                                        format!("Session {}", session_id.chars().take(8).collect::<String>())
                                    });
                                    let enabled = view.session_change_allowed(Some(&session_id));
                                    Some(
                                        div()
                                            .id(format!("session-row-{session_id}"))
                                            .test_support()
                                            .h(px(session_row_height))
                                            .w_full()
                                            .child(
                                                desktop_button(format!("session-{session_id}"))
                                                    .accessibility_label(format!(
                                                        "{label}{}",
                                                        if selected { " (current thread)" } else { "" }
                                                    ))
                                                    .child(navigation_label(
                                                        label,
                                                        IconName::MessageSquare,
                                                        selected,
                                                        session_primary,
                                                    ))
                                                    .ghost()
                                                    .desktop_disabled(!enabled)
                                                    .w_full()
                                                    .justify_start()
                                                    .when(selected, |button| {
                                                        button
                                                            .bg(cx.theme().colors.secondary)
                                                            .text_color(cx.theme().colors.foreground)
                                                    })
                                                    .on_click(cx.listener(move |this, _, _, cx| {
                                                        this.load_session(session_id.clone(), cx)
                                                    })),
                                            )
                                            .into_any_element(),
                                    )
                                })
                                .collect()
                        },
                    )
                    .track_scroll(&self.session_scroll),
                )
                .child(Scrollbar::vertical(&self.session_scroll)),
        );

        sidebar.child(
            div()
                .mt_auto()
                .pt_3()
                .border_t_1()
                .border_color(theme.colors.border)
                .text_xs()
                .text_color(theme.colors.muted_foreground)
                .child(
                    div()
                        .id("sidebar-status")
                        .test_support()
                        .role(gpui_kit::Role::Status)
                        .aria_label(self.status.clone())
                        .child(self.status.clone()),
                ),
        )
    }

    fn render_thread_header(&self) -> impl IntoElement {
        let title = self
            .sessions
            .iter()
            .find(|session| Some(session.session_id.as_str()) == self.selected_session_id.as_deref())
            .and_then(|session| session.title.as_deref())
            .unwrap_or("New thread");
        div()
            .id("thread-header")
            .test_support()
            .w_full()
            .flex()
            .items_center()
            .justify_between()
            .px_8()
            .py_3()
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .text_lg()
                    .font_semibold()
                    .truncate()
                    .child(title.to_string()),
            )
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let toolbar_surface =
            themes::navigation_surfaces(theme.colors.background, theme.colors.secondary, theme.colors.foreground).rail;
        if self.zen_mode {
            return TitleBar::new()
                .w_full()
                .h(px(48.))
                .bg(toolbar_surface)
                .border_b_1()
                .border_color(toolbar_surface)
                .child(div().flex_1())
                .into_any_element();
        }
        let navigation_visible = self.preferences.navigation_visible();
        let navigation_label = if navigation_visible {
            "Toggle navigation"
        } else {
            "Show navigation"
        };
        let navigation_icon = if navigation_visible && self.preferences.navigation_on_right() {
            IconName::PanelRight
        } else {
            IconName::PanelLeft
        };
        let workspace = self
            .workspace
            .as_ref()
            .and_then(|path| path.file_name())
            .and_then(|name| name.to_str())
            .unwrap_or("Workspace");
        TitleBar::new()
            .w_full()
            .h(px(48.))
            .pr_3()
            .bg(toolbar_surface)
            .border_b_1()
            .border_color(toolbar_surface)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        desktop_button("navigate-back")
                            .accessibility_label("Back")
                            .icon(IconName::ArrowLeft)
                            .ghost()
                            .tooltip_with_action("Back", &NavigateBack, Some("VTCodeDesktop"))
                            .desktop_disabled(!self.can_navigate_back())
                            .dispatches_desktop_action(NavigateBack, self.focus_handle.clone()),
                    )
                    .child(
                        desktop_button("navigate-forward")
                            .accessibility_label("Forward")
                            .icon(IconName::ArrowRight)
                            .ghost()
                            .tooltip_with_action("Forward", &NavigateForward, Some("VTCodeDesktop"))
                            .desktop_disabled(!self.can_navigate_forward())
                            .dispatches_desktop_action(NavigateForward, self.focus_handle.clone()),
                    )
                    .child(
                        desktop_button("toggle-sidebar")
                            .accessibility_label(navigation_label)
                            .toggled(navigation_visible && self.sidebar_open)
                            .icon(navigation_icon)
                            .ghost()
                            .tooltip_with_action(navigation_label, &ToggleSidebar, Some("VTCodeDesktop"))
                            .dispatches_desktop_action(ToggleSidebar, self.focus_handle.clone()),
                    )
                    .child(brand_logo(22.))
                    .child(div().text_sm().font_semibold().child("VT Code"))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_sm()
                            .text_color(theme.colors.foreground)
                            .child(format!("/ {workspace}")),
                    ),
            )
            .child(
                Toolbar::new("header-toolbar").small().child(
                    ToolbarGroup::new("header-actions-group")
                        .label("Workspace actions")
                        .gap_1()
                        .when(!navigation_visible, |group| {
                            group
                                .child(
                                    desktop_button("top-new-thread")
                                        .icon(IconName::Plus)
                                        .ghost()
                                        .accessibility_label("New thread")
                                        .tooltip_with_action("New thread", &NewSession, Some("VTCodeDesktop"))
                                        .desktop_disabled(!self.session_change_allowed(None))
                                        .dispatches_desktop_action(NewSession, self.focus_handle.clone()),
                                )
                                .child(
                                    desktop_button("top-utilities")
                                        .icon(IconName::GitBranch)
                                        .ghost()
                                        .accessibility_label("Workspace utilities")
                                        .tooltip_with_action(
                                            "Workspace utilities",
                                            &OpenUtilities,
                                            Some("VTCodeDesktop"),
                                        )
                                        .desktop_disabled(self.workspace.is_none())
                                        .dispatches_desktop_action(OpenUtilities, self.focus_handle.clone()),
                                )
                                .child(
                                    desktop_button("top-zen-mode-toggle")
                                        .icon(IconName::Focus)
                                        .ghost()
                                        .accessibility_label("Enter Zen mode")
                                        .tooltip_with_action("Enter Zen mode", &ToggleZenMode, Some("VTCodeDesktop"))
                                        .dispatches_desktop_action(ToggleZenMode, self.focus_handle.clone()),
                                )
                        })
                        .child(
                            desktop_button("search-commands")
                                .accessibility_label("Search commands")
                                .icon(IconName::Search)
                                .ghost()
                                .tooltip_with_action("Search commands", &OpenCommandPalette, Some("VTCodeDesktop"))
                                .dispatches_desktop_action(OpenCommandPalette, self.focus_handle.clone()),
                        )
                        .child(
                            desktop_button("toggle-inspector")
                                .icon(IconName::PanelRight)
                                .accessibility_label(if self.inspector_open {
                                    "Hide panel"
                                } else {
                                    "Show panel"
                                })
                                .tooltip_with_action("Toggle inspector", &ToggleInspector, Some("VTCodeDesktop"))
                                .ghost()
                                .desktop_disabled(self.workspace.is_none())
                                .dispatches_desktop_action(ToggleInspector, self.focus_handle.clone()),
                        )
                        .child(
                            desktop_button("open-settings")
                                .accessibility_label("Settings")
                                .icon(IconName::Settings2)
                                .tooltip_with_action("Settings", &OpenSettings, Some("VTCodeDesktop"))
                                .ghost()
                                .dispatches_desktop_action(OpenSettings, self.focus_handle.clone()),
                        )
                        .when_some(
                            self.configuration
                                .as_ref()
                                .and_then(|config| config.session_options.as_ref())
                                .and_then(|options| options.speed.as_ref()),
                            |group, speed| {
                                group.content(
                                    Tag::custom(theme.colors.secondary, theme.colors.foreground, theme.colors.border)
                                        .small()
                                        .child(speed.clone()),
                                )
                            },
                        )
                        .content(self.render_chrome_menu("header-overflow", "Window actions")),
                ),
            )
            .into_any_element()
    }

    fn render_file_preview(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let colors = &theme.colors;
        let Some(preview) = self.file_preview.as_ref() else {
            return div();
        };
        let text = preview.text.clone();
        let mono = theme.mono_font_family.clone();
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(colors.background.opacity(0.6))
            .child(
                div()
                    .id("file-preview")
                    .track_focus(&self.preview_focus)
                    .role(gpui_kit::Role::Dialog)
                    .accessibility_id("file-preview")
                    .aria_label(format!("File preview: {}", preview.rel_path))
                    .aria_description("Read-only file preview. Press Escape to close.")
                    .w(px(860.))
                    .max_h(px(640.))
                    .flex()
                    .flex_col()
                    .rounded_md()
                    .border_1()
                    .border_color(colors.border)
                    .bg(colors.secondary)
                    .shadow_md()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .px_3()
                            .py_2()
                            .border_b_1()
                            .border_color(colors.border)
                            .child(
                                div()
                                    .min_w_0()
                                    .flex_1()
                                    .truncate()
                                    .font_medium()
                                    .font_family(mono.clone())
                                    .child(preview.rel_path.clone()),
                            )
                            .child(desktop_button("copy-preview").label("Copy").ghost().on_click(
                                move |_, window, app| {
                                    app.write_to_clipboard(gpui_kit::ClipboardItem::new_string(text.to_string()));
                                    super::notifications::copied_to_clipboard(window, app);
                                },
                            ))
                            .child(
                                desktop_button("close-preview")
                                    .tooltip_with_action("Close preview", &DismissOverlay, Some("VTCodeDesktop"))
                                    .label("Close")
                                    .ghost()
                                    .on_click(cx.listener(|this, _, window, cx| this.close_file_preview(window, cx))),
                            ),
                    )
                    .child(
                        div()
                            .id("file-preview-body")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scrollbar()
                            .id("file-preview-body")
                            .p_3()
                            .text_xs()
                            .font_family(mono)
                            .text_size(theme.mono_font_size)
                            .child(preview.text.clone()),
                    )
                    .when(preview.truncated, |panel| {
                        panel.child(
                            div()
                                .px_3()
                                .py_2()
                                .border_t_1()
                                .border_color(colors.border)
                                .text_xs()
                                .text_color(colors.muted_foreground)
                                .child("Preview truncated at 64 KiB."),
                        )
                    }),
            )
    }

    fn select_inspector_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.update_inspector_tab(index, false, Some(window), cx);
    }

    fn select_inspector_tab_from_keyboard(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.update_inspector_tab(index, true, Some(window), cx);
    }

    fn set_inspector_tab(&mut self, tab: InspectorTab, cx: &mut Context<Self>) {
        if let Some(index) = InspectorTab::ALL.iter().position(|candidate| *candidate == tab) {
            self.update_inspector_tab(index, true, None, cx);
        }
    }

    fn update_inspector_tab(&mut self, index: usize, reveal: bool, focus: Option<&mut Window>, cx: &mut Context<Self>) {
        if let Some(tab) = InspectorTab::ALL.get(index) {
            self.inspector_tab = *tab;
            if reveal {
                self.inspector_tabs_scroll.scroll_to_item(index);
            }
            if let Some(window) = focus {
                self.inspector_focus.focus(window, cx);
            }
            cx.notify();
        }
    }

    fn cycle_inspector_tab(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let count = InspectorTab::ALL.len();
        let current = InspectorTab::ALL
            .iter()
            .position(|tab| *tab == self.inspector_tab)
            .unwrap_or_default();
        let index = if forward {
            (current + 1) % count
        } else {
            (current + count - 1) % count
        };
        self.select_inspector_tab_from_keyboard(index, window, cx);
    }

    /// Right-hand context inspector: derived Changes, Files, and Activity
    /// views over the same transcript the conversation renders.
    fn render_inspector(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let colors = theme.colors;
        let panel = div()
            .w(px(self.inspector_width))
            .min_w(px(MIN_PANE_WIDTH))
            .h_full()
            .min_h_0()
            .flex()
            .flex_col()
            .rounded_lg()
            .bg(themes::mix(colors.background, colors.secondary, 0.3))
            .border_1()
            .border_color(colors.border)
            .child(
                div()
                    .id("inspector-navigation")
                    .test_support()
                    .key_context("VTCodeInspector")
                    .role(gpui_kit::Role::Group)
                    .aria_label("Context inspector tabs")
                    .aria_description("Use Left and Right arrows, Home, or End to choose a tab.")
                    .border_1()
                    .border_color(colors.border.opacity(0.))
                    .focus_visible(|style| style.border_color(colors.ring))
                    .on_action(
                        cx.listener(|view, _: &NextInspectorTab, window, cx| {
                            view.cycle_inspector_tab(true, window, cx)
                        }),
                    )
                    .on_action(cx.listener(|view, _: &PreviousInspectorTab, window, cx| {
                        view.cycle_inspector_tab(false, window, cx)
                    }))
                    .on_action(cx.listener(|view, _: &FirstInspectorTab, window, cx| {
                        view.select_inspector_tab_from_keyboard(0, window, cx)
                    }))
                    .on_action(cx.listener(|view, _: &LastInspectorTab, window, cx| {
                        view.select_inspector_tab_from_keyboard(InspectorTab::ALL.len().saturating_sub(1), window, cx)
                    }))
                    .child(
                        div()
                            .id("inspector-tabs-focus")
                            .test_support()
                            .track_focus(&self.inspector_focus)
                            .child(
                                TabBar::new("inspector-tabs")
                                    .underline()
                                    .small()
                                    .w_full()
                                    .track_scroll(&self.inspector_tabs_scroll)
                                    .selected_index(
                                        InspectorTab::ALL
                                            .iter()
                                            .position(|tab| *tab == self.inspector_tab)
                                            .unwrap_or_default(),
                                    )
                                    .on_click(cx.listener(|view, index: &usize, window, cx| {
                                        view.select_inspector_tab(*index, window, cx)
                                    }))
                                    .children(
                                        InspectorTab::ALL.map(|tab| Tab::new().label(tab.label()).cursor_pointer()),
                                    ),
                            ),
                    ),
            );

        let content = match self.inspector_tab {
            InspectorTab::Changes => self.render_changes_tab(&colors, cx),
            InspectorTab::Files => self.render_inspector_files_tab(cx),
            InspectorTab::Activity => self.render_activity_tab(&colors, cx),
            InspectorTab::Output => self.render_output_tab(cx),
            InspectorTab::Utilities => self.render_utilities(cx),
        };
        panel
            .child(content)
            .when(self.inspector_tab == InspectorTab::Changes, |panel| {
                panel
                    .key_context("VTCodeDiffReview")
                    .on_action(cx.listener(|view, _: &NextReviewFile, _, cx| view.navigate_change_file(true, cx)))
                    .on_action(cx.listener(|view, _: &PreviousReviewFile, _, cx| view.navigate_change_file(false, cx)))
            })
            .into_any_element()
    }

    fn render_changes_tab(
        &mut self,
        colors: &gpui_kit::component::theme::ThemeColor,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.review_open {
            return div()
                .p_4()
                .text_sm()
                .when(self.restored_review_history_incomplete, |container| {
                    container.child(warning_alert("review-history-warning", PARTIAL_REVIEW_HISTORY_MESSAGE, colors))
                })
                .child("Changes are open in the expanded review.")
                .into_any_element();
        }
        let content = self.render_changes_content(false, colors, cx);
        let mut content_panel = div().w_full().flex_1().min_h_0().flex().flex_col();
        if self.restored_review_history_incomplete {
            content_panel = content_panel.child(div().px_3().pt_2().child(warning_alert(
                "review-history-warning",
                PARTIAL_REVIEW_HISTORY_MESSAGE,
                colors,
            )));
        }
        div()
            .id("inspector-changes-tab")
            .w_full()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(
                div().px_3().py_2().border_b_1().border_color(colors.border).child(
                    desktop_button("review-working-tree")
                        .label("Review working tree")
                        .icon(IconName::GitCompare)
                        .tooltip_with_action("Review working tree", &OpenWorkspaceReview, Some("VTCodeDesktop"))
                        .ghost()
                        .small()
                        .desktop_disabled(self.backend.is_none() || self.service_info.is_none())
                        .dispatches_desktop_action(OpenWorkspaceReview, self.focus_handle.clone()),
                ),
            )
            .child(content_panel.child(content))
            .test_support()
            .into_any_element()
    }

    fn render_changes_content(
        &mut self,
        expanded: bool,
        colors: &gpui_kit::component::theme::ThemeColor,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let diff_foreground =
            themes::diff_text_foreground(colors.background, colors.foreground, colors.success, colors.danger);
        let mut body = div().w_full().flex_1().min_h_0().flex().flex_col();

        if self.changes.is_empty() {
            return empty_state(
                "No changes yet",
                "Edits made during this session will appear here for review.",
                IconName::GitCompare,
            )
            .into_any_element();
        }

        let (additions, deletions) = self.changes.totals();
        body = body.child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .px_3()
                .py_2()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_xs()
                        .child(format!("{} files", self.changes.entries().len()))
                        .child(div().text_color(colors.success).child(format!("+{additions}")))
                        .child(div().text_color(colors.danger).child(format!("−{deletions}"))),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(
                            desktop_button("previous-change-file")
                                .icon(IconName::ChevronLeft)
                                .accessibility_label("Previous changed file")
                                .tooltip_with_action(
                                    "Previous changed file",
                                    &PreviousReviewFile,
                                    Some("VTCodeDiffReview"),
                                )
                                .ghost()
                                .small()
                                .desktop_disabled(self.changes.entries().len() < 2)
                                .on_click(cx.listener(|view, _, _, cx| view.navigate_change_file(false, cx))),
                        )
                        .child(
                            desktop_button("next-change-file")
                                .icon(IconName::ChevronRight)
                                .accessibility_label("Next changed file")
                                .tooltip_with_action("Next changed file", &NextReviewFile, Some("VTCodeDiffReview"))
                                .ghost()
                                .small()
                                .desktop_disabled(self.changes.entries().len() < 2)
                                .on_click(cx.listener(|view, _, _, cx| view.navigate_change_file(true, cx))),
                        ),
                ),
        );

        body = body.child(
            div()
                .id("change-list")
                .w_full()
                .max_h(px(if expanded { 220. } else { 112. }))
                .min_h_0()
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap_1()
                .px_2()
                .pb_2()
                .children(self.changes.entries().iter().map(|entry| {
                    let path = entry.path.clone();
                    let selected = self.selected_change.as_deref() == Some(entry.path.as_str());
                    let kind_label = match entry.kind {
                        DiffFileKind::Add => ("A", colors.success),
                        DiffFileKind::Delete => ("D", colors.danger),
                        DiffFileKind::Update => ("M", colors.muted_foreground),
                    };
                    div()
                        .w_full()
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .when(selected, |row| row.bg(colors.secondary))
                        .child(
                            div()
                                .text_xs()
                                .font_semibold()
                                .w(px(14.))
                                .text_color(kind_label.1)
                                .child(kind_label.0),
                        )
                        .child(div().flex_1().min_w_0().text_sm().truncate().child(path.clone()))
                        .child(
                            div()
                                .text_xs()
                                .text_color(colors.muted_foreground)
                                .child(format!("+{} −{}", entry.additions, entry.deletions)),
                        )
                        .child(
                            desktop_button(opaque_path_element_id("change-row", &path))
                                .icon(IconName::ChevronRight)
                                .accessibility_label(format!(
                                    "Review {path}{}",
                                    if selected { " (selected)" } else { "" }
                                ))
                                .tooltip(path.clone())
                                .ghost()
                                .small()
                                .on_click(cx.listener(move |this, _, _, cx| this.select_change(path.clone(), cx))),
                        )
                })),
        );

        // Full syntax-highlighted diff of the selected file.
        if let (Some(path), Some(viewer)) = (self.selected_change.clone(), self.change_viewer.clone()) {
            let patch = self.changes.patch_for(&self.transcript, &path);
            let rel_path = path.clone();
            let mut viewer_panel = div()
                .w_full()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .border_t_1()
                .border_color(colors.border);
            let mut header = div()
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .px_3()
                .py_2()
                .child(div().flex_1().min_w_0().truncate().text_sm().font_medium().child(path));
            if expanded {
                let mode = viewer.read(cx).mode();
                for (id, label, target) in [
                    ("review-unified", "Unified", DiffMode::Unified),
                    ("review-split", "Split", DiffMode::Split),
                ] {
                    let state = viewer.clone();
                    header = header.child(
                        desktop_button(id)
                            .label(label)
                            .accessibility_label(format!(
                                "{label} diff view{}",
                                if mode == target { " (selected)" } else { "" }
                            ))
                            .ghost()
                            .small()
                            .when(mode == target, |button| button.bg(colors.secondary))
                            .on_click(move |_, _, cx| state.update(cx, |state, cx| state.set_mode(target, cx))),
                    );
                }
                header = header
                    .child(
                        desktop_button("review-previous-change")
                            .icon(IconName::ArrowUp)
                            .accessibility_label("Previous change")
                            .tooltip_with_action("Previous change", &PreviousReviewChange, Some("VTCodeDiffReview"))
                            .ghost()
                            .small()
                            .on_click(cx.listener(|view, _, _, cx| view.navigate_review_change(false, cx))),
                    )
                    .child(
                        desktop_button("review-next-change")
                            .icon(IconName::ArrowDown)
                            .accessibility_label("Next change")
                            .tooltip_with_action("Next change", &NextReviewChange, Some("VTCodeDiffReview"))
                            .ghost()
                            .small()
                            .on_click(cx.listener(|view, _, _, cx| view.navigate_review_change(true, cx))),
                    );
            } else {
                header = header.child(
                    desktop_button("expand-change-review")
                        .icon(IconName::Maximize2)
                        .accessibility_label("Expand diff review")
                        .tooltip("Expand diff review")
                        .ghost()
                        .small()
                        .on_click(cx.listener(|view, _, window, cx| view.open_review(window, cx))),
                );
            }
            if let Some(patch) = patch {
                header = header.child(
                    desktop_button("copy-change-patch")
                        .icon(IconName::Copy)
                        .accessibility_label("Copy patch")
                        .tooltip("Copy patch")
                        .ghost()
                        .small()
                        .on_click(move |_, window, app| {
                            app.write_to_clipboard(gpui_kit::ClipboardItem::new_string(patch.to_string()));
                            super::notifications::copied_to_clipboard(window, app);
                        }),
                );
            }
            header = header.child(
                desktop_button("open-change-file")
                    .icon(IconName::ExternalLink)
                    .accessibility_label("Open file")
                    .tooltip("Open file preview")
                    .ghost()
                    .small()
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.open_file_preview(rel_path.clone(), window, cx)),
                    ),
            );
            viewer_panel = viewer_panel.child(header).child(
                div()
                    .id("change-diff-view")
                    .test_support()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .child(
                        Diff::new(&viewer)
                            .line_number(true)
                            .soft_wrap(expanded)
                            .text_color(diff_foreground),
                    ),
            );
            body = body.child(viewer_panel);
        }
        body.into_any_element()
    }

    fn render_change_review(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let workspace_review = self.workspace_review.is_some();
        let content = if workspace_review {
            self.render_workspace_review_content(cx)
        } else {
            self.render_changes_content(true, &colors, cx)
        };
        let title = if workspace_review {
            "Working tree review"
        } else {
            "Review changes"
        };
        let description = if workspace_review {
            "Read-only redacted snapshot · staged and unstaged changes · optional untracked text previews"
        } else {
            "Session patch previews · read-only"
        };
        div()
            .absolute()
            .inset_0()
            .p_6()
            .bg(colors.background.opacity(0.85))
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_down(MouseButton::Left, cx.listener(|view, _, window, cx| view.close_review(window, cx)))
            .child(
                div()
                    .w_full()
                    .h_full()
                    .max_w(px(1500.))
                    .child(
                        div()
                            .id("diff-review-panel")
                            .test_support()
                            .role(gpui_kit::Role::Dialog)
                            .aria_label(title)
                            .key_context("VTCodeDiffReview")
                            .size_full()
                            .flex()
                            .flex_col()
                            .rounded_xl()
                            .border_1()
                            .border_color(colors.border)
                            .bg(colors.secondary)
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_action(
                                cx.listener(|view, _: &NextReviewChange, _, cx| view.navigate_review_change(true, cx)),
                            )
                            .on_action(cx.listener(|view, _: &PreviousReviewChange, _, cx| {
                                view.navigate_review_change(false, cx)
                            }))
                            .on_action(cx.listener(|view, _: &NextReviewFile, _, cx| {
                                view.navigate_workspace_review_file(true, cx)
                            }))
                            .on_action(cx.listener(|view, _: &PreviousReviewFile, _, cx| {
                                view.navigate_workspace_review_file(false, cx)
                            }))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .px_4()
                                    .py_3()
                                    .border_b_1()
                                    .border_color(colors.border)
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap_1()
                                            .child(div().text_lg().font_semibold().child(title))
                                            .child(
                                                div().text_xs().text_color(colors.muted_foreground).child(description),
                                            ),
                                    )
                                    .child(
                                        desktop_button("close-diff-review")
                                            .icon(IconName::X)
                                            .accessibility_label("Close diff review")
                                            .tooltip_with_action("Close review", &DismissOverlay, Some("VTCodeDesktop"))
                                            .ghost()
                                            .on_click(cx.listener(|view, _, window, cx| view.close_review(window, cx))),
                                    ),
                            )
                            .child(content),
                    )
                    .focus_trap("diff-review-trap", &self.review_focus),
            )
            .into_any_element()
    }

    fn render_inspector_files_tab(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let mut body = div().w_full().flex_1().min_h_0().flex().flex_col();
        body = body.child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .px_3()
                .py_2()
                .child(
                    desktop_button("focus-files")
                        .label("Workspace files")
                        .accessibility_label("Focus workspace file tree")
                        .tooltip_with_action("Focus workspace file tree", &OpenWorkspaceFiles, Some("VTCodeDesktop"))
                        .ghost()
                        .dispatches_desktop_action(OpenWorkspaceFiles, self.focus_handle.clone()),
                )
                .child(
                    desktop_button("refresh-files")
                        .label("Refresh")
                        .accessibility_label("Refresh workspace files")
                        .tooltip_with_action("Refresh workspace files", &RefreshWorkspace, Some("VTCodeDesktop"))
                        .ghost()
                        .dispatches_desktop_action(RefreshWorkspace, self.focus_handle.clone()),
                ),
        );
        let directory_paths = self.file_directory_paths.clone();
        let file_tree = self.file_tree.clone();
        let weak_view = cx.entity().downgrade();
        if self.files.is_empty() && self.files_scan_cancel.is_some() {
            body = body.child(
                div()
                    .id("file-list-loading")
                    .test_support()
                    .role(gpui_kit::Role::Status)
                    .aria_label(rust_i18n::t!("app.files.scanning").to_string())
                    .aria_description("Workspace files are being indexed for browsing.")
                    .w_full()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .p_3()
                    .children([
                        skeleton_line("file-scan-skeleton-1", px(196.), cx),
                        skeleton_line("file-scan-skeleton-2", px(148.), cx),
                        skeleton_line("file-scan-skeleton-3", px(218.), cx),
                        skeleton_line("file-scan-skeleton-4", px(172.), cx),
                        skeleton_line("file-scan-skeleton-5", px(204.), cx),
                    ]),
            );
        } else if self.files.is_empty() {
            let (title, description) = if self.workspace.is_some() {
                (
                    rust_i18n::t!("app.files.empty.title").to_string(),
                    rust_i18n::t!("app.files.empty.description").to_string(),
                )
            } else {
                (
                    rust_i18n::t!("app.files.no_workspace.title").to_string(),
                    rust_i18n::t!("app.files.no_workspace.description").to_string(),
                )
            };
            body = body.child(
                div()
                    .id("file-list-empty")
                    .test_support()
                    .role(gpui_kit::Role::Status)
                    .aria_label(title.clone())
                    .w_full()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(empty_state(title, description, IconName::Folder)),
            );
        } else {
            body = body.child(
                div()
                    .id("file-list")
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .on_action(
                        cx.listener(|this, _: &OpenSelectedFile, window, cx| this.open_selected_file(window, cx)),
                    )
                    .child(Tree::new(&self.file_tree, move |_index, entry, selected, _window, app| {
                        let rel_path = entry.item().id.to_string();
                        let is_dir = directory_paths.contains(&rel_path);
                        let display_name = rel_path.rsplit('/').next().unwrap_or(&rel_path).to_string();
                        let icon = if is_dir {
                            if entry.is_expanded() {
                                IconName::FolderOpen
                            } else {
                                IconName::FolderClosed
                            }
                        } else {
                            IconName::FileText
                        };
                        let tree = file_tree.clone();
                        let view = weak_view.clone();
                        let open_path = rel_path.clone();
                        ListItem::new(format!("workspace-file-{rel_path}"))
                            .selected(selected)
                            .cursor_pointer()
                            .accessibility_label(format!("{} {rel_path}", if is_dir { "Folder" } else { "File" }))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_x_2()
                                    .child(Icon::new(icon).small().text_color(app.theme().muted_foreground))
                                    .child(div().text_sm().child(display_name)),
                            )
                            .on_click(move |_, window, app| {
                                tree.update(app, |tree, app| tree.focus(window, app));
                                if is_dir {
                                    return;
                                }
                                if let Some(view) = view.upgrade() {
                                    view.update(app, |view, cx| {
                                        view.open_file_preview(open_path.clone(), window, cx);
                                    });
                                }
                            })
                    })),
            );
        }
        body.into_any_element()
    }

    /// Show a selected historical capture or the newest command/tool output.
    fn render_output_tab(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let colors = theme.colors;
        let mono = theme.mono_font_family.clone();
        let mut pane = div().w_full().flex_1().min_h_0().flex().flex_col();
        if self.restored_activity_history_incomplete {
            pane = pane.child(div().px_3().pt_2().child(warning_alert(
                "activity-history-warning",
                PARTIAL_ACTIVITY_HISTORY_MESSAGE,
                &colors,
            )));
        }
        let Some(output) =
            inspector::output_for(&self.transcript, &self.restored_activity, self.selected_output_id.as_deref())
        else {
            return pane
                .child(empty_state(
                    "No output yet",
                    "Command and tool output will appear here while a turn runs.",
                    IconName::Terminal,
                ))
                .into_any_element();
        };
        let tail = inspector::output_tail(&output.text);
        let is_selected_output = self.selected_output_id.as_deref() == Some(output.id.as_str());
        let has_more_output = self.restored_output_cursors.contains_key(&output.id);
        let output_page_loading = self
            .output_page_pending
            .as_ref()
            .is_some_and(|pending| pending.item_id == output.id);
        let output_page_error = self
            .output_page_error
            .as_ref()
            .and_then(|(item_id, error)| (item_id == &output.id).then(|| error.clone()));
        let copy_output_id = output.id.clone();
        let copy_output_preview = output.text.clone();
        let output_header = div()
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(colors.border)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_sm()
                    .font_medium()
                    .child(output.title.clone()),
            )
            .when(is_selected_output, |header| {
                header
                    .child(
                        Tag::custom(colors.secondary, colors.muted_foreground, colors.border)
                            .small()
                            .rounded_full()
                            .child("History"),
                    )
                    .child(
                        desktop_button("output-follow-latest")
                            .icon(IconName::Activity)
                            .ghost()
                            .small()
                            .accessibility_label("Follow latest output")
                            .tooltip("Show the newest captured output")
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.selected_output_id = None;
                                cx.notify();
                            })),
                    )
            })
            .when(output.running, |header| {
                header.child(
                    Tag::custom(colors.secondary, colors.foreground, colors.border)
                        .small()
                        .child("Running"),
                )
            })
            .child(
                desktop_button("copy-output")
                    .icon(IconName::Copy)
                    .accessibility_label("Copy output")
                    .tooltip("Copy captured output")
                    .ghost()
                    .small()
                    .on_click(cx.listener(move |view, _, window, cx| {
                        let text = restored_output_for_copy(
                            &copy_output_id,
                            &copy_output_preview,
                            &view.restored_output_pages,
                        );
                        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(text));
                        super::notifications::copied_to_clipboard(window, cx);
                    })),
            );
        pane = pane.child(output_header);
        if has_more_output || output_page_loading || output_page_error.is_some() {
            let output_id = output.id.clone();
            let mut recovery = div().w_full().px_3().py_1().flex().items_center().gap_2();
            if has_more_output {
                recovery = recovery.child(
                    desktop_button("load-more-output")
                        .label(if output_page_loading {
                            "Loading captured output…"
                        } else {
                            "Load more captured output"
                        })
                        .ghost()
                        .small()
                        .accessibility_label(if output_page_loading {
                            "Loading more captured output"
                        } else {
                            "Load more captured output"
                        })
                        .desktop_disabled(self.output_page_pending.is_some())
                        .on_click(cx.listener(move |view, _, _, cx| {
                            view.request_more_captured_output(&output_id, cx);
                        })),
                );
            }
            if output_page_loading {
                recovery = recovery.child(
                    div()
                        .id("output-page-loading")
                        .role(gpui_kit::Role::Status)
                        .text_xs()
                        .text_color(colors.muted_foreground)
                        .child("Reading the next bounded page…"),
                );
            }
            if let Some(error) = output_page_error {
                recovery = recovery.child(warning_alert("output-page-error", error, &colors));
            }
            pane = pane.child(recovery);
        }
        pane.child(
            div()
                .id("output-body")
                .test_support()
                .role(gpui_kit::Role::Label)
                .aria_label(tail.clone())
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .p_2()
                .text_xs()
                .font_family(mono)
                .text_size(theme.mono_font_size)
                .child(tail),
        )
        .into_any_element()
    }

    fn render_activity_tab(
        &mut self,
        colors: &gpui_kit::component::theme::ThemeColor,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let rows = inspector::activity_rows(&self.transcript, &self.restored_activity);
        let counts = ActivityCounts::from_rows(&rows);
        let mut body = div()
            .id("activity-list")
            .w_full()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .flex()
            .flex_col()
            .gap_2()
            .p_3();
        if self.restored_activity_history_incomplete {
            body = body.child(warning_alert("activity-history-warning", PARTIAL_ACTIVITY_HISTORY_MESSAGE, colors));
        }
        if rows.is_empty() {
            return body
                .child(empty_state(
                    "No activity yet",
                    "Follow tool calls and verification as VT Code works.",
                    IconName::Activity,
                ))
                .into_any_element();
        }
        let chart_data = [
            ActivityChartDatum {
                label: "Running",
                count: counts.running,
                state: ActivityState::Running,
            },
            ActivityChartDatum {
                label: "Done",
                count: counts.completed,
                state: ActivityState::Completed,
            },
            ActivityChartDatum {
                label: "Failed",
                count: counts.failed,
                state: ActivityState::Failed,
            },
            ActivityChartDatum {
                label: "Interrupted",
                count: counts.interrupted,
                state: ActivityState::Interrupted,
            },
            ActivityChartDatum {
                label: "Other",
                count: counts.other,
                state: ActivityState::Other,
            },
        ]
        .into_iter()
        .filter(|datum| datum.count > 0)
        .collect::<Vec<_>>();
        let summary = counts.summary();
        let chart_colors = *colors;
        body = body.child(
            div()
                .id("activity-status-chart")
                .test_support()
                .role(gpui_kit::Role::Group)
                .aria_label(format!("Activity statuses: {summary}"))
                .w_full()
                .h(px(148.))
                .flex_shrink_0()
                .child(
                    BarChart::new(chart_data)
                        .band(|datum| datum.label)
                        .value(|datum| datum.count as f64)
                        .label(|datum| datum.count.to_string())
                        .alignment(BarAlignment::Left)
                        .value_axis(false)
                        .grid(false)
                        .interactive(false)
                        .appear(false)
                        .fill(move |datum, _, _, _| match datum.state {
                            ActivityState::Running => chart_colors.primary,
                            ActivityState::Completed => chart_colors.success,
                            ActivityState::Failed => chart_colors.danger,
                            ActivityState::Interrupted => chart_colors.warning,
                            ActivityState::Other => chart_colors.muted_foreground,
                        }),
                ),
        );

        let activity_row_height = scale_row_height_for_ui_font(ACTIVITY_ROW_HEIGHT, cx.theme().font_size);
        let item_sizes = self.activity_row_sizes.get(rows.len(), activity_row_height);
        let row_colors = *colors;
        let weak_view = cx.entity().downgrade();
        body = body.child(
            div()
                .id("activity-rows")
                .relative()
                .w_full()
                .flex_1()
                .min_h_0()
                .overflow_hidden()
                .child(
                    v_virtual_list(cx.entity(), "activity-virtual-list", item_sizes, move |_, visible_range, _, _| {
                        visible_range
                            .map(|index| {
                                rows.get(index).map_or_else(
                                    || div().into_any_element(),
                                    |row| {
                                        let output_action = row.has_output.then(|| {
                                            let view = weak_view.clone();
                                            let output_id = row.id.clone();
                                            let output_title = row.title.to_string();
                                            desktop_button(format!("activity-output-{}", row.id))
                                                .icon(IconName::Terminal)
                                                .ghost()
                                                .small()
                                                .accessibility_label(format!("View output for {output_title}"))
                                                .tooltip("View captured output")
                                                .on_click(move |_, window, app| {
                                                    if let Some(view) = view.upgrade() {
                                                        view.update(app, |view, cx| {
                                                            view.select_activity_output(output_id.clone(), window, cx);
                                                        });
                                                    }
                                                })
                                                .into_any_element()
                                        });
                                        Self::render_activity_row(row, row_colors, activity_row_height, output_action)
                                            .into_any_element()
                                    },
                                )
                            })
                            .collect::<Vec<_>>()
                    })
                    .track_scroll(&self.activity_scroll),
                )
                .child(Scrollbar::vertical(&self.activity_scroll)),
        );
        body.into_any_element()
    }

    fn render_activity_status_indicator(
        id_prefix: &str,
        row_id: &str,
        state: ActivityState,
        color: gpui_kit::Hsla,
    ) -> AnyElement {
        if state == ActivityState::Running {
            div()
                .id(format!("{id_prefix}-{row_id}-spinner"))
                .test_support()
                .flex()
                .items_center()
                .justify_center()
                .child(Spinner::new().small().color(color))
                .into_any_element()
        } else {
            let icon = match state {
                ActivityState::Completed => IconName::Check,
                ActivityState::Failed | ActivityState::Interrupted => IconName::X,
                ActivityState::Running | ActivityState::Other => IconName::Activity,
            };
            Icon::new(icon).small().text_color(color).into_any_element()
        }
    }

    fn activity_status_color(state: ActivityState, colors: gpui_kit::component::theme::ThemeColor) -> gpui_kit::Hsla {
        match state {
            ActivityState::Running => colors.primary,
            ActivityState::Completed => colors.success,
            ActivityState::Failed => colors.danger,
            ActivityState::Interrupted => colors.warning,
            ActivityState::Other => colors.muted_foreground,
        }
    }

    fn render_activity_row(
        row: &ActivityRow,
        colors: gpui_kit::component::theme::ThemeColor,
        row_height: f32,
        output_action: Option<AnyElement>,
    ) -> impl IntoElement {
        let status_color = Self::activity_status_color(row.state, colors);
        let status = gpui_kit::SharedString::from(row.status.as_str());
        let mut row_element = div()
            .id(format!("activity-{}", row.id))
            .test_support()
            .role(gpui_kit::Role::Group)
            .aria_label(row.title.clone())
            .aria_description(status.clone())
            .w_full()
            .h(px(row_height))
            .flex()
            .items_center()
            .gap_3()
            .p_2()
            .rounded_lg()
            .border_1()
            .border_color(colors.border)
            .bg(colors.secondary)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(28.))
                    .flex_shrink_0()
                    .rounded_full()
                    .bg(colors.secondary)
                    .child(Self::render_activity_status_indicator(
                        "activity-indicator",
                        &row.id,
                        row.state,
                        status_color,
                    )),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(div().text_sm().font_medium().truncate().child(row.title.clone()))
                    .child(
                        div().flex().min_w_0().child(
                            Tag::custom(colors.secondary, colors.foreground, colors.border)
                                .small()
                                .rounded_full()
                                .max_w_full()
                                .child(div().truncate().child(status)),
                        ),
                    ),
            );
        if let Some(output_action) = output_action {
            row_element = row_element.child(output_action);
        }
        row_element
    }

    fn sync_transcript_layout(&mut self, cx: &mut Context<Self>) {
        use super::transcript::TranscriptLayoutChange;

        let changes = self.transcript.take_layout_changes();
        if changes.is_empty() {
            return;
        }

        self.message_scroller.update(cx, |state, cx| {
            for change in changes {
                match change {
                    TranscriptLayoutChange::Reset(item_count) => state.reset(item_count, cx),
                    TranscriptLayoutChange::Prepend(count) => {
                        let applied = state.prepend(count, cx);
                        debug_assert!(applied, "transcript and message scroller row counts diverged");
                    }
                    TranscriptLayoutChange::Splice { range, count } => {
                        let applied = state.splice(range, count, cx);
                        debug_assert!(applied, "transcript and message scroller row counts diverged");
                    }
                    TranscriptLayoutChange::Remeasure => state.remeasure(cx),
                    TranscriptLayoutChange::RemeasureItems(range) => {
                        let applied = state.remeasure_items(range, cx);
                        debug_assert!(applied, "transcript remeasurement range is out of bounds");
                    }
                    TranscriptLayoutChange::PauseFollowingTail => {
                        // Kit exposes scrolling but no pause-only operation. Scroll to the
                        // current row only when tail-following is active; a reader who has
                        // already scrolled away keeps the exact existing item offset.
                        if state.item_count() > 0 && state.is_following_tail() {
                            let anchor = state.logical_scroll_top();
                            let _scrolled = state.scroll_to_item(anchor.item_ix, cx);
                        }
                    }
                    TranscriptLayoutChange::FollowLatest => state.scroll_to_end(cx),
                }
            }
        });
    }

    fn render_transcript(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_transcript_layout(cx);
        let content = if self.transcript.is_empty() {
            let colors = cx.theme().colors;
            let welcome_copy = welcome_copy(self.welcome_copy_index);
            let greeting = welcome_greeting(self.welcome_period);
            let workspace_name = self
                .workspace
                .as_ref()
                .and_then(|path| path.file_name())
                .map(|name| name.to_string_lossy().into_owned());
            let prompt = welcome_prompt(
                &greeting,
                &welcome_copy.action,
                &welcome_copy.workspace_action,
                workspace_name.as_deref(),
            );
            let title = EmptyTitle::new()
                .text_size(gpui_kit::rems(1.75))
                .font_weight(gpui_kit::FontWeight::NORMAL)
                .text_color(colors.foreground)
                .child(
                    div()
                        .id("welcome-prompt")
                        .test_support()
                        .role(gpui_kit::Role::Heading)
                        .aria_label(prompt.clone())
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .justify_center()
                        .gap_2()
                        .child(if workspace_name.is_some() {
                            format!("{greeting}. {}", welcome_copy.workspace_action)
                        } else {
                            prompt
                        })
                        .when_some(workspace_name, |title, name| {
                            title.child(
                                desktop_button("welcome-workspace")
                                    .accessibility_label(format!("Change workspace, current: {name}"))
                                    .tooltip_with_action("Choose workspace", &ChooseWorkspace, Some("VTCodeDesktop"))
                                    .text()
                                    .text_color(colors.foreground)
                                    .h_auto()
                                    .p_0()
                                    .min_w_0()
                                    .max_w_full()
                                    .desktop_disabled(
                                        !self.workspace_switch_idle()
                                            || self.workspace_picker_open
                                            || self.retained_submission.is_some(),
                                    )
                                    .child(
                                        div()
                                            .min_w_0()
                                            .truncate()
                                            .text_size(gpui_kit::rems(1.75))
                                            .font_weight(gpui_kit::FontWeight::NORMAL)
                                            .border_b_1()
                                            .border_dashed()
                                            .border_color(colors.muted_foreground)
                                            .child(format!("{name}?")),
                                    )
                                    .dispatches_desktop_action(ChooseWorkspace, self.focus_handle.clone()),
                            )
                        }),
                );
            div()
                .id("welcome")
                .test_support()
                .role(gpui_kit::Role::Group)
                .aria_label("VT Code welcome")
                .size_full()
                .child(
                    EmptyState::new()
                        .header(
                            EmptyHeader::new()
                                .max_w(px(CONTENT_WIDTH))
                                .gap_6()
                                .media(EmptyMedia::new().mb_0().child(welcome_brand_mark(36., colors.primary)))
                                .title(title),
                        )
                        .size_full()
                        .when(self.workspace.is_none(), |welcome| {
                            welcome.content(
                                EmptyContent::new().child(
                                    desktop_button("open-first-workspace")
                                        .label(welcome_copy.call_to_action.clone())
                                        .accessibility_label(welcome_copy.call_to_action)
                                        .primary()
                                        .desktop_disabled(
                                            self.workspace_picker_open
                                                || !self.workspace_switch_idle()
                                                || self.retained_submission.is_some(),
                                        )
                                        .on_click(cx.listener(|view, _, window, cx| view.choose_workspace(window, cx))),
                                ),
                            )
                        }),
                )
                .into_any_element()
        } else {
            let remaining_history_capacity = self.transcript.remaining_capacity();
            let mut transcript_content = div().size_full().min_h_0().flex().flex_col();
            if self.history_has_earlier || self.history_page_loading || self.history_page_error.is_some() {
                let mut history_bar = div().w_full().flex().flex_col().items_center().gap_1().px_3().py_1();
                if let Some(error) = &self.history_page_error {
                    history_bar =
                        history_bar.child(warning_alert("history-page-error", error.clone(), &cx.theme().colors));
                }
                if self.history_has_earlier && remaining_history_capacity == 0 {
                    history_bar = history_bar.child(
                        div()
                            .id("history-page-limit")
                            .role(gpui_kit::Role::Status)
                            .text_xs()
                            .text_color(cx.theme().colors.muted_foreground)
                            .child("The 2,000-message local transcript limit has been reached."),
                    );
                } else if self.history_has_earlier {
                    history_bar = history_bar.child(
                        desktop_button("load-earlier-history")
                            .label(if self.history_page_loading {
                                "Loading earlier messages…"
                            } else {
                                "Load earlier messages"
                            })
                            .small()
                            .ghost()
                            .tooltip("Load the previous archived messages while keeping this position")
                            .desktop_disabled(self.history_page_loading || self.running)
                            .on_click(cx.listener(|view, _, _, cx| view.request_earlier_history(cx))),
                    );
                }
                transcript_content = transcript_content.child(history_bar);
            }
            let scroller = MessageScroller::new(
                "transcript-scroller",
                self.message_scroller.clone(),
                cx.processor(|view, index, _, cx| view.render_transcript_row(index, cx)),
            )
            .with_jump_button_label("Jump to latest message")
            .with_jump_button_renderer(|button| button.accessibility_label("Jump to latest message"))
            // Row spacing is applied by render_transcript_row so collapsed tool
            // placeholders do not each reserve the scroller's default 32 px gap.
            .with_row_style(gpui_kit::StyleRefinement::default().pb_0())
            .w_full()
            .flex_1()
            .min_h_0()
            .py_6()
            .into_any_element();
            transcript_content.child(scroller).into_any_element()
        };
        div()
            .id("transcript-scroll")
            .aria_label("Conversation")
            .role(gpui_kit::Role::Region)
            .test_support()
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_hidden()
            .child(content)
    }

    fn render_transcript_row(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(item) = self.transcript.get(index) else {
            return div().into_any_element();
        };
        if item.activity_grouped && !item.activity_group_head && !item.activity_expanded {
            return div().into_any_element();
        }
        let group_header = item
            .activity_group_head
            .then(|| self.transcript.activity_group(index))
            .flatten()
            .map(|range| self.render_activity_group(range, cx));
        let show_item = !item.activity_grouped || item.activity_expanded;
        let content = div()
            .id(format!("transcript-content-{}", item.id))
            .test_support()
            .w_full()
            .max_w(px(CONTENT_WIDTH))
            .min_w_0()
            .flex()
            .flex_col()
            .children(group_header)
            .when(show_item, |column| column.child(self.render_transcript_item(item, cx)));
        div()
            .id(format!("transcript-row-{}", item.id))
            .when(item.role == "tool" || item.role == "diff", |row| row.role(gpui_kit::Role::ListItem))
            .test_support()
            .w_full()
            .flex()
            .justify_center()
            // MessageScroller owns the shared horizontal row inset.
            .when(item.role == "tool", |row| row.pb_1())
            .when(item.role == "user", |row| row.pb_6())
            .when(item.role != "tool" && item.role != "user", |row| row.pb_3())
            .child(content)
            .into_any_element()
    }

    fn render_transcript_item(&self, item: &TranscriptItem, cx: &mut Context<Self>) -> AnyElement {
        if item.role == "tool" {
            return self.render_tool_item(item, cx);
        }
        if item.role == "diff" {
            return self.render_diff_item(item, cx);
        }

        let colors = cx.theme().colors;
        let user = item.role == "user";
        let label = if user { "You" } else { item.title.as_str() };
        let copy_text = item.text.clone();
        Message::new()
            .id(format!("message-{}", item.id))
            .role(gpui_kit::Role::ListItem)
            .alignment(if user {
                MessageAlignment::End
            } else {
                MessageAlignment::Start
            })
            .w_full()
            .max_w(px(CONTENT_WIDTH))
            .gap_1()
            .content(
                MessageContent::new().bubble(
                    Bubble::new()
                        .with_variant(if user {
                            BubbleVariant::Secondary
                        } else {
                            BubbleVariant::Ghost
                        })
                        .content(
                            BubbleContent::new()
                                .rounded(px(18.))
                                .when(user, |surface| {
                                    surface
                                        .border_color(colors.border)
                                        .bg(colors.secondary)
                                        .text_color(colors.foreground)
                                        .px_4()
                                        .py_2()
                                })
                                .child(
                                    div()
                                        .id(format!("message-body-{}", item.id))
                                        .role(gpui_kit::Role::Label)
                                        .aria_label(format!("{label}: {}", item.accessibility_text))
                                        .test_support()
                                        .child(render_markdown(
                                            format!("markdown-{}", item.id),
                                            item.markdown.clone(),
                                            item.role == "assistant",
                                            cx,
                                        )),
                                ),
                        ),
                ),
            )
            .when(!user, |message| {
                message.footer(
                    MessageFooter::new().child(
                        desktop_button(format!("copy-message-{}", item.id))
                            .icon(IconName::Copy)
                            .accessibility_label("Copy message")
                            .tooltip("Copy message")
                            .ghost()
                            .xsmall()
                            .on_click(move |_, window, app| {
                                app.write_to_clipboard(gpui_kit::ClipboardItem::new_string(copy_text.to_string()));
                                super::notifications::copied_to_clipboard(window, app);
                            }),
                    ),
                )
            })
            .into_any_element()
    }

    fn render_activity_group(&self, range: std::ops::Range<usize>, cx: &mut Context<Self>) -> AnyElement {
        let Some(head) = self.transcript.get(range.start) else {
            return div().into_any_element();
        };
        let id = head.id.clone();
        let summary = head.activity_group_summary;
        let failed = summary.failed;
        let running = summary.running;
        let colors = cx.theme().colors;
        let status = if failed > 0 {
            ActivityState::Failed
        } else if running > 0 {
            ActivityState::Running
        } else {
            ActivityState::Other
        };
        let status_color = Self::activity_status_color(status, colors);
        let mut label = format!("{} tool actions", summary.total);
        if running > 0 {
            label.push_str(&format!(" · {running} running"));
        }
        if failed > 0 {
            label.push_str(&format!(" · {failed} failed"));
        }
        desktop_button(format!("activity-group-{}", head.id))
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .justify_start()
                    .gap_2()
                    .child(Icon::new(IconName::Wrench).small())
                    .when(running > 0, |row| {
                        row.child(
                            div()
                                .id(format!("activity-group-running-indicator-{}", head.id))
                                .test_support()
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(
                                    Spinner::new()
                                        .small()
                                        .color(Self::activity_status_color(ActivityState::Running, colors)),
                                ),
                        )
                    })
                    .child(
                        div()
                            .id(format!("activity-group-label-{}", head.id))
                            .test_support()
                            .min_w_0()
                            .truncate()
                            .child(label.clone()),
                    )
                    .child(
                        Icon::new(if head.activity_expanded {
                            IconName::ChevronDown
                        } else {
                            IconName::ChevronRight
                        })
                        .small(),
                    ),
            )
            .accessibility_label(format!("{} {label}", if head.activity_expanded { "Collapse" } else { "Expand" }))
            .toggled(head.activity_expanded)
            .ghost()
            .small()
            .max_w_full()
            .justify_start()
            .text_color(status_color)
            .on_click(cx.listener(move |view, _, _, cx| {
                view.transcript.toggle_activity_group(&id);
                cx.notify();
            }))
            .into_any_element()
    }

    fn render_tool_item(&self, item: &TranscriptItem, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let id = item.id.clone();
        let status = if item.has_activity_status {
            item.text.lines().next().unwrap_or_default()
        } else {
            "Archived output"
        };
        let state = ActivityState::from_status(status);
        let status_color = Self::activity_status_color(state, colors);
        let mut panel = div().w_full().max_w(px(CONTENT_WIDTH)).flex().flex_col().gap_1().child(
            desktop_button(format!("tool-{}", item.id))
                .accessibility_label(format!(
                    "{} for {}, status: {status}",
                    if item.expanded { "Hide details" } else { "Show details" },
                    item.title
                ))
                .tooltip(format!("{} · {status}", item.title))
                .ghost()
                .small()
                .w_full()
                .justify_start()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .min_w_0()
                        .w_full()
                        .text_color(status_color)
                        .child(Self::render_activity_status_indicator("tool-indicator", &item.id, state, status_color))
                        .child(div().min_w_0().truncate().child(item.title.clone()))
                        .when(state != ActivityState::Completed, |row| {
                            row.child(div().min_w_0().truncate().text_xs().child(status.to_string()))
                        })
                        .child(
                            Icon::new(if item.expanded {
                                IconName::ChevronDown
                            } else {
                                IconName::ChevronRight
                            })
                            .small(),
                        ),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.transcript.toggle_tool(&id);
                    cx.notify();
                })),
        );
        if item.expanded {
            let output = item
                .output
                .as_ref()
                .filter(|output| !output.is_empty())
                .cloned()
                .unwrap_or_else(|| item.text.clone());
            let full_output = output.clone();
            let output_excerpt = inspector::output_tail(&output);
            panel = panel.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div().flex().justify_end().child(
                            desktop_button(format!("copy-tool-output-{}", item.id))
                                .icon(IconName::Copy)
                                .accessibility_label(format!("Copy full output for {}", item.title))
                                .tooltip("Copy full captured output")
                                .ghost()
                                .xsmall()
                                .on_click(move |_, window, app| {
                                    app.write_to_clipboard(gpui_kit::ClipboardItem::new_string(
                                        full_output.to_string(),
                                    ));
                                    super::notifications::copied_to_clipboard(window, app);
                                }),
                        ),
                    )
                    .child(
                        div()
                            .id(format!("tool-output-{}", item.id))
                            .role(gpui_kit::Role::Label)
                            .aria_label(output_excerpt.clone())
                            .test_support()
                            .ml_4()
                            .px_3()
                            .py_2()
                            .rounded_md()
                            .border_1()
                            .border_color(colors.border)
                            .bg(colors.background)
                            .font_family(cx.theme().mono_font_family.clone())
                            .text_size(cx.theme().mono_font_size)
                            .child(output_excerpt),
                    ),
            );
        }
        panel.into_any_element()
    }

    fn render_diff_item(&self, item: &TranscriptItem, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let diff_foreground =
            themes::diff_text_foreground(colors.background, colors.foreground, colors.success, colors.danger);
        let mono_font = cx.theme().mono_font_family.clone();
        let Some(diff) = item.diff.as_ref() else {
            return div().into_any_element();
        };
        let id = item.id.clone();
        let status_label = if diff.failed {
            "patch failed"
        } else if diff.incomplete {
            "preview incomplete"
        } else {
            "applied"
        };
        let toggle_id = id.clone();
        let mut stats = div().flex().items_center().gap_2().text_xs();
        if let Some(additions) = diff.additions {
            stats = stats.child(div().text_color(colors.success).child(format!("+{additions}")));
        }
        if let Some(deletions) = diff.deletions {
            stats = stats.child(div().text_color(colors.danger).child(format!("−{deletions}")));
        }

        let mut header = div()
            .w_full()
            .flex()
            .items_center()
            .justify_between()
            .px_3()
            .py_2()
            .bg(colors.secondary)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().font_medium().child(item.title.clone()))
                    .child(stats)
                    .child(
                        div()
                            .text_xs()
                            .text_color(if diff.failed {
                                colors.danger
                            } else {
                                colors.muted_foreground
                            })
                            .child(status_label),
                    ),
            )
            .child(
                desktop_button(format!("diff-{}", item.id))
                    .label(if item.expanded { "Hide diff" } else { "Show diff" })
                    .ghost()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.transcript.toggle_diff(&toggle_id);
                        cx.notify();
                    })),
            );

        if !diff.patch.is_empty() {
            let copy_id = id.clone();
            header = header.child(
                desktop_button(format!("copy-patch-{}", item.id))
                    .label("Copy patch")
                    .ghost()
                    .on_click(cx.listener(move |view, _, window, cx| {
                        let Some(patch) = view
                            .transcript
                            .items_newest_first()
                            .find(|candidate| candidate.id == copy_id)
                            .and_then(|candidate| candidate.diff.as_ref())
                            .map(|diff| diff.patch.clone())
                            .filter(|patch| !patch.is_empty())
                        else {
                            return;
                        };
                        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(patch));
                        super::notifications::copied_to_clipboard(window, cx);
                    })),
            );
        }

        let mut panel = div()
            .relative()
            .w_full()
            .max_w(px(CONTENT_WIDTH))
            .border_1()
            .border_color(colors.border)
            .rounded_md()
            .overflow_hidden()
            .child(header);

        let mut body = div().w_full().flex().flex_col().px_3().py_2().gap_1();
        for file in &diff.files {
            let (kind_label, kind_color) = match file.kind {
                DiffFileKind::Add => ("added", colors.success),
                DiffFileKind::Delete => ("deleted", colors.danger),
                DiffFileKind::Update => ("modified", colors.muted_foreground),
            };
            body = body.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().text_sm().font_family(mono_font.clone()).child(file.path.clone()))
                    .child(div().text_xs().text_color(kind_color).child(kind_label)),
            );
        }
        if diff.incomplete {
            body = body.child(
                div()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child("Some file previews were unavailable for this change set."),
            );
        }

        if item.expanded && !diff.patch.is_empty() {
            let mut lines = ClassifiedDiffLines::new(&diff.patch);
            let mut diff_view = div()
                .id(format!("diff-lines-{}", item.id))
                .w_full()
                .flex()
                .flex_col()
                .mt_2()
                .rounded_md()
                .overflow_hidden()
                .overflow_x_scroll()
                .bg(colors.background);
            for line in &mut lines {
                let (line_color, line_bg, marker_color) = match line.kind {
                    DiffLineKind::Added => (
                        diff_foreground,
                        themes::diff_line_background(colors.background, diff_foreground, colors.success),
                        Some(("+", "+", colors.success)),
                    ),
                    DiffLineKind::Removed => (
                        diff_foreground,
                        themes::diff_line_background(colors.background, diff_foreground, colors.danger),
                        Some(("-", "−", colors.danger)),
                    ),
                    DiffLineKind::Hunk => (colors.muted_foreground, colors.secondary, None),
                    DiffLineKind::Header | DiffLineKind::Context => (colors.foreground, colors.background, None),
                };
                let mut row = div()
                    .w_full()
                    .px_2()
                    .text_xs()
                    .font_family(mono_font.clone())
                    .whitespace_nowrap()
                    .bg(line_bg);
                if let Some((source_marker, marker, marker_color)) = marker_color {
                    let content = line.text.strip_prefix(source_marker).unwrap_or_default();
                    row = row
                        .flex()
                        .child(
                            div()
                                .w_4()
                                .flex_shrink_0()
                                .font_semibold()
                                .text_color(marker_color)
                                .child(marker),
                        )
                        .child(div().text_color(line_color).child(content.to_string()));
                } else {
                    row = row.text_color(line_color).child(line.text.to_string());
                }
                diff_view = diff_view.child(row);
            }
            if lines.truncated {
                diff_view = diff_view.child(
                    div()
                        .w_full()
                        .px_2()
                        .text_xs()
                        .text_color(colors.muted_foreground)
                        .child("Diff truncated after 2,000 lines. Use Copy patch for the full change."),
                );
            }
            body = body.child(diff_view);
        }
        panel = panel.child(body);
        panel.into_any_element()
    }

    fn render_composer(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors;
        let mut panel = div()
            .w_full()
            .max_w(px(CONTENT_WIDTH))
            .min_w_0()
            .flex()
            .flex_col()
            .gap_2()
            .px_4()
            .pb_4()
            .pt_2();
        if !self.attachments.is_empty() {
            panel = panel.child(div().w_full().flex().flex_wrap().gap_2().children(self.attachments.iter().map(
                |attachment| {
                    let path = attachment.path.clone();
                    Attachment::new()
                        .small()
                        .w(px(230.))
                        .media(AttachmentMedia::new().child(Icon::new(IconName::FileCode)))
                        .content(
                            AttachmentContent::new()
                                .title(AttachmentTitle::new(attachment.path.clone()))
                                .description(AttachmentDescription::new(format!(
                                    "{} bytes · workspace file",
                                    attachment.text.len()
                                ))),
                        )
                        .actions(
                            AttachmentActions::new().child(
                                desktop_button(opaque_path_element_id("remove-attachment", &path))
                                    .icon(IconName::X)
                                    .accessibility_label(format!("Remove {}", attachment.path))
                                    .ghost()
                                    .xsmall()
                                    .on_click(
                                        cx.listener(move |this, _, _, cx| this.remove_attachment(path.clone(), cx)),
                                    ),
                            ),
                        )
                },
            )));
        }
        let input = Textarea::new(&self.composer)
            .accessibility_id("composer")
            .aria_label("Message to VT Code")
            .cursor_text()
            .min_h(px(32.));

        if let Some(draft) = self.retained_submission.as_ref() {
            let prompt = draft.prompt.clone();
            let explanation = if draft.delivery_uncertain {
                "Delivery was not confirmed. Check session history before resending. Restore this draft or copy it before dismissing."
            } else {
                "This message was not submitted. Restore this draft or copy it before dismissing."
            };
            panel = panel.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(explanation)
                    .child("Clear the current draft and attachments to restore.")
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(
                                desktop_button("restore-submission")
                                    .label("Restore draft")
                                    .desktop_disabled(
                                        !self.composer.read(cx).value().is_empty()
                                            || !self.attachments.is_empty()
                                            || self.attachments_picker_open
                                            || self.session_request.is_some(),
                                    )
                                    .on_click(cx.listener(|view, _, window, cx| view.restore_submission(window, cx))),
                            )
                            .child(desktop_button("copy-retained-prompt").label("Copy prompt").ghost().on_click(
                                move |_, window, cx| {
                                    cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(prompt.clone()));
                                    super::notifications::copied_to_clipboard(window, cx);
                                },
                            ))
                            .child(desktop_button("dismiss-retained-prompt").label("Dismiss").ghost().on_click(
                                cx.listener(|view, _, _, cx| {
                                    view.retained_submission = None;
                                    cx.notify();
                                }),
                            )),
                    ),
            );
        }

        if let Some(error) = self.error.as_ref() {
            panel = panel.child(error_alert("composer-error", error.clone(), &colors));
        }
        if let Some(interaction) = self.interaction.clone() {
            panel = panel.child(self.render_interaction(&interaction, window, cx));
        }

        let can_submit = self.session_change_idle()
            && self.workspace_permission_ready()
            && !self.attachments_picker_open
            && self.retained_submission.is_none()
            && !self.composer.read(cx).value().trim().is_empty();
        let current_model = self
            .configuration
            .as_ref()
            .map(|configuration| {
                self.models
                    .iter()
                    .find(|model| model.provider == configuration.provider && model.id == configuration.model)
                    .map(|model| model.display_name.clone())
                    .unwrap_or_else(|| configuration.model.clone())
            })
            .unwrap_or_else(|| "Choose model".to_string());
        let policy = self
            .configuration
            .as_ref()
            .and_then(|config| {
                config.session_options.as_ref().map(|options| match options.permission_mode {
                    protocol::PermissionMode::Ask => "Ask before protected actions",
                    protocol::PermissionMode::AutoApproveCommands => "Auto-approve commands",
                })
            })
            .or_else(|| self.configuration.as_ref().and_then(|config| config.sandbox.as_deref()))
            .unwrap_or("Permissions unavailable");
        let policy_label = match policy {
            "Ask before protected actions" => "Ask",
            "Auto-approve commands" => "Auto",
            "Permissions unavailable" => "Policy",
            policy => policy,
        };
        let permission_mode = self
            .configuration
            .as_ref()
            .and_then(|config| config.session_options.as_ref())
            .map(|options| options.permission_mode);
        let (permission_icon, permission_icon_color) =
            composer_controls::permission_indicator(permission_mode, cx.theme().colors);
        let buttons = div()
            .w_full()
            .min_w_0()
            .flex()
            .flex_wrap()
            .items_center()
            .justify_between()
            .gap_2()
            .when(self.zen_mode, |buttons| buttons.justify_end())
            .when(!self.zen_mode, |buttons| {
                buttons.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .min_w_0()
                        .child(
                            desktop_button("attach-files")
                                .icon(IconName::Plus)
                                .accessibility_label("Add files and more @")
                                .tooltip_with_action("Add files and more @", &OpenAddContext, Some("VTCodeDesktop"))
                                .ghost()
                                .small()
                                .desktop_disabled(
                                    self.workspace.is_none()
                                        || self.attachments_picker_open
                                        || self.session_request.is_some(),
                                )
                                .dispatches_desktop_action(OpenAddContext, self.focus_handle.clone()),
                        )
                        .child(
                            desktop_button("composer-policy")
                                .accessibility_label(format!("Execution policy: {policy}"))
                                .tooltip_with_action("Command permissions", &OpenPermissions, Some("VTCodeDesktop"))
                                .icon(Icon::new(permission_icon).text_color(permission_icon_color))
                                .child(
                                    div()
                                        .id("composer-policy-label")
                                        .test_support()
                                        .truncate()
                                        .child(policy_label.to_string()),
                                )
                                .dropdown_caret(true)
                                .ghost()
                                .small()
                                .min_w_0()
                                .max_w(px(112.))
                                .desktop_disabled(!self.can_select_session_options())
                                .dispatches_desktop_action(OpenPermissions, self.focus_handle.clone()),
                        ),
                )
            });
        let model_options = self.configuration.as_ref().and_then(|config| config.session_options.as_ref());
        let selected_effort = model_options
            .filter(|options| options.supported_efforts.contains(&options.reasoning_effort))
            .map(|options| composer_controls::runtime_option_label(&options.reasoning_effort));
        let selected_speed = model_options
            .and_then(|options| options.speed.as_deref())
            .map(composer_controls::runtime_option_label);
        let model_summary = std::iter::once(current_model.as_str())
            .chain(selected_effort.as_deref())
            .chain(selected_speed.as_deref())
            .collect::<Vec<_>>()
            .join(" · ");
        let model_label = div()
            .min_w_0()
            .flex()
            .items_center()
            .gap_1()
            .child(div().min_w_0().max_w(px(176.)).truncate().child(current_model))
            .when_some(selected_effort, |row, effort| {
                row.child(
                    div()
                        .id("composer-model-effort")
                        .test_support()
                        .role(gpui_kit::Role::Label)
                        .aria_label(format!("Reasoning effort: {effort}"))
                        .flex_shrink_0()
                        .text_color(colors.muted_foreground)
                        .child(format!("· {effort}")),
                )
            })
            .when_some(selected_speed, |row, speed| row.child(composer_controls::processing_tier_badge(&speed, cx)));
        let mut model_actions =
            div()
                .min_w_0()
                .max_w_full()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_2()
                .when(!self.zen_mode, |actions| {
                    actions.child(
                        desktop_button("pick-model")
                            .accessibility_label(model_summary)
                            .child(model_label)
                            .dropdown_caret(true)
                            .small()
                            .max_w(px(336.))
                            .overflow_hidden()
                            .tooltip_with_action(
                                "Choose a model · Search by provider, name, or model ID",
                                &OpenModelPicker,
                                Some("VTCodeDesktop"),
                            )
                            .ghost()
                            .desktop_disabled(!self.session_change_allowed(None) || self.attachments_picker_open)
                            .dispatches_desktop_action(OpenModelPicker, self.focus_handle.clone()),
                    )
                });
        if self.running {
            model_actions = model_actions.child(
                desktop_button("cancel-turn")
                    .tooltip_with_action("Cancel active run", &CancelRun, Some("VTCodeDesktop"))
                    .accessibility_label("Cancel run")
                    .icon(IconName::Square)
                    .rounded_full()
                    .danger()
                    .dispatches_desktop_action(CancelRun, self.focus_handle.clone()),
            );
        } else {
            model_actions = model_actions.child(
                desktop_button("send-prompt")
                    .accessibility_label("Send message")
                    .tooltip("Send message (Enter)")
                    .icon(IconName::ArrowUp)
                    .rounded_full()
                    .primary()
                    .desktop_disabled(!can_submit)
                    .on_click(cx.listener(|this, _, window, cx| this.submit_prompt(window, cx))),
            );
        }
        let buttons = buttons.child(model_actions);
        if self.palette_kind.is_composer_popup() {
            if let Some(state) = self.palette.clone() {
                if matches!(self.palette_kind, PaletteKind::Models | PaletteKind::ModelOptions) {
                    panel = panel.child(
                        div()
                            .absolute()
                            .top(gpui_kit::rems(0.5))
                            .right(gpui_kit::rems(1.))
                            .w_0()
                            .h_0()
                            .child(self.render_command_palette(state, window, cx)),
                    );
                } else {
                    panel = panel.child(self.render_command_palette(state, window, cx));
                }
            }
        }
        if !self.zen_mode {
            panel = panel.child(
                div()
                    .id("composer-context")
                    .test_support()
                    .w_full()
                    .min_w_0()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .py_1()
                    .child(self.render_workspace_picker(cx))
                    .child(
                        desktop_button("composer-cli-webmcp")
                            .icon(IconName::Terminal)
                            .label(if self.webmcp_status.as_ref().is_some_and(|status| status.running) {
                                "WebMCP connected"
                            } else {
                                "Connect WebMCP"
                            })
                            .accessibility_label("Open VT Code CLI and WebMCP connection settings")
                            .tooltip("Pair the browser app with this CLI-backed desktop session")
                            .ghost()
                            .small()
                            .on_click(cx.listener(|view, _, window, cx| view.open_webmcp_settings(window, cx))),
                    )
                    .child(self.render_mode_button(cx)),
            );
        }
        div().w_full().flex().justify_center().child(
            panel.child(
                div()
                    .w_full()
                    .on_action(cx.listener(|view, _: &Enter, window, cx| view.handle_composer_enter(window, cx)))
                    .capture_action(cx.listener(|view, _: &MoveUp, window, cx| {
                        if view.move_composer_suggestion(false, window, cx) {
                            cx.stop_propagation();
                        } else {
                            cx.propagate();
                        }
                    }))
                    .capture_action(cx.listener(|view, _: &MoveDown, window, cx| {
                        if view.move_composer_suggestion(true, window, cx) {
                            cx.stop_propagation();
                        } else {
                            cx.propagate();
                        }
                    }))
                    .capture_action(cx.listener(|view, _: &IndentInline, window, cx| {
                        if !view.accept_composer_suggestion(window, cx) {
                            window.focus_next(cx);
                        }
                        cx.stop_propagation();
                    }))
                    .on_key_up(cx.listener(|view, event: &KeyUpEvent, window, cx| {
                        if matches!(event.keystroke.key.as_str(), "left" | "right" | "up" | "down" | "home" | "end") {
                            view.refresh_composer_suggestions_after_input_navigation(window, cx);
                        }
                    }))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|view, _: &MouseUpEvent, window, cx| {
                            view.refresh_composer_suggestions_after_input_navigation(window, cx);
                        }),
                    )
                    .child(
                        div()
                            .id("message-composer-frame")
                            .test_support()
                            .w_full()
                            .min_w_0()
                            .rounded(px(22.))
                            .border_1()
                            .border_color(colors.border)
                            .bg(colors.popover)
                            .child(
                                InputGroup::new("message-composer")
                                    .aria_label("Message composer")
                                    .w_full()
                                    .rounded(px(22.))
                                    .border_0()
                                    .focus_ring(false)
                                    .p_2()
                                    .bg(colors.popover)
                                    .input(input)
                                    .addon(
                                        InputGroupAddon::new("composer-actions")
                                            .align(InputGroupAddonAlignment::BlockEnd)
                                            .child(buttons),
                                    ),
                            ),
                    ),
            ),
        )
    }

    /// Full-width bottom bar: workspace, git branch, provider · model, and
    /// the live turn status.
    fn toggle_status_bar(&mut self, cx: &mut Context<Self>) {
        self.preferences.show_status_bar = !self.preferences.show_status_bar;
        self.save_preferences();
        cx.notify();
    }

    fn set_reduced_motion(&mut self, preference: preferences::ReducedMotionPreference, cx: &mut Context<Self>) {
        if self.preferences.reduced_motion == preference && preference != preferences::ReducedMotionPreference::System {
            return;
        }
        self.preferences.reduced_motion = preference;
        preferences::apply_reduced_motion(preference, self.system_motion_baseline, cx);
        self.save_preferences();
        cx.notify();
    }

    fn render_status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors;
        let workspace = self
            .workspace
            .as_ref()
            .and_then(|path| path.file_name())
            .and_then(|name| name.to_str())
            .unwrap_or("No workspace");
        let agent = if self.running {
            ("Agent: running", colors.primary)
        } else {
            ("Agent: idle", colors.muted_foreground)
        };
        let configuration = self.configuration.as_ref();
        let provider_model = configuration
            .map(|config| format!("{} · {}", config.provider, config.model))
            .unwrap_or_else(|| "No provider configured".to_string());
        let sandbox = configuration.and_then(|config| config.sandbox.clone());
        let context_percent = configuration
            .and_then(|config| config.context_window)
            .zip(self.last_usage.as_ref())
            .and_then(|(window, usage)| context_usage_percent(usage, window));
        let workspace_details = div()
            .flex()
            .items_center()
            .gap_3()
            .min_w_0()
            .child(div().truncate().child(workspace.to_string()))
            .when(self.git_branch.is_some(), |bar| {
                bar.child(
                    div()
                        .truncate()
                        .child(format!("Branch: {}", self.git_branch.clone().unwrap_or_default())),
                )
            })
            .when(sandbox.is_some(), |bar| {
                bar.child(div().truncate().child(format!("Sandbox: {}", sandbox.unwrap_or_default())))
            });
        let runtime_details = div()
            .flex()
            .items_center()
            .gap_3()
            .min_w_0()
            .when(context_percent.is_some(), |bar| {
                bar.child(div().child(format!("Context: {}%", context_percent.unwrap_or_default())))
            })
            .child(div().truncate().child(provider_model))
            .child(
                div()
                    .id("agent-status")
                    .test_support()
                    .role(gpui_kit::Role::Status)
                    .aria_label(agent.0)
                    .child(
                        Marker::new()
                            .loading(self.running)
                            .text_xs()
                            .text_color(agent.1)
                            .w_auto()
                            .content(MarkerContent::new().text(agent.0)),
                    ),
            );

        div()
            .id("status-bar")
            .test_support()
            .role(gpui_kit::Role::Group)
            .aria_label("Status bar")
            .w_full()
            .flex_shrink_0()
            .child(
                StatusBar::new()
                    .left(workspace_details)
                    .right(runtime_details)
                    .w_full()
                    .h(px(26.))
                    .gap_3()
                    .px_3()
                    .py_0()
                    .border_color(colors.border)
                    .bg(colors.background)
                    .text_xs()
                    .text_color(colors.muted_foreground),
            )
    }

    fn toggle_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let target_mode = if self.theme_mode == ThemeMode::Light {
            ThemeMode::Dark
        } else {
            ThemeMode::Light
        };
        self.apply_theme_mode(target_mode, window, cx);
    }

    fn apply_theme_mode(&mut self, target_mode: ThemeMode, window: &mut Window, cx: &mut Context<Self>) {
        let id = if self.preferences.separate_theme_profiles {
            let candidate = match target_mode {
                ThemeMode::Light => self.preferences.light_theme.as_deref(),
                ThemeMode::Dark => self.preferences.dark_theme.as_deref(),
            };
            candidate
                .filter(|id| themes::mode_for_id(id) == Some(target_mode))
                .unwrap_or_else(|| themes::default_id(target_mode))
        } else {
            themes::system_id(target_mode)
        };
        self.apply_theme(id.to_owned(), window, cx);
    }

    fn toggle_separate_theme_profiles(&mut self, cx: &mut Context<Self>) {
        let enabled = !self.preferences.separate_theme_profiles;
        if enabled {
            let current_theme = self.theme_id.clone();
            self.preferences.light_theme = Some(if self.theme_mode == ThemeMode::Light {
                current_theme.clone()
            } else {
                themes::default_id(ThemeMode::Light).to_string()
            });
            self.preferences.dark_theme = Some(if self.theme_mode == ThemeMode::Dark {
                current_theme
            } else {
                themes::default_id(ThemeMode::Dark).to_string()
            });
            self.preferences.follow_cli_theme = Some(false);
        }
        self.preferences.set_separate_theme_profiles(enabled, self.theme_mode);
        self.save_preferences();
        cx.notify();
    }

    /// Apply a theme from the picker and remember the choice.
    fn apply_theme(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.preferences.follow_cli_theme = Some(false);
        self.apply_theme_state(id, Some(window), cx);
    }

    fn apply_theme_state(&mut self, id: String, window: Option<&mut Window>, cx: &mut Context<Self>) {
        self.theme_mode = themes::apply_by_id(&id, window, cx);
        let id = if themes::mode_for_id(&id) == Some(self.theme_mode) {
            id
        } else {
            themes::default_id(self.theme_mode).to_string()
        };
        if self.preferences.separate_theme_profiles {
            match self.theme_mode {
                ThemeMode::Light => self.preferences.light_theme = Some(id.clone()),
                ThemeMode::Dark => self.preferences.dark_theme = Some(id.clone()),
            }
        }
        self.theme_id = id;
        self.apply_typography(cx);
        self.transcript.remeasure();
        self.save_preferences();
        cx.notify();
    }

    fn apply_typography(&mut self, cx: &mut Context<Self>) {
        let typography = self.preferences.typography_for(self.theme_mode);
        let ui = typography.ui_font_size;
        let code = typography.code_font_size;
        let installed = typography::installed_fonts(cx);
        let ui_family = typography::resolve_font(typography.ui_font_family, ".SystemUIFont", &installed);
        let code_family =
            typography::resolve_font(typography.code_font_family, self.default_code_font.as_str(), &installed);
        Theme::update(cx, |theme| {
            theme.font_family = ui_family.into();
            theme.mono_font_family = code_family.into();
            theme.font_size = px(f32::from(ui));
            theme.mono_font_size = px(f32::from(code));
        });
        self.transcript.remeasure();
        cx.notify();
    }

    fn render_reduced_motion(&self, cx: &Context<Self>) -> AnyElement {
        let current = self.preferences.reduced_motion;
        let colors = cx.theme().colors;
        div()
            .id("reduced-motion-setting")
            .test_support()
            .role(gpui_kit::Role::Group)
            .aria_label("Reduce motion")
            .w_full()
            .flex()
            .items_center()
            .justify_between()
            .gap_3()
            .child(
                div().min_w_0().flex_1().flex().flex_col().gap_1().child("Reduce motion").child(
                    div()
                        .text_xs()
                        .text_color(colors.muted_foreground)
                        .child("System uses macOS accessibility settings; select it again to refresh."),
                ),
            )
            .child(
                div()
                    .flex()
                    .gap_1()
                    .children(preferences::ReducedMotionPreference::ALL.into_iter().map(|preference| {
                        let label = preference.label();
                        let selected = preference == current;
                        desktop_button(format!("reduce-motion-{}", preference.id()))
                            .label(label)
                            .accessibility_label(format!(
                                "Reduce motion: {label}{}",
                                if selected { " (selected)" } else { "" }
                            ))
                            .tooltip(match preference {
                                preferences::ReducedMotionPreference::System => "Re-read system accessibility settings",
                                preferences::ReducedMotionPreference::On => "Reduce interface animations",
                                preferences::ReducedMotionPreference::Off => "Allow interface animations",
                            })
                            .ghost()
                            .when(selected, |button| button.bg(colors.secondary).text_color(colors.foreground))
                            .on_click(cx.listener(move |view, _, _, cx| view.set_reduced_motion(preference, cx)))
                    })),
            )
            .into_any_element()
    }

    fn render_font_size(&self, code: bool, cx: &Context<Self>) -> AnyElement {
        let (key, label, description, size, minimum, maximum) = if code {
            (
                "code",
                "Code font size",
                "Code, diffs, and terminal output",
                self.preferences.typography_for(self.theme_mode).code_font_size,
                10,
                24,
            )
        } else {
            (
                "ui",
                "UI font size",
                "Base size for controls and conversation text",
                self.preferences.typography_for(self.theme_mode).ui_font_size,
                12,
                24,
            )
        };
        div()
            .w_full()
            .flex()
            .items_center()
            .justify_between()
            .gap_3()
            .py_2()
            .child(
                div().flex_1().min_w_0().flex().flex_col().gap_1().child(label).child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().colors.muted_foreground)
                        .child(description),
                ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .children([-1i16, 1].into_iter().enumerate().map(|(index, delta)| {
                        desktop_button(format!("font-{key}-{index}"))
                            .label(if delta < 0 { "−" } else { "+" })
                            .accessibility_label(format!("{} {label}", if delta < 0 { "Decrease" } else { "Increase" }))
                            .ghost()
                            .desktop_disabled(if delta < 0 { size <= minimum } else { size >= maximum })
                            .on_click(cx.listener(move |view, _, _, cx| {
                                let current = if code {
                                    view.preferences.typography_for(view.theme_mode).code_font_size
                                } else {
                                    view.preferences.typography_for(view.theme_mode).ui_font_size
                                };
                                let next = if delta < 0 {
                                    current.saturating_sub(1)
                                } else {
                                    current.saturating_add(1)
                                }
                                .clamp(minimum, maximum);
                                if code {
                                    view.preferences.set_code_font_size(view.theme_mode, next);
                                } else {
                                    view.preferences.set_ui_font_size(view.theme_mode, next);
                                }
                                view.apply_typography(cx);
                                view.save_preferences();
                            }))
                    }))
                    .child(
                        div()
                            .id(format!("font-{key}-value"))
                            .role(gpui_kit::Role::Label)
                            .aria_label(format!("{label}: {size} pixels"))
                            .test_support()
                            .text_sm()
                            .child(format!("{size} px")),
                    ),
            )
            .into_any_element()
    }

    /// One theme entry in the settings picker: a color preview chip that
    /// applies live on click.
    fn render_theme_swatch(&self, entry: themes::SwatchEntry, cx: &mut Context<Self>) -> AnyElement {
        let (background, foreground) = entry.preview();
        let entry_id = entry.id();
        let active = self.theme_id == entry_id;
        let light_profile = self.preferences.separate_theme_profiles
            && self.preferences.light_theme.as_deref() == Some(entry_id.as_str());
        let dark_profile = self.preferences.separate_theme_profiles
            && self.preferences.dark_theme.as_deref() == Some(entry_id.as_str());
        let badge = if active {
            "active"
        } else if light_profile && dark_profile {
            "light and dark profiles"
        } else if light_profile {
            "light profile"
        } else if dark_profile {
            "dark profile"
        } else {
            ""
        };
        let selected = !badge.is_empty();
        let id = format!("theme-{entry_id}");
        let mut swatch = div()
            .w_full()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .h(px(40.))
                    .rounded_md()
                    .border_1()
                    .border_color(cx.theme().colors.border)
                    .bg(background)
                    .p_1()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .when_some(entry.accent(), |chip, accent| chip.child(div().h(px(6.)).rounded_sm().bg(accent)))
                    .child(div().h(px(4.)).w_3_4().rounded_sm().bg(foreground))
                    .child(div().h(px(4.)).w_1_2().rounded_sm().bg(foreground.opacity(0.6))),
            )
            .child(
                div()
                    .text_xs()
                    .truncate()
                    .when(selected, |label| label.font_semibold())
                    .child(entry.label()),
            );
        if selected {
            swatch = swatch.child(div().text_xs().text_color(cx.theme().colors.primary).child(badge));
        }
        desktop_button(id)
            .accessibility_label(if selected {
                format!("{} ({badge})", entry.label())
            } else {
                entry.label().to_owned()
            })
            .toggled(selected)
            .ghost()
            .w(px(104.))
            .flex_shrink_0()
            .h(px(96.))
            .p_2()
            .border_1()
            .border_color(if selected {
                cx.theme().colors.primary
            } else {
                cx.theme().colors.border
            })
            .rounded_lg()
            .on_click(cx.listener(move |this, _, window, cx| {
                let theme_id = entry.id();
                this.apply_theme(theme_id, window, cx);
            }))
            .child(swatch)
            .into_any_element()
    }

    fn render_interaction(
        &self,
        interaction: &InteractionRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = cx.theme().colors;
        let submitting = self.pending_interaction_response.as_deref() == Some(interaction.interaction_id.as_str());
        let mut panel = div()
            .id("runtime-interaction")
            .test_support()
            .role(gpui_kit::Role::Group)
            .aria_label(interaction.title.clone())
            .track_focus(&self.interaction_focus)
            .w_full()
            .max_w(px(CONTENT_WIDTH))
            .flex()
            .flex_col()
            .gap_2()
            .border_1()
            .border_color(colors.warning)
            .rounded_md()
            .p_4()
            .bg(colors.warning.opacity(0.08))
            .child(div().font_semibold().child(interaction.title.clone()));
        for detail in &interaction.details {
            panel = panel.child(div().text_sm().child(detail.clone()));
        }
        if submitting {
            panel = panel.child(
                div()
                    .id("interaction-response-status")
                    .test_support()
                    .role(gpui_kit::Role::Status)
                    .aria_label("Sending decision to VT Code")
                    .text_sm()
                    .child("Sending decision to VT Code…"),
            );
        }

        if interaction.kind == protocol::InteractionKind::Question && interaction.steps.len() > 1 {
            let step_index = self.interaction_step_index.min(interaction.steps.len().saturating_sub(1));
            let steps = interaction.steps.iter().enumerate().map(|(index, step)| {
                let title = if step.title.trim().is_empty() {
                    format!("Question {}", index + 1)
                } else {
                    step.title.clone()
                };
                StepperItem::new().child(div().min_w_0().max_w_full().truncate().child(title))
            });
            panel = panel.child(
                div()
                    .id("interaction-question-progress")
                    .test_support()
                    .role(gpui_kit::Role::Group)
                    .aria_label("Question progress")
                    .w_full()
                    .child(
                        div()
                            .id("interaction-question-stepper")
                            .test_support()
                            .role(gpui_kit::Role::List)
                            .aria_label("Question steps")
                            .w_full()
                            .child(
                                Stepper::new("interaction-question-steps")
                                    .small()
                                    .w_full()
                                    .selected_index(step_index)
                                    .disabled(true)
                                    .items(steps),
                            ),
                    ),
            );
        }

        if interaction.kind == protocol::InteractionKind::Question && !interaction.steps.is_empty() {
            let step_index = self.interaction_step_index.min(interaction.steps.len().saturating_sub(1));
            if let Some(step) = interaction.steps.get(step_index) {
                panel = panel.child(self.render_question_step(step, step_index, interaction.steps.len(), window, cx));
            }
        } else {
            let choices = interaction
                .steps
                .first()
                .map(|step| step.choices.as_slice())
                .unwrap_or(interaction.choices.as_slice());
            for choice in choices {
                panel = panel.child(self.render_choice(choice, 0, window, cx));
            }
        }

        panel.child(
            div()
                .w_full()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    desktop_button("cancel-interaction")
                        .label("Cancel request")
                        .ghost()
                        .desktop_disabled(submitting)
                        .on_click(
                            cx.listener(|this, _, window, cx| this.submit_interaction(Vec::new(), true, window, cx)),
                        ),
                )
                .when(
                    interaction.kind == protocol::InteractionKind::Question && !interaction.steps.is_empty(),
                    |row| {
                        row.child(
                            div()
                                .flex()
                                .flex_wrap()
                                .gap_2()
                                .when(self.interaction_step_index > 0, |row| {
                                    row.child(
                                        desktop_button("previous-question")
                                            .label("Previous")
                                            .icon(IconName::ChevronLeft)
                                            .ghost()
                                            .desktop_disabled(submitting)
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.show_interaction_step(
                                                    this.interaction_step_index.saturating_sub(1),
                                                    window,
                                                    cx,
                                                );
                                            })),
                                    )
                                })
                                .child(
                                    desktop_button("continue-question")
                                        .label(if self.interaction_step_index + 1 == interaction.steps.len() {
                                            "Submit answers"
                                        } else {
                                            "Next question"
                                        })
                                        .primary()
                                        .desktop_disabled(submitting)
                                        .on_click(
                                            cx.listener(|this, _, window, cx| {
                                                this.advance_interaction_step(window, cx)
                                            }),
                                        ),
                                ),
                        )
                    },
                ),
        )
    }

    fn render_question_step(
        &self,
        step: &InteractionStep,
        index: usize,
        total: usize,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = cx.theme().colors;
        let selected_choice_id = self.interaction_choice_ids.get(index).and_then(Option::as_deref);
        let selected_choice_index =
            selected_choice_id.and_then(|selected| step.choices.iter().position(|choice| choice.id == selected));
        let choice_radios = step.choices.iter().enumerate().map(|(choice_index, choice)| {
            let label = if choice.recommended {
                format!("{} (Recommended)", choice.label)
            } else {
                choice.label.clone()
            };
            let selected = selected_choice_index == Some(choice_index);
            let mut radio = Radio::new(format!("interaction-question-choice-{choice_index}"))
                .label(label.clone())
                .accessibility_label(label)
                .checked(selected)
                .disabled(self.pending_interaction_response.is_some())
                .w_full()
                .p_3()
                .rounded_md()
                .border_1()
                .border_color(if selected {
                    colors.primary.opacity(0.6)
                } else {
                    colors.border
                })
                .bg(if selected {
                    colors.primary.opacity(0.08)
                } else {
                    colors.popover
                });
            let choice_id = choice.id.clone();
            radio = radio
                .aria_position_in_set(choice_index + 1)
                .aria_size_of_set(step.choices.len())
                .on_change(cx.listener(move |this, checked: &bool, window, cx| {
                    if *checked {
                        this.choose_interaction_option(index, choice_id.clone(), window, cx);
                    }
                }));
            if let Some(description) = choice.description.as_ref() {
                radio = radio.child(div().text_sm().text_color(colors.muted_foreground).child(description.clone()));
            }
            radio
        });
        let options = div()
            .id("interaction-question-options")
            .test_support()
            .role(gpui_kit::Role::RadioGroup)
            .aria_label(step.question.clone())
            .w_full()
            .flex()
            .flex_col()
            .gap_2()
            .children(choice_radios);
        let mut content = div()
            .w_full()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .id("question-step-counter")
                    .test_support()
                    .role(gpui_kit::Role::Status)
                    .aria_label(format!("Question {} of {total}", index + 1))
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child(format!("Question {} of {total}", index + 1)),
            )
            .child(div().font_medium().child(step.question.clone()));
        if !step.choices.is_empty() {
            content = content.child(options);
        }
        if step.allow_freeform {
            content = content.child(
                div()
                    .w_full()
                    .border_1()
                    .border_color(colors.border)
                    .rounded_sm()
                    .bg(colors.popover)
                    .p_2()
                    .child(
                        Textarea::new(&self.question_answer)
                            .accessibility_id("question-answer")
                            .aria_label("Your answer")
                            .cursor_text()
                            .disabled(self.pending_interaction_response.is_some())
                            .bg(colors.popover)
                            .h(px(72.)),
                    ),
            );
        }
        content.into_any_element()
    }

    fn render_choice(
        &self,
        choice: &InteractionChoice,
        step_index: usize,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let choice_id = choice.id.clone();
        let label = if choice.recommended {
            format!("{} (Recommended)", choice.label)
        } else {
            choice.label.clone()
        };
        let selected = self
            .interaction
            .as_ref()
            .is_some_and(|interaction| interaction.kind == protocol::InteractionKind::Question)
            && self.interaction_choice_ids.get(step_index).and_then(Option::as_deref) == Some(choice.id.as_str());
        let mut button = desktop_button(format!("choice-{}", choice.id))
            .accessibility_label(if selected {
                format!("{label} (Selected)")
            } else {
                label.clone()
            })
            .label(label)
            .w_full()
            .ghost()
            .desktop_disabled(self.pending_interaction_response.is_some())
            .selected(selected)
            .when(selected, |button| button.icon(IconName::Check));
        if let Some(description) = choice.description.as_ref() {
            button = button.tooltip(description.clone());
        }
        div().w_full().child(button.on_click(cx.listener(move |this, _, window, cx| {
            this.choose_interaction_option(step_index, choice_id.clone(), window, cx);
        })))
    }

    /// Modal ⌘K palette. The state entity is created on open, never during
    /// render; confirm and cancel run through the owning view.
    fn render_command_palette(
        &self,
        state: Entity<CommandState>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = cx.theme().colors;
        let ui_font_size = cx.theme().font_size;
        let popup_margin = ui_font_size * 0.75;
        let viewport_width = window.viewport_size().width;
        let max_popup_width = if viewport_width > popup_margin * 2. {
            viewport_width - popup_margin * 2.
        } else {
            px(0.)
        };
        let weak = cx.entity().downgrade();
        let groups = if self.palette_kind == PaletteKind::AddContext {
            share_palette_commands(self.composer_resource_groups())
        } else {
            self.palette_groups.clone()
        };
        let confirm_groups = groups
            .iter()
            .map(|(label, entries)| {
                (
                    label.clone(),
                    entries
                        .iter()
                        .filter(|entry| {
                            self.palette_kind == PaletteKind::AddContext || self.palette_action_available(&entry.action)
                        })
                        .cloned()
                        .collect::<Vec<_>>(),
                )
            })
            .filter(|(_, entries)| self.palette_kind == PaletteKind::AddContext || !entries.is_empty())
            .collect::<Vec<_>>();
        let models = self.palette_kind == PaletteKind::Models;
        let prompt_history = self.palette_kind == PaletteKind::PromptHistory;
        let modes = self.palette_kind == PaletteKind::Modes;
        let conversation_search = self.palette_kind == PaletteKind::ConversationSearch;
        let older_history_available = self.history_has_earlier;
        let history_page_loading = self.history_page_loading;
        let history_page_failed = self.history_page_error.is_some();
        let transcript_has_capacity = self.transcript.remaining_capacity() > 0;
        let composer_popup = self.palette_kind.is_composer_popup();
        let inline_completion = matches!(self.palette_kind, PaletteKind::FileMentions | PaletteKind::SlashCommands);
        let title = match self.palette_kind {
            PaletteKind::Models => "Choose a model".to_owned(),
            PaletteKind::ConversationSearch => "Find in conversation".to_owned(),
            PaletteKind::ModelOptions => "Select model".to_owned(),
            PaletteKind::AddContext => "Add files and more @".to_owned(),
            PaletteKind::FileMentions => "Mention a workspace file".to_owned(),
            PaletteKind::SlashCommands => "VT Code commands".to_owned(),
            PaletteKind::PromptHistory => "Recent prompts".to_owned(),
            PaletteKind::Permissions => "Command permissions".to_owned(),
            PaletteKind::Modes => "Agent mode".to_owned(),
            PaletteKind::Fonts(FontTarget::Ui) => "Choose UI font".to_owned(),
            PaletteKind::Fonts(FontTarget::Code) => "Choose code font".to_owned(),
            PaletteKind::SettingsSearch => rust_i18n::t!("app.settings.search.title").to_string(),
            PaletteKind::ConfigSettings => "VT Code configuration".to_owned(),
            PaletteKind::ReviewFiles => "Choose changed file".to_owned(),
            PaletteKind::Commands => "Command palette".to_owned(),
        };
        let quick_model = self.palette_kind == PaletteKind::ModelOptions;
        let catalog_empty = confirm_groups.is_empty();
        let placeholder = if self.palette_kind == PaletteKind::SettingsSearch {
            rust_i18n::t!("app.settings.search.placeholder").to_string()
        } else if self.palette_kind == PaletteKind::ConfigSettings {
            "Search all CLI settings…".to_owned()
        } else if matches!(self.palette_kind, PaletteKind::Fonts(_)) {
            "Search installed fonts…".to_owned()
        } else if models {
            "Search providers, models, or IDs…".to_owned()
        } else if self.palette_kind == PaletteKind::FileMentions {
            "Filter workspace files…".to_owned()
        } else if self.palette_kind == PaletteKind::SlashCommands {
            "Filter VT Code commands…".to_owned()
        } else if self.palette_kind == PaletteKind::ReviewFiles {
            "Search changed files…".to_owned()
        } else if conversation_search {
            "Search messages and visible output…".to_owned()
        } else if prompt_history {
            "Search prompts in this thread…".to_owned()
        } else if modes {
            "Search agent modes…".to_owned()
        } else {
            "Type a command or search…".to_owned()
        };
        let mut command = Command::new(&state)
            .placeholder(placeholder)
            .max_h(px(360.))
            .when(models, |command| {
                command.header(|_, _, _| div().px_3().py_2().font_semibold().child("Choose a model"))
            })
            .when(conversation_search, |command| {
                let muted_foreground = colors.muted_foreground;
                command.header(move |_, _, _| {
                    div()
                        .px_3()
                        .py_2()
                        .text_xs()
                        .text_color(muted_foreground)
                        .child("Searches loaded messages and visible tool output")
                })
            })
            .empty(move |_, _, _| {
                if quick_model {
                    div().h(px(0.)).into_any_element()
                } else {
                    empty_state(
                        if models && catalog_empty {
                            "No models available"
                        } else if prompt_history && catalog_empty {
                            "No prompt history yet"
                        } else if modes && catalog_empty {
                            "Agent modes unavailable"
                        } else if conversation_search && older_history_available {
                            "No match in loaded conversation"
                        } else {
                            "No matching results"
                        },
                        if models && catalog_empty {
                            "Reopen VT Code to reload the model catalog."
                        } else if prompt_history && catalog_empty {
                            "Prompts sent in this thread will appear here."
                        } else if modes && catalog_empty {
                            "Rebuild VT Code Desktop and its bundled CLI together, then reload CLI settings."
                        } else if conversation_search && older_history_available {
                            "Load earlier messages below to search older parts of this conversation."
                        } else {
                            "Try a different search."
                        },
                        IconName::Search,
                    )
                    .into_any_element()
                }
            });
        if inline_completion {
            command = command.searchable(false);
        }
        let show_history_control =
            conversation_search && (older_history_available || history_page_loading || history_page_failed);
        if show_history_control {
            let weak = weak.clone();
            let palette_id = state.entity_id();
            let session_id = self.selected_session_id.clone();
            let can_load_history =
                older_history_available && transcript_has_capacity && !history_page_loading && !self.running;
            let history_label = if history_page_loading {
                "Loading earlier messages…"
            } else if !transcript_has_capacity && older_history_available {
                "Message limit reached"
            } else if history_page_failed {
                "Retry loading earlier messages"
            } else {
                "Load earlier messages"
            };
            command = command.footer(move |_, _, _| {
                let weak = weak.clone();
                let session_id = session_id.clone();
                div()
                    .border_t_1()
                    .border_color(colors.border)
                    .px_3()
                    .py_2()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .when(history_page_failed, |footer| {
                        footer.child(
                            div()
                                .text_xs()
                                .text_color(colors.danger)
                                .child("Could not load older messages. You can retry."),
                        )
                    })
                    .child(
                        desktop_button("conversation-search-load-earlier")
                            .label(history_label)
                            .icon(IconName::ArrowUp)
                            .ghost()
                            .small()
                            .desktop_disabled(!can_load_history)
                            .on_click(move |_, _, cx| {
                                let _updated = weak.update(cx, |view, cx| {
                                    if view.palette_kind != PaletteKind::ConversationSearch
                                        || view.palette.as_ref().is_none_or(|palette| palette.entity_id() != palette_id)
                                        || view.selected_session_id != session_id
                                    {
                                        return;
                                    }
                                    view.request_earlier_history(cx);
                                });
                            }),
                    )
            });
        }
        if quick_model {
            let slider = self.effort_slider.clone();
            let configuration = self.configuration.clone();
            let speed_toggle = configuration.as_ref().and_then(|configuration| {
                let options = configuration.session_options.as_ref()?;
                if !options.fast_mode_supported {
                    return None;
                }
                Some((
                    composer_controls::runtime_option_label(options.fast_mode_label.as_deref()?),
                    options.fast_mode_enabled,
                    configuration.provider.clone(),
                    configuration.model.clone(),
                    self.palette.as_ref().map(|state| state.entity_id()),
                ))
            });
            let speed_toggle_view = weak.clone();
            let label = configuration.as_ref().map(|config| {
                self.models
                    .iter()
                    .find(|model| model.provider == config.provider && model.id == config.model)
                    .map(|model| model.display_name.clone())
                    .unwrap_or_else(|| config.model.clone())
            });
            command = command.searchable(false).header(move |_, _, cx| {
                let speed_control = speed_toggle.as_ref().map(|(label, enabled, provider, model, palette_id)| {
                    let weak = speed_toggle_view.clone();
                    let label = label.clone();
                    let enabled = *enabled;
                    let provider = provider.clone();
                    let model = model.clone();
                    let palette_id = *palette_id;
                    Toggle::new("fast-mode-toggle")
                        .label(label.clone())
                        .icon(IconName::Activity)
                        .tooltip(if enabled {
                            format!("Disable {label} processing")
                        } else {
                            format!("Enable {label} processing")
                        })
                        .outline()
                        .small()
                        .checked(enabled)
                        .rounded_full()
                        .px_2()
                        .font_semibold()
                        .when(enabled && label == "Ultrafast", |toggle| {
                            toggle
                                .bg(colors.primary)
                                .text_color(colors.primary_foreground)
                                .border_color(colors.primary)
                        })
                        .cursor_pointer()
                        .on_click(move |checked, window, cx| {
                            let _updated = weak.update(cx, |view, cx| {
                                if view.palette_kind != PaletteKind::ModelOptions
                                    || view.palette.as_ref().map(|state| state.entity_id()) != palette_id
                                    || !view.model_is_current(&provider, &model)
                                {
                                    return;
                                }
                                view.close_palette(window, cx);
                                let _selected = view.select_session_options(
                                    protocol::SessionOptionsSelectParams {
                                        fast_mode: Some(*checked),
                                        ..Default::default()
                                    },
                                    cx,
                                );
                            });
                        })
                        .into_any_element()
                });
                composer_controls::model_options_header(
                    configuration.as_ref(),
                    slider.as_ref(),
                    label.as_deref(),
                    speed_control,
                    cx,
                )
            });
            let search_all_view = weak.clone();
            let search_all_palette_id = state.entity_id();
            command = command.footer(move |_, _, _| {
                let view = search_all_view.clone();
                div()
                    .id("model-search-all-footer")
                    .border_t_1()
                    .border_color(colors.border)
                    .px_3()
                    .py_2()
                    .child(
                        desktop_button("model-search-all")
                            .label("Search all models…")
                            .icon(IconName::Search)
                            .ghost()
                            .small()
                            .w_full()
                            .on_click(move |_, window, cx| {
                                let _updated = view.update(cx, |view, cx| {
                                    if view.palette_kind == PaletteKind::ModelOptions
                                        && view
                                            .palette
                                            .as_ref()
                                            .is_some_and(|palette| palette.entity_id() == search_all_palette_id)
                                    {
                                        view.open_model_palette(window, cx);
                                    }
                                });
                            }),
                    )
            });
        }
        let palette_query = self.palette_query.clone();
        for (label, entries) in &confirm_groups {
            let mut group = CommandGroup::new().label(label.clone());
            for entry in entries {
                let label = entry.label.clone();
                let description = entry.description.clone();
                let query = palette_query.clone();
                let selected = self.is_current_palette_action(&entry.action);
                let enabled = self.palette_action_available(&entry.action);
                let id = palette_entry_element_id(&entry.action);
                let accessible_label = description
                    .as_ref()
                    .map(|details| format!("{label} · {details}"))
                    .unwrap_or_else(|| label.clone());
                group = group.item(
                    CommandItem::new()
                        .disabled(!enabled)
                        .label(label.clone())
                        .checked(selected)
                        .keywords(entry.keywords.clone())
                        .keywords(entry.shared_keywords.clone())
                        .child(move |_, _| {
                            div()
                                .id(id.clone())
                                .test_support()
                                .flex_1()
                                .min_w_0()
                                .when(enabled, |row| row.cursor_pointer())
                                .flex()
                                .items_center()
                                .gap_3()
                                .py_1()
                                .role(gpui_kit::Role::Label)
                                .aria_label(accessible_label.clone())
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .flex()
                                        .flex_col()
                                        .gap_1()
                                        .child(
                                            gpui_kit::component::label::Label::new(label.clone())
                                                .highlights(query.clone())
                                                .font_medium()
                                                .truncate(),
                                        )
                                        .when_some(description.clone(), |row, details| {
                                            row.child(
                                                div()
                                                    .text_xs()
                                                    .text_color(colors.muted_foreground)
                                                    .truncate()
                                                    .child(details),
                                            )
                                        }),
                                )
                        }),
                );
            }
            command = command.group(group);
        }
        let confirm_weak = weak.clone();
        let cancel_weak = weak.clone();
        let query_weak = weak.clone();
        let palette_id = state.entity_id();
        let command = command
            .on_query(move |query, _, cx| {
                let _updated = query_weak.update(cx, |view, cx| {
                    if view.palette.as_ref().is_none_or(|palette| palette.entity_id() != palette_id) {
                        return;
                    }
                    view.palette_query = query.into();
                    cx.notify();
                });
            })
            .on_confirm(move |path, _window, cx: &mut App| {
                let Some(action) = confirm_groups
                    .get(path.section)
                    .and_then(|(_, entries)| entries.get(path.row))
                    .map(|entry| entry.action.clone())
                else {
                    return;
                };
                let _updated = confirm_weak.update(cx, |view, cx| {
                    if view.palette.as_ref().is_none_or(|palette| palette.entity_id() != palette_id) {
                        return;
                    }
                    view.run_palette_action(action, _window, cx);
                });
            })
            .on_cancel(move |window, cx: &mut App| {
                let _updated = cancel_weak.update(cx, |view, cx| {
                    if view.palette.as_ref().is_some_and(|palette| palette.entity_id() == palette_id) {
                        view.close_palette(window, cx);
                    }
                });
            });

        let panel = div()
            .id("command-palette")
            .test_support()
            .role(gpui_kit::Role::Dialog)
            .aria_label(title)
            .w(gpui_kit::rems(if quick_model {
                22.
            } else if models {
                24.5
            } else {
                35.
            }))
            .max_w(max_popup_width)
            .when(models || quick_model || inline_completion, |panel| {
                panel
                    .rounded_xl()
                    .border_1()
                    .border_color(colors.border)
                    .bg(colors.popover)
                    .overflow_hidden()
            })
            .on_mouse_down_out(move |_, window, cx| {
                let _updated = weak.update(cx, |view, cx| {
                    if view.palette.as_ref().is_some_and(|palette| palette.entity_id() == palette_id) {
                        view.close_palette(window, cx);
                    }
                });
            })
            .on_action(|_: &gpui_kit::base::actions::Cancel, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(command.with_animation(
                ("command-palette-content", state.entity_id()),
                Animation::new(cx.theme().motion_tokens().duration_fast).with_easing({
                    let easing = cx.theme().motion_tokens().easing_enter.clone();
                    move |progress| easing.sample(progress).clamp(0., 1.)
                }),
                |content, progress| content.opacity(progress),
            ))
            .focus_trap("command-palette", &self.palette_focus);
        if composer_popup {
            return deferred(
                anchored()
                    .anchor(if matches!(self.palette_kind, PaletteKind::Models | PaletteKind::ModelOptions) {
                        Anchor::BottomRight
                    } else {
                        Anchor::BottomLeft
                    })
                    .offset(point(px(0.), ui_font_size * -0.5))
                    .snap_to_window_with_margin(popup_margin)
                    .child(panel),
            )
            .priority(2)
            .into_any_element();
        }
        div()
            .absolute()
            .inset_0()
            .flex()
            .justify_center()
            .pt(gpui_kit::rems(5.625))
            .bg(colors.background.opacity(0.4))
            .child(panel)
            .into_any_element()
    }
}

impl Render for DesktopView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors;
        let inspector_visible = self.inspector_open && self.workspace.is_some() && !self.zen_mode;
        let navigation_visible = self.preferences.navigation_visible() || self.zen_mode;
        let navigation_on_right = self.preferences.navigation_on_right();
        let conversation = div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .when(!self.transcript.is_empty() && !self.zen_mode, |column| column.child(self.render_thread_header()))
            .child(self.render_transcript(cx))
            .child(self.render_composer(window, cx));
        let workspace = div()
            .id("workspace-surface")
            .test_support()
            .role(gpui_kit::Role::Group)
            .aria_label("Conversation workspace")
            .flex_1()
            .min_w_0()
            .min_h_0()
            .h_full()
            .flex()
            .gap_1()
            .p_3()
            .bg(colors.background)
            .rounded_xl()
            .border_1()
            .border_color(colors.border)
            .child(conversation)
            .when(inspector_visible, |workspace| {
                workspace.child(
                    div()
                        .relative()
                        .h_full()
                        .flex_shrink_0()
                        .child(self.render_inspector(cx))
                        .child(self.render_resize_handle(ResizeEdge::Inspector, cx)),
                )
            });
        let panes = div()
            .id("pane-root")
            .p_3()
            .gap_2()
            .flex_1()
            .min_h_0()
            .flex()
            .bg(chrome_background(&colors))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, cx| {
                    if this.resizing.take().is_some() {
                        this.save_preferences();
                        cx.notify();
                    }
                }),
            );
        let panes = if navigation_visible {
            let navigation = self.render_navigation(cx);
            let sidebar = div().relative().h_full().flex_shrink_0();
            let sidebar = if self.zen_mode {
                sidebar.child(navigation)
            } else if navigation_on_right {
                sidebar
                    .when(self.sidebar_open, |sidebar| {
                        sidebar.child(self.render_resize_handle(ResizeEdge::Sidebar, cx))
                    })
                    .child(navigation)
            } else {
                sidebar.child(navigation).when(self.sidebar_open, |sidebar| {
                    sidebar.child(self.render_resize_handle(ResizeEdge::Sidebar, cx))
                })
            };
            if navigation_on_right {
                panes.child(workspace).child(sidebar)
            } else {
                panes.child(sidebar).child(workspace)
            }
        } else {
            panes.child(workspace)
        };
        let mut body = div()
            .size_full()
            .flex()
            .flex_col()
            .bg(colors.background)
            .text_color(colors.foreground)
            .track_focus(&self.focus_handle)
            .key_context("VTCodeDesktop")
            .when(self.session_change_allowed(None), |body| {
                body.on_action(cx.listener(|view, _: &NewSession, _, cx| view.create_session(cx)))
            })
            .when(self.selected_session_id.is_some() && self.session_change_allowed(None), |body| {
                body.on_action(cx.listener(|view, _: &CloseSession, _, cx| view.close_session(cx)))
            })
            .when(
                self.retained_submission.is_none() && self.workspace_switch_idle() && !self.workspace_picker_open,
                |body| {
                    body.on_action(
                        cx.listener(|view, _: &ChooseWorkspace, window, cx| view.choose_workspace(window, cx)),
                    )
                },
            )
            .on_action(cx.listener(|view, _: &ToggleSidebar, _, cx| {
                if view.preferences.navigation_visible() {
                    view.sidebar_open = !view.sidebar_open;
                } else {
                    view.preferences.navigation_visible = Some(true);
                    view.save_preferences();
                }
                view.navigation_peek = false;
                cx.notify();
            }))
            .on_action(cx.listener(|view, _: &NavigateBack, _, cx| view.navigate_back(cx)))
            .on_action(cx.listener(|view, _: &NavigateForward, _, cx| view.navigate_forward(cx)))
            .on_action(cx.listener(|view, _: &ToggleZenMode, window, cx| view.toggle_zen_mode(window, cx)))
            .on_action(cx.listener(|view, _: &ToggleStatusBar, _, cx| view.toggle_status_bar(cx)))
            .on_action(cx.listener(|view, _: &ReloadCliSettings, window, cx| {
                view.reload_cli_settings_from_ui(window, cx);
            }))
            .on_action(cx.listener(|view, _: &SyncCliTheme, _, cx| view.sync_theme_to_cli(cx)))
            .on_action(cx.listener(|view, _: &FocusComposer, window, cx| view.activate_composer(window, cx)))
            .when(self.workspace.is_some(), |body| {
                body.on_action(cx.listener(|view, _: &ToggleInspector, _, cx| view.toggle_inspector(cx)))
                    .on_action(cx.listener(|view, _: &OpenUtilities, _, cx| view.open_utilities(cx)))
                    .on_action(cx.listener(|view, _: &OpenWorkspaceFiles, window, cx| {
                        view.focus_workspace_files(window, cx);
                    }))
                    .on_action(cx.listener(|view, _: &RefreshWorkspace, _, cx| view.refresh_workspace_view(cx)))
            })
            .when(self.backend.is_some() && self.service_info.is_some(), |body| {
                body.on_action(cx.listener(|view, _: &OpenWorkspaceReview, window, cx| {
                    view.open_workspace_review(window, cx);
                }))
            })
            .when(self.running, |body| body.on_action(cx.listener(|view, _: &CancelRun, _, cx| view.cancel_turn(cx))))
            .on_action(cx.listener(|view, _: &DismissOverlay, window, cx| view.dismiss_overlay(window, cx)))
            .on_action(cx.listener(|view, _: &OpenCommandPalette, window, cx| view.open_command_palette(window, cx)))
            .on_action(cx.listener(|view, _: &OpenConversationSearch, window, cx| {
                if let Some(session_id) = view.selected_session_id.clone() {
                    view.open_conversation_search(session_id, window, cx);
                }
            }))
            .on_action(
                cx.listener(|view, _: &OpenSettingsSearch, window, cx| view.open_settings_search_palette(window, cx)),
            )
            .when(self.can_select_session_options(), |body| {
                body.on_action(cx.listener(|view, _: &OpenPermissions, window, cx| view.open_permissions(window, cx)))
                    .on_action(cx.listener(|view, _: &OpenModePicker, window, cx| view.open_modes(window, cx)))
            })
            .on_action(cx.listener(|view, _: &OpenAddContext, window, cx| view.open_add_context(window, cx)))
            .on_action(cx.listener(|view, _: &OpenSettings, window, cx| view.open_settings(window, cx)))
            .when(self.session_change_allowed(None), |body| {
                body.on_action(cx.listener(|view, _: &OpenModelPicker, window, cx| view.open_model_options(window, cx)))
            })
            .on_action(cx.listener(|_, _: &CloseWindow, window, _| window.remove_window()))
            .on_action(cx.listener(|_, _: &MinimizeWindow, window, _| window.minimize_window()))
            .on_action(cx.listener(|_, _: &ZoomWindow, window, _| window.zoom_window()))
            .child(self.render_toolbar(cx))
            .child(panes)
            .when(self.preferences.show_status_bar && !self.zen_mode, |body| body.child(self.render_status_bar(cx)));
        if self.file_preview.is_some() {
            body = body.child(self.render_file_preview(cx));
        }
        if self.review_open {
            body = body.child(self.render_change_review(cx));
        }
        if !self.palette_kind.is_composer_popup() {
            if let Some(state) = self.palette.clone() {
                body = body.child(self.render_command_palette(state, window, cx));
            }
        }
        body
    }
}

fn send_request(commands: &mpsc::Sender<BackendCommand>, method: &str, params: Value) -> bool {
    commands
        .try_send(BackendCommand::Request { method: method.to_string(), params })
        .is_ok()
}

fn section_label(text: &'static str, color: gpui_kit::Hsla) -> impl IntoElement {
    div().text_xs().font_semibold().text_color(color).child(text)
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

fn navigation_label(label: String, icon: IconName, selected: bool, accent: gpui_kit::Hsla) -> impl IntoElement {
    div()
        .w_full()
        .min_w_0()
        .flex()
        .items_center()
        .gap_2()
        .child(Icon::new(icon).size_4().when(selected, |icon| icon.text_color(accent)))
        .child(div().flex_1().min_w_0().truncate().text_left().child(label))
        .when(selected, |row| row.child(div().size(px(5.)).rounded_full().bg(accent)))
}

/// Embed the shared artwork so packaged applications never depend on checkout paths.
fn brand_logo(size: f32) -> impl IntoElement {
    static LOGO: std::sync::OnceLock<Arc<gpui_kit::Image>> = std::sync::OnceLock::new();
    let image = LOGO.get_or_init(|| {
        Arc::new(gpui_kit::Image::from_bytes(
            gpui_kit::ImageFormat::Png,
            include_bytes!("../../../../../resources/icons/vtcode-profile-120.png").to_vec(),
        ))
    });
    gpui_kit::img(image.clone()).w(px(size)).h(px(size)).rounded_md()
}

fn welcome_brand_mark(size: f32, color: gpui_kit::Hsla) -> impl IntoElement {
    gpui_kit::svg()
        .data(include_bytes!("../../../../../resources/icons/vtcode-mark.svg"))
        .size(px(size))
        .text_color(color)
        .id("welcome-brand-mark")
        .test_support()
}

pub(super) fn warning_alert(
    id: &'static str,
    message: impl Into<gpui_kit::SharedString>,
    colors: &gpui_kit::component::theme::ThemeColor,
) -> AnyElement {
    status_alert(id, message, colors, colors.warning)
}

fn error_alert(
    id: &'static str,
    message: impl Into<gpui_kit::SharedString>,
    colors: &gpui_kit::component::theme::ThemeColor,
) -> AnyElement {
    status_alert(id, message, colors, colors.danger)
}

fn status_alert(
    id: &'static str,
    message: impl Into<gpui_kit::SharedString>,
    colors: &gpui_kit::component::theme::ThemeColor,
    accent: gpui_kit::Hsla,
) -> AnyElement {
    let message = message.into();
    div()
        .id(id)
        .test_support()
        .role(gpui_kit::Role::Alert)
        .aria_label(message.clone())
        .w_full()
        .px_3()
        .py_2()
        .rounded_md()
        .border_1()
        .border_color(accent)
        .bg(colors.secondary)
        .text_sm()
        .text_color(colors.secondary_foreground)
        .child(message)
        .into_any_element()
}

fn empty_state(
    title: impl Into<gpui_kit::SharedString>,
    description: impl Into<gpui_kit::SharedString>,
    icon: IconName,
) -> EmptyState {
    let title = title.into();
    EmptyState::new().header(
        EmptyHeader::new()
            .media(EmptyMedia::new().child(if matches!(icon, IconName::Bot) {
                brand_logo(40.).into_any_element()
            } else {
                Icon::new(icon).size_6().into_any_element()
            }))
            .title(EmptyTitle::new().text_lg().font_semibold().child(title))
            .description(EmptyDescription::new().child(description.into())),
    )
}

fn command(label: impl Into<String>, action: PaletteAction) -> PaletteCommand {
    PaletteCommand {
        label: label.into(),
        description: None,
        keywords: Vec::new(),
        shared_keywords: Vec::new(),
        action,
    }
}

fn transcript_search_snippet(text: &str) -> String {
    let line = text.lines().find(|line| !line.trim().is_empty()).unwrap_or_default().trim();
    let mut chars = line.chars();
    let snippet = chars.by_ref().take(96).collect::<String>();
    if chars.next().is_some() {
        format!("{snippet}…")
    } else {
        snippet
    }
}

fn is_searchable_conversation_role(role: &str) -> bool {
    matches!(role, "user" | "assistant" | "tool")
}

/// Keep palette label identity attached to the command when search/filtering changes row order.
/// Hash payloads so file paths and draft text do not appear in rendered element IDs.
fn palette_entry_element_id(action: &PaletteAction) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    std::mem::discriminant(action).hash(&mut hasher);
    match action {
        PaletteAction::SelectFont { target, family } => {
            match target {
                FontTarget::Ui => 0_u8,
                FontTarget::Code => 1_u8,
            }
            .hash(&mut hasher);
            family.hash(&mut hasher);
        }
        PaletteAction::EditConfigSetting(value)
        | PaletteAction::LoadSession(value)
        | PaletteAction::PreviewFile(value)
        | PaletteAction::SelectWorkspaceReviewFile(value)
        | PaletteAction::AttachFile(value)
        | PaletteAction::InsertFileMention(value)
        | PaletteAction::CompleteSlashCommand(value)
        | PaletteAction::InsertHistoryPrompt(value)
        | PaletteAction::InsertDraft(value) => value.hash(&mut hasher),
        PaletteAction::OpenConversationSearch { session_id } => session_id.hash(&mut hasher),
        PaletteAction::JumpToTranscriptItem { session_id, item_id } => {
            session_id.hash(&mut hasher);
            item_id.hash(&mut hasher);
        }
        PaletteAction::ShowActivityOutput { session_id, item_id } => {
            session_id.hash(&mut hasher);
            item_id.hash(&mut hasher);
        }
        PaletteAction::SelectSettingsTab(tab) => std::mem::discriminant(tab).hash(&mut hasher),
        PaletteAction::SelectModel { provider, model } => {
            provider.hash(&mut hasher);
            model.hash(&mut hasher);
        }
        PaletteAction::SelectSessionOptions(options) => {
            match options.permission_mode {
                None => 0_u8,
                Some(protocol::PermissionMode::Ask) => 1,
                Some(protocol::PermissionMode::AutoApproveCommands) => 2,
            }
            .hash(&mut hasher);
            options.mode.hash(&mut hasher);
            options.reasoning_effort.hash(&mut hasher);
            options.fast_mode.hash(&mut hasher);
        }
        PaletteAction::NewSession
        | PaletteAction::CloseSession
        | PaletteAction::ChooseModel
        | PaletteAction::ChooseWorkspace
        | PaletteAction::RefreshSessions
        | PaletteAction::RefreshFiles
        | PaletteAction::ToggleInspector
        | PaletteAction::ToggleZenMode
        | PaletteAction::ToggleTheme
        | PaletteAction::AttachFiles => {}
    }
    format!("palette-label-{:016x}", hasher.finish())
}

fn model_palette_command(model: &protocol::ModelInfo, current: bool) -> PaletteCommand {
    let context = model
        .context_window
        .map(|tokens| format!(" · {} context", format_context_window(tokens)))
        .unwrap_or_default();
    let current = if current { " · Current" } else { "" };
    let capabilities = model.capabilities.as_ref().map(model_capability_labels).unwrap_or_default();
    let mut entry = command(
        model.display_name.clone(),
        PaletteAction::SelectModel {
            provider: model.provider.clone(),
            model: model.id.clone(),
        },
    );
    let mut details = vec![model.provider.clone()];
    details.extend(capabilities.iter().cloned());
    if !context.is_empty() {
        details.push(context.trim_start_matches(" · ").to_string());
    }
    details.push(model.id.clone());
    if !current.is_empty() {
        details.push(current.trim_start_matches(" · ").to_string());
    }
    entry.description = Some(details.join(" · "));
    entry.keywords = vec![model.provider.clone(), model.id.clone()];
    entry.keywords.extend(capabilities);
    entry
}

fn model_capability_labels(capabilities: &protocol::ModelCapabilities) -> Vec<String> {
    let mut labels = Vec::new();
    if capabilities.reasoning {
        labels.push("Reasoning".into());
    }
    if capabilities.vision {
        labels.push("Vision".into());
    }
    if capabilities.tool_call {
        labels.push("Tools".into());
    }
    if capabilities.structured_output {
        labels.push("Structured output".into());
    }
    if capabilities.is_pro {
        labels.push("Pro".into());
    }
    for (modality, label) in [("audio", "Audio"), ("video", "Video"), ("pdf", "PDF")] {
        if capabilities
            .input_modalities
            .iter()
            .any(|value| value.eq_ignore_ascii_case(modality))
        {
            labels.push(label.into());
        }
    }
    labels
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
        prompt: Some("Choose workspace".into()),
    }
}

/// Short single-line sidebar title derived from a session's first prompt,
/// mirroring the archive's first-prompt preview.
fn session_title_from_prompt(prompt: &str) -> String {
    const MAX_TITLE_CHARS: usize = 72;
    let first_line = prompt.lines().find(|line| !line.trim().is_empty()).unwrap_or_default();
    let trimmed = first_line.trim();
    let mut title: String = trimmed.chars().take(MAX_TITLE_CHARS).collect();
    if trimmed.chars().count() > MAX_TITLE_CHARS {
        title.push('…');
    }
    title
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

/// Human title and lifecycle summary for a non-message activity item.
fn describe_activity(details: &vtcode_exec_events::ThreadItemDetails) -> (String, String) {
    use vtcode_exec_events::ThreadItemDetails;
    match details {
        ThreadItemDetails::ToolInvocation(tool) => (tool.tool_name.clone(), tool_status_label(&tool.status)),
        ThreadItemDetails::ToolOutput(output) => ("Tool output".to_string(), tool_status_label(&output.status)),
        ThreadItemDetails::CommandExecution(command) => {
            let mut label = command_status_label(&command.status);
            if let Some(code) = command.exit_code {
                if matches!(command.status, vtcode_exec_events::CommandExecutionStatus::Failed) {
                    label.push_str(&format!(" (exit {code})"));
                }
            }
            (command.command.clone(), label)
        }
        ThreadItemDetails::McpToolCall(call) => {
            let label = match call.status {
                Some(vtcode_exec_events::McpToolCallStatus::Started) => "started",
                Some(vtcode_exec_events::McpToolCallStatus::Completed) => "completed",
                Some(vtcode_exec_events::McpToolCallStatus::Failed) => "failed",
                None => "activity",
            }
            .to_string();
            (format!("MCP · {}", call.tool_name), label)
        }
        ThreadItemDetails::WebSearch(search) => {
            let label = match search.provider.as_deref() {
                Some(provider) => format!("web search via {provider}"),
                None => "web search".to_string(),
            };
            (search.query.clone(), label)
        }
        ThreadItemDetails::Harness(event) => {
            let label = event.message.clone().unwrap_or_else(|| "harness activity".to_string());
            let title = event
                .command
                .clone()
                .or_else(|| event.path.clone())
                .unwrap_or_else(|| "Harness".to_string());
            (title, label)
        }
        ThreadItemDetails::Error(error) => ("Runtime item error".to_string(), error.message.clone()),
        ThreadItemDetails::AgentMessage(_)
        | ThreadItemDetails::Plan(_)
        | ThreadItemDetails::Reasoning(_)
        | ThreadItemDetails::Decision(_)
        | ThreadItemDetails::FileChange(_) => ("Activity".to_string(), "activity".to_string()),
    }
}

#[derive(serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ActivityDetailsMetadata<'a> {
    CommandExecution {
        command: &'a str,
        #[serde(skip_serializing_if = "Option::is_none")]
        arguments: Option<&'a Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        exit_code: Option<&'a i32>,
        status: &'a vtcode_exec_events::CommandExecutionStatus,
    },
    ToolOutput {
        call_id: &'a str,
        #[serde(skip_serializing_if = "Option::is_none")]
        tool_call_id: Option<&'a str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        spool_path: Option<&'a str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        exit_code: Option<&'a i32>,
        status: &'a vtcode_exec_events::ToolCallStatus,
    },
}

/// Keep large tool output out of the metadata serialization path.
fn activity_details_payload(details: &vtcode_exec_events::ThreadItemDetails) -> String {
    use vtcode_exec_events::ThreadItemDetails;

    let serialized = match details {
        ThreadItemDetails::CommandExecution(command) => {
            serde_json::to_string_pretty(&ActivityDetailsMetadata::CommandExecution {
                command: &command.command,
                arguments: command.arguments.as_ref(),
                exit_code: command.exit_code.as_ref(),
                status: &command.status,
            })
        }
        ThreadItemDetails::ToolOutput(output) => serde_json::to_string_pretty(&ActivityDetailsMetadata::ToolOutput {
            call_id: &output.call_id,
            tool_call_id: output.tool_call_id.as_deref(),
            spool_path: output.spool_path.as_deref(),
            exit_code: output.exit_code.as_ref(),
            status: &output.status,
        }),
        _ => serde_json::to_string_pretty(details),
    };
    serialized.unwrap_or_else(|_| "{}".to_string())
}

fn tool_status_label(status: &vtcode_exec_events::ToolCallStatus) -> String {
    match status {
        vtcode_exec_events::ToolCallStatus::Completed => "completed",
        vtcode_exec_events::ToolCallStatus::Failed => "failed",
        vtcode_exec_events::ToolCallStatus::InProgress => "in progress",
    }
    .to_string()
}

fn command_status_label(status: &vtcode_exec_events::CommandExecutionStatus) -> String {
    match status {
        vtcode_exec_events::CommandExecutionStatus::Completed => "completed",
        vtcode_exec_events::CommandExecutionStatus::Failed => "failed",
        vtcode_exec_events::CommandExecutionStatus::InProgress => "running",
    }
    .to_string()
}

/// One classified line of a unified diff.
struct DiffLine<'a> {
    kind: DiffLineKind,
    text: &'a str,
}

const MAX_DIFF_LINES: usize = 2_000;

struct ClassifiedDiffLines<'a> {
    lines: std::str::Lines<'a>,
    remaining: usize,
    checked_limit: bool,
    truncated: bool,
}

impl<'a> ClassifiedDiffLines<'a> {
    fn new(patch: &'a str) -> Self {
        Self {
            lines: patch.lines(),
            remaining: MAX_DIFF_LINES,
            checked_limit: false,
            truncated: false,
        }
    }
}

impl<'a> Iterator for ClassifiedDiffLines<'a> {
    type Item = DiffLine<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            if !self.checked_limit {
                self.truncated = self.lines.next().is_some();
                self.checked_limit = true;
            }
            return None;
        }

        let Some(text) = self.lines.next() else {
            self.remaining = 0;
            self.checked_limit = true;
            return None;
        };
        self.remaining -= 1;
        Some(DiffLine { kind: classify_diff_line(text), text })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DiffLineKind {
    Header,
    Hunk,
    Added,
    Removed,
    Context,
}

fn classify_diff_line(line: &str) -> DiffLineKind {
    if line.starts_with("+++") || line.starts_with("---") || line.starts_with("diff ") {
        DiffLineKind::Header
    } else if line.starts_with("@@") {
        DiffLineKind::Hunk
    } else if line.starts_with('+') {
        DiffLineKind::Added
    } else if line.starts_with('-') {
        DiffLineKind::Removed
    } else {
        DiffLineKind::Context
    }
}

fn desktop_window_options() -> WindowOptions {
    let mut options = TitleBar::window_options();
    if let Some(titlebar) = options.titlebar.as_mut() {
        titlebar.traffic_light_position = Some(point(px(12.), px(17.)));
    }
    options
}

fn chrome_background(colors: &gpui_kit::component::theme::ThemeColor) -> gpui_kit::Hsla {
    themes::navigation_surfaces(colors.background, colors.secondary, colors.foreground).rail
}

fn item_update_identity(event: &BackendEvent) -> Option<(&str, Option<&str>, &str)> {
    let BackendEvent::ThreadEvent(notification) = event else {
        return None;
    };
    let vtcode_exec_events::ThreadEvent::ItemUpdated(update) = notification.event.event() else {
        return None;
    };
    Some((&notification.session_id, notification.turn_id.as_deref(), &update.item.id))
}

fn is_item_update_event(event: &BackendEvent) -> bool {
    item_update_identity(event).is_some()
}

fn can_coalesce_item_updates(previous: &BackendEvent, next: &BackendEvent) -> bool {
    matches!(
        (item_update_identity(previous), item_update_identity(next)),
        (Some((previous_session, previous_turn, previous_item)), Some((next_session, next_turn, next_item)))
            if previous_session == next_session && previous_turn == next_turn && previous_item == next_item
    )
}

/// Keep local paths out of element IDs while preserving path-based identity across sorting.
fn opaque_path_element_id(prefix: &str, path: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut hasher);
    format!("{prefix}-{:016x}", hasher.finish())
}

fn restored_history_element_id(session_id: &str, role: &str, text: &str, occurrence: usize) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    session_id.hash(&mut hasher);
    role.hash(&mut hasher);
    text.hash(&mut hasher);
    occurrence.hash(&mut hasher);
    format!("history-{:016x}", hasher.finish())
}

fn restored_archive_element_id(session_id: &str, archive_index: usize) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    session_id.hash(&mut hasher);
    format!("history-{:016x}-{archive_index}", hasher.finish())
}

/// Prepare stable transcript rows and Markdown accessibility projections for archived messages.
fn prepare_session_history(session_id: &str, messages: Vec<SessionHistoryMessage>) -> Vec<TranscriptItem> {
    let mut occurrences = HashMap::<u64, usize>::new();
    messages
        .into_iter()
        .map(|message| {
            let SessionHistoryMessage { archive_index, role, text } = message;
            let title = match role.as_str() {
                "user" => "You".to_string(),
                "assistant" => "VT Code".to_string(),
                "tool" => "Tool result".to_string(),
                _ => role.clone(),
            };
            let id = archive_index.map_or_else(
                || {
                    let mut fingerprint = std::collections::hash_map::DefaultHasher::new();
                    role.hash(&mut fingerprint);
                    text.hash(&mut fingerprint);
                    let occurrence = occurrences.entry(fingerprint.finish()).or_default();
                    let id = restored_history_element_id(session_id, &role, &text, *occurrence);
                    *occurrence = (*occurrence).saturating_add(1);
                    id
                },
                |index| restored_archive_element_id(session_id, index),
            );
            // Preserve collapsed archive tool text without inventing activity status.
            let mut item = TranscriptItem::new(id, role, title.into(), text);
            item.has_activity_status = false;
            item.prepare_render_content();
            item
        })
        .collect()
}

/// Interactive desktop buttons use a pointing hand; disabled buttons keep the arrow.
fn desktop_button(id: impl Into<gpui_kit::ElementId>) -> Button {
    Button::new(id).tab_stop(true).cursor_pointer()
}

trait DesktopButtonExt {
    fn desktop_disabled(self, disabled: bool) -> Self;
    fn dispatches_desktop_action(self, action: impl gpui_kit::Action + 'static, focus: FocusHandle) -> Self;
}

impl DesktopButtonExt for Button {
    fn desktop_disabled(self, disabled: bool) -> Self {
        self.disabled(disabled).cursor(if disabled {
            CursorStyle::Arrow
        } else {
            CursorStyle::PointingHand
        })
    }

    fn dispatches_desktop_action(self, action: impl gpui_kit::Action + 'static, focus: FocusHandle) -> Self {
        self.on_click(move |_, window, cx| focus.dispatch_action(&action, window, cx))
    }
}

#[cfg(test)]
mod tests;
