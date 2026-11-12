//! App startup: menus, keybindings, window reopen, construction, and preference persistence.
use super::super::{SharedUrlInbox, UrlInbox};
use super::*;
use gpui_kit::WindowBounds;
use std::{cell::RefCell, rc::Rc};

/// Documentation and issue tracker opened by the Help menu and About dialog.
pub(super) const HELP_URL: &str = "https://github.com/vinhnx/vtcode";

/// Modified chords from the fixed table in [`init_keybindings`].
/// `shortcuts::bindings()` rejects overrides colliding with these: the deeper
/// fixed binding would shadow the rebind (e.g. `ctrl-m` opening the model
/// picker instead of the rebound command while the composer is focused).
/// Bare fixed chords need no entry — the modifier rule rejects them already.
/// Keep in sync with the table below.
pub(super) const FIXED_RESERVED_CHORDS: &[&str] = &[
    // Function keys pass the configurable table's modifier rule, so the
    // tree-scoped rename chord needs an explicit entry (bare delete and
    // backspace are rejected by that rule already).
    "f2",
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
    "ctrl-s",
    "ctrl-g",
    "cmd-s",
    "cmd-j",
];

pub(super) fn init_keybindings(cx: &mut App, preferences: &preferences::Preferences) {
    bind_configurable_shortcuts(cx, &preferences.shortcuts);
    cx.bind_keys([
        KeyBinding::new("cmd-w", CloseWindow, Some(key_context::DESKTOP)),
        KeyBinding::new("cmd-m", MinimizeWindow, Some(key_context::DESKTOP)),
        KeyBinding::new("ctrl-cmd-f", ToggleFullScreen, Some(key_context::DESKTOP)),
        KeyBinding::new("cmd-j", ToggleTerminal, Some(key_context::DESKTOP)),
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
        KeyBinding::new("f2", RenameSelectedFile, Some(gpui_kit::base::tree_key_context())),
        KeyBinding::new("delete", DeleteSelectedFile, Some(gpui_kit::base::tree_key_context())),
        KeyBinding::new("backspace", DeleteSelectedFile, Some(gpui_kit::base::tree_key_context())),
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
        // CLI scope-cycle twin: opens history when closed, cycles the search
        // scope while open. Ctrl+R keeps the palette toggle contract.
        KeyBinding::new("ctrl-s", CycleHistoryScope, Some(key_context::DESKTOP)),
        // CLI picker parity: Ctrl+G cancels the history search and restores the
        // draft (Ctrl+C stays Kit's Copy in the input context, so it cannot be
        // claimed here without also hijacking Cmd+C).
        KeyBinding::new("ctrl-g", CancelPromptHistory, Some(key_context::DESKTOP)),
        // Fixed like window chords: the app has exactly one saveable
        // surface, so Cmd+S always means "save the preview draft" and the
        // root handler no-ops unless an edit session is armed.
        KeyBinding::new("cmd-s", SavePreviewSource, Some(key_context::DESKTOP)),
        KeyBinding::new("up", TranscriptPrevious, Some(key_context::TRANSCRIPT)),
        KeyBinding::new("down", TranscriptNext, Some(key_context::TRANSCRIPT)),
        KeyBinding::new("home", TranscriptFirst, Some(key_context::TRANSCRIPT)),
        KeyBinding::new("end", TranscriptLast, Some(key_context::TRANSCRIPT)),
        KeyBinding::new("up", SessionPrevious, Some(key_context::SESSIONS)),
        KeyBinding::new("down", SessionNext, Some(key_context::SESSIONS)),
        KeyBinding::new("home", SessionFirst, Some(key_context::SESSIONS)),
        KeyBinding::new("end", SessionLast, Some(key_context::SESSIONS)),
        KeyBinding::new("enter", OpenSelectedSession, Some(key_context::SESSIONS)),
        KeyBinding::new("backspace", DeleteHighlightedSession, Some(key_context::SESSIONS)),
        KeyBinding::new("delete", DeleteHighlightedSession, Some(key_context::SESSIONS)),
        KeyBinding::new("left", PreviousThreadTab, Some(key_context::THREAD_TABS)),
        KeyBinding::new("right", NextThreadTab, Some(key_context::THREAD_TABS)),
        KeyBinding::new("home", FirstThreadTab, Some(key_context::THREAD_TABS)),
        KeyBinding::new("end", LastThreadTab, Some(key_context::THREAD_TABS)),
        KeyBinding::new("enter", OpenSelectedThreadTab, Some(key_context::THREAD_TABS)),
        KeyBinding::new("up", ActivityPrevious, Some(key_context::ACTIVITY)),
        KeyBinding::new("down", ActivityNext, Some(key_context::ACTIVITY)),
        KeyBinding::new("home", ActivityFirst, Some(key_context::ACTIVITY)),
        KeyBinding::new("end", ActivityLast, Some(key_context::ACTIVITY)),
    ]);
    if preferences.vim_navigation {
        apply_vim_navigation(cx, true);
    }
}

