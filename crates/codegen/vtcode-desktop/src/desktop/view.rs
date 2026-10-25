use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, SyncSender, TryRecvError, sync_channel};
use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::base::{Disableable, FocusTrapElement, StyledExt};
use gpui_kit::component::TitleBar;
use gpui_kit::component::attachment::{
    Attachment, AttachmentActions, AttachmentContent, AttachmentDescription, AttachmentMedia, AttachmentTitle,
};
use gpui_kit::component::bubble::{Bubble, BubbleContent, BubbleVariant};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::command::{Command, CommandGroup, CommandItem, CommandState};
use gpui_kit::component::dialog::{DialogDescription, DialogHeader, DialogTitle};
use gpui_kit::component::diff::{Diff, DiffMode, DiffState};
use gpui_kit::component::empty::{
    Empty as EmptyState, EmptyContent, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle,
};
use gpui_kit::component::group_box::{GroupBox, GroupBoxVariants};
use gpui_kit::component::input::{Copy, Cut, Enter, Paste, Redo, SelectAll, Textarea, TextareaState, Undo};
use gpui_kit::component::input::{InputGroup, InputGroupAddon, InputGroupAddonAlignment};
use gpui_kit::component::list::ListItem;
use gpui_kit::component::menu::DropdownMenu;
use gpui_kit::component::message::{Message, MessageAlignment, MessageContent, MessageFooter, MessageHeader};
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::tag::Tag;
use gpui_kit::component::theme::{ActiveTheme, Theme, ThemeMode};
use gpui_kit::component::tree::{Tree, TreeEvent, TreeItem, TreeState};
use gpui_kit::component::{Icon, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::{
    Anchor, AnyElement, App, Context, CursorStyle, Entity, FocusHandle, Focusable, KeyBinding, Menu, MenuItem,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, OsAction, PathPromptOptions, Render, Subscription,
    SystemMenuType, Task, TestSupportExt, Window, WindowOptions, actions, anchored, deferred, div, list, point, px,
};
use serde_json::{Value, json};
use tokio::sync::mpsc;
use vtcode_desktop_protocol as protocol;
use vtcode_desktop_protocol::{
    ConfigurationSnapshot, InteractionChoice, InteractionRequest, InteractionResponse, InteractionStep,
    SessionHistoryMessage, SessionLoadResult, SessionSummary, ThreadEventNotification,
};

use super::files::{self, FileEntry, FilePreview};
use super::inspector::{self, ActivityState, ChangesProjection, InspectorTab};
use super::markdown::render_markdown;
use super::preferences;
use super::themes;
use super::transcript::{DiffCard, DiffFile, DiffFileKind, Transcript, TranscriptItem};
use super::transport::{
    BackendCommand, BackendEvent, BackendHandle, UI_EVENT_CAPACITY, launch_backend, request_fits_frame,
};

mod composer_controls;
mod configuration;
mod navigation;
mod shared_preferences;
mod shortcuts;
mod typography;
mod utilities;
mod workspace_review;
use typography::FontTarget;
use workspace_review::WorkspaceReviewState;

const MAX_UI_EVENTS_PER_TICK: usize = 64;
const CONTENT_WIDTH: f32 = 760.;

/// Pane-width drag bounds, shared with the preferences clamp.
const MIN_PANE_WIDTH: f32 = preferences::MIN_PANE_WIDTH;
const MAX_PANE_WIDTH: f32 = preferences::MAX_PANE_WIDTH;

actions!(
    desktop,
    [
        OpenCommandPalette,
        OpenModelPicker,
        OpenAddContext,
        OpenPermissions,
        OpenModePicker,
        OpenSelectedFile,
        NewSession,
        CloseSession,
        ChooseWorkspace,
        FocusComposer,
        ToggleInspector,
        ToggleSidebar,
        ToggleStatusBar,
        ReloadCliSettings,
        SyncCliTheme,
        NextInspectorTab,
        PreviousInspectorTab,
        FirstInspectorTab,
        LastInspectorTab,
        NextReviewChange,
        PreviousReviewChange,
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

fn init_keybindings(cx: &mut App) {
    let preferences = preferences::Preferences::load();
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
        KeyBinding::new("f7", NextReviewChange, Some("VTCodeDiffReview")),
        KeyBinding::new("shift-f7", PreviousReviewChange, Some("VTCodeDiffReview")),
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
            MenuItem::action("Focus Composer", FocusComposer),
            MenuItem::action("Toggle Inspector", ToggleInspector),
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
#[derive(Clone, Debug)]
struct PaletteCommand {
    label: String,
    description: Option<String>,
    keywords: Vec<String>,
    action: PaletteAction,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PaletteKind {
    Commands,
    Models,
    ModelOptions,
    AddContext,
    Permissions,
    Modes,
    Fonts(FontTarget),
}

/// What confirming a palette entry does. Executed through existing methods so
/// palette and toolbar stay behaviorally identical.
#[derive(Clone, Debug)]
enum PaletteAction {
    NewSession,
    CloseSession,
    ChooseModel,
    ChooseWorkspace,
    RefreshFiles,
    ToggleInspector,
    ToggleTheme,
    SelectFont { target: FontTarget, family: Option<String> },
    EditConfigSetting(String),
    LoadSession(String),
    PreviewFile(String),
    SelectModel { provider: String, model: String },
    SelectSessionOptions(protocol::SessionOptionsSelectParams),
    AttachFiles,
    AttachFile(String),
    InsertDraft(String),
}

pub(crate) fn run() {
    let application = gpui_kit::application().with_assets(gpui_kit::assets::AllAssets);
    let _application = application.on_reopen(show_main_window);
    application.run(|cx: &mut App| {
        gpui_kit::init(cx);
        init_keybindings(cx);
        init_native_menus(cx);
        show_main_window(cx);
    });
}

fn show_main_window(cx: &mut App) {
    if let Err(error) = open_or_activate_main_window(cx) {
        vtcode_commons::diagnostics::stderr(format_args!("failed to open VT Code Desktop window: {error}"));
    }
}

/// Reopening must preserve an existing window's draft, session, and backend.
fn open_or_activate_main_window(cx: &mut App) -> anyhow::Result<gpui_kit::AnyWindowHandle> {
    if let Some(window) = cx.windows().first().copied() {
        window.update(cx, |_, window, _| window.activate_window())?;
        cx.activate(true);
        return Ok(window);
    }
    // Saved preferences decide the starting theme; Ciapre is the default.
    let preferences = preferences::Preferences::load();
    let start_theme = preferences.theme.unwrap_or_else(|| themes::DEFAULT_THEME_ID.to_string());
    let (window, view) = gpui_kit::open_window(desktop_window_options(), cx, |window, cx| {
        window.set_window_title("VT Code");
        let theme_mode = themes::apply_by_id(&start_theme, Some(window), cx);
        let view = cx.new(|cx| DesktopView::new(window, cx, theme_mode));
        view.update(cx, |view, cx| view.load_preferences(window, cx));
        view
    })?;
    // Reopen the most recent workspace when it still exists.
    let last_workspace = view.read(cx).preferences.recent_workspaces.first().map(PathBuf::from);
    if let Some(path) = last_workspace
        && path.is_dir()
    {
        view.update(cx, |view, cx| view.select_workspace(path, cx));
    }
    let weak_view = view.downgrade();
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor().timer(Duration::from_millis(24)).await;
            if weak_view.update(cx, |view, cx| view.drain_events(cx)).is_err() {
                break;
            }
        }
    })
    .detach();
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

struct DesktopView {
    focus_handle: FocusHandle,
    workspace: Option<PathBuf>,
    pending_workspace: Option<PathBuf>,
    workspace_picker_open: bool,
    backend: Option<BackendHandle>,
    event_rx: Receiver<BackendEvent>,
    event_tx: SyncSender<BackendEvent>,
    sessions: Vec<SessionSummary>,
    selected_session_id: Option<String>,
    transcript: Transcript,
    restored_activity: Vec<TranscriptItem>,
    active_turn_id: Option<String>,
    finished_turn_id: Option<String>,
    cancel_requested: bool,
    cancel_request_sent: bool,
    running: bool,
    status: String,
    error: Option<String>,
    configuration: Option<ConfigurationSnapshot>,
    service_info: Option<protocol::InitializeResult>,
    interaction: Option<InteractionRequest>,
    interaction_choice_ids: Vec<Option<String>>,
    interaction_answer_texts: Vec<Option<String>>,
    interaction_step_index: usize,
    composer: Entity<TextareaState>,
    question_answer: Entity<TextareaState>,
    theme_mode: ThemeMode,
    files: Vec<FileEntry>,
    file_tree: Entity<TreeState>,
    expanded_file_directories: HashSet<String>,
    _file_tree_subscription: Subscription,
    files_task: Option<Task<()>>,
    preview_task: Option<Task<()>>,
    attachments_task: Option<Task<()>>,
    file_preview: Option<FilePreview>,
    preview_focus: FocusHandle,
    preview_return_focus: Option<FocusHandle>,
    palette_return_focus: Option<FocusHandle>,
    palette_focus: FocusHandle,
    inspector_open: bool,
    inspector_tab: InspectorTab,
    inspector_focus: FocusHandle,
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
    effort_slider: Option<Entity<gpui_kit::component::slider::SliderState>>,
    composer_resources: Vec<protocol::ComposerResource>,
    resources_loading: bool,
    resources_loaded: bool,
    palette_groups: Vec<(String, Vec<PaletteCommand>)>,
    settings_open: bool,
    settings_focus: FocusHandle,
    settings_return_focus: Option<FocusHandle>,
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
    retained_submission: Option<SubmissionDraft>,
    last_usage: Option<vtcode_exec_events::Usage>,
    theme_id: String,
    preferences: preferences::Preferences,
    system_motion_baseline: bool,
    default_code_font: gpui_kit::SharedString,
    sidebar_open: bool,
    navigation_peek: bool,
    shortcut_inputs: Vec<Entity<gpui_kit::component::input::InputState>>,
    shortcut_message: Option<String>,
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
    start_x: f32,
    start_width: f32,
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
    fn new(window: &mut Window, cx: &mut Context<Self>, theme_mode: ThemeMode) -> Self {
        let (event_tx, event_rx) = sync_channel(UI_EVENT_CAPACITY);
        let focus_handle = cx.focus_handle();
        focus_handle.focus(window, cx);
        let system_motion_baseline = preferences::system_motion_baseline(cx);
        let preferences = preferences::Preferences::load();
        preferences::apply_reduced_motion(preferences.reduced_motion, system_motion_baseline, cx);
        let shortcut_inputs = shortcuts::inputs(&preferences.shortcuts, window, cx);
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
        Self {
            shortcut_inputs,
            shortcut_message: None,
            default_code_font: typography::default_code_font(cx),
            focus_handle,
            workspace: None,
            pending_workspace: None,
            workspace_picker_open: false,
            backend: None,
            event_rx,
            event_tx,
            sessions: Vec::new(),
            selected_session_id: None,
            transcript: Transcript::default(),
            restored_activity: Vec::new(),
            active_turn_id: None,
            finished_turn_id: None,
            cancel_requested: false,
            cancel_request_sent: false,
            running: false,
            status: "Choose a workspace to get started".to_string(),
            error: None,
            configuration: None,
            service_info: None,
            interaction: None,
            interaction_choice_ids: Vec::new(),
            interaction_answer_texts: Vec::new(),
            interaction_step_index: 0,
            composer: cx.new(|cx| {
                TextareaState::new(window, cx)
                    .placeholder("Ask VT Code to make changes…")
                    .auto_grow(1, 6)
                    .submit_on_enter(true)
            }),
            question_answer: cx.new(|cx| TextareaState::new(window, cx).auto_grow(1, 4)),
            theme_mode,
            files: Vec::new(),
            file_tree,
            expanded_file_directories: HashSet::new(),
            _file_tree_subscription: file_tree_subscription,
            files_task: None,
            preview_task: None,
            attachments_task: None,
            file_preview: None,
            preview_focus: cx.focus_handle(),
            preview_return_focus: None,
            palette_return_focus: None,
            palette_focus: cx.focus_handle(),
            inspector_open: false,
            inspector_tab: InspectorTab::default(),
            inspector_focus: cx.focus_handle().tab_stop(true),
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
            effort_slider: None,
            composer_resources: Vec::new(),
            resources_loading: false,
            resources_loaded: false,
            palette_groups: Vec::new(),
            settings_open: false,
            settings_focus: cx.focus_handle().tab_stop(true),
            settings_return_focus: None,
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
            retained_submission: None,
            last_usage: None,
            theme_id: themes::DEFAULT_THEME_ID.to_string(),
            preferences,
            system_motion_baseline,
            sidebar_open: false,
            navigation_peek: false,
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

    fn choose_workspace(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
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
        cx.spawn(async move |view, cx| {
            let result = paths.await;
            let _updated = view.update(cx, |view, cx| {
                view.workspace_picker_open = false;
                match result {
                    Ok(Ok(Some(paths))) => {
                        if let Some(path) = paths.into_iter().next() {
                            view.select_workspace(path, cx);
                        }
                    }
                    Ok(Err(error)) => view.error = Some(format!("Unable to open the workspace picker: {error}")),
                    Ok(Ok(None)) => {}
                    Err(_) => view.error = Some("The workspace picker closed without returning a result.".to_string()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn workspace_switch_idle(&self) -> bool {
        !self.running && self.pending_workspace.is_none() && self.session_request.is_none()
    }

    fn select_workspace(&mut self, path: PathBuf, cx: &mut Context<Self>) {
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
        let canonical = match vtcode_commons::paths::canonicalize(&path) {
            Ok(path) if path.is_dir() => path,
            _ => {
                self.error = Some("Choose an existing workspace folder.".to_string());
                cx.notify();
                return;
            }
        };
        if self.workspace.as_ref() == Some(&canonical) {
            return;
        }
        if let Some(backend) = self.backend.as_ref() {
            self.pending_workspace = Some(canonical);
            self.status = "Shutting down the current workspace service…".to_string();
            if backend.commands.try_send(BackendCommand::Stop).is_err() {
                self.pending_workspace = None;
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
            Err(error) => self.error = Some(error),
        }
        cx.notify();
    }

    fn prepare_workspace(&mut self, workspace: &Path, cx: &mut Context<Self>) {
        // A same-workspace restart reloads CLI configuration without losing draft sources.
        let same_workspace = self.workspace.as_deref() == Some(workspace);
        let draft_sources = same_workspace.then(|| std::mem::take(&mut self.attachments));
        self.workspace = Some(workspace.to_path_buf());
        self.remember_workspace(workspace);
        self.pending_workspace = None;
        self.sessions.clear();
        self.selected_session_id = None;
        self.reset_session_state();
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
        self.composer_resources.clear();
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
            self.error = Some("The local service request queue is unavailable.".to_string());
        }
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
        let Some(workspace) = self.workspace.clone() else {
            return;
        };
        self.files_task = Some(cx.spawn(async move |view, cx| {
            let root = workspace.clone();
            let (entries, branch) = cx
                .background_spawn(async move { (files::scan_workspace(&root), files::git_branch(&root)) })
                .await;
            let _updated = view.update(cx, |view, cx| {
                if view.workspace.as_ref() == Some(&workspace) {
                    view.replace_file_entries(entries, cx);
                    view.git_branch = branch;
                    cx.notify();
                }
            });
        }));
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
        self.settings_open = false;
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
        self.settings_focus.focus(window, cx);
        cx.notify();
    }

    fn close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings_open = false;
        if let Some(focus) = self.settings_return_focus.take() {
            focus.focus(window, cx);
        } else {
            self.focus_composer(window, cx);
        }
        cx.notify();
    }

    fn toggle_inspector(&mut self, cx: &mut Context<Self>) {
        if self.workspace.is_some() {
            self.inspector_open = !self.inspector_open;
            self.save_preferences();
            cx.notify();
        }
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
        let return_to_settings = matches!(self.palette_kind, PaletteKind::Fonts(_)) && self.palette.is_some();
        self.palette = None;
        self.effort_slider = None;
        self.palette_groups.clear();
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

    fn dismiss_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.config_editor.is_some() {
            self.close_cli_config_editor(window, cx);
        } else if self.navigation_peek {
            self.navigation_peek = false;
            cx.notify();
        } else if self.settings_open {
            self.close_settings(window, cx);
        } else if self.review_open {
            self.close_review(window, cx);
        } else if self.palette.is_some() {
            self.close_palette(window, cx);
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
        const MAX_SESSION_ENTRIES: usize = 10;
        const MAX_FILE_ENTRIES: usize = 50;

        let mut groups: Vec<(String, Vec<PaletteCommand>)> = Vec::new();
        groups.push((
            "Session".to_string(),
            vec![
                command("New session", PaletteAction::NewSession),
                command("Close session", PaletteAction::CloseSession),
                command("Switch workspace…", PaletteAction::ChooseWorkspace),
                command("Refresh workspace files", PaletteAction::RefreshFiles),
                command("Choose a model…", PaletteAction::ChooseModel),
            ],
        ));
        groups.push((
            "View".to_string(),
            vec![
                command("Toggle inspector panel", PaletteAction::ToggleInspector),
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
        let sessions: Vec<PaletteCommand> =
            self.sessions
                .iter()
                .take(MAX_SESSION_ENTRIES)
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
            .take(MAX_FILE_ENTRIES)
            .map(|entry| {
                command(format!("Preview file: {}", entry.rel_path), PaletteAction::PreviewFile(entry.rel_path.clone()))
            })
            .collect();
        if !file_commands.is_empty() {
            groups.push(("Files".to_string(), file_commands));
        }

        self.show_palette(PaletteKind::Commands, groups, window, cx);
    }

    fn open_model_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.session_change_allowed(None) || self.attachments_picker_open {
            return;
        }
        let mut groups = std::collections::BTreeMap::<String, Vec<PaletteCommand>>::new();
        let active_model = self
            .configuration
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
            });
        for model in self.models.iter().chain(active_model.iter()) {
            let context = model
                .context_window
                .map(|tokens| format!(" · {tokens} tokens"))
                .unwrap_or_default();
            let current = if self.model_is_current(&model.provider, &model.id) {
                " · Current"
            } else {
                ""
            };
            let mut entry = command(
                model.display_name.clone(),
                PaletteAction::SelectModel {
                    provider: model.provider.clone(),
                    model: model.id.clone(),
                },
            );
            entry.description = Some(format!("{} · {}{context}{current}", model.provider, model.id));
            entry.keywords = vec![model.provider.clone(), model.id.clone()];
            groups.entry(model.provider.clone()).or_default().push(entry);
        }
        let mut groups = groups.into_iter().collect::<Vec<_>>();
        for (_, entries) in &mut groups {
            entries.sort_by_key(|entry| !self.is_current_palette_action(&entry.action));
        }
        groups.sort_by_key(|(_, entries)| !entries.iter().any(|entry| self.is_current_palette_action(&entry.action)));
        self.show_palette(PaletteKind::Models, groups, window, cx);
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
                let selected = match target {
                    FontTarget::Ui => &self.preferences.ui_font_family,
                    FontTarget::Code => &self.preferences.code_font_family,
                };
                family == selected
            }
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
        if self.review_open {
            self.close_review(window, cx);
        }
        if self.palette.is_some() {
            self.close_palette(window, cx);
        }
        self.settings_open = false;
        self.palette_return_focus = window.focused(cx);
        let state = cx.new(|cx| CommandState::new(window, cx));
        self.palette = Some(state.clone());
        self.palette_kind = kind;
        self.palette_groups = groups;
        // The palette installs its input state during its first render, so
        // focusing has to wait for that frame.
        cx.on_next_frame(window, |this, window, cx| this.focus_palette_state(window, cx));
        cx.notify();
    }

    fn focus_palette_state(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(state) = self.palette.clone() {
            let current = (matches!(
                self.palette_kind,
                PaletteKind::Models | PaletteKind::Permissions | PaletteKind::Modes | PaletteKind::Fonts(_)
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
            PaletteAction::ChooseModel => self.open_model_palette(window, cx),
            PaletteAction::ChooseWorkspace => self.choose_workspace(window, cx),
            PaletteAction::RefreshFiles => self.refresh_files(cx),
            PaletteAction::ToggleInspector => self.toggle_inspector(cx),
            PaletteAction::ToggleTheme => self.toggle_theme(window, cx),
            PaletteAction::SelectFont { target, family } => self.select_font(target, family, cx),
            PaletteAction::EditConfigSetting(path) => self.open_cli_config_editor(path, window, cx),
            PaletteAction::SelectSessionOptions(options) => self.select_session_options(options, cx),
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
            PaletteAction::LoadSession(session_id) => self.load_session(session_id, cx),
            PaletteAction::PreviewFile(rel_path) => self.open_file_preview(rel_path, window, cx),
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
            PaletteAction::SelectSessionOptions(_) => self.can_select_session_options(),
            PaletteAction::AttachFiles | PaletteAction::AttachFile(_) => {
                self.workspace.is_some() && !self.attachments_picker_open && self.session_request.is_none()
            }
            PaletteAction::InsertDraft(_) => true,
            PaletteAction::CloseSession => self.selected_session_id.is_some() && self.session_change_allowed(None),
            PaletteAction::LoadSession(session_id) => self.session_change_allowed(Some(session_id)),
            PaletteAction::ChooseWorkspace => {
                self.retained_submission.is_none() && self.workspace_switch_idle() && !self.workspace_picker_open
            }
            PaletteAction::RefreshFiles | PaletteAction::ToggleInspector | PaletteAction::PreviewFile(_) => {
                self.workspace.is_some()
            }
            PaletteAction::ToggleTheme | PaletteAction::SelectFont { .. } => true,
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
        if !self.session_change_idle() || self.attachments_picker_open || self.retained_submission.is_some() {
            return;
        }
        let prompt = self.composer.read(cx).value().to_string();
        if prompt.trim().is_empty() {
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

    fn submit_interaction(
        &mut self,
        choice_ids: Vec<String>,
        cancelled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(interaction) = self.interaction.clone() else {
            return;
        };
        let mut answer_texts = self.interaction_answer_texts.clone();
        if !interaction.steps.is_empty() {
            let current_answer = self.question_answer.read(cx).value().to_string();
            if let Some(answer) = answer_texts.get_mut(self.interaction_step_index) {
                *answer = (!current_answer.trim().is_empty()).then_some(current_answer);
            }
        }
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
        if self.request(protocol::method::INTERACTION_RESPOND, json!(response)) {
            self.interaction = None;
            self.interaction_choice_ids.clear();
            self.interaction_answer_texts.clear();
            self.interaction_step_index = 0;
            self.question_answer.update(cx, |state, cx| state.set_value("", window, cx));
            self.status = if cancelled {
                "Interaction cancelled"
            } else {
                "Decision sent"
            }
            .to_string();
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
        let Some(interaction) = self.interaction.as_ref() else {
            return;
        };
        if interaction.kind == protocol::InteractionKind::Question && !interaction.steps.is_empty() {
            if let Some(selected) = self.interaction_choice_ids.get_mut(step_index) {
                *selected = Some(choice_id);
            }
            cx.notify();
            return;
        }
        self.submit_interaction(vec![choice_id], false, window, cx);
    }

    fn advance_interaction_step(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(interaction) = self.interaction.as_ref() else {
            return;
        };
        let answer = self.question_answer.read(cx).value().to_string();
        if let Some(stored) = self.interaction_answer_texts.get_mut(self.interaction_step_index) {
            *stored = (!answer.trim().is_empty()).then_some(answer);
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
            self.interaction_step_index += 1;
            self.question_answer.update(cx, |state, cx| state.set_value("", window, cx));
            cx.notify();
        }
    }

    fn drain_events(&mut self, cx: &mut Context<Self>) {
        let mut changed = false;
        for _ in 0..MAX_UI_EVENTS_PER_TICK {
            match self.event_rx.try_recv() {
                Ok(event) => {
                    changed = true;
                    self.handle_backend_event(event, cx);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.backend = None;
                    self.fail_workspace_review("Local service disconnected.".into());
                    self.running = false;
                    self.status = "Local service disconnected".to_string();
                    changed = true;
                    break;
                }
            }
        }
        if changed {
            cx.notify();
        }
    }

    fn handle_backend_event(&mut self, event: BackendEvent, cx: &mut Context<Self>) {
        match event {
            BackendEvent::Response { method, result, error } => {
                if self.session_request == Some(method.as_str()) {
                    self.session_request = None;
                }
                if let Some(error) = error {
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
                                self.service_info = Some(info);
                                self.status = "Connected to the local VT Code service".to_string();
                                for method in [
                                    protocol::method::CONFIG_DISCOVER,
                                    protocol::method::CONFIG_SETTINGS_READ,
                                    protocol::method::CONFIG_MODELS,
                                    protocol::method::SESSION_LIST,
                                ] {
                                    if !self.request(method, json!({})) {
                                        self.error =
                                            Some("The local service request queue is unavailable.".to_string());
                                        break;
                                    }
                                }
                            }
                            _ => {
                                self.service_info = None;
                                self.backend = None;
                                self.error = Some("The local service returned an invalid or incompatible initialization response. Rebuild the desktop app and its bundled helper together.".to_string());
                                self.status = "Could not initialize the local VT Code service".to_string();
                            }
                        }
                    }
                    protocol::method::CONFIG_DISCOVER => {
                        self.configuration = serde_json::from_value::<ConfigurationSnapshot>(result).ok();
                        self.sync_cli_theme(cx);
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
                    protocol::method::CONFIG_MODELS => {
                        self.models = serde_json::from_value::<Vec<protocol::ModelInfo>>(result).unwrap_or_default();
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
                            let attachments = std::mem::take(&mut self.attachments);
                            self.reset_session_state();
                            self.attachments = attachments;
                            self.selected_session_id = None;
                            self.configuration = Some(snapshot);
                            self.status = "Runtime options updated. The next message starts a new session.".into();
                        }
                        Err(_) => {
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
                        let mut refreshed = serde_json::from_value::<Vec<SessionSummary>>(result).unwrap_or_default();
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
                        self.status = "Session closed. Start a new session to continue.".to_string();
                    }
                    protocol::method::SESSION_LOAD => {
                        if let Ok(loaded) = serde_json::from_value::<SessionLoadResult>(result) {
                            if self.retained_submission.is_some()
                                && self.selected_session_id.as_deref() != Some(loaded.session.session_id.as_str())
                            {
                                self.error = Some(
                                    "The service returned a different session; the unsent message was retained."
                                        .to_string(),
                                );
                                return;
                            }
                            let retained_submission = self.retained_submission.take();
                            self.load_history(loaded.messages);
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
                            self.status = if loaded.review_history_incomplete || loaded.activity_history_incomplete {
                                "Session loaded. Recent inspector history is partial or unavailable."
                            } else {
                                "Session loaded. Submit a prompt to continue."
                            }
                            .to_string();
                        }
                    }
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
                    protocol::method::INTERACTION_RESPOND => {
                        self.status = "Decision accepted".to_string();
                    }
                    _ => {}
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
            BackendEvent::Interaction(interaction) => {
                if self.selected_session_id.as_deref() != Some(interaction.session_id.as_str())
                    || (interaction.turn_id != "session-start"
                        && !self.notification_matches(&interaction.session_id, Some(&interaction.turn_id)))
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
                self.fail_workspace_review("The local service exited before Git review completed.".into());
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
                if let Some(workspace) = self.pending_workspace.take() {
                    self.start_workspace(workspace, cx);
                } else if authentication_required {
                    self.status = "Provider credentials required".to_string();
                    self.error = Some(
                        "VT Code could not authenticate the configured provider. In Terminal, run `vtcode auth` or `vtcode login <provider>`, then restart the desktop app.".to_string(),
                    );
                } else if self.status != "Connected to the local VT Code service" {
                    self.status = match status {
                        Some(code) => format!("Local service exited with code {code}"),
                        None => "Local service exited".to_string(),
                    };
                }
            }
            BackendEvent::LaunchFailed(error) => {
                self.fail_workspace_review("The local service could not start.".into());
                self.session_request = None;
                if let Some(draft) = self.first_submission.take() {
                    self.retained_submission = Some(draft);
                }
                self.recover_pending_submission(true);
                self.error = Some(error);
                self.status = "Could not connect to the local VT Code service".to_string();
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
                self.refresh_sessions_after_turn();
            }
            ThreadEvent::TurnFailed(event) => {
                self.finish_unverified_turn(&event.message);
            }
            ThreadEvent::TurnBlocked(event) => {
                self.finish_unverified_turn(&event.message);
            }
            ThreadEvent::PlanDelta(delta) => {
                let id = delta.item_id.clone();
                let text = delta.delta.clone();
                self.append_or_update_item(id, "assistant", "Plan", &text, true);
            }
            ThreadEvent::ItemStarted(event) => self.update_runtime_item(&event.item),
            ThreadEvent::ItemUpdated(event) => self.update_runtime_item(&event.item),
            ThreadEvent::ItemCompleted(event) => self.update_runtime_item(&event.item),
            ThreadEvent::Error(event) => {
                let message = event.message.clone();
                self.transcript.push(TranscriptItem::new(
                    format!("runtime-error-{}", uuid_like_id()),
                    "notice".to_string(),
                    "Runtime error".to_string(),
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
        self.refresh_sessions_after_turn();
        self.transcript.push(TranscriptItem::new(
            format!("event-{}", uuid_like_id()),
            "notice".to_string(),
            "Run status".to_string(),
            message.to_string(),
        ));
    }

    /// Reload the session list once a turn ends so archive-derived titles
    /// and completion flags replace the local placeholders.
    fn refresh_sessions_after_turn(&mut self) {
        if self.backend.is_some() {
            let _ = self.request(protocol::method::SESSION_LIST, json!({}));
        }
    }

    fn update_runtime_item(&mut self, item: &vtcode_exec_events::ThreadItem) {
        self.restored_activity.retain(|restored| restored.id != item.id);
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
                let payload = serde_json::to_string_pretty(details).unwrap_or_else(|_| "{}".to_string());
                let text = format!("{status}\n\n{payload}");
                // Keep the raw aggregated output for the inspector's Output
                // tab; it updates in place as streaming events arrive.
                let output = match details {
                    ThreadItemDetails::CommandExecution(command) => Some(command.aggregated_output.clone()),
                    ThreadItemDetails::ToolOutput(tool_output) => Some(tool_output.output.clone()),
                    _ => None,
                };
                if historical {
                    let mut item = TranscriptItem::new(id, "tool".to_string(), title, text);
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
        let mut item = TranscriptItem::new(id, "diff".to_string(), "File changes".to_string(), card.patch.clone());
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
        self.inspector_tab = InspectorTab::Changes;
        cx.notify();
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
            title.to_string(),
            text.to_string(),
        ));
    }

    /// Clear session-owned projections together so another conversation cannot inherit them.
    fn reset_session_state(&mut self) {
        self.transcript.clear();
        self.restored_activity.clear();
        self.changes = ChangesProjection::default();
        self.selected_change = None;
        self.change_viewer = None;
        self.change_viewer_dirty = false;
        self.review_open = false;
        self.workspace_review = None;
        self.review_return_focus = None;
        self.last_usage = None;
        self.finished_turn_id = None;
        self.interaction = None;
        self.interaction_choice_ids.clear();
        self.interaction_answer_texts.clear();
        self.interaction_step_index = 0;
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

    fn load_history(&mut self, messages: Vec<SessionHistoryMessage>) {
        self.reset_session_state();
        for (index, message) in messages.into_iter().enumerate() {
            let title = match message.role.as_str() {
                "user" => "You".to_string(),
                "assistant" => "VT Code".to_string(),
                "tool" => "Tool result".to_string(),
                _ => message.role.clone(),
            };
            // Preserve collapsed archive tool text without inventing activity status.
            let mut item = TranscriptItem::new(format!("history-{index}"), message.role, title, message.text);
            item.has_activity_status = false;
            self.transcript.push(item);
        }
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
        let (id, cursor) = match edge {
            ResizeEdge::Sidebar => ("resize-sidebar", CursorStyle::ResizeLeftRight),
            ResizeEdge::Inspector => ("resize-inspector", CursorStyle::ResizeLeftRight),
        };
        div()
            .id(id)
            .test_support()
            .w(px(5.))
            .h_full()
            .cursor(cursor)
            .hover(|style| style.bg(cx.theme().colors.primary.opacity(0.4)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    this.resizing = Some(PaneResize {
                        edge,
                        start_x: f32::from(event.position.x),
                        start_width: match edge {
                            ResizeEdge::Sidebar => this.sidebar_width,
                            ResizeEdge::Inspector => this.inspector_width,
                        },
                    });
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
    }

    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let mut sidebar = div()
            .w(px(self.sidebar_width))
            .min_w(px(MIN_PANE_WIDTH))
            .h_full()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .bg(themes::mix(theme.colors.background, theme.colors.secondary, 0.3))
            .rounded_lg()
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
                            .label(if self.workspace.is_some() { "Change" } else { "Open" })
                            .ghost()
                            .desktop_disabled(
                                self.workspace_picker_open
                                    || !self.workspace_switch_idle()
                                    || self.retained_submission.is_some(),
                            )
                            .on_click(cx.listener(|this, _, window, cx| this.choose_workspace(window, cx))),
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
                        .on_click(cx.listener(|this, _, _, cx| this.create_session(cx))),
                )
                .child(
                    desktop_button("close-session")
                        .tooltip_with_action("Close session", &CloseSession, Some("VTCodeDesktop"))
                        .icon(IconName::X)
                        .accessibility_label("Close thread")
                        .ghost()
                        .desktop_disabled(self.selected_session_id.is_none() || !self.session_change_allowed(None))
                        .on_click(cx.listener(|this, _, _, cx| this.close_session(cx))),
                ),
        );

        sidebar = sidebar.child(section_label("PROJECTS", theme.colors.muted_foreground));
        let recent = self.recent_workspaces();
        if !recent.is_empty() {
            sidebar = sidebar.child(div().w_full().flex().flex_col().gap_1().children(recent.iter().enumerate().map(
                |(index, path)| {
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
                        desktop_button(format!("workspace-{index}"))
                            .accessibility_label(format!(
                                "{label}{}",
                                if current { " (current workspace)" } else { "" }
                            ))
                            .child(navigation_label(label, IconName::Folder, current, theme.colors.primary))
                            .ghost()
                            .w_full()
                            .desktop_disabled(!self.workspace_switch_idle() || self.retained_submission.is_some())
                            .when(current, |button| {
                                button.bg(theme.colors.secondary).text_color(theme.colors.foreground)
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.select_workspace(PathBuf::from(workspace_path.clone()), cx)
                            })),
                    )
                },
            )));
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
        sidebar = sidebar.child(
            div()
                .id("session-list")
                .flex_1()
                .min_h_0()
                .w_full()
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap_1()
                .children(self.sessions.iter().enumerate().map(|(index, session)| {
                    let session_id = session.session_id.clone();
                    let selected = self.selected_session_id.as_deref() == Some(&session.session_id);
                    let label = session.title.clone().unwrap_or_else(|| {
                        format!("Session {}", session.session_id.chars().take(8).collect::<String>())
                    });
                    div().w_full().child(
                        desktop_button(format!("session-{index}"))
                            .accessibility_label(format!("{label}{}", if selected { " (current thread)" } else { "" }))
                            .child(navigation_label(label, IconName::MessageSquare, selected, theme.colors.primary))
                            .ghost()
                            .desktop_disabled(!self.session_change_allowed(Some(&session.session_id)))
                            .w_full()
                            .justify_start()
                            .when(selected, |button| {
                                button.bg(theme.colors.secondary).text_color(theme.colors.foreground)
                            })
                            .on_click(cx.listener(move |this, _, _, cx| this.load_session(session_id.clone(), cx))),
                    )
                })),
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
        div().w_full().flex().items_center().justify_between().px_8().py_3().child(
            div()
                .min_w_0()
                .flex_1()
                .text_lg()
                .font_semibold()
                .truncate()
                .child(title.to_string()),
        )
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let workspace = self
            .workspace
            .as_ref()
            .and_then(|path| path.file_name())
            .and_then(|name| name.to_str())
            .unwrap_or("Workspace");
        let configuration = self.configuration.as_ref();
        let provider = configuration.map(|config| config.provider.as_str()).unwrap_or("Provider");
        TitleBar::new()
            .w_full()
            .h(px(48.))
            .bg(chrome_background(&theme.colors))
            .border_0()
            .pr_3()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        desktop_button("toggle-sidebar")
                            .accessibility_label("Toggle navigation")
                            .icon(IconName::PanelLeft)
                            .ghost()
                            .tooltip_with_action("Toggle navigation", &ToggleSidebar, Some("VTCodeDesktop"))
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.sidebar_open = !view.sidebar_open;
                                view.navigation_peek = false;
                                cx.notify();
                            })),
                    )
                    .child(brand_logo(22.))
                    .child(div().text_sm().font_semibold().child("VT Code"))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_sm()
                            .text_color(theme.colors.muted_foreground)
                            .child(format!("/ {workspace}")),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        desktop_button("search-commands")
                            .accessibility_label("Search commands")
                            .icon(IconName::Search)
                            .ghost()
                            .tooltip_with_action("Search commands", &OpenCommandPalette, Some("VTCodeDesktop"))
                            .on_click(cx.listener(|view, _, window, cx| view.open_command_palette(window, cx))),
                    )
                    .child(
                        Tag::custom(theme.colors.secondary, theme.colors.foreground, theme.colors.border)
                            .small()
                            .rounded_full()
                            .child(provider.to_string()),
                    )
                    .child(
                        desktop_button("toggle-inspector")
                            .tooltip_with_action("Toggle inspector", &ToggleInspector, Some("VTCodeDesktop"))
                            .label(if self.inspector_open {
                                "Hide panel"
                            } else {
                                "Show panel"
                            })
                            .ghost()
                            .desktop_disabled(self.workspace.is_none())
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_inspector(cx))),
                    )
                    .child(
                        desktop_button("toggle-theme")
                            .small()
                            .icon(IconName::SunMoon)
                            .label(if self.theme_mode == ThemeMode::Light {
                                "Dark"
                            } else {
                                "Light"
                            })
                            .ghost()
                            .on_click(cx.listener(|view, _, window, cx| view.toggle_theme(window, cx))),
                    )
                    .child(
                        desktop_button("open-settings")
                            .accessibility_label("Settings")
                            .icon(IconName::Settings2)
                            .tooltip_with_action("Settings", &OpenSettings, Some("VTCodeDesktop"))
                            .ghost()
                            .on_click(cx.listener(|view, _, window, cx| view.open_settings(window, cx))),
                    )
                    .when_some(
                        self.configuration
                            .as_ref()
                            .and_then(|config| config.session_options.as_ref())
                            .and_then(|options| options.speed.as_ref()),
                        |row, speed| {
                            row.child(
                                Tag::custom(theme.colors.secondary, theme.colors.foreground, theme.colors.border)
                                    .small()
                                    .child(speed.clone()),
                            )
                        },
                    )
                    .child(self.render_chrome_menu("header-overflow", "Window actions")),
            )
    }

    fn render_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let swatches = themes::SYSTEM_SWATCHES
            .iter()
            .copied()
            .chain(themes::all_themes().iter().copied().map(themes::SwatchEntry::Palette));
        let mut swatch_rows = Vec::new();
        for entry in swatches {
            swatch_rows.push(self.render_theme_swatch(entry, cx));
        }
        let theme = cx.theme();
        let colors = &theme.colors;
        let mut panel = div()
            .id("settings-panel")
            .test_support()
            .role(gpui_kit::Role::Dialog)
            .aria_label("Settings")
            .w(px(740.))
            .max_w_full()
            .max_h_full()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_5()
            .p_6()
            .rounded_xl()
            .border_1()
            .border_color(colors.border)
            .bg(colors.background)
            .shadow_lg()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .text_sm()
            .child(
                div()
                    .flex()
                    .items_start()
                    .justify_between()
                    .child(
                        DialogHeader::new()
                            .child(DialogTitle::new().text_2xl().child("Make it yours"))
                            .child(DialogDescription::new().child("Appearance · VT Code Desktop")),
                    )
                    .child(
                        desktop_button("close-settings")
                            .icon(IconName::X)
                            .accessibility_label("Close settings")
                            .ghost()
                            .on_click(cx.listener(|view, _, window, cx| view.close_settings(window, cx))),
                    ),
            )
            .child(
                gpui_kit::component::switch::Switch::new("show-status-bar")
                    .label("Show status bar")
                    .checked(self.preferences.show_status_bar)
                    .on_click(cx.listener(|view, _, _, cx| view.toggle_status_bar(cx))),
            )
            .child(section_label("VISUAL STYLE", colors.muted_foreground))
            .child(
                div()
                    .id("theme-grid")
                    .test_support()
                    .flex_shrink_0()
                    .max_h(px(220.))
                    .overflow_y_scroll()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .children(swatch_rows),
            );

        panel = panel
            .child(
                GroupBox::new()
                    .flex_shrink_0()
                    .outline()
                    .title("Typography")
                    .child(self.render_font_family(FontTarget::Ui, cx))
                    .child(self.render_font_family(FontTarget::Code, cx))
                    .child(self.render_font_size(false, cx))
                    .child(self.render_font_size(true, cx))
                    .child(render_markdown(
                        "appearance-code-preview".into(),
                        "```rust\nfn main() { /* VT Code */ }\n```".into(),
                        cx,
                    )),
            )
            .child(
                GroupBox::new()
                    .flex_shrink_0()
                    .outline()
                    .title("Accessibility")
                    .child(self.render_reduced_motion(cx)),
            )
            .child(
                desktop_button("reset-appearance")
                    .label("Reset appearance to defaults")
                    .ghost()
                    .on_click(cx.listener(|view, _, window, cx| {
                        view.preferences.ui_font_family = None;
                        view.preferences.code_font_family = None;
                        view.preferences.ui_font_size = None;
                        view.preferences.code_font_size = None;
                        view.preferences.reduced_motion = preferences::ReducedMotionPreference::System;
                        preferences::apply_reduced_motion(
                            view.preferences.reduced_motion,
                            view.system_motion_baseline,
                            cx,
                        );
                        view.apply_theme(themes::DEFAULT_THEME_ID.to_string(), window, cx);
                    })),
            );

        if let Some(configuration) = self.configuration.as_ref() {
            panel = panel.child(div().font_semibold().child("Provider")).child(
                div().text_color(colors.muted_foreground).child(format!(
                    "{} · {}",
                    if configuration.provider.is_empty() {
                        "not configured"
                    } else {
                        &configuration.provider
                    },
                    configuration.model
                )),
            );
            if !configuration.credential_setup_command.is_empty() {
                let command = configuration.credential_setup_command.clone();
                panel = panel.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_xs()
                                .font_family(theme.mono_font_family.clone())
                                .text_color(colors.muted_foreground)
                                .truncate()
                                .child(command.clone()),
                        )
                        .child(desktop_button("copy-credential-command").label("Copy").ghost().on_click(
                            move |_, _, app| {
                                app.write_to_clipboard(gpui_kit::ClipboardItem::new_string(command.to_string()))
                            },
                        )),
                );
            }
        }

        panel = panel
            .child(self.render_shortcuts(cx))
            .child(self.render_shared_preferences(cx))
            .child(self.render_cli_configuration(cx));
        if let Some(info) = self.service_info.as_ref() {
            panel = panel.child(div().font_semibold().child("Service")).child(
                div().text_color(colors.muted_foreground).child(format!(
                    "vtcode {} · protocol {}.{}",
                    info.server_version, info.protocol.major, info.protocol.minor
                )),
            );
        }
        panel.into_any_element()
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
                    .bg(colors.background)
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
                            .child(desktop_button("copy-preview").label("Copy").ghost().on_click(move |_, _, app| {
                                app.write_to_clipboard(gpui_kit::ClipboardItem::new_string(text.to_string()))
                            }))
                            .child(
                                desktop_button("close-preview")
                                    .tooltip("Close preview (Escape)")
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
                            .overflow_y_scroll()
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
        if let Some(tab) = InspectorTab::ALL.get(index) {
            self.inspector_tab = *tab;
            self.inspector_focus.focus(window, cx);
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
        self.select_inspector_tab(index, window, cx);
    }

    /// Right-hand context inspector: derived Changes, Files, and Activity
    /// views over the same transcript the conversation renders.
    fn render_inspector(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let colors = theme.colors;
        let panel =
            div()
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
                        .track_focus(&self.inspector_focus)
                        .key_context("VTCodeInspector")
                        .role(gpui_kit::Role::Group)
                        .aria_label("Context inspector tabs")
                        .aria_description("Use Left and Right arrows, Home, or End to choose a tab.")
                        .border_1()
                        .border_color(colors.border.opacity(0.))
                        .focus_visible(|style| style.border_color(colors.ring))
                        .on_action(cx.listener(|view, _: &NextInspectorTab, window, cx| {
                            view.cycle_inspector_tab(true, window, cx)
                        }))
                        .on_action(cx.listener(|view, _: &PreviousInspectorTab, window, cx| {
                            view.cycle_inspector_tab(false, window, cx)
                        }))
                        .on_action(cx.listener(|view, _: &FirstInspectorTab, window, cx| {
                            view.select_inspector_tab(0, window, cx)
                        }))
                        .on_action(cx.listener(|view, _: &LastInspectorTab, window, cx| {
                            view.select_inspector_tab(InspectorTab::ALL.len().saturating_sub(1), window, cx)
                        }))
                        .child(
                            TabBar::new("inspector-tabs")
                                .underline()
                                .small()
                                .w_full()
                                .selected_index(
                                    InspectorTab::ALL
                                        .iter()
                                        .position(|tab| *tab == self.inspector_tab)
                                        .unwrap_or_default(),
                                )
                                .on_click(cx.listener(|view, index: &usize, window, cx| {
                                    view.select_inspector_tab(*index, window, cx)
                                }))
                                .children(InspectorTab::ALL.map(|tab| Tab::new().label(tab.label()))),
                        ),
                );

        let content = match self.inspector_tab {
            InspectorTab::Changes => self.render_changes_tab(&colors, cx),
            InspectorTab::Files => self.render_inspector_files_tab(cx),
            InspectorTab::Activity => self.render_activity_tab(&colors),
            InspectorTab::Output => self.render_output_tab(cx),
            InspectorTab::Utilities => self.render_utilities(cx),
        };
        panel.child(content).into_any_element()
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
                .child("Changes are open in the expanded review.")
                .into_any_element();
        }
        let content = self.render_changes_content(false, colors, cx);
        div()
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
                        .ghost()
                        .small()
                        .desktop_disabled(self.backend.is_none() || self.service_info.is_none())
                        .on_click(cx.listener(|view, _, window, cx| view.open_workspace_review(window, cx))),
                ),
            )
            .child(content)
            .into_any_element()
    }

    fn render_changes_content(
        &mut self,
        expanded: bool,
        colors: &gpui_kit::component::theme::ThemeColor,
        cx: &mut Context<Self>,
    ) -> AnyElement {
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
                        .child(div().text_color(colors.primary).child(format!("+{additions}")))
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
                                .tooltip("Previous changed file")
                                .ghost()
                                .small()
                                .desktop_disabled(self.changes.entries().len() < 2)
                                .on_click(cx.listener(|view, _, _, cx| view.navigate_change_file(false, cx))),
                        )
                        .child(
                            desktop_button("next-change-file")
                                .icon(IconName::ChevronRight)
                                .accessibility_label("Next changed file")
                                .tooltip("Next changed file")
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
                .children(self.changes.entries().iter().enumerate().map(|(index, entry)| {
                    let path = entry.path.clone();
                    let selected = self.selected_change.as_deref() == Some(entry.path.as_str());
                    let kind_label = match entry.kind {
                        DiffFileKind::Add => ("A", colors.primary),
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
                            desktop_button(format!("change-row-{index}"))
                                .icon(IconName::ChevronRight)
                                .accessibility_label(format!("Review {path}"))
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
                            .tooltip("Previous change (⇧F7)")
                            .ghost()
                            .small()
                            .on_click(cx.listener(|view, _, _, cx| view.navigate_review_change(false, cx))),
                    )
                    .child(
                        desktop_button("review-next-change")
                            .icon(IconName::ArrowDown)
                            .accessibility_label("Next change")
                            .tooltip("Next change (F7)")
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
                        .on_click(move |_, _, app| {
                            app.write_to_clipboard(gpui_kit::ClipboardItem::new_string(patch.to_string()))
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
                    .overflow_y_scroll()
                    .child(Diff::new(&viewer).line_number(true).soft_wrap(expanded)),
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
                            .bg(colors.background)
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_action(
                                cx.listener(|view, _: &NextReviewChange, _, cx| view.navigate_review_change(true, cx)),
                            )
                            .on_action(cx.listener(|view, _: &PreviousReviewChange, _, cx| {
                                view.navigate_review_change(false, cx)
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
                                            .tooltip("Close review (Escape)")
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
                        .ghost()
                        .on_click(cx.listener(|view, _, window, cx| {
                            view.file_tree.update(cx, |tree, cx| {
                                if tree.selected_index().is_none() && tree.entry(0).is_some() {
                                    tree.set_selected_index(Some(0), cx);
                                }
                                tree.focus(window, cx);
                            });
                        })),
                )
                .child(
                    desktop_button("refresh-files")
                        .label("Refresh")
                        .ghost()
                        .on_click(cx.listener(|this, _, _, cx| this.refresh_files(cx))),
                ),
        );
        let directory_paths: HashSet<_> = self
            .files
            .iter()
            .filter(|entry| entry.is_dir)
            .map(|entry| entry.rel_path.clone())
            .collect();
        let file_tree = self.file_tree.clone();
        let weak_view = cx.entity().downgrade();
        body = body.child(
            div()
                .id("file-list")
                .flex_1()
                .min_h_0()
                .w_full()
                .on_action(cx.listener(|this, _: &OpenSelectedFile, window, cx| this.open_selected_file(window, cx)))
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
        body.into_any_element()
    }

    /// Live tail of the newest command or tool output, the desktop's
    /// terminal pane.
    fn render_output_tab(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let colors = theme.colors;
        let mono = theme.mono_font_family.clone();
        let Some(output) = inspector::latest_output(&self.transcript, &self.restored_activity) else {
            return empty_state(
                "No output yet",
                "Command and tool output will appear here while a turn runs.",
                IconName::Terminal,
            )
            .into_any_element();
        };
        let tail = inspector::output_tail(&output.text);
        div()
            .w_full()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(
                div()
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
                            .on_click(move |_, _, app| {
                                app.write_to_clipboard(gpui_kit::ClipboardItem::new_string(output.text.clone()))
                            }),
                    ),
            )
            .child(
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

    fn render_activity_tab(&self, colors: &gpui_kit::component::theme::ThemeColor) -> AnyElement {
        let rows = inspector::activity_rows(&self.transcript, &self.restored_activity);
        let mut body = div()
            .id("activity-list")
            .w_full()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_2()
            .p_3();
        if rows.is_empty() {
            return empty_state(
                "No activity yet",
                "Follow tool calls and verification as VT Code works.",
                IconName::Activity,
            )
            .into_any_element();
        }
        for row in rows {
            let (status_color, status_icon) = match row.state {
                ActivityState::Failed => (colors.danger, IconName::X),
                ActivityState::Running => (colors.primary, IconName::Activity),
                ActivityState::Interrupted => (colors.muted_foreground, IconName::X),
                ActivityState::Completed => (colors.muted_foreground, IconName::Check),
                ActivityState::Other => (colors.muted_foreground, IconName::Activity),
            };
            body = body.child(
                div()
                    .id(format!("activity-{}", row.id))
                    .test_support()
                    .role(gpui_kit::Role::Group)
                    .aria_label(row.title.clone())
                    .aria_description(row.status.clone())
                    .w_full()
                    .flex()
                    .items_start()
                    .gap_3()
                    .p_3()
                    .rounded_lg()
                    .border_1()
                    .border_color(colors.border)
                    .bg(colors.background)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_center()
                            .size(px(28.))
                            .flex_shrink_0()
                            .rounded_full()
                            .bg(colors.secondary)
                            .child(Icon::new(status_icon).small().text_color(status_color)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(div().text_sm().font_medium().child(row.title))
                            .child(
                                div().flex().min_w_0().child(
                                    Tag::custom(colors.secondary, colors.foreground, colors.border)
                                        .small()
                                        .rounded_full()
                                        .max_w_full()
                                        .child(div().truncate().child(row.status)),
                                ),
                            ),
                    ),
            );
        }
        body.into_any_element()
    }

    fn render_transcript(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let content = if self.transcript.is_empty() {
            EmptyState::new()
                .header(
                    EmptyHeader::new()
                        .media(EmptyMedia::new().child(brand_logo(40.)))
                        .title(EmptyTitle::new().text_3xl().font_semibold().child("What will you build?"))
                        .description(
                            EmptyDescription::new()
                                .text_sm()
                                .child("Explore your code, make a change, or review your work."),
                        ),
                )
                .size_full()
                .content(
                    EmptyContent::new()
                        .flex_row()
                        .flex_wrap()
                        .justify_center()
                        .gap_3()
                        .child(
                            desktop_button("welcome-browse-files")
                                .accessibility_label("Browse files")
                                .ghost()
                                .small()
                                .icon(IconName::Folder)
                                .label("Explore files")
                                .desktop_disabled(self.workspace.is_none())
                                .on_click(cx.listener(|view, _, _, cx| {
                                    view.inspector_open = true;
                                    view.inspector_tab = InspectorTab::Files;
                                    view.save_preferences();
                                    cx.notify();
                                })),
                        )
                        .child(
                            desktop_button("welcome-review-changes")
                                .accessibility_label("Review changes")
                                .ghost()
                                .small()
                                .icon(IconName::GitCompare)
                                .label("Review changes")
                                .desktop_disabled(self.workspace.is_none())
                                .on_click(cx.listener(|view, _, _, cx| {
                                    view.inspector_open = true;
                                    view.inspector_tab = InspectorTab::Changes;
                                    view.save_preferences();
                                    cx.notify();
                                })),
                        )
                        .child(
                            desktop_button("welcome-choose-model")
                                .label("Choose a model")
                                .ghost()
                                .small()
                                .desktop_disabled(!self.session_change_allowed(None))
                                .on_click(cx.listener(|view, _, window, cx| view.open_model_palette(window, cx))),
                        ),
                )
                .into_any_element()
        } else {
            list(
                self.transcript.list_state(),
                cx.processor(|view, index, _, cx| {
                    if let Some(item) = view.transcript.get(index) {
                        let group = view.transcript.activity_group(index);
                        let group_head = group.as_ref().and_then(|range| view.transcript.get(range.start));
                        if group.as_ref().is_some_and(|range| index != range.start)
                            && group_head.is_some_and(|head| !head.activity_expanded)
                        {
                            return div().into_any_element();
                        }
                        let group_header = group
                            .as_ref()
                            .filter(|range| index == range.start)
                            .map(|range| view.render_activity_group(range.clone(), cx));
                        let show_item = group.is_none() || group_head.is_some_and(|head| head.activity_expanded);
                        div()
                            .id(format!("transcript-row-{}", item.id))
                            .role(gpui_kit::Role::ListItem)
                            .test_support()
                            .w_full()
                            .px_8()
                            .when(item.role == "tool", |row| row.pb_1())
                            .when(item.role != "tool", |row| row.pb_4())
                            .children(group_header)
                            .when(show_item, |row| row.child(view.render_transcript_item(item, cx)))
                            .into_any_element()
                    } else {
                        div().into_any_element()
                    }
                }),
            )
            .size_full()
            .py_6()
            .into_any_element()
        };
        div()
            .id("transcript-scroll")
            .aria_label("Conversation")
            .role(gpui_kit::Role::List)
            .test_support()
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_hidden()
            .child(content)
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
            .alignment(if user {
                MessageAlignment::End
            } else {
                MessageAlignment::Start
            })
            .w_full()
            .max_w(px(CONTENT_WIDTH))
            .header(
                MessageHeader::new().child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .when(!user, |row| row.child(brand_logo(18.)))
                        .child(
                            div()
                                .text_xs()
                                .font_semibold()
                                .text_color(colors.muted_foreground)
                                .child(label.to_string()),
                        ),
                ),
            )
            .content(
                MessageContent::new().bubble(
                    Bubble::new()
                        .with_variant(if user {
                            BubbleVariant::Outline
                        } else {
                            BubbleVariant::Ghost
                        })
                        .content(
                            BubbleContent::new()
                                .rounded_xl()
                                .when(user, |surface| {
                                    surface.bg(colors.secondary).text_color(colors.foreground).px_4().py_3()
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
                                            cx,
                                        )),
                                ),
                        ),
                ),
            )
            .footer(
                MessageFooter::new().child(
                    desktop_button(format!("copy-message-{}", item.id))
                        .icon(IconName::Copy)
                        .accessibility_label("Copy message")
                        .tooltip("Copy message")
                        .ghost()
                        .xsmall()
                        .on_click(move |_, _, app| {
                            app.write_to_clipboard(gpui_kit::ClipboardItem::new_string(copy_text.to_string()))
                        }),
                ),
            )
            .into_any_element()
    }

    fn render_activity_group(&self, range: std::ops::Range<usize>, cx: &mut Context<Self>) -> AnyElement {
        let Some(head) = self.transcript.get(range.start) else {
            return div().into_any_element();
        };
        let id = head.id.clone();
        let mut failed = 0;
        let mut running = 0;
        for item in range.clone().filter_map(|index| self.transcript.get(index)) {
            match ActivityState::from_status(item.text.lines().next().unwrap_or_default()) {
                ActivityState::Failed => failed += 1,
                ActivityState::Running => running += 1,
                _ => {}
            }
        }
        let mut label = format!("{} tool actions", range.len());
        if running > 0 {
            label.push_str(&format!(" · {running} running"));
        }
        if failed > 0 {
            label.push_str(&format!(" · {failed} failed"));
        }
        desktop_button(format!("activity-group-{}", head.id))
            .icon(if head.activity_expanded {
                IconName::ChevronDown
            } else {
                IconName::ChevronRight
            })
            .label(label.clone())
            .accessibility_label(format!("{} {label}", if head.activity_expanded { "Collapse" } else { "Expand" }))
            .ghost()
            .small()
            .when(failed > 0, |button| button.text_color(cx.theme().colors.danger))
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
        let icon = match state {
            ActivityState::Completed => IconName::Check,
            ActivityState::Failed | ActivityState::Interrupted => IconName::X,
            ActivityState::Running | ActivityState::Other => IconName::Activity,
        };
        let mut panel = div().w_full().max_w(px(CONTENT_WIDTH)).flex().flex_col().gap_1().child(
            desktop_button(format!("tool-{}", item.id))
                .accessibility_label(format!(
                    "{} for {}",
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
                        .text_color(if state == ActivityState::Failed {
                            colors.danger
                        } else {
                            colors.muted_foreground
                        })
                        .child(Icon::new(icon).small())
                        .child(div().flex_1().min_w_0().truncate().child(item.title.clone()))
                        .child(div().text_xs().child(status.to_string()))
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
            let output = item.output.as_deref().filter(|output| !output.is_empty()).unwrap_or(&item.text);
            panel = panel.child(
                div()
                    .id(format!("tool-output-{}", item.id))
                    .role(gpui_kit::Role::Label)
                    .aria_label(output.to_string())
                    .test_support()
                    .ml_4()
                    .pl_3()
                    .py_2()
                    .border_l_1()
                    .border_color(colors.border)
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_size(cx.theme().mono_font_size)
                    .child(output.to_string()),
            );
        }
        panel.into_any_element()
    }

    fn render_diff_item(&self, item: &TranscriptItem, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let mono_font = cx.theme().mono_font_family.clone();
        let Some(diff) = item.diff.as_ref() else {
            return div().into_any_element();
        };
        let id = item.id.clone();
        let mut stats = String::new();
        if let Some(additions) = diff.additions {
            stats.push_str(&format!("+{additions} "));
        }
        if let Some(deletions) = diff.deletions {
            stats.push_str(&format!("−{deletions} "));
        }
        let status_label = if diff.failed {
            "patch failed"
        } else if diff.incomplete {
            "preview incomplete"
        } else {
            "applied"
        };

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
                    .child(
                        div()
                            .text_xs()
                            .text_color(colors.muted_foreground)
                            .child(stats.trim_end().to_string()),
                    )
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
                        this.transcript.toggle_diff(&id);
                        cx.notify();
                    })),
            );

        if !diff.patch.is_empty() {
            let copy_patch = diff.patch.clone();
            header = header.child(
                desktop_button(format!("copy-patch-{}", item.id))
                    .label("Copy patch")
                    .ghost()
                    .on_click(move |_, _, app| {
                        app.write_to_clipboard(gpui_kit::ClipboardItem::new_string(copy_patch.to_string()))
                    }),
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
                DiffFileKind::Add => ("added", colors.primary),
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
            let (lines, truncated) = classify_diff_lines(&diff.patch);
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
            for line in &lines {
                let (line_color, line_bg) = match line.kind {
                    DiffLineKind::Added => (colors.foreground, colors.primary.opacity(0.12)),
                    DiffLineKind::Removed => (colors.foreground, colors.danger.opacity(0.12)),
                    DiffLineKind::Hunk => (colors.muted_foreground, colors.secondary),
                    DiffLineKind::Header | DiffLineKind::Context => (colors.foreground, colors.background),
                };
                diff_view = diff_view.child(
                    div()
                        .w_full()
                        .px_2()
                        .text_xs()
                        .font_family(mono_font.clone())
                        .whitespace_nowrap()
                        .text_color(line_color)
                        .bg(line_bg)
                        .child(line.text.to_string()),
                );
            }
            if truncated {
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
            panel = panel.child(div().w_full().flex().flex_wrap().gap_2().children(
                self.attachments.iter().enumerate().map(|(index, attachment)| {
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
                                desktop_button(format!("remove-attachment-{index}"))
                                    .icon(IconName::X)
                                    .accessibility_label(format!("Remove {}", attachment.path))
                                    .ghost()
                                    .xsmall()
                                    .on_click(
                                        cx.listener(move |this, _, _, cx| this.remove_attachment(path.clone(), cx)),
                                    ),
                            ),
                        )
                }),
            ));
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
                                move |_, _, cx| {
                                    cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(prompt.clone()))
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
            panel = panel.child(
                div()
                    .id("composer-error")
                    .test_support()
                    .role(gpui_kit::Role::Alert)
                    .aria_label(error.clone())
                    .w_full()
                    .text_sm()
                    .text_color(colors.danger)
                    .child(error.clone()),
            );
        }
        if let Some(interaction) = self.interaction.clone() {
            panel = panel.child(self.render_interaction(&interaction, window, cx));
        }

        let can_submit = self.session_change_idle()
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
            .and_then(|config| config.sandbox.as_deref())
            .unwrap_or("Permissions unavailable");
        let buttons = div()
            .w_full()
            .min_w_0()
            .flex()
            .flex_wrap()
            .items_center()
            .justify_between()
            .gap_2()
            .child(
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
                            .on_click(cx.listener(|view, _, window, cx| view.open_add_context(window, cx))),
                    )
                    .child(
                        desktop_button("composer-policy")
                            .accessibility_label(format!("Execution policy: {policy}"))
                            .tooltip_with_action("Command permissions", &OpenPermissions, Some("VTCodeDesktop"))
                            .icon(IconName::Shield)
                            .child(div().truncate().child(policy.to_string()))
                            .dropdown_caret(true)
                            .ghost()
                            .small()
                            .min_w_0()
                            .max_w(px(230.))
                            .desktop_disabled(!self.can_select_session_options())
                            .on_click(cx.listener(|view, _, window, cx| view.open_permissions(window, cx))),
                    ),
            );
        let mut model_actions = div().min_w_0().max_w_full().flex().flex_wrap().items_center().gap_2().child(
            desktop_button("pick-model")
                .accessibility_label(current_model.clone())
                .child(div().min_w_0().max_w(px(260.)).truncate().child(current_model))
                .dropdown_caret(true)
                .small()
                .max_w(px(320.))
                .overflow_hidden()
                .tooltip("Search models by provider, name, or model ID")
                .ghost()
                .desktop_disabled(!self.session_change_allowed(None) || self.attachments_picker_open)
                .on_click(cx.listener(|view, _, window, cx| view.open_model_options(window, cx))),
        );
        if self.running {
            model_actions = model_actions.child(
                desktop_button("cancel-turn")
                    .tooltip_with_action("Cancel active run", &CancelRun, Some("VTCodeDesktop"))
                    .accessibility_label("Cancel run")
                    .icon(IconName::Square)
                    .rounded_full()
                    .danger()
                    .on_click(cx.listener(|this, _, _, cx| this.cancel_turn(cx))),
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
        let workspace = self
            .workspace
            .as_ref()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Choose workspace".into());
        if self.palette_kind.is_composer_popup() {
            if let Some(state) = self.palette.clone() {
                if matches!(self.palette_kind, PaletteKind::Models | PaletteKind::ModelOptions) {
                    panel = panel.child(
                        div()
                            .absolute()
                            .top(px(8.))
                            .right(px(16.))
                            .w_0()
                            .h_0()
                            .child(self.render_command_palette(state, cx)),
                    );
                } else {
                    panel = panel.child(self.render_command_palette(state, cx));
                }
            }
        }
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
                .rounded_lg()
                .bg(colors.background)
                .child(
                    desktop_button("composer-workspace")
                        .icon(IconName::Folder)
                        .label(workspace)
                        .ghost()
                        .small()
                        .max_w(px(240.))
                        .overflow_hidden()
                        .accessibility_label("Choose workspace")
                        .desktop_disabled(
                            !self.workspace_switch_idle()
                                || self.retained_submission.is_some()
                                || self.workspace_picker_open,
                        )
                        .on_click(cx.listener(|view, _, window, cx| view.choose_workspace(window, cx))),
                )
                .child(
                    desktop_button("composer-cli-webmcp")
                        .icon(IconName::Terminal)
                        .label("CLI / WebMCP")
                        .accessibility_label("VT Code CLI and WebMCP documentation")
                        .tooltip("Shared CLI runtime · WebMCP integration")
                        .ghost()
                        .small()
                        .on_click(|_, _, cx| {
                            cx.open_url("https://github.com/vinhnx/VTCode/blob/main/docs/development/webmcp.md")
                        }),
                )
                .child(self.render_mode_button(cx))
                .child(
                    desktop_button("composer-settings")
                        .icon(IconName::Settings2)
                        .ghost()
                        .small()
                        .ml_auto()
                        .accessibility_label("Composer settings")
                        .tooltip_with_action("Settings", &OpenSettings, Some("VTCodeDesktop"))
                        .on_click(cx.listener(|view, _, window, cx| view.open_settings(window, cx))),
                ),
        );
        div().w_full().flex().justify_center().child(
            panel.child(
                div()
                    .w_full()
                    .on_action(cx.listener(|view, _: &Enter, window, cx| view.submit_prompt(window, cx)))
                    .child(
                        InputGroup::new("message-composer")
                            .aria_label("Message composer")
                            .w_full()
                            .rounded(px(22.))
                            .p_2()
                            .bg(colors.secondary)
                            .input(input)
                            .addon(
                                InputGroupAddon::new("composer-actions")
                                    .align(InputGroupAddonAlignment::BlockEnd)
                                    .child(buttons),
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
        div()
            .id("status-bar")
            .test_support()
            .w_full()
            .h(px(26.))
            .flex()
            .items_center()
            .justify_between()
            .gap_3()
            .px_3()
            .border_t_1()
            .border_color(colors.border)
            .text_xs()
            .text_color(colors.muted_foreground)
            .child(
                div()
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
                    }),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
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
                            .text_color(agent.1)
                            .child(agent.0),
                    ),
            )
    }

    fn toggle_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let id = if self.theme_mode == ThemeMode::Light {
            themes::SYSTEM_DARK_ID
        } else {
            themes::SYSTEM_LIGHT_ID
        };
        self.apply_theme(id.to_owned(), window, cx);
    }

    /// Apply a theme from the picker and remember the choice.
    fn apply_theme(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.preferences.follow_cli_theme = Some(false);
        self.apply_theme_state(id, Some(window), cx);
    }

    fn apply_theme_state(&mut self, id: String, window: Option<&mut Window>, cx: &mut Context<Self>) {
        self.theme_mode = themes::apply_by_id(&id, window, cx);
        self.theme_id = id;
        self.apply_typography(cx);
        self.transcript.remeasure();
        self.save_preferences();
        cx.notify();
    }

    fn apply_typography(&mut self, cx: &mut Context<Self>) {
        let ui = self.preferences.ui_font_size();
        let code = self.preferences.code_font_size();
        let ui_family = typography::resolve_font(
            self.preferences.ui_font_family.as_deref(),
            ".SystemUIFont",
            typography::installed_fonts(cx),
        );
        let code_family = typography::resolve_font(
            self.preferences.code_font_family.as_deref(),
            self.default_code_font.as_str(),
            typography::installed_fonts(cx),
        );
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
                self.preferences.code_font_size(),
                10,
                24,
            )
        } else {
            (
                "ui",
                "UI font size",
                "Base size for controls and conversation text",
                self.preferences.ui_font_size(),
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
                                    view.preferences.code_font_size()
                                } else {
                                    view.preferences.ui_font_size()
                                };
                                let next = if delta < 0 {
                                    current.saturating_sub(1)
                                } else {
                                    current.saturating_add(1)
                                }
                                .clamp(minimum, maximum);
                                if code {
                                    view.preferences.code_font_size = Some(next);
                                } else {
                                    view.preferences.ui_font_size = Some(next);
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
        let selected = self.theme_id == entry.id();
        let id = format!("theme-{}", entry.id());
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
            swatch = swatch.child(div().text_xs().text_color(cx.theme().colors.primary).child("active"));
        }
        desktop_button(id)
            .accessibility_label(format!("{}{}", entry.label(), if selected { " (active)" } else { "" }))
            .ghost()
            .w(px(104.))
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
        let mut panel = div()
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
                    desktop_button("cancel-interaction").label("Cancel request").ghost().on_click(
                        cx.listener(|this, _, window, cx| this.submit_interaction(Vec::new(), true, window, cx)),
                    ),
                )
                .when(
                    interaction.kind == protocol::InteractionKind::Question && !interaction.steps.is_empty(),
                    |row| {
                        row.child(
                            desktop_button("continue-question")
                                .label(if self.interaction_step_index + 1 == interaction.steps.len() {
                                    "Submit answers"
                                } else {
                                    "Next question"
                                })
                                .primary()
                                .on_click(cx.listener(|this, _, window, cx| this.advance_interaction_step(window, cx))),
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
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = cx.theme().colors;
        let mut content = div()
            .w_full()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child(format!("Question {} of {total}", index + 1)),
            )
            .child(div().font_medium().child(step.question.clone()));
        for choice in &step.choices {
            content = content.child(self.render_choice(choice, index, window, cx));
        }
        if step.allow_freeform {
            content = content.child(
                div().w_full().border_1().border_color(colors.border).rounded_sm().p_2().child(
                    Textarea::new(&self.question_answer)
                        .accessibility_id("question-answer")
                        .aria_label("Your answer")
                        .h(px(72.)),
                ),
            );
        }
        content
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
        let mut button = desktop_button(format!("choice-{}", choice.id)).label(label).w_full().ghost();
        if let Some(description) = choice.description.as_ref() {
            button = button.tooltip(description.clone());
        }
        let _ = step_index;
        div().w_full().child(button.on_click(cx.listener(move |this, _, window, cx| {
            this.choose_interaction_option(step_index, choice_id.clone(), window, cx);
        })))
    }

    /// Modal ⌘K palette. The state entity is created on open, never during
    /// render; confirm and cancel run through the owning view.
    fn render_command_palette(&self, state: Entity<CommandState>, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let weak = cx.entity().downgrade();
        let groups = if self.palette_kind == PaletteKind::AddContext {
            self.composer_resource_groups()
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
        let composer_popup = self.palette_kind.is_composer_popup();
        let title = match self.palette_kind {
            PaletteKind::Models => "Choose a model",
            PaletteKind::ModelOptions => "Model and reasoning effort",
            PaletteKind::AddContext => "Add files and more @",
            PaletteKind::Permissions => "Command permissions",
            PaletteKind::Modes => "Agent mode",
            PaletteKind::Fonts(FontTarget::Ui) => "Choose UI font",
            PaletteKind::Fonts(FontTarget::Code) => "Choose code font",
            PaletteKind::Commands => "Command palette",
        };
        let catalog_empty = confirm_groups.is_empty();
        let quick_model = self.palette_kind == PaletteKind::ModelOptions;
        let mut command = Command::new(&state)
            .placeholder(if matches!(self.palette_kind, PaletteKind::Fonts(_)) {
                "Search installed fonts…"
            } else if models {
                "Search providers, models, or IDs…"
            } else {
                "Type a command or search…"
            })
            .max_h(px(360.))
            .when(models, |command| {
                command.header(|_, _, _| div().px_3().py_2().font_semibold().child("Choose a model"))
            })
            .empty(move |_, _, _| {
                empty_state(
                    if models && catalog_empty {
                        "No models available"
                    } else {
                        "No matching results"
                    },
                    if models && catalog_empty {
                        "Reopen VT Code to reload the model catalog."
                    } else {
                        "Try a different search."
                    },
                    IconName::Search,
                )
            });
        if quick_model {
            let slider = self.effort_slider.clone();
            let configuration = self.configuration.clone();
            let label = configuration.as_ref().map(|config| {
                self.models
                    .iter()
                    .find(|model| model.provider == config.provider && model.id == config.model)
                    .map(|model| model.display_name.clone())
                    .unwrap_or_else(|| config.model.clone())
            });
            command = command.searchable(false).header(move |_, _, cx| {
                composer_controls::model_options_header(configuration.as_ref(), slider.as_ref(), label.as_deref(), cx)
            });
        }
        for (section_index, (label, entries)) in confirm_groups.iter().enumerate() {
            let mut group = CommandGroup::new().label(label.clone());
            for (row_index, entry) in entries.iter().enumerate() {
                let label = entry.label.clone();
                let description = entry.description.clone();
                let selected = self.is_current_palette_action(&entry.action);
                let id = format!("palette-label-{section_index}-{row_index}");
                let accessible_label = description
                    .as_ref()
                    .map(|details| format!("{label} · {details}"))
                    .unwrap_or_else(|| label.clone());
                group = group.item(
                    CommandItem::new()
                        .disabled(!self.palette_action_available(&entry.action))
                        .label(label.clone())
                        .checked(selected)
                        .keywords(entry.keywords.clone())
                        .child(move |_, _| {
                            div()
                                .id(id.clone())
                                .test_support()
                                .flex_1()
                                .min_w_0()
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
                                        .child(div().truncate().font_medium().child(label.clone()))
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
                                .when(selected, |row| {
                                    row.child(Icon::new(IconName::Check).small().text_color(colors.foreground))
                                })
                        }),
                );
            }
            command = command.group(group);
        }
        let confirm_weak = weak.clone();
        let cancel_weak = weak.clone();
        let command = command
            .on_confirm(move |path, _window, cx: &mut App| {
                let Some(action) = confirm_groups
                    .get(path.section)
                    .and_then(|(_, entries)| entries.get(path.row))
                    .map(|entry| entry.action.clone())
                else {
                    return;
                };
                let _updated = confirm_weak.update(cx, |view, cx| {
                    view.run_palette_action(action, _window, cx);
                });
            })
            .on_cancel(move |window, cx: &mut App| {
                let _updated = cancel_weak.update(cx, |view, cx| view.close_palette(window, cx));
            });

        let panel = div()
            .id("command-palette")
            .test_support()
            .role(gpui_kit::Role::Dialog)
            .aria_label(title)
            .w(px(if models { 440. } else { 560. }))
            .max_w_full()
            .when(models, |panel| {
                panel
                    .rounded_xl()
                    .border_1()
                    .border_color(colors.border)
                    .bg(colors.background)
                    .overflow_hidden()
            })
            .on_mouse_down_out(move |_, window, cx| {
                let _updated = weak.update(cx, |view, cx| view.close_palette(window, cx));
            })
            .on_action(|_: &gpui_kit::base::actions::Cancel, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(command)
            .focus_trap("command-palette", &self.palette_focus);
        if composer_popup {
            return deferred(
                anchored()
                    .anchor(if matches!(self.palette_kind, PaletteKind::Models | PaletteKind::ModelOptions) {
                        Anchor::BottomRight
                    } else {
                        Anchor::BottomLeft
                    })
                    .offset(point(px(0.), px(-8.)))
                    .snap_to_window_with_margin(px(12.))
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
            .pt(px(90.))
            .bg(colors.background.opacity(0.4))
            .child(panel)
            .into_any_element()
    }
}

impl Render for DesktopView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.review_open && self.review_focus.contains_focused(window, cx) {
            self.review_return_focus = None;
            self.focus_composer(window, cx);
        }
        let colors = cx.theme().colors;
        let inspector_visible = self.inspector_open && self.workspace.is_some();
        let sidebar = self.render_navigation(cx);
        let panes = div()
            .id("pane-root")
            .p_3()
            .gap_1()
            .flex_1()
            .min_h_0()
            .flex()
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                let Some(drag) = this.resizing else { return };
                let delta = f32::from(event.position.x) - drag.start_x;
                let width = drag.start_width
                    + match drag.edge {
                        ResizeEdge::Sidebar => delta,
                        ResizeEdge::Inspector => -delta,
                    };
                let clamped = width.clamp(MIN_PANE_WIDTH, MAX_PANE_WIDTH);
                match drag.edge {
                    ResizeEdge::Sidebar => this.sidebar_width = clamped,
                    ResizeEdge::Inspector => this.inspector_width = clamped,
                }
                cx.stop_propagation();
                cx.notify();
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, cx| {
                    if this.resizing.take().is_some() {
                        this.save_preferences();
                        cx.notify();
                    }
                }),
            )
            .child(sidebar)
            .when(self.sidebar_open, |panes| panes.child(self.render_resize_handle(ResizeEdge::Sidebar, cx)))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .flex_col()
                    .child(self.render_thread_header())
                    .child(self.render_transcript(cx))
                    .child(self.render_composer(window, cx)),
            )
            .when(inspector_visible, |panes| {
                panes
                    .child(self.render_resize_handle(ResizeEdge::Inspector, cx))
                    .child(self.render_inspector(cx))
            });
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
                view.sidebar_open = !view.sidebar_open;
                view.navigation_peek = false;
                cx.notify();
            }))
            .on_action(cx.listener(|view, _: &ToggleStatusBar, _, cx| view.toggle_status_bar(cx)))
            .on_action(cx.listener(|view, _: &ReloadCliSettings, _, cx| view.reload_cli_settings(cx)))
            .on_action(cx.listener(|view, _: &SyncCliTheme, _, cx| view.sync_theme_to_cli(cx)))
            .on_action(cx.listener(|view, _: &FocusComposer, window, cx| view.activate_composer(window, cx)))
            .when(self.workspace.is_some(), |body| {
                body.on_action(cx.listener(|view, _: &ToggleInspector, _, cx| view.toggle_inspector(cx)))
            })
            .when(self.running, |body| body.on_action(cx.listener(|view, _: &CancelRun, _, cx| view.cancel_turn(cx))))
            .on_action(cx.listener(|view, _: &DismissOverlay, window, cx| view.dismiss_overlay(window, cx)))
            .on_action(cx.listener(|view, _: &OpenCommandPalette, window, cx| view.open_command_palette(window, cx)))
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
            .when(self.preferences.show_status_bar, |body| body.child(self.render_status_bar(cx)));
        if self.workspace.is_none() {
            body = body.child(
                div().absolute().inset_0().flex().items_center().justify_center().child(
                    desktop_button("open-first-workspace")
                        .label("Choose a workspace folder")
                        .primary()
                        .desktop_disabled(
                            self.workspace_picker_open
                                || !self.workspace_switch_idle()
                                || self.retained_submission.is_some(),
                        )
                        .on_click(cx.listener(|this, _, window, cx| this.choose_workspace(window, cx))),
                ),
            );
        }
        if self.file_preview.is_some() {
            body = body.child(self.render_file_preview(cx));
        }
        if self.review_open {
            body = body.child(self.render_change_review(cx));
        }
        if self.settings_open {
            body = body.child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .p_8()
                    .bg(colors.background.opacity(0.85))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|view, _, window, cx| view.close_settings(window, cx)),
                    )
                    .child(
                        div()
                            .w(px(740.))
                            .max_w_full()
                            .h_full()
                            .min_w_0()
                            .min_h_0()
                            .child(self.render_settings(cx))
                            .focus_trap("settings-trap", &self.settings_focus),
                    ),
            );
        }
        if !self.palette_kind.is_composer_popup() {
            if let Some(state) = self.palette.clone() {
                body = body.child(self.render_command_palette(state, cx));
            }
        }
        if self.config_editor.is_some() {
            body = body.child(self.render_cli_config_editor(cx));
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
    static LOGO: std::sync::OnceLock<std::sync::Arc<gpui_kit::Image>> = std::sync::OnceLock::new();
    let image = LOGO.get_or_init(|| {
        std::sync::Arc::new(gpui_kit::Image::from_bytes(
            gpui_kit::ImageFormat::Png,
            include_bytes!("../../../../../resources/icons/vtcode-profile-120.png").to_vec(),
        ))
    });
    gpui_kit::img(image.clone()).w(px(size)).h(px(size)).rounded_md()
}

fn empty_state(title: &'static str, description: impl Into<gpui_kit::SharedString>, icon: IconName) -> EmptyState {
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
        action,
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

enum DiffLineKind {
    Header,
    Hunk,
    Added,
    Removed,
    Context,
}

/// Split a unified diff into classified lines, bounded to keep one transcript
/// row renderable regardless of patch size.
fn classify_diff_lines(patch: &str) -> (Vec<DiffLine<'_>>, bool) {
    const MAX_DIFF_LINES: usize = 2_000;
    let mut lines = Vec::new();
    let mut truncated = false;
    for line in patch.lines() {
        if lines.len() == MAX_DIFF_LINES {
            truncated = true;
            break;
        }
        let kind = if line.starts_with("+++") || line.starts_with("---") || line.starts_with("diff ") {
            DiffLineKind::Header
        } else if line.starts_with("@@") {
            DiffLineKind::Hunk
        } else if line.starts_with('+') {
            DiffLineKind::Added
        } else if line.starts_with('-') {
            DiffLineKind::Removed
        } else {
            DiffLineKind::Context
        };
        lines.push(DiffLine { kind, text: line });
    }
    (lines, truncated)
}

fn desktop_window_options() -> WindowOptions {
    let mut options = TitleBar::window_options();
    if let Some(titlebar) = options.titlebar.as_mut() {
        titlebar.traffic_light_position = Some(point(px(12.), px(17.)));
    }
    options
}

fn chrome_background(colors: &gpui_kit::component::theme::ThemeColor) -> gpui_kit::Hsla {
    themes::mix(colors.background, colors.secondary, 0.08)
}

/// Native clickable controls share one cursor policy; disabled controls keep the arrow.
fn desktop_button(id: impl Into<gpui_kit::ElementId>) -> Button {
    Button::new(id).cursor_pointer()
}

trait DesktopButtonExt {
    fn desktop_disabled(self, disabled: bool) -> Self;
}

impl DesktopButtonExt for Button {
    fn desktop_disabled(self, disabled: bool) -> Self {
        self.disabled(disabled).cursor(if disabled {
            CursorStyle::Arrow
        } else {
            CursorStyle::PointingHand
        })
    }
}

#[cfg(test)]
mod tests;
