use super::*;
use gpui_kit::component::theme::ActiveTheme;

#[gpui_kit::test]
fn selected_controls_use_a_readable_stronger_surface(cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        for id in [SYSTEM_LIGHT_ID, SYSTEM_DARK_ID]
            .into_iter()
            .chain(all_themes().iter().map(|theme| theme.id))
        {
            let _mode = apply_by_id(id, None, cx);
            let colors = cx.theme().colors;
            assert!(contrast_ratio(colors.foreground, colors.button_active) >= 4.5, "{id}: selected label");
            assert!(
                contrast_ratio(colors.secondary, colors.button_active) + 0.001
                    >= contrast_ratio(colors.secondary, colors.popover),
                "{id}: selection must not be weaker than the raised surface"
            );
        }
    });
}

/// The subtle-border contract: opaque, at the hairline floor on the panel
/// seats it outlines most, and separated from every other fill it can sit on.
fn assert_subtle_surface_borders(id: &str, colors: &ThemeColor) {
    assert_eq!(colors.border.a, 1., "{id}: card and input borders must be opaque");
    assert_eq!(colors.sidebar_border, colors.border, "{id}");
    assert_eq!(colors.input, colors.border, "{id}");
    for (name, fill) in [("panel", colors.secondary), ("group box", colors.group_box)] {
        let ratio = contrast_ratio(colors.border, fill);
        assert!(ratio >= 1.5, "{id}: border on {name} is invisible ({ratio:.2})");
        assert!(ratio <= 2.5, "{id}: border on {name} is harsh, not subtle ({ratio:.2})");
    }
    for (name, fill) in [("canvas", colors.background), ("popover", colors.popover)] {
        let ratio = contrast_ratio(colors.border, fill);
        assert!(ratio >= 1.04, "{id}: border on {name} vanishes ({ratio:.2})");
    }
}

#[gpui_kit::test]
fn palettes_synchronize_component_and_base_tokens_with_readable_controls(cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        for palette in all_themes() {
            let _mode = apply_by_id(palette.id, None, cx);
            let kit = cx.global::<Theme>();
            let base = gpui_kit::base::Theme::global(cx);
            assert_eq!(
                kit.tokens.primary,
                gpui_kit::component::theme::ThemeToken::from(palette.accent),
                "{}",
                palette.id
            );
            assert_eq!(base.tokens.colors.background, palette.background, "{}", palette.id);
            assert_eq!(base.tokens.colors.primary, palette.accent, "{}", palette.id);
            assert_subtle_surface_borders(palette.id, &kit.colors);
            assert!(
                contrast_ratio(kit.colors.background, kit.colors.tab_bar_segmented) >= 1.04,
                "{}: selected segmented tabs must stand out from the track",
                palette.id
            );
            for foreground in [kit.colors.tab_foreground, kit.colors.tab_active_foreground] {
                assert!(contrast_ratio(foreground, palette.background) >= 4.5, "{}: unreadable tab", palette.id);
            }
            for background in [kit.colors.primary, kit.colors.primary_hover, kit.colors.primary_active] {
                assert!(
                    contrast_ratio(kit.colors.primary_foreground, background) >= 4.5,
                    "{}: unreadable primary control",
                    palette.id
                );
            }
            for (background, foreground) in [
                (kit.colors.secondary, kit.colors.secondary_foreground),
                (kit.colors.accent, kit.colors.accent_foreground),
                (kit.colors.group_box, kit.colors.group_box_foreground),
                (kit.colors.sidebar, kit.colors.sidebar_foreground),
                (kit.colors.popover, kit.colors.popover_foreground),
            ] {
                assert!(contrast_ratio(foreground, background) >= 4.5, "{}: unreadable component surface", palette.id);
            }
            for background in [
                kit.colors.background,
                kit.colors.secondary,
                kit.colors.popover,
                mix(kit.colors.background, kit.colors.secondary, 0.08),
            ] {
                assert!(
                    contrast_ratio(kit.colors.muted_foreground, background) >= 4.5,
                    "{}: muted text must stay readable on raised surfaces",
                    palette.id
                );
            }
            assert_ne!(kit.colors.accent, kit.colors.primary, "brand and hover surfaces must differ");
            assert_eq!(kit.colors.button_primary, kit.colors.primary);
            assert_eq!(kit.colors.button_primary_foreground, kit.colors.primary_foreground);
            assert_eq!(kit.colors.button_primary_hover, kit.colors.primary_hover);
            assert_eq!(kit.colors.button_primary_active, kit.colors.primary_active);
        }
    });
}

#[gpui_kit::test]
fn system_themes_show_subtle_surface_borders(cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        for id in [SYSTEM_LIGHT_ID, SYSTEM_DARK_ID] {
            let _mode = apply_by_id(id, None, cx);
            let colors = cx.theme().colors;
            assert_subtle_surface_borders(id, &colors);
            assert!(
                contrast_ratio(colors.background, colors.tab_bar_segmented) >= 1.04,
                "{id}: selected segmented tabs must stand out from the track"
            );
        }
    });
}