/// Bind the configurable shortcut table, falling back to defaults when a
/// hand-edited table fails validation. Startup binds once; shortcut saves
/// reinstall through [`reinstall_shortcuts`] so retired chords die with them.
fn bind_configurable_shortcuts(cx: &mut App, shortcuts: &std::collections::BTreeMap<String, String>) {
    match shortcuts::bindings(shortcuts, cx) {
        Ok(bindings) => cx.bind_keys(bindings),
        Err(error) => {
            eprintln!("Desktop shortcuts: {error}; using defaults");
            if let Ok(bindings) = shortcuts::bindings(&Default::default(), cx) {
                cx.bind_keys(bindings);
            }
        }
    }
}

/// Live-apply a saved shortcut table: shadow retired chords with `NoAction`,
/// then bind the fresh table. Same-chord conflicts resolve toward the latest
/// binding, so shadows must precede the table. The fixed table never changes
/// and stays bound once from [`init_keybindings`]. Reinstalling appends
/// identical duplicates for unchanged chords; dispatch stops at the first
/// hit, so they are harmless, and saves are rare user gestures.
pub(super) fn reinstall_shortcuts(
    cx: &mut App,
    old_shortcuts: &std::collections::BTreeMap<String, String>,
    new_shortcuts: &std::collections::BTreeMap<String, String>,
) {
    cx.bind_keys(shortcuts::stale_shadows(old_shortcuts, new_shortcuts, cx));
    bind_configurable_shortcuts(cx, new_shortcuts);
}

/// (chord, key context) rows of the experimental vim layer. Vim chords reuse
/// the plain navigation actions, so toggling is pure keymap surgery: live
/// bindings while the toggle holds, `NoAction` shadows when it drops (keymaps
/// only append — see [`reinstall_shortcuts`]). Scoped to the navigation
/// containers on purpose: a bare letter bound under `DESKTOP` would hijack
/// typing in every text input, and a fixed-then-gated handler would still
/// swallow the keystroke when off (bubble listeners stop by default). Bare
/// chords need no `FIXED_RESERVED_CHORDS` entry — the configurable table's
/// modifier rule rejects them already.
pub(super) const VIM_CHORDS: &[(&str, &str)] = &[
    ("j", key_context::SESSIONS),
    ("k", key_context::SESSIONS),
    ("g", key_context::SESSIONS),
    ("G", key_context::SESSIONS),
    ("i", key_context::SESSIONS),
    ("/", key_context::SESSIONS),
    ("j", key_context::TRANSCRIPT),
    ("k", key_context::TRANSCRIPT),
    ("g", key_context::TRANSCRIPT),
    ("G", key_context::TRANSCRIPT),
    ("i", key_context::TRANSCRIPT),
    ("/", key_context::TRANSCRIPT),
    ("h", key_context::INSPECTOR),
    ("l", key_context::INSPECTOR),
    ("i", key_context::INSPECTOR),
    ("/", key_context::INSPECTOR),
    ("h", key_context::SETTINGS),
    ("l", key_context::SETTINGS),
    ("i", key_context::SETTINGS),
    ("/", key_context::SETTINGS),
    ("h", key_context::THREAD_TABS),
    ("l", key_context::THREAD_TABS),
    ("i", key_context::THREAD_TABS),
    ("/", key_context::THREAD_TABS),
    ("j", key_context::ACTIVITY),
    ("k", key_context::ACTIVITY),
    ("g", key_context::ACTIVITY),
    ("shift-g", key_context::ACTIVITY),
    ("i", key_context::ACTIVITY),
    ("/", key_context::ACTIVITY),
];

