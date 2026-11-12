use super::*;

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
        assert!(window.try_find("theme-list-light").is_none(), "light grid hides while editing dark");
        assert_eq!(window.find("theme-list-dark").label(), Some("Dark"));
        assert!(window.try_find("theme-catppuccin-latte").is_none(), "light swatch hides while editing dark");
        assert_eq!(window.find("theme-ciapre").checked(), Some(true));
        assert_eq!(window.find("edit-dark-profile").label(), Some("Edit the Dark appearance profile (selected)"));
        assert_eq!(window.find("edit-light-profile").label(), Some("Edit the Light appearance profile"));
        assert_eq!(window.find("edit-dark-profile").checked(), Some(true));
        assert_eq!(window.find("edit-light-profile").checked(), Some(false));
        window.hover("edit-dark-profile", cx);
        window.render_frame(cx);
        assert_eq!(window.find("edit-dark-profile").checked(), Some(true));
        assert_eq!(window.find("edit-light-profile").checked(), Some(false));

        window.click("edit-light-profile", cx);
        window.render_frame(cx);
        assert!(view.read(cx).current_route() == Route::Settings);
        assert_eq!(window.find("edit-light-profile").label(), Some("Edit the Light appearance profile (selected)"));
        assert_eq!(window.find("edit-dark-profile").label(), Some("Edit the Dark appearance profile"));
        assert_eq!(window.find("edit-light-profile").checked(), Some(true));
        assert_eq!(window.find("edit-dark-profile").checked(), Some(false));
        assert_eq!(view.read(cx).theme_mode, ThemeMode::Light);
        assert_eq!(view.read(cx).theme_id, "catppuccin-latte");
        assert!(window.try_find("theme-list-dark").is_none(), "dark grid hides while editing light");
        assert_eq!(window.find("theme-list-light").label(), Some("Light"));
        scroll_theme_into_view(window, &view, "theme-catppuccin-latte", cx);
        assert_eq!(window.find("theme-catppuccin-latte").label(), Some("Catppuccin Latte (active)"));
        // The grouped picker pushes typography below the fold; scroll down
        // for the font row, then back up for the profile buttons above.
        scroll_settings_by(window, px(-240.), cx);
        window.render_frame(cx);
        window.within("settings-panel").click("font-ui-1", cx);
        window.render_frame(cx);
        assert_eq!(cx.theme().font_size, px(17.));
        assert_eq!(window.find("font-ui-value").label(), Some("UI font size: 17 pixels"));
        assert_eq!(view.read(cx).preferences.typography_for(ThemeMode::Light).ui_font_size, 17);
        scroll_settings_by(window, px(240.), cx);
        window.render_frame(cx);
        window.click("edit-dark-profile", cx);
        window.render_frame(cx);
        assert!(view.read(cx).current_route() == Route::Settings);
        assert_eq!(view.read(cx).theme_mode, ThemeMode::Dark);
        assert_eq!(cx.theme().font_size, px(16.));
        assert!(window.try_find("theme-catppuccin-latte").is_none(), "light swatch hides again back on dark");
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
        assert!(
            window.try_find("theme-catppuccin-latte-profile").is_none(),
            "light caption hides while editing dark"
        );
        assert_eq!(window.find("theme-ciapre-profile").label(), Some("Dark"));
        scroll_theme_into_view(window, &view, "theme-github-dark", cx);
        assert_eq!(window.find("theme-github-dark-profile").label(), None);
        scroll_theme_into_view(window, &view, "theme-ciapre", cx);
        let strip = window.find("theme-list-dark").bounds();
        let swatch = window.find("theme-ciapre").bounds();
        assert!(
            swatch.bottom() <= strip.bottom(),
            "caption lane must fit inside the strip: swatch={swatch:?} strip={strip:?}"
        );
        window.click("edit-light-profile", cx);
        window.render_frame(cx);
        scroll_theme_into_view(window, &view, "theme-catppuccin-latte", cx);
        assert_eq!(window.find("theme-catppuccin-latte-profile").label(), Some("Light"));
        assert!(window.try_find("theme-ciapre-profile").is_none(), "dark caption hides while editing light");
        let strip = window.find("theme-list-light").bounds();
        let swatch = window.find("theme-catppuccin-latte").bounds();
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
    // Width stays narrow so both groups overflow; height fits the stacked
    // groups plus the typography pin below them.
    let (window, view) = open_desktop_sized(cx, 760., 900.);
    cx.update(|cx| cx.set_reduce_motion(true));
    cx.update_window(window, |_, window, cx| {
        window.click("open-settings", cx);
        window.render_frame(cx);
        let light_strip = window.find("theme-list-light").bounds();
        let dark_strip = window.find("theme-list-dark").bounds();
        let first_light = window.find("theme-system-light").bounds();
        let first_dark = window.find("theme-ciapre").bounds();
        let typography_top = window.find("choose-ui-font").bounds().top();
        // Overflow precondition in row-stride pixels: both rows hold more
        // swatches than the narrow viewport fits, so paging has somewhere
        // to go. Off-screen swatches stay unmounted (see
        // `theme_rows_mount_only_the_visible_window`), hence no find here.
        for (mode, strip) in [(ThemeMode::Light, light_strip), (ThemeMode::Dark, dark_strip)] {
            let count = super::super::themes::picker_entries_for(mode).len();
            let stride = super::super::appearance_theme_picker::THEME_SWATCH_STRIDE;
            assert!(px(count as f32 * stride) > strip.size.width, "the {mode:?} group must overflow horizontally");
        }
        assert_eq!(first_light.size.width, px(104.), "swatches must not shrink to fit");
        window.click("next-themes", cx);
        window.render_frame(cx);
        // One page is three strides: the fourth swatch of each row lands
        // exactly where the first sat, proving the items moved with the
        // handle (the paged-out first swatches unmount).
        for (mode, edge) in [
            (ThemeMode::Light, first_light.left()),
            (ThemeMode::Dark, first_dark.left()),
        ] {
            let entries = super::super::themes::picker_entries_for(mode);
            let third = format!("theme-{}", entries[3].id());
            assert_eq!(window.find(third).bounds().left(), edge);
        }
        window.click("previous-themes", cx);
        window.render_frame(cx);
        assert_eq!(window.find("theme-system-light").bounds().left(), first_light.left());
        assert_eq!(window.find("theme-ciapre").bounds().left(), first_dark.left());
        assert_eq!(window.find("choose-ui-font").bounds().top(), typography_top);

        // The stacked groups overflow the narrow panel: reveal the dark row,
        // then re-baseline the typography pin so the assertions below keep
        // proving that horizontal theme scrolling leaves it untouched.
        scroll_settings_by(window, px(-250.), cx);
        window.render_frame(cx);
        let typography_top = window.find("choose-ui-font").bounds().top();
        scroll_theme_into_view(window, &view, "theme-catppuccin-mocha", cx);
        let mocha = window.find("theme-catppuccin-mocha").bounds();
        assert!(mocha.left() >= dark_strip.left() && mocha.right() <= dark_strip.right());
        assert_eq!(window.find("choose-ui-font").bounds().top(), typography_top);
        window.click("theme-catppuccin-mocha", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).theme_id, "catppuccin-mocha");
        assert_eq!(window.find("theme-catppuccin-mocha").checked(), Some(true));
        assert_eq!(window.find("theme-catppuccin-mocha").label(), Some("Catppuccin Mocha (active)"));

        window.scroll("theme-list-dark", gpui_kit::ScrollDelta::Lines(point(0., 200.)), cx);
        window.render_frame(cx);
        assert_eq!(window.find("choose-ui-font").bounds().top(), typography_top);
        scroll_settings_by(window, px(250.), cx);
        window.render_frame(cx);
        window.click("theme-system-light", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).theme_id, "system-light");
    })
    .unwrap();
}

