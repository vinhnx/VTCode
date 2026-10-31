use super::*;

#[gpui_kit::test]
fn command_palette_searches_all_loaded_sessions_and_workspace_files(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(4);
    view.update(cx, |view, _| {
        view.backend = Some(BackendHandle { commands });
        view.sessions = (0..16)
            .map(|index| vtcode_desktop_protocol::SessionSummary {
                session_id: format!("archived-{index}"),
                title: Some(format!("Archived thread {index}")),
                workspace: std::env::temp_dir().display().to_string(),
                completed: true,
                resumable: true,
                size_bytes: 0,
                updated_at: None,
            })
            .collect();
        view.files = (0..60)
            .map(|index| super::super::FileEntry {
                rel_path: format!("src/generated-{index:02}.rs"),
                depth: 1,
                is_dir: false,
            })
            .collect();
    });

    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_command_palette(window, cx));
        window.render_frame(cx);
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);

        let older_thread = palette_action_id(&super::super::PaletteAction::LoadSession("archived-15".into()));
        window.within("command-palette").input("Archived thread 15", cx);
        window.render_frame(cx);
        let palette = view.read(cx).palette.as_ref().unwrap().clone();
        assert_eq!(palette.read(cx).matched_count(), 1);
        assert_eq!(window.find(older_thread).label(), Some("Open session: Archived thread 15"));

        let later_file = palette_action_id(&super::super::PaletteAction::PreviewFile("src/generated-59.rs".into()));
        window.within("command-palette").press("cmd-a", cx);
        window.within("command-palette").input("generated-59.rs", cx);
        window.render_frame(cx);
        assert_eq!(palette.read(cx).query(cx), "generated-59.rs");
        assert_eq!(palette.read(cx).matched_count(), 1);
        assert_eq!(window.find(later_file).label(), Some("Open file: src/generated-59.rs"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn command_palette_keeps_dialog_accessible_with_reduced_motion(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update(|cx| cx.set_reduce_motion(true));

    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_command_palette(window, cx));
        window.render_frame(cx);
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);

        assert_eq!(window.find("command-palette").role(), Some(gpui_kit::Role::Dialog));
        assert_eq!(window.find("command-palette").label(), Some("Command palette"));
        assert!(view.read(cx).palette_focus.contains_focused(window, cx));
    })
    .unwrap();
}

#[gpui_kit::test]
fn context_usage_percent_handles_unknown_and_saturated_windows() {
    use super::super::context_usage_percent;
    use vtcode_exec_events::Usage;

    let usage = Usage {
        input_tokens: 30_000,
        cached_input_tokens: 10_000,
        cache_creation_tokens: 5_000,
        output_tokens: 5_000,
    };
    assert_eq!(context_usage_percent(&usage, 200_000), Some(25));
    assert_eq!(context_usage_percent(&usage, 0), None, "unknown window hides the field");
    let huge = Usage {
        input_tokens: u64::MAX,
        cached_input_tokens: u64::MAX,
        cache_creation_tokens: 0,
        output_tokens: 0,
    };
    assert_eq!(context_usage_percent(&huge, 100), Some(100), "saturates at 100");
}

#[gpui_kit::test]
fn saved_preferences_restore_theme_widths_and_inspector_state(cx: &mut TestAppContext) {
    let path = isolate_preferences();
    // Seed a preferences file with a non-default theme and pane widths.
    std::fs::write(
        &path,
        json!({
            "theme": "gruvbox-dark",
            "inspectorOpen": false,
            "sidebarWidth": 230.0,
            "inspectorWidth": 400.0,
        })
        .to_string(),
    )
    .expect("seed prefs");

    cx.update(gpui_kit::init);
    let (window, view) = cx.update(|cx| {
        gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
            let theme_mode = ThemeMode::Light;
            let entity = cx.new(|cx| DesktopView::new(window, cx, theme_mode));
            entity.update(cx, |view, cx| view.load_preferences(window, cx));
            entity
        })
        .unwrap()
    });
    cx.update_window(window, |_, _, cx| {
        view.update(cx, |view, cx| {
            assert_eq!(view.theme_id, "gruvbox-dark");
            assert_eq!(view.theme_mode, ThemeMode::Dark);
            assert!(!view.inspector_open);
            assert!((view.sidebar_width - 230.0).abs() < f32::EPSILON);
            assert!((view.inspector_width - 400.0).abs() < f32::EPSILON);
            cx.notify();
        });
    })
    .unwrap();
    drop(std::fs::remove_file(&path));
}