#[test]
fn every_theme_keeps_wcag_aa_foreground_contrast() {
    for theme in all_themes() {
        let ratio = contrast_ratio(theme.foreground, theme.background);
        assert!(ratio >= 4.5, "{}: foreground contrast {ratio:.2} is below WCAG AA", theme.id);
        let muted = aa_muted(theme.background, theme.foreground);
        let muted_ratio = contrast_ratio(muted, theme.background);
        assert!(muted_ratio >= 4.5, "{}: muted text contrast {muted_ratio:.2} is below WCAG AA", theme.id);
    }
}

#[gpui_kit::test]
fn every_theme_keeps_chart_series_distinguishable(cx: &mut gpui_kit::TestAppContext) {
    // Data colors are not held to the 4.5 text floor (labels and the legend
    // carry the same information), only to the shared vanish floor: a series
    // must neither sink into the canvas nor collapse into another series
    // the way an invisible border would.
    cx.update(|cx| {
        gpui_kit::init(cx);
        let mut collisions = Vec::new();
        for id in [SYSTEM_LIGHT_ID, SYSTEM_DARK_ID]
            .into_iter()
            .chain(all_themes().iter().map(|theme| theme.id))
        {
            let _mode = apply_by_id(id, None, cx);
            let colors = cx.theme().colors;
            let series = [colors.chart_1, colors.chart_2, colors.chart_3, colors.chart_4];
            for (index, color) in series.iter().enumerate() {
                let on_canvas = contrast_ratio(*color, colors.background);
                if on_canvas < CHART_DISTINGUISH_FLOOR {
                    collisions
                        .push(format!("{id}: chart series {} vanishes on the canvas ({on_canvas:.2})", index + 1));
                }
                for (other_index, other) in series.iter().enumerate().skip(index + 1) {
                    let pair = contrast_ratio(*color, *other);
                    if pair < CHART_DISTINGUISH_FLOOR {
                        collisions.push(format!(
                            "{id}: chart series {} and {} collide ({pair:.2})",
                            index + 1,
                            other_index + 1
                        ));
                    }
                }
            }
        }
        assert!(collisions.is_empty(), "chart series collisions:\n{}", collisions.join("\n"));
    });
}

#[test]
fn every_theme_stays_within_srgb_gamut() {
    for theme in all_themes() {
        for (name, color) in [
            ("background", theme.background),
            ("foreground", theme.foreground),
            ("accent", theme.accent),
            ("secondary", theme.secondary),
            ("success", theme.success),
            ("danger", theme.danger),
            ("warning", theme.warning),
        ] {
            let rgba = gpui_kit::Rgba::from(color);
            for (channel, value) in [("r", rgba.r), ("g", rgba.g), ("b", rgba.b)] {
                assert!(
                    (0.0..=1.0).contains(&value),
                    "{}: {name}.{channel}={value} is outside sRGB gamut; macOS clips it on Display P3 and the contrast math no longer holds",
                    theme.id
                );
            }
        }
    }
}

#[test]
fn faded_chrome_meets_non_text_contrast_on_canvas() {
    for theme in all_themes() {
        let faded = faded_chrome(theme.background, theme.foreground);
        let ratio = contrast_ratio(faded, theme.background);
        let muted = contrast_ratio(aa_muted(theme.background, theme.foreground), theme.background);
        assert!(ratio >= 3.0, "{}: faded chrome contrast {ratio:.2} is below the 3:1 non-text floor", theme.id);
        assert!(
            ratio < muted,
            "{}: faded chrome ({ratio:.2}) must stay fainter than muted text ({muted:.2})",
            theme.id
        );
    }
    let flat: Hsla = rgb(0x808080).into();
    assert_eq!(faded_chrome(flat, flat), flat, "no-contrast surfaces keep the foreground untouched");
}

#[gpui_kit::test]
fn focus_indicators_remain_visible_on_interactive_surfaces(cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        let ids = [SYSTEM_LIGHT_ID, SYSTEM_DARK_ID]
            .into_iter()
            .chain(all_themes().iter().map(|theme| theme.id));
        for id in ids {
            let _mode = apply_by_id(id, None, cx);
            let colors = cx.theme().colors;
            for (name, surface) in [
                ("canvas", colors.background),
                ("panel", colors.secondary),
                ("popover", colors.popover),
            ] {
                let ratio = contrast_ratio(colors.ring, surface);
                assert!(ratio >= 3.0, "{id}: focus indicator contrast on {name} is {ratio:.2}");
            }
        }
    });
}

