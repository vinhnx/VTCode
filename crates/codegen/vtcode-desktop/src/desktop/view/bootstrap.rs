//! App startup: menus, keybindings, window reopen, construction, and preference persistence.
use super::*;

/// Documentation and issue tracker opened by the Help menu and About dialog.
pub(super) const HELP_URL: &str = "https://github.com/vinhnx/vtcode";

/// Modified chords from the fixed table in [`init_keybindings`].
/// `shortcuts::bindings()` rejects overrides colliding with these: the deeper
/// fixed binding would shadow the rebind (e.g. `ctrl-m` opening the model
/// picker instead of the rebound command while the composer is focused).
/// Bare fixed chords need no entry — the modifier rule rejects them already.
/// Keep in sync with the table below.
pub(super) const FIXED_RESERVED_CHORDS: &[&str] = &[
    "ctrl-w",
    "alt-d",
    "ctrl-u",
    "ctrl-k",
    "ctrl-f",
    "ctrl-b",
    "ctrl-j",
    "alt-enter",
    "ctrl-m",
    "ctrl-p",
    "ctrl-n",
];

pub(super) fn init_keybindings(cx: &mut App, preferences: &preferences::Preferences) {
    match shortcuts::bindings(&preferences.shortcuts, cx) {
        Ok(bindings) => cx.bind_keys(bindings),
        Err(error) => {
            eprintln!("Desktop shortcuts: {error}; using defaults");
            if let Ok(bindings) = shortcuts::bindings(&Default::default(), cx) {
                cx.bind_keys(bindings);
            }
        }
    }
    cx.bind_keys([
        KeyBinding::new("cmd-w", CloseWindow, Some(key_context::DESKTOP)),
        KeyBinding::new("cmd-m", MinimizeWindow, Some(key_context::DESKTOP)),
        KeyBinding::new("ctrl-cmd-f", ToggleFullScreen, Some(key_context::DESKTOP)),
        KeyBinding::new("cmd-shift-n", ShowMainWindow, None),
        KeyBinding::new("cmd-q", QuitApplication, None),
        KeyBinding::new("cmd-h", HideApplication, None),
        KeyBinding::new("alt-cmd-h", HideOtherApplications, None),
        KeyBinding::new("right", NextInspectorTab, Some(key_context::INSPECTOR)),
        KeyBinding::new("left", PreviousInspectorTab, Some(key_context::INSPECTOR)),
        KeyBinding::new("home", FirstInspectorTab, Some(key_context::INSPECTOR)),
        KeyBinding::new("end", LastInspectorTab, Some(key_context::INSPECTOR)),
        KeyBinding::new("right", NextSettingsTab, Some(key_context::SETTINGS)),
        KeyBinding::new("left", PreviousSettingsTab, Some(key_context::SETTINGS)),
        KeyBinding::new("down", NextSettingsTab, Some(key_context::SETTINGS)),
        KeyBinding::new("up", PreviousSettingsTab, Some(key_context::SETTINGS)),
        KeyBinding::new("home", FirstSettingsTab, Some(key_context::SETTINGS)),
        KeyBinding::new("end", LastSettingsTab, Some(key_context::SETTINGS)),
        KeyBinding::new("enter", OpenSelectedFile, Some(gpui_kit::base::tree_key_context())),
        // No fixed `cmd-[`: the configurable NavigateBack default owns that
        // chord with superset behavior (route-back, then session history),
        // so a fixed twin would be dead weight — the old twin never
        // double-fired (bubble listeners stop by default) and NavigateBack
        // already runs the route-back-first logic.
        // CLI readline parity, composer-scoped and fixed like the CLI (these
        // reuse Kit's own editing actions, so undo/redo keep working).
        KeyBinding::new("ctrl-w", DeleteToPreviousWordStart, Some(key_context::COMPOSER)),
        KeyBinding::new("alt-d", DeleteToNextWordEnd, Some(key_context::COMPOSER)),
        KeyBinding::new("ctrl-u", DeleteToBeginningOfLine, Some(key_context::COMPOSER)),
        KeyBinding::new("ctrl-k", DeleteToEndOfLine, Some(key_context::COMPOSER)),
        KeyBinding::new("ctrl-f", MoveRight, Some(key_context::COMPOSER)),
        KeyBinding::new("ctrl-b", MoveLeft, Some(key_context::COMPOSER)),
        KeyBinding::new("ctrl-j", Enter { secondary: false, shift: true }, Some(key_context::COMPOSER)),
        KeyBinding::new("alt-enter", Enter { secondary: false, shift: true }, Some(key_context::COMPOSER)),
        KeyBinding::new("ctrl-m", OpenModelPicker, Some(key_context::COMPOSER)),
        KeyBinding::new("ctrl-p", HistoryPrevious, Some(key_context::COMPOSER)),
        KeyBinding::new("ctrl-n", HistoryNext, Some(key_context::COMPOSER)),
        KeyBinding::new("up", TranscriptPrevious, Some(key_context::TRANSCRIPT)),
        KeyBinding::new("down", TranscriptNext, Some(key_context::TRANSCRIPT)),
    ]);
}