#[gpui_kit::test]
fn dragging_the_sidebar_divider_resizes_the_pane(cx: &mut TestAppContext) {
    use gpui_kit::{InputEvent, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent};

    let dispatch_mouse = |window: &mut gpui_kit::Window, position, event: MousePhase, cx: &mut gpui_kit::App| {
        match event {
            MousePhase::Down => {
                let _ = window.dispatch_event(
                    MouseDownEvent {
                        position,
                        button: MouseButton::Left,
                        modifiers: Modifiers::default(),
                        click_count: 1,
                        first_mouse: false,
                    }
                    .to_platform_input(),
                    cx,
                );
            }
            MousePhase::Move => {
                let _ = window.dispatch_event(
                    MouseMoveEvent {
                        position,
                        pressed_button: Some(MouseButton::Left),
                        modifiers: Modifiers::default(),
                    }
                    .to_platform_input(),
                    cx,
                );
            }
            MousePhase::Up => {
                let _ = window.dispatch_event(
                    MouseUpEvent {
                        position,
                        button: MouseButton::Left,
                        modifiers: Modifiers::default(),
                        click_count: 1,
                    }
                    .to_platform_input(),
                    cx,
                );
            }
        }
        window.render_frame(cx);
    };

    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| view.preferences.navigation_on_right = Some(false));
        window.render_frame(cx);
        let handle = window.find("resize-sidebar").bounds();
        let start = point(handle.center().x, handle.center().y);
        let initial_sidebar_width = view.read(cx).sidebar_width;

        // Press on the divider, drag right by 60px, release.
        dispatch_mouse(window, start, MousePhase::Down, cx);
        let resize = view.read(cx).resizing.expect("splitter press begins a resize");
        assert_eq!(resize.start_pointer_x_window, start.x, "drag start uses window-space pointer coordinates");
        assert_eq!(resize.start_width, px(initial_sidebar_width), "drag start retains pixel units for pane width");
        dispatch_mouse(window, point(start.x + px(6.), start.y), MousePhase::Move, cx);
        assert!(cx.has_active_drag(), "GPUI starts a typed resize drag");
        dispatch_mouse(window, point(start.x + px(60.), start.y), MousePhase::Move, cx);
        assert!(view.read(cx).resizing.is_some(), "splitter drag remains active");
        dispatch_mouse(window, point(start.x + px(60.), start.y), MousePhase::Up, cx);
        let width = view.read(cx).sidebar_width;
        assert!(width > 300.0 && width < 340.0, "sidebar grew by the drag delta: {width}");

        // Dragging past the bound clamps. Re-find the handle: the first
        // drag moved it.
        let handle = window.find("resize-sidebar").bounds();
        let start = point(handle.center().x, handle.center().y);
        dispatch_mouse(window, start, MousePhase::Down, cx);
        dispatch_mouse(window, point(start.x + px(6.), start.y), MousePhase::Move, cx);
        dispatch_mouse(window, point(start.x + px(600.), start.y), MousePhase::Move, cx);
        dispatch_mouse(window, point(start.x + px(600.), start.y), MousePhase::Up, cx);
        let width = view.read(cx).sidebar_width;
        assert!((width - crate::desktop::preferences::MAX_PANE_WIDTH).abs() < f32::EPSILON, "clamped width: {width}");

        let inspector_start_width = view.read(cx).inspector_width;
        let handle = window.find("resize-inspector").bounds();
        let start = point(handle.center().x, handle.center().y);
        dispatch_mouse(window, start, MousePhase::Down, cx);
        dispatch_mouse(window, point(start.x - px(6.), start.y), MousePhase::Move, cx);
        dispatch_mouse(window, point(start.x - px(40.), start.y), MousePhase::Move, cx);
        dispatch_mouse(window, point(start.x - px(40.), start.y), MousePhase::Up, cx);
        let view_state = view.read(cx);
        assert!(view_state.resizing.is_none(), "splitter release ends the resize");
        assert!(
            view_state.inspector_width > inspector_start_width + 30.
                && view_state.inspector_width < inspector_start_width + 50.,
            "moving the inspector edge left grows it: {}",
            view_state.inspector_width
        );
        assert_eq!(view_state.preferences.inspector_width, Some(view_state.inspector_width));

        view.update(cx, |view, cx| {
            view.preferences.navigation_on_right = Some(true);
            view.sidebar_width = 300.;
            cx.notify();
        });
        window.render_frame(cx);
        let initial_right_sidebar_width = view.read(cx).sidebar_width;
        let handle = window.find("resize-sidebar").bounds();
        let start = point(handle.center().x, handle.center().y);
        dispatch_mouse(window, start, MousePhase::Down, cx);
        dispatch_mouse(window, point(start.x - px(6.), start.y), MousePhase::Move, cx);
        dispatch_mouse(window, point(start.x - px(40.), start.y), MousePhase::Move, cx);
        dispatch_mouse(window, point(start.x - px(40.), start.y), MousePhase::Up, cx);
        assert!(
            view.read(cx).sidebar_width > initial_right_sidebar_width + 30.
                && view.read(cx).sidebar_width < initial_right_sidebar_width + 50.,
            "moving the right navigation edge left grows it: {}",
            view.read(cx).sidebar_width
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn pane_splitters_expose_accessibility_and_keyboard_resize(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| view.preferences.navigation_on_right = Some(false));
        window.render_frame(cx);
        let splitter = window.find("resize-sidebar");
        assert_eq!(splitter.role(), Some(gpui_kit::Role::Splitter));
        assert_eq!(splitter.label(), Some("Resize navigation"));

        for _ in 0..128 {
            if view.read(cx).sidebar_resize_focus.is_focused(window) {
                break;
            }
            window.press("tab", cx);
            window.render_frame(cx);
        }
        assert!(view.read(cx).sidebar_resize_focus.is_focused(window), "Tab reaches the sidebar splitter");
        window.press("right", cx);
        assert_eq!(view.read(cx).sidebar_width, 280.);
        window.press("shift-right", cx);
        assert_eq!(view.read(cx).sidebar_width, 330.);
        window.press("left", cx);
        assert_eq!(view.read(cx).sidebar_width, 320.);
        assert_eq!(view.read(cx).preferences.sidebar_width, Some(320.));
        window.click("resize-sidebar", cx);
        assert!(view.read(cx).sidebar_resize_focus.is_focused(window), "pointer interaction focuses the splitter");

        for _ in 0..128 {
            if view.read(cx).inspector_resize_focus.is_focused(window) {
                break;
            }
            window.press("tab", cx);
            window.render_frame(cx);
        }
        assert!(view.read(cx).inspector_resize_focus.is_focused(window), "Tab reaches the inspector splitter");
        window.press("left", cx);
        assert_eq!(view.read(cx).inspector_width, 450.);
        window.press("shift-right", cx);
        assert_eq!(view.read(cx).inspector_width, 400.);
        assert_eq!(view.read(cx).preferences.inspector_width, Some(400.));
    })
    .unwrap();
}

