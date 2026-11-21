//! App startup: menus, window reopen, construction, and preference persistence
//! (keybindings live in `bootstrap/keybindings.rs`).
use super::super::{SharedUrlInbox, UrlInbox};
use super::*;
use gpui_kit::WindowBounds;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

/// Documentation and issue tracker opened by the Help menu and About dialog.
pub(super) const HELP_URL: &str = "https://github.com/vinhnx/vtcode";

mod keybindings;
pub(super) use keybindings::{FIXED_RESERVED_CHORDS, apply_vim_navigation, init_keybindings, reinstall_shortcuts};
mod handoff;
pub(super) use handoff::*;
mod prefs;

pub(crate) fn run(startup_link: Option<protocol::handoff_url::HandoffLink>) {
    let prefs_phase = vtcode_commons::startup_trace::phase_started();
    let preferences = preferences::Preferences::load();
    super::super::localization::initialize(preferences.ui_language());
    vtcode_commons::startup_trace::record_phase("desktop_preferences_and_localization", prefs_phase);
    let inbox: SharedUrlInbox = Rc::new(RefCell::new(UrlInbox::default()));
    let inbox_events = inbox.clone();
    let application = gpui_kit::application().with_assets(super::super::assets::DesktopAssets);
    // `vtcode://` delivery to the running app (CLI `/desktop` fast path).
    // The callback runs on the main thread without app access, so it only
    // touches the inbox; dispatch happens through `AsyncApp` once init ran.
    let _application = application.on_reopen(show_main_window).on_open_urls(move |urls| {
        let ready = {
            let mut inbox = inbox_events.borrow_mut();
            for url in &urls {
                inbox.push_url(url);
            }
            inbox.take_ready()
        };
        if let Some((app, links)) = ready {
            app.update(|cx| dispatch_handoff_links(cx, links));
        }
    });
    application.run(move |cx: &mut App| {
        let kit_phase = vtcode_commons::startup_trace::phase_started();
        gpui_kit::init(cx);
        vtcode_commons::startup_trace::record_phase("desktop_kit_init", kit_phase);
        let chrome_phase = vtcode_commons::startup_trace::phase_started();
        super::super::notifications::initialize(cx);
        init_keybindings(cx, &preferences);
        menu_bar::init_native_menus(cx);
        if preferences.show_in_menu_bar && !menu_bar::set_enabled(true, cx) {
            vtcode_commons::diagnostics::stderr(format_args!("failed to create the macOS menu bar item"));
        }
        vtcode_commons::startup_trace::record_phase("desktop_chrome", chrome_phase);
        inbox.borrow_mut().set_app(cx.to_async());
        let window_phase = vtcode_commons::startup_trace::phase_started();
        if let Err(error) = open_or_activate_main_window(cx, Some(preferences), startup_link) {
            vtcode_commons::diagnostics::stderr(format_args!("failed to open VT Code Desktop window: {error}"));
        } else {
            // The window (and its global) exists now: drain URLs that
            // arrived before init finished on a cold launch.
            let links = inbox.borrow_mut().take_pending();
            if !links.is_empty() {
                dispatch_handoff_links(cx, links);
            }
        }
        vtcode_commons::startup_trace::record_phase("desktop_window_open", window_phase);
        // Async milestones print individually; welcome launches may never reach session-ready.
        vtcode_commons::startup_trace::emit_summary();
    });
}