#[gpui_kit::test]
fn caret_follows_theme_accent_with_foreground_fallback(cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        for palette in all_themes() {
            let _mode = apply_by_id(palette.id, None, cx);
            let colors = cx.theme().colors;
            let surfaces = [colors.background, colors.secondary, colors.popover];
            let accent_readable = surfaces.iter().all(|surface| contrast_ratio(palette.accent, *surface) >= 3.0);
            if accent_readable {
                assert_eq!(colors.caret, palette.accent, "{}: caret follows the theme accent", palette.id);
            } else {
                assert_eq!(
                    colors.caret, colors.foreground,
                    "{}: unreadable accent falls back to the foreground",
                    palette.id
                );
            }
            if palette.id == DEFAULT_THEME_ID {
                assert_eq!(colors.caret, palette.accent, "the default theme renders its caret in the brand accent");
            }
            for (name, surface) in [
                ("canvas", colors.background),
                ("panel", colors.secondary),
                ("popover", colors.popover),
            ] {
                let ratio = contrast_ratio(colors.caret, surface);
                assert!(ratio >= 3.0, "{}: caret contrast on {name} is {ratio:.2}", palette.id);
            }
        }
        for id in [SYSTEM_LIGHT_ID, SYSTEM_DARK_ID] {
            let _mode = apply_by_id(id, None, cx);
            let colors = cx.theme().colors;
            assert!(
                colors.caret == colors.primary || colors.caret == colors.foreground,
                "{id}: system caret derives from the primary accent or the foreground"
            );
            for (name, surface) in [
                ("canvas", colors.background),
                ("panel", colors.secondary),
                ("popover", colors.popover),
            ] {
                let ratio = contrast_ratio(colors.caret, surface);
                assert!(ratio >= 3.0, "{id}: caret contrast on {name} is {ratio:.2}",);
            }
        }
    });
}

#[gpui_kit::test]
fn caret_falls_back_to_foreground_for_unreadable_accents(cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        // Asymmetric fixtures: an accent identical to the canvas is unreadable
        // in both directions, so the caret must take the foreground instead.
        let black: Hsla = rgb(0x000000).into();
        let white: Hsla = rgb(0xFFFFFF).into();
        for (id, mode, background, foreground) in [
            ("synthetic-dark", ThemeMode::Dark, black, white),
            ("synthetic-light", ThemeMode::Light, white, black),
        ] {
            let theme = DesktopTheme {
                id,
                label: id,
                mode,
                background,
                foreground,
                accent: background,
                secondary: foreground,
                success: foreground,
                danger: foreground,
                warning: foreground,
            };
            let _mode = apply_palette(theme.seeds(), None, cx);
            let colors = cx.theme().colors;
            assert_eq!(colors.caret, colors.foreground, "{id}: unreadable accent falls back");
            assert_ne!(colors.caret, theme.accent, "{id}: fallback leaves the accent behind");
        }
    });
}

#[gpui_kit::test]
fn diff_colors_and_line_surfaces_are_readable_in_every_theme(cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        let ids = [SYSTEM_LIGHT_ID, SYSTEM_DARK_ID]
            .into_iter()
            .chain(all_themes().iter().map(|theme| theme.id));
        for id in ids {
            let _mode = apply_by_id(id, None, cx);
            let colors = cx.theme().colors;
            for (kind, status, status_foreground) in [
                ("addition", colors.success, colors.success_foreground),
                ("deletion", colors.danger, colors.danger_foreground),
            ] {
                let status_ratio = contrast_ratio(status, colors.background);
                assert!(status_ratio >= 4.5, "{id}: {kind} marker contrast {status_ratio:.2}");
                assert!(
                    contrast_ratio(status_foreground, status) >= 4.5,
                    "{id}: {kind} filled status contrast is below WCAG AA"
                );
                let diff_foreground =
                    diff_text_foreground(colors.background, colors.foreground, colors.success, colors.danger);
                let kit_line_background = composite_status_surface(colors.background, status, 0.12);
                let kit_text_ratio = contrast_ratio(diff_foreground, kit_line_background);
                assert!(kit_text_ratio >= 4.5, "{id}: {kind} Kit diff text contrast {kit_text_ratio:.2}");
                let kit_marker_ratio = contrast_ratio(status, kit_line_background);
                assert!(kit_marker_ratio >= 4.5, "{id}: {kind} Kit diff marker contrast {kit_marker_ratio:.2}");
                let line_background = diff_line_background(colors.background, diff_foreground, status);
                let line_ratio = contrast_ratio(diff_foreground, line_background);
                assert!(line_ratio >= 4.5, "{id}: {kind} diff text contrast {line_ratio:.2}");
                let marker_ratio = contrast_ratio(status, line_background);
                assert!(marker_ratio >= 4.5, "{id}: {kind} marker on diff surface contrast {marker_ratio:.2}");
            }
        }
    });
}

#[gpui_kit::test]
fn hover_surfaces_are_unified_and_contrast_safe_in_every_theme(cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        let ids = [SYSTEM_LIGHT_ID, SYSTEM_DARK_ID]
            .into_iter()
            .chain(all_themes().iter().map(|theme| theme.id));
        for id in ids {
            let _mode = apply_by_id(id, None, cx);
            let colors = cx.theme().colors;
            let hover = hover_surface(colors.background, colors.foreground);
            for (name, actual) in [
                ("accent", colors.accent),
                ("button", colors.button_hover),
                ("secondary button", colors.button_secondary_hover),
                ("secondary", colors.secondary_hover),
                ("list", colors.list_hover),
                ("sidebar", colors.sidebar_accent),
            ] {
                assert_eq!(actual, hover, "{id}: {name} hover uses the shared surface");
            }
            let text_ratio = contrast_ratio(colors.foreground, hover);
            assert!(text_ratio >= 4.5, "{id}: hover text contrast {text_ratio:.2}");
            assert!(
                contrast_ratio(colors.background, hover) >= 1.01,
                "{id}: hover surface is too close to the surrounding background"
            );
            assert_eq!(
                (hover.l - colors.background.l).signum(),
                (colors.foreground.l - colors.background.l).signum(),
                "{id}: hover should move gently toward the foreground"
            );
        }
    });
}