#[gpui_kit::test]
fn theme_switches_refresh_both_directions_and_preserve_typography_and_draft(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.find_all("toggle-theme").is_empty());
        view.update(cx, |view, cx| {
            view.preferences.ui_font_size = Some(19);
            view.preferences.code_font_size = Some(14);
            view.apply_theme("solarized-light".into(), window, cx);
            view.composer
                .update(cx, |input, cx| input.set_value("keep this draft", window, cx));
            view.transcript.push(TranscriptItem::new(
                "theme-content".into(),
                "assistant".into(),
                "Assistant".into(),
                "Text and `code` stay visible.".into(),
            ));
        });
        for (mode, id) in [(ThemeMode::Dark, "system-dark"), (ThemeMode::Light, "system-light")] {
            view.update(cx, |view, cx| view.run_palette_action(super::super::PaletteAction::ToggleTheme, window, cx));
            window.render_frame(cx);
            assert_eq!(view.read(cx).theme_mode, mode);
            assert_eq!(view.read(cx).theme_id, id);
            assert_eq!(crate::desktop::preferences::Preferences::load().theme.as_deref(), Some(id));
            assert_eq!(cx.theme().font_size, px(19.));
            assert_eq!(cx.theme().mono_font_size, px(14.));
            assert_eq!(gpui_kit::base::Theme::global(cx).tokens.colors.foreground, cx.theme().colors.foreground);
            assert_eq!(view.read(cx).composer.read(cx).value().as_str(), "keep this draft");
            assert_eq!(view.read(cx).transcript.get(0).unwrap().id, "theme-content");
        }
    })
    .unwrap();
}