pub(super) fn init_native_menus(cx: &mut App) {
    register_global_actions(cx);
    set_native_menus(cx, false);
}

/// Register the global action handlers backing native menu items. Called once
/// at startup: `on_action` handlers accumulate, so re-registering on every
/// menu rebuild would run actions (OpenHelp, ShowMainWindow) N+1 times.
fn register_global_actions(cx: &mut App) {
    let _app = cx.on_action(|_: &QuitApplication, cx| cx.quit());
    let _app = cx.on_action(|_: &HideApplication, cx| cx.hide());
    let _app = cx.on_action(|_: &HideOtherApplications, cx| cx.hide_other_apps());
    let _app = cx.on_action(|_: &ShowAllApplications, cx| cx.unhide_other_apps());
    let _app = cx.on_action(|_: &ShowMainWindow, cx| show_main_window(cx));
    let _app = cx.on_action(|_: &OpenHelp, cx| cx.open_url(HELP_URL));
}

/// (Re)install the native menus. Re-invoked when full-screen state changes so
/// the View item reads Enter or Exit Full Screen as HIG requires.
pub(super) fn set_native_menus(cx: &mut App, full_screen: bool) {
    cx.set_menus([
        Menu::new(app::NAME).items([
            MenuItem::action(rust_i18n::t!("app.menus.about").to_string(), AboutApplication),
            MenuItem::separator(),
            MenuItem::action(rust_i18n::t!("app.menus.settings_menu").to_string(), OpenSettings),
            MenuItem::separator(),
            MenuItem::os_submenu(rust_i18n::t!("app.menus.services").to_string(), SystemMenuType::Services),
            MenuItem::separator(),
            MenuItem::action(rust_i18n::t!("app.menus.hide_app").to_string(), HideApplication),
            MenuItem::action(rust_i18n::t!("app.menus.hide_others").to_string(), HideOtherApplications),
            MenuItem::action(rust_i18n::t!("app.menus.show_all").to_string(), ShowAllApplications),
            MenuItem::separator(),
            MenuItem::action(rust_i18n::t!("app.menus.quit_app").to_string(), QuitApplication),
        ]),
        Menu::new(rust_i18n::t!("app.menus.file_menu").to_string()).items([
            MenuItem::action(rust_i18n::t!("app.menus.new_session").to_string(), NewSession),
            MenuItem::action(rust_i18n::t!("app.menus.open_workspace").to_string(), ChooseWorkspace),
            MenuItem::action(rust_i18n::t!("app.menus.close_session").to_string(), CloseSession),
            MenuItem::action(rust_i18n::t!("app.menus.choose_model").to_string(), OpenModelPicker),
        ]),
        Menu::new(rust_i18n::t!("app.menus.edit_menu").to_string()).items([
            MenuItem::os_action(rust_i18n::t!("app.menus.undo").to_string(), Undo, OsAction::Undo),
            MenuItem::os_action(rust_i18n::t!("app.menus.redo").to_string(), Redo, OsAction::Redo),
            MenuItem::separator(),
            MenuItem::os_action(rust_i18n::t!("app.menus.cut").to_string(), Cut, OsAction::Cut),
            MenuItem::os_action(rust_i18n::t!("app.menus.copy").to_string(), Copy, OsAction::Copy),
            MenuItem::os_action(rust_i18n::t!("app.menus.paste").to_string(), Paste, OsAction::Paste),
            MenuItem::os_action(rust_i18n::t!("app.menus.select_all").to_string(), SelectAll, OsAction::SelectAll),
        ]),
        Menu::new(rust_i18n::t!("app.menus.view_menu").to_string()).items([
            MenuItem::action(rust_i18n::t!("app.menus.command_palette").to_string(), OpenCommandPalette),
            MenuItem::action(rust_i18n::t!("app.menus.find_conversation").to_string(), OpenConversationSearch),
            MenuItem::action(rust_i18n::t!("app.menus.focus_composer").to_string(), FocusComposer),
            MenuItem::action(rust_i18n::t!("app.menus.toggle_inspector").to_string(), ToggleInspector),
            MenuItem::action(rust_i18n::t!("app.menus.toggle_sidebar").to_string(), ToggleSidebar),
            MenuItem::action(rust_i18n::t!("app.menus.toggle_zen").to_string(), ToggleZenMode),
            MenuItem::separator(),
            MenuItem::action(rust_i18n::t!("app.menus.cancel_run").to_string(), CancelRun),
            MenuItem::separator(),
            // macOS convention: the full-screen toggle closes the View menu;
            // ⌃⌘F is the system-standard shortcut (HIG: full screen offers a
            // distraction-free context). The label tracks the current state.
            MenuItem::action(
                if full_screen {
                    rust_i18n::t!("app.menus.exit_full_screen").to_string()
                } else {
                    rust_i18n::t!("app.menus.enter_full_screen").to_string()
                },
                ToggleFullScreen,
            ),
        ]),
        Menu::new(rust_i18n::t!("app.menus.window_menu").to_string()).items([
            MenuItem::action(rust_i18n::t!("app.menus.show_app").to_string(), ShowMainWindow),
            MenuItem::action(rust_i18n::t!("app.menus.minimize").to_string(), MinimizeWindow),
            MenuItem::action(rust_i18n::t!("app.menus.zoom").to_string(), ZoomWindow),
            MenuItem::action(rust_i18n::t!("app.menus.close_window").to_string(), CloseWindow),
        ]),
        Menu::new(rust_i18n::t!("app.menus.help_menu").to_string()).items([MenuItem::action(
            rust_i18n::t!("app.menus.help_website").to_string(),
            OpenHelp,
        )]),
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
        if preferences.show_in_menu_bar && !menu_bar::set_enabled(true, cx) {
            vtcode_commons::diagnostics::stderr(format_args!("failed to create the macOS menu bar item"));
        }
        if let Err(error) = open_or_activate_main_window(cx, Some(preferences)) {
            vtcode_commons::diagnostics::stderr(format_args!("failed to open VT Code Desktop window: {error}"));
        }
    });
}