#[gpui_kit::test]
fn theme_picker_groups_swatches_by_mode(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    cx.update_window(window, |_, window, cx| {
        window.click("open-settings", cx);
        window.render_frame(cx);
        assert_eq!(window.find("theme-list-light").label(), Some("Light"));
        assert_eq!(window.find("theme-list-dark").label(), Some("Dark"));
        assert_eq!(window.find("theme-list").label(), Some("Visual themes"));
        let light = window.find("theme-list-light").bounds();
        let dark = window.find("theme-list-dark").bounds();
        assert!(light.bottom() <= dark.top(), "the light group sits above the dark group");
        // Asymmetric fixtures on both sides: system bases plus a light and a
        // dark palette theme each must land in their own vertical lane.
        // Deep swatches scroll into view first: horizontal paging never
        // changes the vertical lane, and off-screen swatches stay unmounted.
        for id in ["theme-system-light", "theme-solarized-light", "theme-catppuccin-latte"] {
            scroll_theme_into_view(window, &view, id, cx);
            let item = window.find(id).bounds();
            let middle = item.center().y;
            assert!(
                middle >= light.top() && middle <= light.bottom(),
                "{id} sits in the light group: item={item:?} light={light:?}"
            );
        }
        for id in [
            "theme-system-dark",
            "theme-ciapre",
            "theme-catppuccin-mocha",
            "theme-github-dark",
        ] {
            scroll_theme_into_view(window, &view, id, cx);
            let item = window.find(id).bounds();
            let middle = item.center().y;
            assert!(
                middle >= dark.top() && middle <= dark.bottom(),
                "{id} sits in the dark group: item={item:?} dark={dark:?}"
            );
        }
    })
    .unwrap();
}