/// Reopening must preserve an existing window's draft, session, and backend.
pub(super) fn open_or_activate_main_window(
    cx: &mut App,
    preferences: Option<preferences::Preferences>,
    startup_link: Option<protocol::handoff_url::HandoffLink>,
) -> anyhow::Result<gpui_kit::AnyWindowHandle> {
    // A dock/Finder reopen should return to the user's focused workspace when
    // multiple windows exist; keep the first-window fallback for the current
    // single-window lifecycle when the platform has no active window.
    if let Some(window) = cx.active_window().or_else(|| cx.windows().first().copied()) {
        window.update(cx, |_, window, _| window.activate_window())?;
        cx.activate(true);
        return Ok(window);
    }
    // A saved preference wins; otherwise follow the OS appearance so a fresh
    // launch opens Noir in dark mode and Vitesse Light in light mode.
    let preferences = preferences.unwrap_or_else(preferences::Preferences::load);
    let start_mode = ThemeMode::from(cx.window_appearance());
    let start_theme = preferences
        .theme
        .clone()
        .unwrap_or_else(|| themes::default_id(start_mode).to_string());
    let start_maximized = preferences.window_maximized();
    let translucent = preferences.translucent_sidebar && !reduced_transparency_requested();
    let mut options = desktop_window_options(translucent);
    // A maximized launch fills the main screen's visible frame explicitly:
    // AppKit `zoom:` targets the content-derived standard frame (GPUI sets
    // no standard-frame delegate), which leaves gaps and never satisfies
    // `is_maximized`, so the bounds observer would persist windowed and
    // regress the next launch. Zoom stays as the fallback when the probe
    // fails (nil screen, off-main-thread, degenerate frame).
    let mut zoom_after_open = false;
    if start_maximized {
        if let Some(bounds) = main_screen_visible_bounds() {
            options.window_bounds = Some(WindowBounds::Windowed(bounds));
        } else {
            zoom_after_open = true;
        }
    }
    let (window, view) = gpui_kit::open_window(options, cx, move |window, cx| {
        window.set_window_title(app::NAME);
        let (theme_mode, start_theme) = prefs::apply_initial_theme(&preferences, &start_theme, window, cx);
        let view = cx.new(|cx| DesktopView::new_with_preferences(window, cx, theme_mode, start_theme, preferences));
        view.update(cx, |view, cx| view.apply_loaded_preferences(cx));
        view
    })?;
    window.update(cx, |_, window, cx| {
        DesktopView::apply_sidebar_background(view.read(cx).effective_translucent_sidebar(), window, cx);
        window.on_window_should_close(cx, |_, cx| {
            if menu_bar::should_hide(cx) {
                menu_bar::hide(cx);
                false
            } else {
                true
            }
        });
    })?;
    cx.set_global(MainWindow { view: view.downgrade(), window });
    if zoom_after_open {
        // Fallback when the visible-frame probe failed: macOS ignores
        // WindowBounds::Maximized at creation, so zoom after opening to
        // reach the native zoomed state the Zoom menu toggles. The
        // headless test platform leaves zoom unimplemented.
        #[cfg(not(test))]
        window.update(cx, |_, window, _| window.zoom_window())?;
    }
    // A CLI handoff (`/desktop`) selects its workspace and loads its thread
    // through pending navigation; otherwise reopen the most recent workspace.
    // An unreadable handoff workspace stays on welcome with the standard
    // notice instead of falling back: opening a different directory than the
    // requested one would silently land the user in the wrong workspace.
    if let Some(link) = startup_link {
        view.update(cx, |view, cx| view.select_startup_handoff(link, cx));
    } else {
        let last_workspace = view.read(cx).preferences.recent_workspaces.first().map(PathBuf::from);
        if let Some(path) = last_workspace {
            view.update(cx, |view, cx| view.select_workspace_on_startup(path, cx));
        }
    }
    view.update(cx, |view, cx| {
        view.start_event_drain(cx);
        view.start_goal_ticker(cx);
        view.start_login_item_reconciliation(cx);
    });
    cx.activate(true);
    Ok(window)
}

impl DesktopView {
    #[cfg(test)]
    pub(super) fn new(window: &mut Window, cx: &mut Context<Self>, theme_mode: ThemeMode) -> Self {
        Self::new_with_preferences(
            window,
            cx,
            theme_mode,
            themes::default_id(theme_mode).to_string(),
            preferences::Preferences::load(),
        )
    }