/// Live out or retire the vim layer. Same-chord conflicts resolve toward the
/// latest binding, so enabling re-binds over stale shadows and disabling
/// shadows over stale live rows; toggles are rare user gestures, so the
/// appended duplicates are harmless (same tradeoff as [`reinstall_shortcuts`]).
pub(super) fn apply_vim_navigation(cx: &mut App, enabled: bool) {
    if enabled {
        cx.bind_keys([
            KeyBinding::new("j", SessionNext, Some(key_context::SESSIONS)),
            KeyBinding::new("k", SessionPrevious, Some(key_context::SESSIONS)),
            KeyBinding::new("g", SessionFirst, Some(key_context::SESSIONS)),
            KeyBinding::new("G", SessionLast, Some(key_context::SESSIONS)),
            KeyBinding::new("i", FocusComposer, Some(key_context::SESSIONS)),
            KeyBinding::new("/", OpenConversationSearch, Some(key_context::SESSIONS)),
            KeyBinding::new("j", TranscriptNext, Some(key_context::TRANSCRIPT)),
            KeyBinding::new("k", TranscriptPrevious, Some(key_context::TRANSCRIPT)),
            KeyBinding::new("g", TranscriptFirst, Some(key_context::TRANSCRIPT)),
            KeyBinding::new("G", TranscriptLast, Some(key_context::TRANSCRIPT)),
            KeyBinding::new("i", FocusComposer, Some(key_context::TRANSCRIPT)),
            KeyBinding::new("/", OpenConversationSearch, Some(key_context::TRANSCRIPT)),
            KeyBinding::new("h", PreviousInspectorTab, Some(key_context::INSPECTOR)),
            KeyBinding::new("l", NextInspectorTab, Some(key_context::INSPECTOR)),
            KeyBinding::new("i", FocusComposer, Some(key_context::INSPECTOR)),
            KeyBinding::new("/", OpenConversationSearch, Some(key_context::INSPECTOR)),
            KeyBinding::new("h", PreviousSettingsTab, Some(key_context::SETTINGS)),
            KeyBinding::new("l", NextSettingsTab, Some(key_context::SETTINGS)),
            KeyBinding::new("i", FocusComposer, Some(key_context::SETTINGS)),
            KeyBinding::new("/", OpenConversationSearch, Some(key_context::SETTINGS)),
            KeyBinding::new("h", PreviousThreadTab, Some(key_context::THREAD_TABS)),
            KeyBinding::new("l", NextThreadTab, Some(key_context::THREAD_TABS)),
            KeyBinding::new("i", FocusComposer, Some(key_context::THREAD_TABS)),
            KeyBinding::new("/", OpenConversationSearch, Some(key_context::THREAD_TABS)),
            KeyBinding::new("j", ActivityNext, Some(key_context::ACTIVITY)),
            KeyBinding::new("k", ActivityPrevious, Some(key_context::ACTIVITY)),
            KeyBinding::new("g", ActivityFirst, Some(key_context::ACTIVITY)),
            KeyBinding::new("shift-g", ActivityLast, Some(key_context::ACTIVITY)),
            KeyBinding::new("i", FocusComposer, Some(key_context::ACTIVITY)),
            KeyBinding::new("/", OpenConversationSearch, Some(key_context::ACTIVITY)),
        ]);
    } else {
        cx.bind_keys(
            VIM_CHORDS
                .iter()
                .map(|(chord, context)| KeyBinding::new(chord, NoAction {}, Some(*context))),
        );
    }
}

