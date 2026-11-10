use super::super::*;
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
            assert_eq!(colors.secondary_active, press, "{id}: Ghost selected/open uses the shared press fill");
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

/// Kit keeps primary button text identical across states, so the hover and
/// press fills must hold AA with the resting text in every palette, and the
/// rest -> hover -> press ramp must never run backwards. System themes keep
/// Kit's own base ramp, so only palette themes (the app-owned ramp) are in
/// scope here.
#[gpui_kit::test]
fn primary_state_ramps_hold_aa_and_run_rest_to_press(cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        for palette in all_themes() {
            let id = palette.id;
            let _mode = apply_by_id(id, None, cx);
            let colors = cx.theme().colors;
            // The collapse asserts below argue from resting AA; pin it here
            // so this test cannot mislead when rest itself breaks.
            let rest_ratio = contrast_ratio(colors.primary_foreground, colors.primary);
            assert!(rest_ratio >= 4.5, "{id}: primary text on rest is {rest_ratio:.2}");
            for (name, fill) in [
                ("hover", colors.primary_hover),
                ("press", colors.primary_active),
                ("button hover", colors.button_primary_hover),
                ("button press", colors.button_primary_active),
            ] {
                let ratio = contrast_ratio(colors.primary_foreground, fill);
                assert!(ratio >= 4.5, "{id}: primary text on {name} is {ratio:.2}");
            }
            assert!(
                contrast_ratio(colors.primary_active, colors.primary)
                    >= contrast_ratio(colors.primary_hover, colors.primary),
                "{id}: primary press is shallower than hover"
            );
            // Primary hover and press must read as state changes, not as the
            // resting accent under a different name. Rest holds AA (pinned
            // above), so step one always passes and the scan never collapses
            // onto the accent.
            assert_ne!(colors.primary_hover, colors.primary, "{id}: primary hover collapses onto the resting accent");
            assert_ne!(colors.primary_active, colors.primary, "{id}: primary press collapses onto the resting accent");
        }
    });
}

/// Mirror of `hover_takes_strongest_readable_step_in_every_theme` for the
/// accent ramp: neither state may leave a stronger readable step on the
/// table inside its own cap.
#[gpui_kit::test]
fn primary_states_take_strongest_readable_step_in_every_theme(cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        for palette in all_themes() {
            let id = palette.id;
            let _mode = apply_by_id(id, None, cx);
            let colors = cx.theme().colors;
            let toward = filled_button_foreground(colors.primary_foreground);
            for (name, fill, cap) in [
                ("hover", colors.primary_hover, PRIMARY_HOVER_MAX_STEP),
                ("press", colors.primary_active, PRIMARY_ACTIVE_MAX_STEP),
            ] {
                let depth = contrast_ratio(fill, colors.primary);
                for step in 1_u8..=cap {
                    let candidate = mix(colors.primary, toward, f32::from(step) / 100.0);
                    if contrast_ratio(colors.primary_foreground, candidate) >= 4.5 {
                        assert!(
                            depth + 0.001 >= contrast_ratio(candidate, colors.primary),
                            "{id}: primary {name} leaves visibility on the table at step {step}"
                        );
                    }
                }
            }
        }
    });
}

/// Roomy dark accent with light text: the scan takes the full cap step
/// toward the dark pole and keeps the accent's hue and saturation.
#[test]
fn primary_state_scan_takes_the_cap_step_with_headroom() {
    let accent = Hsla { h: 0.6, s: 0.5, l: 0.25, a: 1.0 };
    let white = Hsla { h: 0.0, s: 0.0, l: 1.0, a: 1.0 };
    let black = Hsla { h: 0.0, s: 0.0, l: 0.0, a: 1.0 };
    let expected = mix(accent, black, 0.30);
    assert!(contrast_ratio(white, expected) >= 4.5, "fixture must leave headroom");
    let fill = primary_state_surface(accent, black, white, 30);
    assert_eq!(fill, expected);
    assert_eq!(fill.h, accent.h, "the scan shifts lightness, not hue");
    assert_eq!(fill.s, accent.s, "the scan shifts lightness, not saturation");
}

/// Asymmetric mirror: roomy light accent with dark text scans toward the
/// light pole instead.
#[test]
fn primary_state_scan_mirrors_toward_the_light_pole() {
    let accent = Hsla { h: 0.1, s: 0.4, l: 0.75, a: 1.0 };
    let white = Hsla { h: 0.0, s: 0.0, l: 1.0, a: 1.0 };
    let black = Hsla { h: 0.0, s: 0.0, l: 0.0, a: 1.0 };
    let expected = mix(accent, white, 0.18);
    assert!(contrast_ratio(black, expected) >= 4.5, "fixture must leave headroom");
    assert_eq!(primary_state_surface(accent, white, black, 18), expected);
}

/// No readable step inside the cap: the state degrades to the accent itself
/// instead of shipping an unreadable fill.
#[test]
fn primary_state_scan_degrades_to_the_accent_without_headroom() {
    let accent = Hsla { h: 0.0, s: 0.0, l: 0.93, a: 1.0 };
    let white = Hsla { h: 0.0, s: 0.0, l: 1.0, a: 1.0 };
    let black = Hsla { h: 0.0, s: 0.0, l: 0.0, a: 1.0 };
    assert!(contrast_ratio(white, accent) < 4.5, "fixture must start unreadable");
    assert_eq!(primary_state_surface(accent, black, white, 30), accent);
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