#[gpui_kit::test]
fn button_press_states_stay_visible_in_every_theme(cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        let ids = [SYSTEM_LIGHT_ID, SYSTEM_DARK_ID]
            .into_iter()
            .chain(all_themes().iter().map(|theme| theme.id));
        for id in ids {
            let _mode = apply_by_id(id, None, cx);
            let colors = cx.theme().colors;
            let press = active_surface(colors.background, colors.foreground);
            assert_eq!(colors.button_active, press, "{id}: Default press uses the shared press fill");
            assert_eq!(colors.button_secondary_active, press, "{id}: Secondary press uses the shared press fill");
            let text_ratio = contrast_ratio(colors.foreground, press);
            assert!(text_ratio >= 4.5, "{id}: press text contrast {text_ratio:.2}");
            assert_eq!(
                (press.l - colors.background.l).signum(),
                (colors.foreground.l - colors.background.l).signum(),
                "{id}: press should move gently toward the foreground"
            );
            let hover = hover_surface(colors.background, colors.foreground);
            assert!(
                contrast_ratio(press, colors.background) >= contrast_ratio(hover, colors.background),
                "{id}: press must read at least as deep as hover"
            );
            if hover != colors.button {
                assert_ne!(press, colors.button, "{id}: press is invisible where hover is visible");
            }
        }
    });
}

#[gpui_kit::test]
fn filled_surfaces_are_distinct_and_readable_in_every_theme(cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        let ids = [SYSTEM_LIGHT_ID, SYSTEM_DARK_ID]
            .into_iter()
            .chain(all_themes().iter().map(|theme| theme.id));
        for id in ids {
            let _mode = apply_by_id(id, None, cx);
            let colors = cx.theme().colors;
            for (name, surface, foreground) in [
                ("panel", colors.secondary, colors.secondary_foreground),
                ("group box", colors.group_box, colors.group_box_foreground),
                ("sidebar", colors.sidebar, colors.sidebar_foreground),
                ("popover and input", colors.popover, colors.popover_foreground),
            ] {
                assert!(
                    contrast_ratio(colors.background, surface) >= 1.04,
                    "{id}: {name} fill is too close to the canvas"
                );
                assert!(contrast_ratio(foreground, surface) >= 4.5, "{id}: {name} text contrast is below WCAG AA");
                assert!(
                    contrast_ratio(colors.muted_foreground, surface) >= 4.5,
                    "{id}: muted {name} text contrast is below WCAG AA"
                );
            }
            assert!(
                contrast_ratio(colors.muted_foreground, colors.background) >= 4.5,
                "{id}: muted text contrast is below WCAG AA on the canvas"
            );
            assert!(
                contrast_ratio(colors.secondary, colors.popover) >= 1.01,
                "{id}: input/popover fill should be distinct from panels"
            );
        }
    });
}

#[gpui_kit::test]
fn label_grade_text_stays_crisp_on_every_surface(cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        let ids = [SYSTEM_LIGHT_ID, SYSTEM_DARK_ID]
            .into_iter()
            .chain(all_themes().iter().map(|theme| theme.id));
        for id in ids {
            let _mode = apply_by_id(id, None, cx);
            let colors = cx.theme().colors;
            let sidebar = navigation_surfaces(colors.background, colors.secondary, colors.foreground).panel;
            // Label-grade text is full foreground on every fill: headings must
            // never render dimmer than the body copy they introduce.
            // Resting tab labels are AA-grade instead (see the tab-state test):
            // dimming them leaves room for a visible hover/selected shift.
            assert_eq!(
                label_foreground(&colors),
                colors.foreground,
                "{id}: section labels stay at full foreground (sidebar fill {sidebar:?})"
            );
        }
    });
}

#[gpui_kit::test]
fn segmented_tab_states_are_distinct_and_readable(cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        let ids = [SYSTEM_LIGHT_ID, SYSTEM_DARK_ID]
            .into_iter()
            .chain(all_themes().iter().map(|theme| theme.id));
        for id in ids {
            let _mode = apply_by_id(id, None, cx);
            let colors = cx.theme().colors;
            let track = colors.tab_bar_segmented;
            // The track keeps label text readable while standing clear of the
            // canvas pill, at maximal separation for its achieved readability:
            // no fill the foreground reads better on stands further out.
            let achieved = contrast_ratio(colors.foreground, track);
            assert!(achieved >= 4.5, "{id}: tab label on track is unreadable");
            for step in 4_u8..=30 {
                let candidate = mix(colors.background, colors.foreground, f32::from(step) / 100.0);
                if contrast_ratio(colors.foreground, candidate) >= achieved - 0.001
                    && contrast_ratio(colors.background, candidate) >= 1.04
                {
                    assert!(
                        contrast_ratio(colors.background, track)
                            >= contrast_ratio(colors.background, candidate) - 0.001,
                        "{id}: track leaves separation on the table at step {step}"
                    );
                }
            }
            // Rest labels sit on the track; the canvas and panel seats stay
            // readable for the other tab variants that share the token.
            for (name, surface) in [
                ("track", track),
                ("canvas", colors.background),
                ("panel", colors.secondary),
            ] {
                let ratio = contrast_ratio(colors.tab_foreground, surface);
                assert!(ratio >= 4.5, "{id}: resting tab label on {name} is {ratio:.2}");
            }
            // Hover and selected labels use the full foreground on the track
            // and on the canvas-colored pill respectively.
            assert_eq!(colors.tab_active_foreground, colors.foreground, "{id}");
            assert!(contrast_ratio(colors.foreground, track) >= 4.5, "{id}: hovered tab label is unreadable");
            // On the default theme the resting label is visibly dimmer than
            // the hover/selected label, so the highlight reads as a state.
            if id == DEFAULT_THEME_ID {
                let rest = contrast_ratio(colors.tab_foreground, track);
                let active = contrast_ratio(colors.tab_active_foreground, track);
                assert!(active > rest + 0.5, "{id}: hover shift is too subtle (rest {rest:.2}, active {active:.2})");
            }
        }
    });
}