pub(crate) fn run(startup_link: Option<protocol::handoff_url::HandoffLink>) {
    let prefs_phase = vtcode_commons::startup_trace::phase_started();
    let mut preferences = preferences::Preferences::load();
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
        login_items::reconcile_launch_preference(&mut preferences, cx);
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

/// Main window for `vtcode://` delivery. Weak: a closed window must drop
/// its view instead of leaking it through the global.
struct MainWindow {
    view: gpui_kit::WeakEntity<DesktopView>,
    window: gpui_kit::AnyWindowHandle,
}

impl gpui_kit::Global for MainWindow {}

/// Route handoff links to the live main window. Unknown sessions and
/// workspaces fail through the same notices as the argv path. A handoff
/// with no live window reopens first: dropping the links silently would
/// strand CLI `/desktop` invocations against a windowless app, and
/// reopening also raises a hidden or backgrounded window since the user
/// invoked the app. Only a failed reopen drops the links: there is no
/// surface to report on.
pub(super) fn dispatch_handoff_links(cx: &mut App, links: Vec<protocol::handoff_url::HandoffLink>) {
    if links.is_empty() {
        return;
    }
    if open_or_activate_main_window(cx, None, None).is_err() {
        vtcode_commons::diagnostics::stderr(format_args!("handoff delivery missed its window"));
        return;
    }
    let Some(main) = cx.try_global::<MainWindow>() else {
        return;
    };
    let Some(view) = main.view.upgrade() else {
        return;
    };
    let window = main.window;
    if window
        .update(cx, |_, window, cx| {
            view.update(cx, |view, cx| {
                for link in links {
                    view.handle_handoff_link(link, window, cx);
                }
            });
        })
        .is_err()
    {
        vtcode_commons::diagnostics::stderr(format_args!("handoff delivery missed its window"));
    }
}

pub(super) fn show_main_window(cx: &mut App) {
    if let Err(error) = open_or_activate_main_window(cx, None, None) {
        vtcode_commons::diagnostics::stderr(format_args!("failed to open VT Code Desktop window: {error}"));
    }
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
    // Saved preferences decide the starting theme; Ciapre is the default.
    let preferences = preferences.unwrap_or_else(preferences::Preferences::load);
    let start_theme = preferences
        .theme
        .clone()
        .unwrap_or_else(|| themes::DEFAULT_THEME_ID.to_string());
    let start_customs = preferences.custom_themes.clone();
    let start_maximized = preferences.window_maximized();
    let mut options = desktop_window_options();
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
        let theme_mode = themes::apply_selection(&start_theme, &start_customs, Some(window), cx);
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
    let mut handoff = None;
    let mut handoff_invalid = false;
    if let Some(link) = startup_link {
        match files::canonicalize_workspace_folder(&link.workspace) {
            Some(workspace) => handoff = Some((workspace, link.session_id)),
            None => handoff_invalid = true,
        }
    }
    if let Some((workspace, session_id)) = handoff {
        window.update(cx, |_, _, cx| view.update(cx, |view, cx| view.arm_session_link(workspace, session_id, cx)))?;
    } else if handoff_invalid {
        window.update(cx, |_, _, cx| {
            view.update(cx, |view, cx| {
                view.error = Some(rust_i18n::t!("app.workspace.invalid_folder").to_string());
                cx.notify();
            })
        })?;
    } else {
        let last_workspace = view.read(cx).preferences.recent_workspaces.first().map(PathBuf::from);
        if let Some(path) = last_workspace {
            window.update(cx, |_, _, cx| view.update(cx, |view, cx| view.select_workspace_on_startup(path, cx)))?;
        }
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
        // dropdowns are correct on their first render. One scan feeds both
        // the file destination and the CLI terminal pickers.
        let detected = files::detect_installed_editors();
        let offered = preferences::FileOpenDestination::offered(destination, &detected);
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
        let offered_terminals = preferences::CliTerminal::offered(terminal, &detected);
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
        let file_preview_source = cx.new(|cx| EditorState::new(window, cx).language("markdown"));
        let preview_source_subscription = cx.subscribe_in(&file_preview_source, window, |_, _, event, _, cx| {
            // Re-render so the Save action tracks the dirty state live.
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        });
        // One native read at construction; render and scroll never touch the OS.
        let login_item_status = login_items::status(cx);
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
            activity_projection_cache: None,
            output_view_cache: None,
            tool_tail_cache: HashMap::new(),
            transcript_markdown_style: None,
            selected_output_id: None,
            restored_review_history_incomplete: false,
            restored_activity_history_incomplete: false,
            restored_timeline: Arc::new(Vec::new()),
            active_turn_id: None,
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
            changelog_mode: updates::ChangelogMode::Latest,
            changelog_focus: cx.focus_handle(),
            changelog_return_focus: None,
            changelog_scroll: ScrollHandle::default(),
            release_notes_banner_visible_now: false,
            dismissed_release_notes_version: None,
            service_info: None,
            interaction: None,
            interaction_focus: cx.focus_handle(),
            pending_interaction_response: None,
            interaction_edit_pending: false,
            interaction_choice_ids: Vec::new(),
            interaction_answer_texts: Vec::new(),
            interaction_step_index: 0,
            interaction_immediate_choice_id: None,
            interaction_plan_expanded: false,
            composer,
            _composer_subscription: composer_subscription,
            ui_language_select,
            _ui_language_subscription: ui_language_subscription,
            file_destination_select,
            _file_destination_subscription: file_destination_subscription,
            cli_terminal_select,
            _cli_terminal_subscription: cli_terminal_subscription,
            turn_completion_select,
            _turn_completion_subscription: turn_completion_subscription,
            question_answer: cx.new(|cx| TextareaState::new(window, cx).auto_grow(1, 4)),
            theme_mode,
            files: Vec::new(),
            files_truncated: false,
            file_directory_paths: Arc::new(HashSet::new()),
            file_tree,
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
            file_preview_source,
            _preview_source_subscription: preview_source_subscription,
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
            terminal_panel_height: 240.0,
            terminal_resize_focus: cx.focus_handle().tab_stop(true),
            terminal_scroll: ScrollHandle::default(),
            inspector_open: false,
            inspector_tab: InspectorTab::default(),
            inspector_focus: cx.focus_handle().tab_stop(true),
            inspector_tabs_scroll: ScrollHandle::default(),
            changes: ChangesProjection::default(),
            selected_change: None,
            change_kind_filter: None,
            change_viewer: None,
            change_viewer_dirty: false,
            route_history: RouteHistory::new(),
            workspace_review: None,
            review_focus: cx.focus_handle().tab_stop(true),
            browser_focus: cx.focus_handle().tab_stop(true),
            browser_list_focus: cx.focus_handle().tab_stop(true),
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
            history_scope: HistoryScope::default(),
            recent_prompts: Vec::new(),
            recent_prompts_scope: None,
            recent_prompts_loading: false,
            palette_groups: Vec::new(),
            config_page_open: false,
            config_page_group: None,
            theme_scroll_light: ScrollHandle::default(),
            theme_scroll_dark: ScrollHandle::default(),
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
            queue_previews: Vec::new(),
            queue_ages_secs: Vec::new(),
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
            cli_handoff: None,
            last_usage: None,
            theme_id: themes::DEFAULT_THEME_ID.to_string(),
            preferences,
            login_item_status,
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

    /// Apply saved preferences: theme, app icon, pane widths, and inspector state.
    pub(super) fn load_preferences(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let preferences = self.preferences.clone();
        self.preferences.follow_cli_theme = Some(preferences.follow_cli_theme.unwrap_or(preferences.theme.is_none()));
        self.restore_saved_theme(window, cx);
        self.apply_typography(cx);
        self.refresh_dock_icon();
        self.sidebar_width = preferences.sidebar_width();
        self.terminal_panel_height = preferences.terminal_panel_height();
        self.terminal_panel_height = preferences.terminal_panel_height();
        self.inspector_width = preferences.inspector_width();
        self.inspector_open = false;
        cx.notify();
    }

    /// Persist the current comfort settings.
    pub(super) fn save_preferences(&mut self) {
        self.preferences.theme = Some(self.theme_id.clone());
        self.preferences.inspector_open = Some(self.inspector_open);
        self.preferences.zen_mode = Some(self.zen_mode);
        self.preferences.set_widths(self.sidebar_width, self.inspector_width);
        self.preferences.set_terminal_panel_height(self.terminal_panel_height);
        self.preferences.save();
    }
}