#[gpui_kit::test]
fn separate_theme_profiles_switch_and_restore_each_selected_theme(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.apply_theme("ciapre".into(), window, cx));
        window.click("open-settings", cx);
        window.render_frame(cx);
        window.click("separate-theme-profiles", cx);
        window.render_frame(cx);

        {
            let preferences = &view.read(cx).preferences;
            assert!(preferences.separate_theme_profiles);
            assert_eq!(preferences.dark_theme.as_deref(), Some("ciapre"));
            assert_eq!(preferences.light_theme.as_deref(), Some("catppuccin-latte"));
        }
        assert_eq!(window.find("theme-catppuccin-latte").label(), Some("Catppuccin Latte (light profile)"),);
        assert_eq!(window.find("theme-catppuccin-latte").checked(), Some(false));
        assert_eq!(window.find("theme-ciapre").checked(), Some(true));
        assert_eq!(window.find("edit-dark-profile").label(), Some("Edit the Dark appearance profile (selected)"));
        assert_eq!(window.find("edit-light-profile").label(), Some("Edit the Light appearance profile"));
        assert_eq!(window.find("edit-dark-profile").checked(), Some(true));
        assert_eq!(window.find("edit-light-profile").checked(), Some(false));

        window.click("edit-light-profile", cx);
        window.render_frame(cx);
        assert!(view.read(cx).route == Route::Settings);
        assert_eq!(window.find("edit-light-profile").label(), Some("Edit the Light appearance profile (selected)"));
        assert_eq!(window.find("edit-dark-profile").label(), Some("Edit the Dark appearance profile"));
        assert_eq!(window.find("edit-light-profile").checked(), Some(true));
        assert_eq!(window.find("edit-dark-profile").checked(), Some(false));
        assert_eq!(view.read(cx).theme_mode, ThemeMode::Light);
        assert_eq!(view.read(cx).theme_id, "catppuccin-latte");
        window.within("settings-panel").click("font-ui-1", cx);
        window.render_frame(cx);
        assert_eq!(cx.theme().font_size, px(17.));
        assert_eq!(window.find("font-ui-value").label(), Some("UI font size: 17 pixels"));
        assert_eq!(view.read(cx).preferences.typography_for(ThemeMode::Light).ui_font_size, 17);
        window.click("edit-dark-profile", cx);
        window.render_frame(cx);
        assert!(view.read(cx).route == Route::Settings);
        assert_eq!(view.read(cx).theme_mode, ThemeMode::Dark);
        assert_eq!(cx.theme().font_size, px(16.));
        window.click("edit-light-profile", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).theme_mode, ThemeMode::Light);
        assert_eq!(cx.theme().font_size, px(17.));

        scroll_theme_into_view(window, &view, "theme-solarized-light", cx);
        window.click("theme-solarized-light", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).theme_id, "solarized-light");
        assert_eq!(view.read(cx).preferences.light_theme.as_deref(), Some("solarized-light"));

        window.within("settings-panel").click("settings-dismiss", cx);
        window.render_frame(cx);
        view.update(cx, |view, cx| view.run_palette_action(super::super::PaletteAction::ToggleTheme, window, cx));
        window.render_frame(cx);
        assert_eq!(view.read(cx).theme_mode, ThemeMode::Dark);
        assert_eq!(cx.theme().font_size, px(16.));
        assert_eq!(view.read(cx).theme_id, "ciapre");
        view.update(cx, |view, cx| view.run_palette_action(super::super::PaletteAction::ToggleTheme, window, cx));
        window.render_frame(cx);
        assert_eq!(view.read(cx).theme_mode, ThemeMode::Light);
        assert_eq!(cx.theme().font_size, px(17.));
        assert_eq!(view.read(cx).theme_id, "solarized-light");

        let saved = crate::desktop::preferences::Preferences::load();
        assert!(saved.separate_theme_profiles);
        assert_eq!(saved.light_theme.as_deref(), Some("solarized-light"));
        assert_eq!(saved.dark_theme.as_deref(), Some("ciapre"));
        assert_eq!(saved.typography_for(ThemeMode::Light).ui_font_size, 17);
        assert_eq!(saved.typography_for(ThemeMode::Dark).ui_font_size, 16);
    })
    .unwrap();
}