#[gpui_kit::test]
fn folder_tab_states_join_canvas_content(cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        let ids = [SYSTEM_LIGHT_ID, SYSTEM_DARK_ID]
            .into_iter()
            .chain(all_themes().iter().map(|theme| theme.id));
        for id in ids {
            let _mode = apply_by_id(id, None, cx);
            let kit = cx.global::<Theme>();
            // The selected folder tab joins the canvas content below the bar.
            assert_eq!(kit.colors.tab_active, kit.colors.background, "{id}");
            assert_eq!(
                kit.tokens.tab_active,
                gpui_kit::component::theme::ThemeToken::from(kit.colors.background),
                "{id}: the selected folder fill must reconcile into the tokens Kit reads"
            );
            // Transparent band: the bar never stripes a non-canvas surface.
            assert_eq!(kit.colors.tab_bar.a, 0., "{id}");
            // Resting labels sit on canvas (thread strip, inspector) and on
            // popover (attachment picker).
            for (name, surface) in [("canvas", kit.colors.background), ("popover", kit.colors.popover)] {
                let ratio = contrast_ratio(kit.colors.tab_foreground, surface);
                assert!(ratio >= 4.5, "{id}: resting folder label on {name} is {ratio:.2}");
            }
        }
    });
}

#[gpui_kit::test]
fn hairlines_stay_visible_on_every_fill(cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        let ids = [SYSTEM_LIGHT_ID, SYSTEM_DARK_ID]
            .into_iter()
            .chain(all_themes().iter().map(|theme| theme.id));
        for id in ids {
            let _mode = apply_by_id(id, None, cx);
            let colors = cx.theme().colors;
            for (name, fill) in [
                ("canvas", colors.background),
                ("panel", colors.secondary),
                ("group box", colors.group_box),
                ("popover", colors.popover),
            ] {
                let divider = hairline(fill, colors.foreground);
                assert_ne!(divider, fill, "{id}: divider vanishes on {name}");
                let ratio = contrast_ratio(divider, fill);
                assert!(ratio >= 1.5, "{id}: divider on {name} is invisible ({ratio:.2})");
            }
        }
    });
}

#[gpui_kit::test]
fn navigation_surfaces_are_distinct_and_readable(cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        let ids = [SYSTEM_LIGHT_ID, SYSTEM_DARK_ID]
            .into_iter()
            .chain(all_themes().iter().map(|theme| theme.id));
        for id in ids {
            let _mode = apply_by_id(id, None, cx);
            let colors = cx.theme().colors;
            let surfaces = navigation_surfaces(colors.background, colors.secondary, colors.foreground);
            assert!(
                contrast_ratio(colors.background, surfaces.rail) >= 1.02,
                "{id}: shell must contrast with the workspace top and left edges"
            );
            assert!(
                contrast_ratio(colors.background, surfaces.panel) >= 1.02,
                "{id}: sidebar must contrast with its surrounding workspace"
            );
            assert!(
                contrast_ratio(surfaces.panel, surfaces.rail) >= 1.02,
                "{id}: sidebar must remain distinct from the navigation rail"
            );
            assert!(contrast_ratio(colors.foreground, surfaces.panel) >= 4.5, "{id}: sidebar text must meet WCAG AA");
            assert!(contrast_ratio(colors.foreground, surfaces.rail) >= 4.5, "{id}: shell text must meet WCAG AA");
        }
    });
}

#[gpui_kit::test]
fn translucent_sidebar_preserves_contrast_over_light_and_dark_backdrops(cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        let mut translucent_count = 0;
        for id in [SYSTEM_LIGHT_ID, SYSTEM_DARK_ID]
            .into_iter()
            .chain(all_themes().iter().map(|theme| theme.id))
        {
            let _mode = apply_by_id(id, None, cx);
            let colors = cx.theme().colors;
            let surface = translucent_sidebar_surface(&colors);
            if id == DEFAULT_THEME_ID {
                assert!(surface.a < 1.0, "default theme must show the blurred backdrop");
            }
            if surface.a >= 1.0 {
                continue;
            }
            translucent_count += 1;
            let rgba = gpui_kit::Rgba::from(surface);
            for backdrop in [0.0, 1.0] {
                let composite = Hsla::from(gpui_kit::Rgba {
                    r: rgba.r * rgba.a + backdrop * (1.0 - rgba.a),
                    g: rgba.g * rgba.a + backdrop * (1.0 - rgba.a),
                    b: rgba.b * rgba.a + backdrop * (1.0 - rgba.a),
                    a: 1.0,
                });
                assert!(contrast_ratio(colors.foreground, composite) >= 4.5, "{id}: foreground");
                assert!(contrast_ratio(colors.muted_foreground, composite) >= 4.5, "{id}: muted");
                assert!(contrast_ratio(label_foreground(&colors), composite) >= 4.5, "{id}: label");
                assert!(contrast_ratio(colors.primary, composite) >= 3.0, "{id}: icon");
            }
        }
        assert!(translucent_count > 0, "at least one theme must permit translucency");
    });
}