#[gpui_kit::test]
fn theme_chevrons_scroll_only_the_visible_profile_grid(cx: &mut TestAppContext) {
    // Narrow window so the single visible grid still overflows horizontally.
    let (window, view) = open_desktop_sized(cx, 760., 900.);
    cx.update(|cx| cx.set_reduce_motion(true));
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.apply_theme("ciapre".into(), window, cx));
        window.click("open-settings", cx);
        window.render_frame(cx);
        window.click("separate-theme-profiles", cx);
        window.render_frame(cx);
        // Editing the dark profile: only the dark grid renders.
        assert!(window.try_find("theme-list-light").is_none());
        assert_eq!(window.find("theme-list-dark").label(), Some("Dark"));
        let first_dark = window.find("theme-ciapre").bounds();
        let dark_offset = view.read(cx).theme_scroll_dark.offset();
        let light_offset = view.read(cx).theme_scroll_light.offset();
        window.click("next-themes", cx);
        window.render_frame(cx);
        // One page is three strides: the fourth swatch lands where the
        // first sat (the paged-out first swatch unmounts).
        let entries = super::super::themes::picker_entries_for(ThemeMode::Dark);
        let third = format!("theme-{}", entries[3].id());
        assert_eq!(window.find(third).bounds().left(), first_dark.left());
        assert_ne!(view.read(cx).theme_scroll_dark.offset().x, dark_offset.x);
        assert_eq!(view.read(cx).theme_scroll_light.offset(), light_offset, "the hidden grid keeps its scroll offset");
        window.click("edit-light-profile", cx);
        window.render_frame(cx);
        assert!(window.try_find("theme-list-dark").is_none());
        assert_eq!(window.find("theme-list-light").label(), Some("Light"));
    })
    .unwrap();
}

/// The theme rows virtualize: only the visible window mounts, so the 60+
/// dark swatches never build on one frame. Deep swatches mount on demand.
#[gpui_kit::test]
fn theme_rows_mount_only_the_visible_window(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.click("open-settings", cx);
        window.render_frame(cx);
        assert!(window.try_find("theme-ciapre").is_some(), "the first dark swatch mounts");
        assert!(
            window.try_find("theme-catppuccin-mocha").is_none(),
            "deep swatches stay unmounted until scrolled into view"
        );
        scroll_theme_into_view(window, &view, "theme-catppuccin-mocha", cx);
        assert!(window.try_find("theme-catppuccin-mocha").is_some(), "scrolling mounts the deep swatch");
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
        scroll_theme_into_view(window, &view, "theme-ayu", cx);
        assert_eq!(window.find("theme-ayu").label(), Some("ayu · Ayu Dark"));
        scroll_theme_into_view(window, &view, "theme-ayu-dark", cx);
        assert_eq!(window.find("theme-ayu-dark").label(), Some("ayu-dark · Ayu Dark"));
        scroll_theme_into_view(window, &view, "theme-github-dark", cx);
        window.click("theme-github-dark", cx);
        window.render_frame(cx);
        scroll_theme_into_view(window, &view, "theme-ciapre", cx);
        assert_eq!(window.find("theme-ciapre").checked(), Some(false));
        scroll_theme_into_view(window, &view, "theme-github-dark", cx);
        assert_eq!(window.find("theme-github-dark").checked(), Some(true));
        {
            let view = view.read(cx);
            assert_eq!(view.theme_id, "github-dark");
            assert_eq!(view.theme_mode, ThemeMode::Dark);
        }
        scroll_theme_into_view(window, &view, "theme-solarized-light", cx);
        window.click("theme-solarized-light", cx);
        window.render_frame(cx);
        scroll_theme_into_view(window, &view, "theme-github-dark", cx);
        assert_eq!(window.find("theme-github-dark").checked(), Some(false));
        scroll_theme_into_view(window, &view, "theme-solarized-light", cx);
        assert_eq!(window.find("theme-solarized-light").checked(), Some(true));
        {
            let view = view.read(cx);
            assert_eq!(view.theme_id, "solarized-light");
            assert_eq!(view.theme_mode, ThemeMode::Light);
        }
    })
    .unwrap();
}