#[gpui_kit::test]
fn inactive_system_swatch_carries_no_selection_state(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    cx.update_window(window, |_, window, cx| {
        window.click("open-settings", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).theme_id, "ciapre");
        assert_eq!(window.find("theme-system-light").label(), Some("System Light"));
        assert_eq!(window.find("theme-system-light").checked(), Some(false));
        let active_label = window.find("theme-ciapre").label().unwrap_or_default().to_string();
        assert!(active_label.ends_with(" (active)"), "unexpected active label: {active_label}");
        assert_eq!(window.find("theme-ciapre").checked(), Some(true));
    })
    .unwrap();
}

#[gpui_kit::test]
fn profile_swatches_carry_a_visible_caption(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.apply_theme("ciapre".into(), window, cx));
        window.click("open-settings", cx);
        window.render_frame(cx);
        window.click("separate-theme-profiles", cx);
        window.render_frame(cx);
        assert_eq!(window.find("theme-catppuccin-latte-profile").label(), Some("Light"));
        assert_eq!(window.find("theme-ciapre-profile").label(), Some("Dark"));
        assert_eq!(window.find("theme-github-dark-profile").label(), None);
        let strip = window.find("theme-list").bounds();
        let swatch = window.find("theme-ciapre").bounds();
        assert!(
            swatch.bottom() <= strip.bottom(),
            "caption lane must fit inside the strip: swatch={swatch:?} strip={strip:?}"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn typography_rows_keep_a_compact_vertical_rhythm(cx: &mut TestAppContext) {
    let (window, _) = open_desktop(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    cx.update_window(window, |_, window, cx| {
        window.click("open-settings", cx);
        window.render_frame(cx);
        let ui = window.find("choose-ui-font").bounds();
        let code = window.find("choose-code-font").bounds();
        let gap = code.top() - ui.bottom();
        assert!(gap < px(36.), "typography rows must sit close together: {gap:?}");
    })
    .unwrap();
}

#[gpui_kit::test]
fn appearance_themes_scroll_horizontally_without_moving_typography(cx: &mut TestAppContext) {
    let (window, view) = open_desktop_sized(cx, 760., 700.);
    cx.update(|cx| cx.set_reduce_motion(true));
    cx.update_window(window, |_, window, cx| {
        window.click("open-settings", cx);
        window.render_frame(cx);
        let strip = window.find("theme-list").bounds();
        let first = window.find("theme-system-light").bounds();
        let typography_top = window.find("choose-ui-font").bounds().top();
        assert!(
            ["theme-github-dark", "theme-catppuccin-mocha", "theme-solarized-light"]
                .into_iter()
                .any(|id| window.find(id).bounds().left() > strip.right()),
            "the list must overflow horizontally"
        );
        assert_eq!(first.size.width, px(104.), "swatches must not shrink to fit");
        window.click("next-themes", cx);
        window.render_frame(cx);
        assert!(window.find("theme-system-light").bounds().left() < first.left());
        window.click("previous-themes", cx);
        window.render_frame(cx);
        assert_eq!(window.find("theme-system-light").bounds().left(), first.left());

        scroll_theme_into_view(window, &view, "theme-catppuccin-mocha", cx);
        let mocha = window.find("theme-catppuccin-mocha").bounds();
        assert!(mocha.left() >= strip.left() && mocha.right() <= strip.right());
        assert_eq!(window.find("choose-ui-font").bounds().top(), typography_top);
        window.click("theme-catppuccin-mocha", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).theme_id, "catppuccin-mocha");
        assert_eq!(window.find("theme-catppuccin-mocha").checked(), Some(true));
        assert_eq!(window.find("theme-catppuccin-mocha").label(), Some("Catppuccin Mocha (active)"));

        window.scroll("theme-list", gpui_kit::ScrollDelta::Lines(point(0., 200.)), cx);
        window.render_frame(cx);
        assert_eq!(window.find("choose-ui-font").bounds().top(), typography_top);
        window.click("theme-system-light", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).theme_id, "system-light");
    })
    .unwrap();
}