#[test]
fn accent_badges_stay_readable() {
    for theme in all_themes() {
        let text = filled_button_foreground(theme.accent);
        let ratio = contrast_ratio(text, theme.accent);
        assert!(ratio >= 3.0, "{}: accent badge contrast {ratio:.2} is too low", theme.id);
    }
}

#[test]
fn permission_fallback_seeds_stay_distinct_and_readable() {
    let (ask_seed, auto_seed) = distinct_permission_seeds();
    assert_ne!(ask_seed, auto_seed, "Ask and Auto-approve fallbacks must differ");
    for theme in all_themes() {
        let resting = filled_surface(theme.background, theme.foreground, RESTING_FILL_MAX_STEP, None);
        let raised = filled_surface(theme.background, theme.foreground, RAISED_FILL_MAX_STEP, Some(resting));
        for (background, seat) in [(theme.background, "canvas"), (raised, "raised popover")] {
            let ask = readable_indicator_color(background, theme.foreground, ask_seed);
            let auto = readable_indicator_color(background, theme.foreground, auto_seed);
            for (name, color) in [("Ask", ask), ("Auto-approve", auto)] {
                let ratio = contrast_ratio(color, background);
                assert!(ratio >= 4.5, "{}: {name} fallback on {seat} is {ratio:.2}", theme.id);
            }
        }
    }
}

#[test]
fn theme_ids_are_unique_and_default_exists() {
    let mut ids: Vec<&str> = all_themes().iter().map(|theme| theme.id).collect();
    let count = ids.len();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), count, "theme ids must be unique");
    assert!(find(DEFAULT_THEME_ID).is_some(), "the default theme must exist");
    assert!(find(SYSTEM_LIGHT_ID).is_none() && find(SYSTEM_DARK_ID).is_none());
}

#[test]
fn theme_modes_have_valid_defaults_and_system_ids() {
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        assert_eq!(mode_for_id(default_id(mode)), Some(mode));
        assert_eq!(mode_for_id(system_id(mode)), Some(mode));
    }
    assert_eq!(mode_for_id("solarized-light"), Some(ThemeMode::Light));
    assert_eq!(mode_for_id("ciapre"), Some(ThemeMode::Dark));
    assert_eq!(mode_for_id("invalid-theme"), None);
}

#[test]
fn cli_sync_ids_cover_sentinels_and_reject_unknown_ids() {
    assert_eq!(cli_id_for(SYSTEM_LIGHT_ID), Some(default_id(ThemeMode::Light)));
    assert_eq!(cli_id_for(SYSTEM_DARK_ID), Some(default_id(ThemeMode::Dark)));
    assert_ne!(cli_id_for(SYSTEM_LIGHT_ID), cli_id_for(SYSTEM_DARK_ID));
    assert_eq!(cli_id_for("ciapre-blue"), Some("ciapre-blue"));
    assert_eq!(cli_id_for("invalid-theme"), None);
}

#[test]
fn every_cli_theme_is_offered_in_the_desktop_picker() {
    let desktop_ids: Vec<&str> = all_themes().iter().map(|theme| theme.id).collect();
    for id in theme::available_themes() {
        let found = find(id).unwrap_or_else(|| panic!("{id} missing from the desktop picker"));
        assert_eq!(matches!(found.mode, ThemeMode::Light), theme::is_light_theme(id), "{id} mode");
        assert!(desktop_ids.contains(&id));
    }
    assert_eq!(desktop_ids.len(), theme::available_themes().len());
}

