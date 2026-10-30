//! App startup: menus, keybindings, window reopen, construction, and preference persistence.
use super::*;

pub(super) fn init_keybindings(cx: &mut App, preferences: &preferences::Preferences) {
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
        KeyBinding::new("cmd-w", CloseWindow, Some(key_context::DESKTOP)),
        KeyBinding::new("cmd-m", MinimizeWindow, Some(key_context::DESKTOP)),
        KeyBinding::new("cmd-shift-n", ShowMainWindow, None),
        KeyBinding::new("cmd-q", QuitApplication, None),
        KeyBinding::new("cmd-h", HideApplication, None),
        KeyBinding::new("alt-cmd-h", HideOtherApplications, None),
        KeyBinding::new("right", NextInspectorTab, Some("VTCodeInspector")),
        KeyBinding::new("left", PreviousInspectorTab, Some("VTCodeInspector")),
        KeyBinding::new("home", FirstInspectorTab, Some("VTCodeInspector")),
        KeyBinding::new("end", LastInspectorTab, Some("VTCodeInspector")),
        KeyBinding::new("right", NextSettingsTab, Some(key_context::SETTINGS)),
        KeyBinding::new("left", PreviousSettingsTab, Some(key_context::SETTINGS)),
        KeyBinding::new("home", FirstSettingsTab, Some(key_context::SETTINGS)),
        KeyBinding::new("end", LastSettingsTab, Some(key_context::SETTINGS)),
        KeyBinding::new("enter", OpenSelectedFile, Some(gpui_kit::base::tree_key_context())),
    ]);
}