#[gpui_kit::test]
fn settings_theme_picker_applies_a_theme_live(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.click("open-settings", cx);
        window.render_frame(cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).theme_id, "ciapre", "Ciapre is the default theme");
        assert_eq!(window.find("theme-ciapre").checked(), Some(true));
        assert_eq!(window.find("theme-ayu").label(), Some("ayu · Ayu Dark"));
        assert_eq!(window.find("theme-ayu-dark").label(), Some("ayu-dark · Ayu Dark"));
        scroll_theme_into_view(window, &view, "theme-github-dark", cx);
        window.click("theme-github-dark", cx);
        window.render_frame(cx);
        assert_eq!(window.find("theme-ciapre").checked(), Some(false));
        assert_eq!(window.find("theme-github-dark").checked(), Some(true));
        {
            let view = view.read(cx);
            assert_eq!(view.theme_id, "github-dark");
            assert_eq!(view.theme_mode, ThemeMode::Dark);
        }
        scroll_theme_into_view(window, &view, "theme-solarized-light", cx);
        window.click("theme-solarized-light", cx);
        window.render_frame(cx);
        assert_eq!(window.find("theme-github-dark").checked(), Some(false));
        assert_eq!(window.find("theme-solarized-light").checked(), Some(true));
        {
            let view = view.read(cx);
            assert_eq!(view.theme_id, "solarized-light");
            assert_eq!(view.theme_mode, ThemeMode::Light);
        }
    })
    .unwrap();
}

#[gpui_kit::test]
fn file_change_items_render_as_toggleable_diff_cards(cx: &mut TestAppContext) {
    use vtcode_desktop_protocol::ThreadEventNotification;
    use vtcode_exec_events::{
        FileChangeItem, FileUpdateChange, ItemCompletedEvent, PatchApplyStatus, PatchChangeKind, ThreadEvent,
        ThreadItem, ThreadItemDetails,
    };

    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.handle_thread_event(ThreadEventNotification {
                session_id: "session-d".to_string(),
                turn_id: Some("turn-1".to_string()),
                event: vtcode_exec_events::VersionedThreadEvent::new(ThreadEvent::ItemCompleted(ItemCompletedEvent {
                    item: ThreadItem {
                        context: None,
                        id: "change-1".to_string(),
                        details: ThreadItemDetails::FileChange(Box::new(FileChangeItem {
                            diff_incomplete: Some(false),
                            changes: vec![FileUpdateChange {
                                path: "src/main.rs".to_string(),
                                kind: PatchChangeKind::Update,
                            }],
                            status: PatchApplyStatus::Completed,
                            unified_diff: Some("@@ -1,2 +1,3 @@\n context\n-old line\n+new line\n".to_string()),
                            additions: Some(1),
                            deletions: Some(1),
                        })),
                    },
                })),
            });
            let item = view.transcript.get(0).expect("diff row");
            assert_eq!(item.role, "diff");
            let diff = item.diff.as_ref().expect("structured diff payload");
            assert_eq!(diff.files.len(), 1);
            assert_eq!(diff.files[0].path, "src/main.rs");
            assert_eq!(diff.additions, Some(1));
            assert_eq!(diff.deletions, Some(1));
            assert!(!diff.failed);
            assert!(diff.patch.contains("+new line"));
        });
        window.render_frame(cx);

        // Diffs show by default: the patch view renders with no click, and the
        // copy action works straight away.
        assert!(view.read(cx).transcript.get(0).unwrap().expanded, "diff cards start expanded");
        assert!(
            window.try_find("diff-lines-change-1").is_some(),
            "the patch view renders without expanding the card"
        );
        window.click("copy-patch-change-1", cx);
        assert!(
            cx.read_from_clipboard().unwrap().text().unwrap().contains("+new line"),
            "copy patch must copy the raw unified diff"
        );

        let updated_patch = "@@ -1 +1 @@\n-old line\n+latest line\n";
        view.update(cx, |view, cx| {
            view.upsert_diff_card(
                "change-1".into(),
                &FileChangeItem {
                    diff_incomplete: Some(false),
                    changes: vec![FileUpdateChange {
                        path: "src/main.rs".into(),
                        kind: PatchChangeKind::Update,
                    }],
                    status: PatchApplyStatus::Completed,
                    unified_diff: Some(updated_patch.into()),
                    additions: Some(1),
                    deletions: Some(1),
                },
            );
            cx.notify();
        });
        assert!(view.read(cx).transcript.get(0).unwrap().expanded, "a patch update preserves the expanded card");
        window.click("copy-patch-change-1", cx);
        assert_eq!(cx.read_from_clipboard().unwrap().text().as_deref(), Some(updated_patch));

        // Collapsing hides the patch view but keeps the card; expanding shows
        // it again.
        window.click("diff-change-1", cx);
        window.render_frame(cx);
        assert!(!view.read(cx).transcript.get(0).unwrap().expanded);
        assert!(window.try_find("diff-lines-change-1").is_none(), "collapsing the card hides the patch view");
        window.click("diff-change-1", cx);
        window.render_frame(cx);
        assert!(view.read(cx).transcript.get(0).unwrap().expanded);
        assert!(window.try_find("diff-lines-change-1").is_some(), "re-expanding the card shows the patch view again");
    })
    .unwrap();
}