pub(super) fn show_main_window(cx: &mut App) {
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
    window.update(cx, |_, window, cx| {
        DesktopView::apply_sidebar_background(view.read(cx).preferences.translucent_sidebar, window, cx);
        window.on_window_should_close(cx, |_, cx| {
            if menu_bar::should_hide(cx) {
                menu_bar::hide(cx);
                false
            } else {
                true
            }
        });
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
        let window_bounds_subscription = cx.observe_window_bounds(window, |view, window, cx| {
            view.persist_window_maximized(window);
            view.refresh_full_screen_menus(window, cx);
        });
        let welcome_activation_subscription = cx.observe_window_activation(window, |view, window, cx| {
            if window.is_window_active() {
                view.refresh_welcome_period(cx);
            }
        });
        focus_handle.focus(window, cx);
        let system_motion_baseline = preferences::system_motion_baseline(cx);
        preferences::apply_reduced_motion(preferences.reduced_motion, system_motion_baseline, cx);
        let contrast_slider = cx.new(|_| {
            gpui_kit::component::slider::SliderState::new()
                .min(0.)
                .max(100.)
                .step(1.)
                .default_value(f32::from(preferences.contrast()))
        });
        let contrast_slider_subscription = cx.observe_in(&contrast_slider, window, |view, slider, window, cx| {
            view.set_contrast(slider.read(cx).value().start(), window, cx);
        });
        let shortcut_capture_focus = cx.focus_handle().tab_stop(true);
        let shortcut_draft = shortcuts::draft_from(&preferences.shortcuts);
        let shortcut_search_input = cx
            .new(|cx| InputState::new(window, cx).placeholder(rust_i18n::t!(i18n::SHORTCUTS_SEARCH_ARIA).to_string()));
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
                .placeholder(rust_i18n::t!(i18n::COMPOSER_INPUT_PLACEHOLDER).to_string())
                .auto_grow(1, 6)
                .submit_on_enter(true)
                // Pinned, not defaulted: off-macOS Kit binds ctrl-f to Search
                // on the deeper input context, and only a non-searchable input
                // propagates it back out to our fixed ctrl-f caret binding.
                .searchable(false)
        });
        let composer_subscription = cx.subscribe_in(&composer, window, |view, _, event, window, cx| {
            if matches!(event, InputEvent::Change) {
                // Recall abandonment is NOT reset here: `Change` flushes after
                // the update, and recall's own writes emit it too, so a reset
                // here would self-clear every traversal step in production.
                // `recall_history` detects draft mutation synchronously.
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
        let config_search_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(rust_i18n::t!("app.configuration.search_placeholder").to_string())
        });
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
            open_tabs: Vec::new(),
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
            last_escape_press: None,
            composer_history_index: None,
            composer_history_stash: gpui_kit::SharedString::from(""),
            transcript_focus: cx.focus_handle().tab_stop(true),
            keyboard_focused_message: None,
            running: false,
            turn_content_seen: false,
            status: rust_i18n::t!("app.workspace.startup_hint").to_string(),
            ui_progress_active: false,
            background_activity_count: 0,
            error: None,
            configuration: None,
            pending_permission_choice: None,
            full_access_confirm: None,
            full_access_focus: cx.focus_handle(),
            running_confirm: None,
            running_confirm_focus: cx.focus_handle(),
            armed_running_switch: None,
            about_open: false,
            about_focus: cx.focus_handle(),
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
            route_history: RouteHistory::new(),
            workspace_review: None,
            review_focus: cx.focus_handle().tab_stop(true),
            browser_focus: cx.focus_handle().tab_stop(true),
            git_branch: None,
            palette: None,
            palette_kind: PaletteKind::Commands,
            palette_query: gpui_kit::SharedString::default(),
            effort_slider: None,
            contrast_slider,
            contrast_focus: cx.focus_handle().tab_stop(true),
            _contrast_slider_subscription: contrast_slider_subscription,
            effort_focus: cx.focus_handle().tab_stop(true),
            _effort_slider_subscriptions: Vec::new(),
            composer_resources: Vec::new(),
            slash_commands: Vec::new(),
            resources_loading: false,
            resources_loaded: false,
            palette_groups: Vec::new(),
            config_page_open: false,
            config_page_group: None,
            theme_scroll: ScrollHandle::default(),
            settings_scroll: ScrollHandle::default(),
            settings_nav_scroll: ScrollHandle::default(),
            config_rail_scroll: ScrollHandle::default(),
            utilities_scroll: ScrollHandle::default(),
            output_scroll: ScrollHandle::default(),
            output_tail_len: 0,
            output_follow_selection: None,
            review_scroll: ScrollHandle::default(),
            session_scroll: VirtualListScrollHandle::new(),
            activity_scroll: VirtualListScrollHandle::new(),
            change_list_scroll: VirtualListScrollHandle::new(),
            browser_scroll: VirtualListScrollHandle::new(),
            session_row_sizes: FixedRowSizeCache::default(),
            activity_row_sizes: FixedRowSizeCache::default(),
            change_row_sizes: FixedRowSizeCache::default(),
            browser_row_sizes: FixedRowSizeCache::default(),
            settings_focus: cx.focus_handle().tab_stop(true),
            settings_navigation_focus: cx.focus_handle().tab_stop(true),
            settings_tab: settings::SettingsTab::Appearance,
            storage: storage::StorageState::default(),
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
            secret_input: cx.new(|cx| {
                InputState::new(window, cx)
                    .masked(true)
                    .placeholder(rust_i18n::t!("app.secrets.paste_key").to_string())
            }),
            secret_editor_error: None,
            secret_save_pending: false,
            models: Vec::new(),
            attachments: Vec::new(),
            attachment_thumbnails: HashMap::new(),
            attachment_picker: None,
            attachment_picker_focus: cx.focus_handle(),
            menus_show_exit_full_screen: false,
            picker_thumbnail_task: None,
            attachments_picker_open: false,
            pending_submission: None,
            pending_first_submission: false,
            first_submission: None,
            queued_submissions: Vec::new(),
            queue_depth: 0,
            queue_previews: Vec::new(),
            queue_dropped_total: 0,
            take_latest_pending: false,
            pending_composer_restore: None,
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
            zen_drag_armed: false,
            sidebar_width: 270.0,
            inspector_width: 440.0,
            resizing: None,
        }
    }

    /// Apply saved preferences: theme, app icon, pane widths, and inspector state.
    pub(super) fn load_preferences(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let preferences = self.preferences.clone();
        self.preferences.follow_cli_theme = Some(preferences.follow_cli_theme.unwrap_or(preferences.theme.is_none()));
        if let Some(theme) = preferences.theme.as_deref() {
            self.theme_mode = themes::apply_with_contrast(theme, preferences.contrast(), Some(window), cx);
            self.theme_id = theme.to_string();
        } else if preferences.contrast() > 0 {
            self.theme_mode = themes::apply_with_contrast(&self.theme_id, preferences.contrast(), Some(window), cx);
        }
        self.apply_typography(cx);
        self.refresh_dock_icon();
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