#[test]
fn ported_rows_use_the_shared_palette_colors() {
    for id in theme::available_themes() {
        let colors = theme::builtin_theme_colors(id).expect("registered theme palette");
        let row = find(id).unwrap_or_else(|| panic!("{id} missing"));
        let background = rgb(theme::rgb_u32(colors.background)).into();
        let foreground = rgb(theme::rgb_u32(colors.foreground)).into();
        for (field, expected, actual) in [
            ("background", colors.background, row.background),
            ("foreground", colors.foreground, row.foreground),
            ("accent", colors.accent, row.accent),
            ("secondary", colors.secondary, row.secondary),
        ] {
            assert_eq!(rgb_hex(actual), theme::rgb_u32(expected), "{id}: {field} must match the shared palette");
        }
        assert_eq!(
            row.success,
            readable_semantic_color(background, foreground, rgb(theme::rgb_u32(colors.success)).into()),
            "{id}: addition color must use the shared semantic color at readable contrast"
        );
        assert_eq!(
            row.danger,
            readable_semantic_color(background, foreground, rgb(theme::rgb_u32(colors.danger)).into()),
            "{id}: deletion color must use the shared semantic color at readable contrast"
        );
        assert_eq!(
            row.warning,
            readable_semantic_color(background, foreground, rgb(theme::rgb_u32(colors.warning)).into()),
            "{id}: warning color must use the shared semantic color at readable contrast"
        );
        assert_eq!(row.label, theme::theme_label(id).unwrap_or(id), "{id} label");
    }
}
#[gpui_kit::test]
fn notice_severity_tones_hold_aa_on_canvas(cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        let ids = [SYSTEM_LIGHT_ID, SYSTEM_DARK_ID]
            .into_iter()
            .chain(all_themes().iter().map(|theme| theme.id));
        for id in ids {
            let _mode = apply_by_id(id, None, cx);
            let colors = cx.theme().colors;
            for (name, tone) in [("error", colors.danger), ("warning", colors.warning)] {
                let ratio = contrast_ratio(tone, colors.background);
                assert!(ratio >= 4.5, "{id}: notice {name} title contrast is {ratio:.2}");
            }
        }
    });
}

/// Convert a GPUI color back to `0xRRGGBB` for palette comparisons.
///
/// Rows are stored from 8-bit channels, so each component round-trips
/// through the same 0..=255 scale; the float clamp keeps the cast exact.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to 0..=255"
)]
fn rgb_hex(color: Hsla) -> u32 {
    let rgba = gpui_kit::Rgba::from(color);
    let channel = |value: f32| u32::from((value.clamp(0.0, 1.0) * 255.0).round() as u8);
    (channel(rgba.r) << 16) | (channel(rgba.g) << 8) | channel(rgba.b)
}

#[test]
fn contrast_blend_covers_fallback_and_blend_arms_in_both_directions() {
    let black = Hsla { h: 0.0, s: 0.0, l: 0.0, a: 1.0 };
    let white = Hsla { h: 0.0, s: 0.0, l: 1.0, a: 1.0 };
    // Below the 5.0 crispness floor the foreground passes through
    // untouched — including the AA band between 4.5 and 5.0, which is
    // readable but cannot fund a dimmer muted tone.
    let faint = Hsla { h: 0.0, s: 0.0, l: 0.08, a: 1.0 };
    assert!(contrast_ratio(faint, black) < 4.5);
    assert_eq!(aa_muted(black, faint), faint);
    let aa_only = Hsla { h: 0.0, s: 0.0, l: 0.47, a: 1.0 };
    let aa_ratio = contrast_ratio(aa_only, black);
    assert!((4.5..5.0).contains(&aa_ratio), "fixture must sit in the AA-only band ({aa_ratio:.2})");
    assert_eq!(aa_muted(black, aa_only), aa_only);
    // Above it both theme directions blend toward the background while
    // holding the threshold.
    for (background, foreground) in [(black, white), (white, black)] {
        let muted = aa_muted(background, foreground);
        assert_ne!(muted, foreground);
        assert!(contrast_ratio(muted, background) >= 5.0, "muted text misses the crispness floor");
        assert!(
            (muted.l - background.l).abs() < (foreground.l - background.l).abs(),
            "blend must move toward the background"
        );
    }
}

#[gpui_kit::test]
fn code_token_colors_follow_the_kit_mode_default(cx: &mut gpui_kit::TestAppContext) {
    use gpui_kit::component::highlighter::HighlightTheme;
    cx.update(|cx| {
        gpui_kit::init(cx);
        // Kit 0.8.0 exposes no constructor for per-palette syntax tokens
        // (`ThemeStyle` fields are private), so every palette of a mode shares
        // the Kit default highlight theme. Code-block *surfaces* still follow
        // the palette through `theme.muted`.
        for palette in all_themes() {
            let _mode = apply_by_id(palette.id, None, cx);
            let highlight = &cx.global::<Theme>().highlight_theme;
            let expected = match palette.mode {
                ThemeMode::Light => HighlightTheme::default_light(),
                ThemeMode::Dark => HighlightTheme::default_dark(),
            };
            assert_eq!(**highlight, *expected, "{}: syntax tokens use the Kit mode default", palette.id);
        }
    });
}

#[test]
fn contrast_ratio_matches_known_values() {
    let black = Hsla { h: 0.0, s: 0.0, l: 0.0, a: 1.0 };
    let white = Hsla { h: 0.0, s: 0.0, l: 1.0, a: 1.0 };
    assert!((contrast_ratio(black, white) - 21.0).abs() < 0.1);
    assert!((contrast_ratio(black, black) - 1.0).abs() < 0.01);
}