#[test]
fn transcript_diff_classification_is_bounded_and_preserves_line_kinds() {
    let patch = "diff --git a/file.rs b/file.rs\n--- a/file.rs\n+++ b/file.rs\n@@ -1 +1 @@\n-old\n+new\n context\n";
    let mut lines = super::super::ClassifiedDiffLines::new(patch);
    let classified = lines.by_ref().map(|line| (line.kind, line.text)).collect::<Vec<_>>();
    assert!(!lines.truncated);
    assert_eq!(
        classified,
        [
            (super::super::DiffLineKind::Header, "diff --git a/file.rs b/file.rs"),
            (super::super::DiffLineKind::Header, "--- a/file.rs"),
            (super::super::DiffLineKind::Header, "+++ b/file.rs"),
            (super::super::DiffLineKind::Hunk, "@@ -1 +1 @@"),
            (super::super::DiffLineKind::Removed, "-old"),
            (super::super::DiffLineKind::Added, "+new"),
            (super::super::DiffLineKind::Context, " context"),
        ]
    );

    let at_limit = (0..super::super::MAX_DIFF_LINES)
        .map(|index| format!("+line-{index}\n"))
        .collect::<String>();
    let mut lines = super::super::ClassifiedDiffLines::new(&at_limit);
    assert_eq!(lines.by_ref().count(), super::super::MAX_DIFF_LINES);
    assert!(!lines.truncated);

    let oversized = (0..super::super::MAX_DIFF_LINES + 1)
        .map(|index| format!("+line-{index}\n"))
        .collect::<String>();
    let mut lines = super::super::ClassifiedDiffLines::new(&oversized);
    assert_eq!(lines.by_ref().count(), super::super::MAX_DIFF_LINES);
    assert!(lines.truncated);
}

#[gpui_kit::test]
fn measured_transcript_follows_tall_messages_and_preserves_scroll_away(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let scroller = cx.update(|cx| {
        view.update(cx, |view, _| {
            for index in 0..20 {
                view.transcript.push(TranscriptItem::new(
                    format!("message-{index}"),
                    "assistant".to_string(),
                    "VT Code".into(),
                    format!("Message {index}\n\n{}", "wrapped words ".repeat(index + 1)),
                ));
            }
            view.transcript.push(TranscriptItem::new(
                "stream".to_string(),
                "assistant".to_string(),
                "VT Code".into(),
                "A tall paragraph.\n\n".repeat(100),
            ));
            view.message_scroller.clone()
        })
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.within("pane-root").find_by_label("Conversation").role(), Some(gpui_kit::Role::Region));
        let last = window.find("transcript-row-stream").bounds();
        let viewport = window.find("transcript-scroll").bounds();
        assert!(last.size.height > viewport.size.height);
        assert!(last.bottom() <= viewport.bottom() + px(1.));
        assert!(last.bottom() >= viewport.bottom() - px(32.), "last={last:?}, viewport={viewport:?}");
    })
    .unwrap();
    cx.update_window(window, |_, window, cx| {
        window.scroll("transcript-scroll", gpui_kit::ScrollDelta::Pixels(point(px(0.), px(-120.))), cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.executor().advance_clock(Duration::from_millis(250));
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let anchor = scroller.read(cx).logical_scroll_top();
        assert!(!scroller.read(cx).is_following_tail());
        assert!(scroller.read(cx).is_scrolled_up());
        view.update(cx, |view, _| {
            view.append_or_update_item(
                "stream".to_string(),
                "assistant",
                "VT Code",
                &"Additional text.\n\n".repeat(150),
                false,
            );
        });
        window.render_frame(cx);
        let after = scroller.read(cx).logical_scroll_top();
        assert_eq!(after.item_ix, anchor.item_ix);
        assert!((after.offset_in_item - anchor.offset_in_item).abs() <= px(1.));
        assert_eq!(
            window
                .within("transcript-scroller")
                .find_by_label("Jump to latest message")
                .role(),
            Some(gpui_kit::Role::Button)
        );
        window.click((gpui_kit::ElementId::from("transcript-scroller"), "jump-to-latest"), cx);
        window.render_frame(cx);
        assert!(scroller.read(cx).is_following_tail());
    })
    .unwrap();
}