pub(super) fn init_native_menus(cx: &mut App) {
    let _app = cx.on_action(|_: &QuitApplication, cx| cx.quit());
    let _app = cx.on_action(|_: &HideApplication, cx| cx.hide());
    let _app = cx.on_action(|_: &HideOtherApplications, cx| cx.hide_other_apps());
    let _app = cx.on_action(|_: &ShowAllApplications, cx| cx.unhide_other_apps());
    let _app = cx.on_action(|_: &ShowMainWindow, cx| show_main_window(cx));
    cx.set_menus([
        Menu::new(app::NAME).items([
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

pub(crate) fn run() {
    let preferences = preferences::Preferences::load();
    super::super::localization::initialize(preferences.ui_language());
    let application = gpui_kit::application().with_assets(super::super::assets::DesktopAssets);
    let _application = application.on_reopen(show_main_window);
    application.run(move |cx: &mut App| {
        gpui_kit::init(cx);
        super::super::notifications::initialize(cx);
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
pub(super) fn open_or_activate_main_window(
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
    let start_maximized = preferences.window_maximized();
    let (window, view) = gpui_kit::open_window(desktop_window_options(), cx, move |window, cx| {
        window.set_window_title(app::NAME);
        let theme_mode = themes::apply_by_id(&start_theme, Some(window), cx);
        let view = cx.new(|cx| DesktopView::new_with_preferences(window, cx, theme_mode, preferences));
        view.update(cx, |view, cx| view.load_preferences(window, cx));
        view
    })?;
    if start_maximized {
        // macOS ignores WindowBounds::Maximized at creation, so zoom after
        // opening to reach the native zoomed state the Zoom menu toggles.
        // The headless test platform leaves zoom unimplemented.
        #[cfg(not(test))]
        window.update(cx, |_, window, _| window.zoom_window())?;
    }
    // Reopen the most recent workspace when it still exists.
    let last_workspace = view.read(cx).preferences.recent_workspaces.first().map(PathBuf::from);
    if let Some(path) = last_workspace {
        window.update(cx, |_, _, cx| view.update(cx, |view, cx| view.select_workspace_on_startup(path, cx)))?;
    }
    view.update(cx, |view, cx| view.start_event_drain(cx));
    cx.activate(true);
    Ok(window)
}

impl DesktopView {
    #[cfg(test)]
    pub(super) fn new(window: &mut Window, cx: &mut Context<Self>, theme_mode: ThemeMode) -> Self {
        Self::new_with_preferences(window, cx, theme_mode, preferences::Preferences::load())
    }

    pub(super) fn new_with_preferences(
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
        let window_bounds_subscription = cx.observe_window_bounds(window, |view, window, _| {
            view.persist_window_maximized(window);
        });
        let welcome_activation_subscription = cx.observe_window_activation(window, |view, window, cx| {
            if window.is_window_active() {
                view.refresh_welcome_period(cx);
            }
        });
        focus_handle.focus(window, cx);
        let system_motion_baseline = preferences::system_motion_baseline(cx);
        preferences::apply_reduced_motion(preferences.reduced_motion, system_motion_baseline, cx);
        let shortcut_capture_focus = cx.focus_handle().tab_stop(true);
        let shortcut_draft = shortcuts::draft_from(&preferences.shortcuts);
        let shortcut_search_input = cx.new(|cx| InputState::new(window, cx).placeholder("Search shortcuts"));
        let shortcut_search_subscription = cx.subscribe_in(&shortcut_search_input, window, |_, _, event, _, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        });
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
                .placeholder(rust_i18n::t!("app.composer.input_placeholder").to_string())
                .auto_grow(1, 6)
                .submit_on_enter(true)
        });
        let composer_subscription = cx.subscribe_in(&composer, window, |view, _, event, window, cx| {
            if matches!(event, InputEvent::Change) {
                view.update_composer_suggestions(window, cx);
            }
        });
        let language = preferences.ui_language();
        let ui_language_select = cx.new(|cx| {
            gpui_kit::component::select::SelectState::new(
                UiLanguageOption::all(),
                UiLanguageOption::selected_index(language),
                window,
                cx,
            )
            .searchable(true)
        });
        let ui_language_subscription = cx.subscribe_in(&ui_language_select, window, |view, _, event, window, cx| {
            if let SelectEvent::Confirm(Some(language)) = event {
                view.set_ui_language(*language, window, cx);
            }
        });
        let destination = preferences.file_open_destination();
        // Bounded Info.plist scan; fast enough to run inline so the
        // dropdown is correct on its first render.
        let offered = preferences::FileOpenDestination::offered(destination, &files::detect_installed_editors());
        let file_destination_select = cx.new(|cx| {
            gpui_kit::component::select::SelectState::new(
                preferences::FileDestinationOption::for_destinations(&offered),
                preferences::FileDestinationOption::selected_index(&offered, destination),
                window,
                cx,
            )
            .searchable(false)
        });
        let file_destination_subscription =
            cx.subscribe_in(&file_destination_select, window, |view, _, event, _, cx| {
                if let SelectEvent::Confirm(Some(destination)) = event {
                    view.set_file_open_destination(*destination, cx);
                }
            });
        let turn_completion_select = cx.new(|cx| {
            gpui_kit::component::select::SelectState::new(
                preferences::TurnCompletionModeOption::for_modes(),
                preferences::TurnCompletionModeOption::selected_index(preferences.notifications.turn_completion()),
                window,
                cx,
            )
            .searchable(false)
        });
        let turn_completion_subscription = cx.subscribe_in(&turn_completion_select, window, |view, _, event, _, cx| {
            if let SelectEvent::Confirm(Some(mode)) = event {
                view.set_turn_completion_mode(*mode, cx);
            }
        });
        let config_search_input = cx.new(|cx| InputState::new(window, cx).placeholder("Search settings…"));
        let config_search_subscription = cx.subscribe_in(&config_search_input, window, |view, _, event, _, cx| {
            if matches!(event, InputEvent::Change) {
                view.refresh_config_catalog(cx);
                cx.notify();
            }
        });
        Self {
            shortcut_draft,
            shortcut_search_input,
            _shortcut_search_subscription: shortcut_search_subscription,
            shortcut_capture: None,
            shortcut_capture_focus,
            shortcut_message: None,
            shortcuts_editing: false,
            default_code_font: typography::default_code_font(cx),
            focus_handle,
            _focus_lost_subscription: focus_lost_subscription,
            _window_bounds_subscription: window_bounds_subscription,
            _welcome_activation_subscription: welcome_activation_subscription,
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
            restored_activity_revision: 0,
            activity_projection_cache: None,
            output_view_cache: None,
            tool_tail_cache: HashMap::new(),
            transcript_markdown_style: None,
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
            ui_language_select,
            _ui_language_subscription: ui_language_subscription,
            file_destination_select,
            _file_destination_subscription: file_destination_subscription,
            turn_completion_select,
            _turn_completion_subscription: turn_completion_subscription,
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
            external_launch_task: None,
            file_preview: None,
            preview_editor_error: None,
            #[cfg(test)]
            test_launch_intercept: None,
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
            effort_focus: cx.focus_handle().tab_stop(true),
            _effort_slider_subscriptions: Vec::new(),
            composer_resources: Vec::new(),
            slash_commands: Vec::new(),
            resources_loading: false,
            resources_loaded: false,
            palette_groups: Vec::new(),
            settings_open: false,
            config_page_open: false,
            config_page_group: None,
            theme_scroll: ScrollHandle::default(),
            settings_scroll: ScrollHandle::default(),
            settings_tabs_scroll: ScrollHandle::default(),
            config_rail_scroll: ScrollHandle::default(),
            utilities_scroll: ScrollHandle::default(),
            output_scroll: ScrollHandle::default(),
            review_scroll: ScrollHandle::default(),
            session_scroll: VirtualListScrollHandle::new(),
            activity_scroll: VirtualListScrollHandle::new(),
            change_list_scroll: VirtualListScrollHandle::new(),
            session_row_sizes: FixedRowSizeCache::default(),
            activity_row_sizes: FixedRowSizeCache::default(),
            change_row_sizes: FixedRowSizeCache::default(),
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
            webmcp_status_dirty: false,
            webmcp_notice: None,
            update_status: None,
            update_request_pending: false,
            cli_config_settings: None,
            config_catalog_cache: None,
            config_catalog_epoch: 0,
            config_search_corpus: None,
            config_visible_row_limit: configuration::CONFIG_GROUP_PAGE_SIZE,
            config_target: protocol::ConfigTarget::Workspace,
            config_search_input,
            _config_search_subscription: config_search_subscription,
            config_value_input: cx.new(|cx| TextareaState::new(window, cx).auto_grow(4, 12)),
            config_editor: None,
            config_editor_focus: cx.focus_handle().tab_stop(true),
            config_editor_return_focus: None,
            config_editor_error: None,
            config_write_pending: false,
            config_write_notice: None,
            secret_input: cx.new(|cx| InputState::new(window, cx).masked(true).placeholder("Paste API key…")),
            secret_editor_error: None,
            secret_save_pending: false,
            models: Vec::new(),
            attachments: Vec::new(),
            attachments_picker_open: false,
            pending_submission: None,
            pending_first_submission: false,
            first_submission: None,
            session_request: None,
            pending_new_thread: false,
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
            mcp_app_pending: HashMap::new(),
            mcp_app_task: None,
            mcp_app_bodies: HashMap::new(),
            mcp_app_errors: HashMap::new(),
            retained_submission: None,
            last_usage: None,
            theme_id: themes::DEFAULT_THEME_ID.to_string(),
            preferences,
            system_motion_baseline,
            sidebar_open: false,
            navigation_peek: false,
            hovered_message: None,
            zen_mode: true,
            sidebar_width: 270.0,
            inspector_width: 440.0,
            resizing: None,
        }
    }

    /// Apply saved preferences: theme, pane widths, and inspector state.
    pub(super) fn load_preferences(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
    pub(super) fn save_preferences(&mut self) {
        self.preferences.theme = Some(self.theme_id.clone());
        self.preferences.inspector_open = Some(self.inspector_open);
        self.preferences.set_widths(self.sidebar_width, self.inspector_width);
        self.preferences.save();
    }
}
