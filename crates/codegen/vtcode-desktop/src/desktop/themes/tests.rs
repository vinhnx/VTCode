use super::*;
use gpui_kit::component::theme::ActiveTheme;

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
            assert_eq!(kit.colors.border.a, 0.);
            assert_eq!(kit.colors.sidebar_border, kit.colors.border, "{}", palette.id);
            assert_eq!(kit.colors.input.a, 0.);
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
fn system_themes_hide_decorative_surface_borders(cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        for id in [SYSTEM_LIGHT_ID, SYSTEM_DARK_ID] {
            let _mode = apply_by_id(id, None, cx);
            let colors = cx.theme().colors;
            assert_eq!(colors.border.a, 0., "{id}");
            assert_eq!(colors.sidebar_border, colors.border, "{id}");
            assert_eq!(colors.input.a, 0., "{id}");
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
            // Label-grade text targets 7:1, falling back to the palette
            // foreground itself when a theme has no headroom above it.
            // Resting tab labels are AA-grade instead (see the tab-state test):
            // dimming them leaves room for a visible hover/selected shift.
            for (name, surface) in [
                ("canvas", colors.background),
                ("panel", colors.secondary),
                ("popover", colors.popover),
                ("sidebar", sidebar),
            ] {
                let foreground_ratio = contrast_ratio(colors.foreground, surface);
                let ratio = contrast_ratio(label_foreground(&colors), surface);
                assert!(
                    ratio >= LABEL_CONTRAST || ratio >= foreground_ratio - 0.01,
                    "{id}: section label contrast on {name} is {ratio:.2} (foreground {foreground_ratio:.2})"
                );
            }
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
fn secondary_copy_stays_brighter_than_muted_floor(cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        let ids = [SYSTEM_LIGHT_ID, SYSTEM_DARK_ID]
            .into_iter()
            .chain(all_themes().iter().map(|theme| theme.id));
        for id in ids {
            let _mode = apply_by_id(id, None, cx);
            let colors = cx.theme().colors;
            let secondary = muted_text(&colors);
            for (name, surface) in [
                ("canvas", colors.background),
                ("panel", colors.secondary),
                ("group box", colors.group_box),
                ("popover", colors.popover),
            ] {
                let ratio = contrast_ratio(secondary, surface);
                assert!(ratio >= 4.5, "{id}: secondary copy on {name} is {ratio:.2}");
                let muted_ratio = contrast_ratio(colors.muted_foreground, surface);
                assert!(
                    ratio >= muted_ratio - 0.01,
                    "{id}: secondary copy ({ratio:.2}) is dimmer than muted ({muted_ratio:.2}) on {name}"
                );
            }
            let headroom = contrast_ratio(colors.foreground, colors.popover);
            let popover_ratio = contrast_ratio(secondary, colors.popover);
            assert!(
                popover_ratio >= 6.0_f32.min(headroom) - 0.05,
                "{id}: secondary copy misses its 6:1 target ({popover_ratio:.2}, headroom {headroom:.2})"
            );
        }
    });
}

#[test]
fn secondary_copy_degrades_gracefully_without_headroom() {
    // Asymmetric fixtures: a headroom-rich dark palette, a light
    // palette (opposite blend direction), and the measured
    // lowest-headroom palette. Each must sit at min(6:1, headroom):
    // the first two reach the brighter target, the last one holds
    // the highest ratio its palette allows instead of dropping to AA.
    let lowest = all_themes()
        .iter()
        .min_by(|a, b| {
            contrast_ratio(a.foreground, a.background)
                .partial_cmp(&contrast_ratio(b.foreground, b.background))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .expect("theme registry is non-empty");
    for id in ["ciapre", "catppuccin-latte", lowest.id] {
        let theme = find(id).unwrap_or_else(|| panic!("{id} missing"));
        let headroom = contrast_ratio(theme.foreground, theme.background);
        let muted = text_at_contrast(theme.background, theme.foreground, 6.0_f32.min(headroom));
        let ratio = contrast_ratio(muted, theme.background);
        assert!(ratio >= 4.5, "{id}: unreadable secondary copy ({ratio:.2})");
        assert!(
            ratio >= 6.0_f32.min(headroom) - 0.05,
            "{id}: missed min(6:1, headroom {headroom:.2}) with {ratio:.2}"
        );
        // The blend moves toward the background in both directions.
        assert!(
            (muted.l - theme.background.l).abs() <= (theme.foreground.l - theme.background.l).abs(),
            "{id}: blend moved away from the background"
        );
    }
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

#[test]
fn accent_badges_stay_readable() {
    for theme in all_themes() {
        let text = filled_button_foreground(theme.accent);
        let ratio = contrast_ratio(text, theme.accent);
        assert!(ratio >= 3.0, "{}: accent badge contrast {ratio:.2} is too low", theme.id);
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
    // Below the threshold the foreground passes through untouched.
    let faint = Hsla { h: 0.0, s: 0.0, l: 0.08, a: 1.0 };
    assert!(contrast_ratio(faint, black) < 4.5);
    assert_eq!(aa_muted(black, faint), faint);
    // Above it both theme directions blend toward the background while
    // holding the threshold.
    for (background, foreground) in [(black, white), (white, black)] {
        let muted = aa_muted(background, foreground);
        assert_ne!(muted, foreground);
        assert!(contrast_ratio(muted, background) >= 4.5, "unreadable muted text");
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