    pub(super) fn new_with_preferences(
        window: &mut Window,
        cx: &mut Context<Self>,
        theme_mode: ThemeMode,
        start_theme: String,
        preferences: preferences::Preferences,
    ) -> Self {
        let (event_tx, event_rx) = BackendEventSender::channel(UI_EVENT_CAPACITY);
        let focus_handle = cx.focus_handle();
        // One retained scroll position per theme mode, shared with the child
        // grid views that own the virtual list (see `ThemeGrid`). The handles
        // stay here so chevrons and tests control both grids uniformly.
        let theme_scroll_light = VirtualListScrollHandle::new();
        let theme_scroll_dark = VirtualListScrollHandle::new();
        let view_handle = cx.entity();
        let light_view = view_handle.clone();
        let dark_view = view_handle.clone();
        let theme_grid_light = cx.new(|_| {
            appearance_theme_picker::ThemeGrid::new(ThemeMode::Light, theme_scroll_light.clone(), &light_view)
        });
        let theme_grid_dark =
            cx.new(|_| appearance_theme_picker::ThemeGrid::new(ThemeMode::Dark, theme_scroll_dark.clone(), &dark_view));
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
        // `observe_in` fires on every `SliderState::notify` (drag ticks) and
        // applies the palette there without persisting. Persistence is handled
        // here instead: once on pointer `Release`, and on discrete `Change`s
        // that have no drag behind them (assistive-technology Increment/
        // Decrement call `step_thumb`, which emits `Change` but never
        // `Release`). Arrow keys call `set_value`, which emits no event and
        // persists at its own call site in `settings_accessibility`.
        let contrast_event_subscription = cx.subscribe_in(
            &contrast_slider,
            window,
            |view, _, event: &gpui_kit::component::slider::SliderEvent, _, _| {
                use gpui_kit::component::slider::SliderEvent;
                match event {
                    SliderEvent::Change(_) if !view.contrast_drag_active => view.save_preferences(),
                    SliderEvent::Release(_) => {
                        view.contrast_drag_active = false;
                        view.save_preferences();
                    }
                    _ => {}
                }
            },
        );
        let shortcut_capture_focus = cx.focus_handle().tab_stop(true);
        let shortcut_draft = shortcuts::draft_from(&preferences.shortcuts);
        let shortcut_search_input = cx
            .new(|cx| InputState::new(window, cx).placeholder(rust_i18n::t!(i18n::SHORTCUTS_SEARCH_ARIA).to_string()));
        let shortcut_search_subscription = cx.subscribe_in(&shortcut_search_input, window, |_, _, event, _, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        });
        let output_filter_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(rust_i18n::t!("app.inspector.filter_output_lines").to_string())
        });
        let output_filter_subscription = cx.subscribe_in(&output_filter_input, window, |_, _, event, _, cx| {
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
        // English literal: new strings stay untranslated until full
        // native-quality coverage ships for every locale.
        let file_filter_input =
            cx.new(|cx| InputState::new(window, cx).placeholder(files_panel::FILTER_FILES_LABEL.to_string()));
        let file_filter_subscription = cx.subscribe_in(&file_filter_input, window, |view, _, event, _, cx| {
            if matches!(event, InputEvent::Change) {
                view.rebuild_file_tree(cx);
            }
        });
        let picker_tree = cx.new(|cx| TreeState::new(cx));
        let picker_tree_subscription = cx.subscribe(&picker_tree, |view, _, event, cx| {
            match event {
                TreeEvent::Expanded(path) => {
                    let _inserted = view.expanded_picker_directories.insert(path.to_string());
                }
                TreeEvent::Collapsed(path) => {
                    let _removed = view.expanded_picker_directories.remove(path.as_ref());
                }
            }
            cx.notify();
        });
        let picker_filter_input =
            cx.new(|cx| InputState::new(window, cx).placeholder(files_panel::FILTER_FILES_LABEL.to_string()));
        let picker_filter_subscription = cx.subscribe_in(&picker_filter_input, window, |view, _, event, _, cx| {
            if matches!(event, InputEvent::Change) {
                view.rebuild_picker_tree(cx);
            }
        });
        // The inline queue editor is shared: only one queued row edits at a
        // time. Enter commits the row being edited (the click path shares the
        // same commit method); the row's Cancel button dismisses the edit.
        let queue_edit_input = cx.new(|cx| InputState::new(window, cx));
        let queue_edit_subscription = cx.subscribe_in(&queue_edit_input, window, |view, _, event, window, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                view.commit_queue_edit(window, cx);
            }
        });
        let composer_highlight_styles =
            Rc::new(RefCell::new(composer_highlight::ComposerHighlightStyles::from_colors(&cx.theme().colors)));
        let composer = cx.new(|cx| {
            let styles = composer_highlight_styles.clone();
            let mut state = EditorState::new(window, cx)
                .placeholder(rust_i18n::t!(i18n::COMPOSER_INPUT_PLACEHOLDER).to_string())
                .submit_on_enter(true)
                // The composer is prose, not source: no line numbers, folding,
                // auto-indent or bracket pairing. Only the token highlighter
                // styles runs of text.
                .line_number(false)
                .folding(false)
                .auto_close(false)
                // Code-editor affordances the old Textarea never had: no
                // indentation guides and no auto-indent on newlines.
                .indent_guides(false)
                .smart_indent(false)
                .language(composer_highlight::PROMPT_LANGUAGE)
                // Pinned, not defaulted: off-macOS Kit binds ctrl-f to Search
                // on the deeper input context, and only a non-searchable input
                // propagates it back out to our fixed ctrl-f caret binding.
                .searchable(false);
            state.set_highlighter_factory(composer_highlight::prompt_highlighter_factory(styles), cx);
            state
        });
        composer.read(cx).focus_handle(cx).focus(window, cx);
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
        let (terminal_position_select, terminal_position_subscription) =
            Self::build_terminal_position_select(&preferences, window, cx);
        let (chat_width_select, chat_width_subscription) = Self::build_chat_width_select(&preferences, window, cx);
        let destination = preferences.file_open_destination();
        let offered = preferences::FileOpenDestination::offered(destination, &[]);
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
        let terminal = preferences.cli_terminal();
        let offered_terminals = preferences::CliTerminal::offered(terminal, &[]);
        let cli_terminal_select = cx.new(|cx| {
            gpui_kit::component::select::SelectState::new(
                preferences::CliTerminalOption::for_terminals(&offered_terminals),
                preferences::CliTerminalOption::selected_index(&offered_terminals, terminal),
                window,
                cx,
            )
            .searchable(false)
        });
        let cli_terminal_subscription = cx.subscribe_in(&cli_terminal_select, window, |view, _, event, _, cx| {
            if let SelectEvent::Confirm(Some(terminal)) = event {
                view.set_cli_terminal(*terminal, cx);
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
        // Read before `preferences` moves into the struct below.
        let zen_mode = preferences.zen_mode();
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
            terminal: None,
            terminal_visible: false,
            terminal_monitor: None,
            terminal_monitor_row: None,
            pending_workspace: None,
            navigation_history: NavigationHistory::default(),
            pending_navigation: None,
            swipe_scroll: SwipeScrollAccumulator::default(),
            swipe_arming: SwipeArming::default(),
            workspace_selection_pending: false,
            workspace_selection_task: None,
            workspace_picker_open: false,
            workspace_picker_task: None,
            backend: None,
            event_rx,
            event_tx,
            event_drain_task: None,
            sessions: Vec::new(),
            session_list_limit: SESSION_LIST_PAGE_SIZE,
            session_list_has_older: false,
            session_list_received: false,
            selected_session_id: None,
            open_tabs: Vec::new(),
            tab_strip_focus: cx.focus_handle().tab_stop(true),
            tab_strip_scroll: ScrollHandle::default(),
            active_primary_agent_display: None,
            transcript: Transcript::default(),
            message_scroller,
            transcript_layout_settled: false,
            filter_matches: None,
            restored_activity: Vec::new(),
            restored_activity_revision: 0,
            restored_poll_redirects: HashMap::new(),
            activity_projection_cache: None,
            output_view_cache: None,
            tool_tail_cache: HashMap::new(),
            transcript_markdown_style: None,
            selected_output_id: None,
            restored_review_history_incomplete: false,
            restored_activity_history_incomplete: false,
            restored_timeline: Arc::new(Vec::new()),
            active_turn_id: None,
            active_turn_started_at: None,
            finished_turn_id: None,
            turn_undos: HashMap::new(),
            cancel_requested: false,
            cancel_request_sent: false,
            cancel_sent_turn_id: None,
            last_escape_press: None,
            composer_history_index: None,
            composer_history_stash: gpui_kit::SharedString::from(""),
            history_filter_stash: gpui_kit::SharedString::from(""),
            transcript_focus: cx.focus_handle().tab_stop(true),
            session_focus: cx.focus_handle().tab_stop(true),
            keyboard_focused_session: None,
            keyboard_focused_message: None,
            transcript_flash: None,
            transcript_flash_generation: 0,
            running: false,
            turn_content_seen: false,
            status: rust_i18n::t!("app.workspace.startup_hint").to_string(),
            ui_progress_active: false,
            background_activity_count: 0,
            tracker_snapshot: None,
            goal_snapshot: None,
            goal_snapshot_at: None,
            goal_tick_task: None,
            goal_expanded: false,
            goal_notified: None,
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
            license_viewer: None,
            license_viewer_focus: cx.focus_handle().tab_stop(true),
            license_return_focus: None,
            license_open_task: None,
            changelog_open: false,
            changelog_focus: cx.focus_handle(),
            changelog_return_focus: None,
            changelog_scroll: ScrollHandle::default(),
            release_notes_banner_visible_now: false,
            dismissed_release_notes_version: None,
            service_info: None,
            interaction: None,
            interaction_focus: cx.focus_handle(),
            interaction_scroll: ScrollHandle::new(),
            interaction_choice_bounds: Default::default(),
            pending_interaction_response: None,
            interaction_edit_pending: false,
            interaction_choice_ids: Vec::new(),
            interaction_choice_groups: Vec::new(),
            interaction_answer_texts: Vec::new(),
            interaction_step_index: 0,
            interaction_immediate_choice_id: None,
            interaction_plan_expanded: false,
            composer,
            composer_highlight_styles,
            _composer_subscription: composer_subscription,
            ui_language_select,
            _ui_language_subscription: ui_language_subscription,
            terminal_position_select,
            _terminal_position_subscription: terminal_position_subscription,
            chat_width_select,
            _chat_width_subscription: chat_width_subscription,
            file_destination_select,
            _file_destination_subscription: file_destination_subscription,
            cli_terminal_select,
            destination_discovery_task: None,
            _cli_terminal_subscription: cli_terminal_subscription,
            turn_completion_select,
            _turn_completion_subscription: turn_completion_subscription,
            question_answer: cx.new(|cx| TextareaState::new(window, cx).auto_grow(1, 4)),
            theme_mode,
            files: Vec::new(),
            files_truncated: false,
            file_directory_paths: Arc::new(HashSet::new()),
            file_tree,
            file_tree_requested: false,
            pending_tree_select: None,
            expanded_file_directories: HashSet::new(),
            _file_tree_subscription: file_tree_subscription,
            file_filter_input,
            _file_filter_subscription: file_filter_subscription,
            file_search_hit_count: None,
            files_task: None,
            files_scan_cancel: None,
            preview_task: None,
            attachments_task: None,
            external_launch_task: None,
            file_preview: None,
            preview_editor_error: None,
            #[cfg(test)]
            test_launch_intercept: None,
            #[cfg(test)]
            test_trash_sink: None,
            preview_focus: cx.focus_handle(),
            preview_return_focus: None,
            file_rename: None,
            rename_focus: cx.focus_handle(),
            rename_return_focus: None,
            palette_return_focus: None,
            palette_focus: cx.focus_handle(),
            sidebar_resize_focus: cx.focus_handle().tab_stop(true),
            inspector_resize_focus: cx.focus_handle().tab_stop(true),
            terminal_focus: cx.focus_handle().tab_stop(true),
            terminal_pump_generation: 0,
            terminal_pump_task: None,
            terminal_panel_height: 240.0,
            terminal_panel_width: 360.0,
            terminal_resize_focus: cx.focus_handle().tab_stop(true),
            terminal_scroll: ScrollHandle::default(),
            terminal_wheel_remainder: 0.0,
            terminal_grid_bounds: Rc::new(Cell::new(None)),
            terminal_select_drag: None,
            terminal_select_anchor: None,
            inspector_open: false,
            inspector_tab: InspectorTab::default(),
            inspector_focus: cx.focus_handle().tab_stop(true),
            inspector_tabs_scroll: ScrollHandle::default(),
            changes: ChangesProjection::default(),
            selected_change: None,
            selected_change_source: None,
            change_kind_filter: None,
            change_viewer: None,
            change_viewer_dirty: false,
            route_history: RouteHistory::new(),
            route_motion: NavigationMotion::default(),
            chat_motion: NavigationMotion::default(),
            settings_motion: NavigationMotion::default(),
            inspector_motion: NavigationMotion::default(),
            workspace_review: None,
            review_focus: cx.focus_handle().tab_stop(true),
            browser_focus: cx.focus_handle().tab_stop(true),
            browser_list_focus: cx.focus_handle().tab_stop(true),
            git_branch: None,
            palette: None,
            palette_kind: PaletteKind::Commands,
            composer_pill_bounds: ComposerPillBounds::default(),
            history_scope_focus: cx.focus_handle().tab_stop(true),
            palette_query: gpui_kit::SharedString::default(),
            effort_slider: None,
            contrast_slider,
            contrast_focus: cx.focus_handle().tab_stop(true),
            motion_choice_focus: cx.focus_handle().tab_stop(true),
            _contrast_slider_subscription: contrast_slider_subscription,
            _contrast_event_subscription: contrast_event_subscription,
            contrast_drag_active: false,
            effort_focus: cx.focus_handle().tab_stop(true),
            _effort_slider_subscriptions: Vec::new(),
            composer_resources: Vec::new(),
            slash_commands: Vec::new(),
            resources_loading: false,
            resources_loaded: false,
            history_scope: HistoryScope::default(),
            recent_prompts: Vec::new(),
            recent_prompts_scope: None,
            recent_prompts_loading: false,
            palette_groups: Vec::new(),
            config_page_open: false,
            config_page_group: None,
            theme_scroll_light,
            theme_scroll_dark,
            theme_grid_light,
            theme_grid_dark,
            theme_editor: appearance_themes::CustomThemeState::new(window, cx),
            settings_scroll: ScrollHandle::default(),
            settings_nav_scroll: ScrollHandle::default(),
            config_rail_scroll: ScrollHandle::default(),
            utilities_scroll: ScrollHandle::default(),
            output_scroll: ScrollHandle::default(),
            output_filter_input,
            _output_filter_subscription: output_filter_subscription,
            output_tail_seen: None,
            output_follow_selection: None,
            review_scroll: ScrollHandle::default(),
            session_scroll: VirtualListScrollHandle::new(),
            activity_scroll: VirtualListScrollHandle::new(),
            activity_filter: None,
            activity_follow_filter: None,
            activity_seen_head: None,
            activity_focus: cx.focus_handle().tab_stop(true),
            keyboard_focused_activity: None,
            change_list_scroll: VirtualListScrollHandle::new(),
            browser_scroll: VirtualListScrollHandle::new(),
            session_row_sizes: FixedRowSizeCache::default(),
            activity_row_sizes: FixedRowSizeCache::default(),
            tasks_row_sizes: FixedRowSizeCache::default(),
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
            update_backfill_in_flight: false,
            update_install_state: protocol::UpdateInstallState::Idle,
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
            picker_tab_focus: cx.focus_handle().tab_stop(true),
            menus_show_exit_full_screen: false,
            picker_tree,
            expanded_picker_directories: HashSet::new(),
            _picker_tree_subscription: picker_tree_subscription,
            picker_filter_input,
            _picker_filter_subscription: picker_filter_subscription,
            picker_search_hit_count: None,
            attachments_picker_open: false,
            pending_submission: None,
            pending_first_submission: false,
            first_submission: None,
            queued_submissions: Vec::new(),
            queue_depth: 0,
            queue_has_dispatch_ids: false,
            queue_previews: Vec::new(),
            queue_dropped_total: 0,
            take_latest_pending: false,
            steer_pending: std::collections::BTreeMap::new(),
            editing_queue_echo_id: None,
            queue_edit_pending: None,
            queue_edit_input,
            _queue_edit_subscription: queue_edit_subscription,
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
            cli_handoff: None,
            last_usage: None,
            theme_id: themes::canonical_selection(&start_theme, theme_mode, &preferences.custom_themes),
            preferences,
            login_item_status: None,
            system_motion_baseline,
            sidebar_open: false,
            navigation_peek: false,
            hovered_message: None,
            zen_mode,
            zen_drag_armed: false,
            sidebar_width: 270.0,
            inspector_width: preferences::DEFAULT_INSPECTOR_WIDTH,
            resizing: None,
        }
    }
}