#[gpui_kit::test]
fn fills_take_weakest_separating_step_in_every_theme(cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        let ids = [SYSTEM_LIGHT_ID, SYSTEM_DARK_ID]
            .into_iter()
            .chain(all_themes().iter().map(|theme| theme.id));
        for id in ids {
            let _mode = apply_by_id(id, None, cx);
            let colors = cx.theme().colors;
            // No weaker step meets the floors: fills minimize within the
            // readability + separation contract (mirror of the
            // maximal-separation pin on the segmented track).
            let resting = contrast_ratio(colors.background, colors.secondary);
            for step in 4_u8..=RESTING_FILL_MAX_STEP {
                let candidate = mix(colors.background, colors.foreground, f32::from(step) / 100.0);
                if contrast_ratio(colors.foreground, candidate) >= 4.5
                    && contrast_ratio(colors.background, candidate) >= 1.04
                {
                    assert!(
                        resting <= contrast_ratio(colors.background, candidate) + 0.001,
                        "{id}: resting fill leaves subtlety on the table at step {step}"
                    );
                }
            }
            // Raised fills additionally keep the layer gap above resting.
            let raised = contrast_ratio(colors.background, colors.popover);
            for step in 4_u8..=RAISED_FILL_MAX_STEP {
                let candidate = mix(colors.background, colors.foreground, f32::from(step) / 100.0);
                if contrast_ratio(colors.foreground, candidate) >= 4.5
                    && contrast_ratio(colors.background, candidate) >= 1.04
                    && contrast_ratio(colors.secondary, candidate) >= 1.02
                {
                    assert!(
                        raised <= contrast_ratio(colors.background, candidate) + 0.001,
                        "{id}: raised fill leaves subtlety on the table at step {step}"
                    );
                }
            }
        }
    });
}

#[gpui_kit::test]
fn hover_takes_strongest_readable_step_in_every_theme(cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        let ids = [SYSTEM_LIGHT_ID, SYSTEM_DARK_ID]
            .into_iter()
            .chain(all_themes().iter().map(|theme| theme.id));
        for id in ids {
            let _mode = apply_by_id(id, None, cx);
            let colors = cx.theme().colors;
            // Unlike resting fills, a state change must stay visible: no
            // stronger step keeps text readable, and hover never collapses
            // onto the resting panel fill it highlights.
            assert_ne!(colors.accent, colors.secondary, "{id}: hover is pixel-identical to the panel it highlights");
            let hover = contrast_ratio(colors.background, colors.accent);
            for step in 4_u8..=HOVER_FILL_MAX_STEP {
                let candidate = mix(colors.background, colors.foreground, f32::from(step) / 100.0);
                if contrast_ratio(colors.foreground, candidate) >= 4.5 {
                    assert!(
                        hover + 0.001 >= contrast_ratio(colors.background, candidate),
                        "{id}: hover leaves visibility on the table at step {step}"
                    );
                }
            }
        }
    });
}

#[test]
fn picker_swatches_group_every_entry_by_mode() {
    // The oracle pins picker-side grouping against the CLI registry flags
    // without reusing SwatchEntry::mode: system sentinels map explicitly and
    // palettes follow vtcode-ui's own light/dark flag (whose correctness is
    // covered by vtcode-ui's own scheme tests, not here).
    fn expected_mode(id: &str) -> ThemeMode {
        if id == SYSTEM_LIGHT_ID {
            ThemeMode::Light
        } else if id == SYSTEM_DARK_ID {
            ThemeMode::Dark
        } else if theme::is_light_theme(id) {
            ThemeMode::Light
        } else {
            ThemeMode::Dark
        }
    }

    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        let entries: Vec<_> = picker_swatches_for(mode).collect();
        assert!(!entries.is_empty(), "{mode:?} group must not be empty");
        for entry in &entries {
            assert_eq!(entry.mode(), mode, "{}", entry.id());
            assert_eq!(expected_mode(&entry.id()), mode, "{} disagrees with the CLI registry", entry.id());
        }
    }

    // Featured VT Code themes keep their headline slot inside their own mode
    // group: all three are dark, so the light group starts at the system base.
    let light_ids: Vec<_> = picker_swatches_for(ThemeMode::Light).map(|entry| entry.id()).collect();
    let dark_ids: Vec<_> = picker_swatches_for(ThemeMode::Dark).map(|entry| entry.id()).collect();
    assert_eq!(light_ids.first().map(String::as_str), Some(SYSTEM_LIGHT_ID));
    let dark_head: Vec<&str> = dark_ids.iter().take(4).map(String::as_str).collect();
    assert_eq!(dark_head, [DEFAULT_THEME_ID, "ciapre-dark", "ciapre-blue", SYSTEM_DARK_ID]);

    // Combined groups cover every selectable entry exactly once: no drops,
    // no duplicates across the mode boundary.
    let mut expected: Vec<String> = theme::available_themes()
        .into_iter()
        .filter(|id| theme::builtin_theme_colors(id).is_some())
        .map(str::to_string)
        .chain([SYSTEM_LIGHT_ID.to_string(), SYSTEM_DARK_ID.to_string()])
        .collect();
    expected.sort();
    let mut actual: Vec<String> = picker_swatches().map(|entry| entry.id()).collect();
    actual.sort();
    assert_eq!(actual, expected);

    // Grouped order: every light swatch precedes every dark swatch.
    let modes: Vec<ThemeMode> = picker_swatches().map(|entry| entry.mode()).collect();
    let first_dark = modes
        .iter()
        .position(|mode| *mode == ThemeMode::Dark)
        .expect("dark themes exist");
    assert!(modes[..first_dark].iter().all(|mode| *mode == ThemeMode::Light));
    assert!(modes[first_dark..].iter().all(|mode| *mode == ThemeMode::Dark));
}