#[gpui_kit::test]
fn transcript_eviction_preserves_reader_anchor(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let scroller = cx.update(|cx| {
        view.update(cx, |view, _| {
            for index in 0..MAX_TRANSCRIPT_ITEMS {
                view.transcript.push(TranscriptItem::new(
                    index.to_string(),
                    "assistant".to_string(),
                    "VT Code".into(),
                    format!("Message {index}"),
                ));
            }
            view.message_scroller.clone()
        })
    });

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        scroller.update(cx, |state, cx| assert!(state.scroll_to_item(12, cx)));
        window.render_frame(cx);
        assert_eq!(scroller.read(cx).logical_scroll_top().item_ix, 12);

        view.update(cx, |view, _| {
            view.transcript.push(TranscriptItem::new(
                "newest".to_string(),
                "assistant".to_string(),
                "VT Code".into(),
                "Newest message".to_string(),
            ));
        });
        window.render_frame(cx);

        let anchor = scroller.read(cx).logical_scroll_top();
        assert_eq!(view.read(cx).transcript.get(anchor.item_ix).unwrap().id, "12");
        assert_eq!(scroller.read(cx).item_count(), MAX_TRANSCRIPT_ITEMS);
        assert!(!scroller.read(cx).is_following_tail());
    })
    .unwrap();
}

#[gpui_kit::test]
fn rich_transcript_copy_actions_keep_raw_markdown_and_exact_code(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let source = "# Review\n\n**Bold** and <b>inline HTML</b>\n\n```rust\nfn sample() -> u8 { 7 }\n```";
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.transcript.push(TranscriptItem::new(
                "copy-test".to_string(),
                "assistant".to_string(),
                "VT Code".into(),
                source.to_string(),
            ));
        });
        window.render_frame(cx);
        // Message actions reveal on hover; keyboard users copy via the palette command.
        window.hover("transcript-row-copy-test", cx);
        window.render_frame(cx);
        window.click("copy-message-copy-test", cx);
        assert_eq!(cx.read_from_clipboard().unwrap().text().as_deref(), Some(source));
        assert_eq!(window.notifications(cx).len(), 1);
        focus_element_by_tab(window, "copy-code", cx);
        window.press("space", cx);
        assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap().trim_end(), "fn sample() -> u8 { 7 }");
        assert_eq!(window.notifications(cx).len(), 1);
    })
    .unwrap();
}

#[gpui_kit::test]
fn transcript_remeasures_wrapped_markdown_when_window_narrows(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let wide_height = cx
        .update_window(window, |_, window, cx| {
            view.update(cx, |view, _| {
                view.transcript.push(TranscriptItem::new(
                    "resize".to_string(),
                    "assistant".to_string(),
                    "VT Code".into(),
                    format!("## Summary\n\n{}", "A longer paragraph that wraps naturally. ".repeat(50)),
                ));
            });
            window.render_frame(cx);
            window.find("transcript-row-resize").bounds().size.height
        })
        .unwrap();

    cx.simulate_window_resize(window, size(px(720.), px(900.)));
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let row = window.find("transcript-row-resize").bounds();
        assert!(row.size.height > wide_height);
        let scroller = view.read(cx).message_scroller.clone();
        assert!(scroller.read(cx).is_following_tail());
        assert!(row.bottom() <= window.find("transcript-scroll").bounds().bottom() + px(1.));
    })
    .unwrap();
}

#[gpui_kit::test]
fn slash_theme_accepts_capitalized_ids_like_the_cli(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(4);
    view.update(cx, |view, _| {
        view.backend = Some(BackendHandle { commands });
    });
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.composer
                .update(cx, |input, cx| input.set_value("/theme CIAPRE-DARK", window, cx));
            view.submit_prompt(window, cx);
        });
        window.render_frame(cx);
        assert_eq!(view.read(cx).theme_id, "ciapre-dark");
        assert!(view.read(cx).error.is_none());

        view.update(cx, |view, cx| {
            view.composer
                .update(cx, |input, cx| input.set_value("/theme NoSuchTheme", window, cx));
            view.submit_prompt(window, cx);
        });
        window.render_frame(cx);
        assert_eq!(view.read(cx).theme_id, "ciapre-dark", "unknown ids keep the active theme");
        assert_eq!(
            view.read(cx).error.as_deref(),
            Some("Theme 'NoSuchTheme' is not available. Choose a theme from Appearance settings.")
        );
    })
    .unwrap();
}
