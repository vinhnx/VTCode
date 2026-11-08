use super::*;
use gpui_kit::component::theme::ActiveTheme;

#[gpui_kit::test]
fn contrast_setting_increases_foreground_contrast_for_every_theme(cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        for id in [SYSTEM_LIGHT_ID, SYSTEM_DARK_ID]
            .into_iter()
            .chain(all_themes().iter().map(|theme| theme.id))
        {
            let mut previous = 0.0;
            for amount in [0, 40, 100] {
                let _mode = apply_with_contrast(id, amount, None, cx);
                let colors = cx.theme().colors;
                let ratio = contrast_ratio(colors.foreground, colors.background);
                assert!(ratio >= 4.5, "{id} at {amount}: {ratio}");
                assert!(ratio + 0.001 >= previous, "{id} at {amount}: {ratio} < {previous}");
                for surface in [colors.secondary, colors.popover, colors.tab_bar_segmented] {
                    assert!(contrast_ratio(colors.foreground, surface) >= 4.5, "{id} at {amount}: unreadable surface");
                }
                previous = ratio;
            }
        }
    });
}

/// Custom themes ride the same seed derivation as built-in rows, so their
/// derived tokens must clear the same floors even when a saved seed was hand
/// edited into an unreadable value.
#[gpui_kit::test]
fn custom_theme_seeds_hold_the_same_contrast_floors(cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        // Asymmetric fixtures: a readable dark pair with hostile accent and
        // semantic seeds, and a readable light pair whose accent is nearly the
        // background. Both must self-correct at apply time.
        let dark = CustomTheme {
            id: "custom-dark".to_string(),
            name: "Synthetic Dark".to_string(),
            mode: ThemeMode::Dark,
            background: rgb(0x101010).into(),
            foreground: rgb(0xf0f0f0).into(),
            accent: rgb(0x111111).into(),
            secondary: rgb(0x121212).into(),
            success: rgb(0x131313).into(),
            danger: rgb(0x141414).into(),
            warning: rgb(0x151515).into(),
        };
        let light = CustomTheme {
            id: "custom-light".to_string(),
            name: "Synthetic Light".to_string(),
            mode: ThemeMode::Light,
            background: rgb(0xfafafa).into(),
            foreground: rgb(0x101010).into(),
            accent: rgb(0xf9f9f9).into(),
            secondary: rgb(0xf8f8f8).into(),
            success: rgb(0xf7f7f7).into(),
            danger: rgb(0xf6f6f6).into(),
            warning: rgb(0xf5f5f5).into(),
        };
        for custom in [dark, light] {
            let customs = [custom.clone()];
            let _mode = apply_selection(&custom.id, &customs, None, cx);
            let colors = cx.theme().colors;
            let canvas = contrast_ratio(colors.foreground, colors.background);
            assert!(canvas >= 4.5, "{}: canvas text is {canvas:.2}", custom.id);
            for (name, surface) in [
                ("panel", colors.secondary),
                ("group box", colors.group_box),
                ("popover", colors.popover),
            ] {
                let ratio = contrast_ratio(colors.foreground, surface);
                assert!(ratio >= 4.5, "{}: text on {name} is {ratio:.2}", custom.id);
            }
            for (name, status) in [
                ("accent", colors.primary),
                ("success", colors.success),
                ("danger", colors.danger),
                ("warning", colors.warning),
            ] {
                let ratio = contrast_ratio(status, colors.background);
                assert!(ratio >= 4.5, "{}: {name} is {ratio:.2}", custom.id);
            }
            // Focus visibility is checked separately at 3:1.
            for surface in [colors.background, colors.secondary, colors.popover] {
                let ratio = contrast_ratio(colors.ring, surface);
                assert!(ratio >= 3.0, "{}: focus ring is {ratio:.2} against {surface:?}", custom.id);
            }
        }
    });
}

#[gpui_kit::test]
fn profile_captions_hold_muted_contrast_under_contrast_boost(cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        for id in [SYSTEM_LIGHT_ID, SYSTEM_DARK_ID]
            .into_iter()
            .chain(all_themes().iter().map(|theme| theme.id))
        {
            for amount in [0, 40, 100] {
                let _mode = apply_with_contrast(id, amount, None, cx);
                let colors = cx.theme().colors;
                // The Light/Dark profile caption paints muted_foreground on
                // the group-box seat (inactive swatch, transparent ghost
                // button) and the popover seat (active swatch highlight):
                // same floor-or-ceiling contract as
                // muted_text_holds_crispness_floor_or_palette_ceiling, held
                // across the whole contrast range.
                for (name, surface) in [("group box", colors.group_box), ("popover", colors.popover)] {
                    let muted = contrast_ratio(colors.muted_foreground, surface);
                    let ceiling = contrast_ratio(colors.foreground, surface).min(5.0);
                    assert!(
                        muted + 0.01 >= ceiling,
                        "{id} at {amount}: caption on {name} is {muted:.2}, ceiling {ceiling:.2}"
                    );
                }
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
fn flagship_surfaces_and_muted_text_read_crisp_not_grey(cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        assert_eq!(DEFAULT_THEME_ID, "ciapre", "Ciapre is the flagship default");
        let _mode = apply_by_id(DEFAULT_THEME_ID, None, cx);
        let colors = cx.theme().colors;
        // Layers stack in a strict order above the canvas (direction-free:
        // contrast ratios, not signed lightness lifts, so the pin survives
        // palettes with different spans and blend directions).
        let panel = contrast_ratio(colors.secondary, colors.background);
        let popover = contrast_ratio(colors.popover, colors.background);
        let hover = contrast_ratio(colors.accent, colors.background);
        assert!(popover > panel, "popover must sit above panels ({popover:.3} vs {panel:.3})");
        assert!(hover >= panel, "hover must stay at least as raised as panels ({hover:.3} vs {panel:.3})");
        // Muted text clears a crispness floor above WCAG AA on every seat.
        for (name, surface) in [
            ("canvas", colors.background),
            ("panel", colors.secondary),
            ("popover", colors.popover),
        ] {
            let ratio = contrast_ratio(colors.muted_foreground, surface);
            assert!(ratio >= 5.0, "default muted text on {name} is washed ({ratio:.2})");
        }
    });
}

#[gpui_kit::test]
fn foreground_reads_on_accent_hover_for_every_theme(cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        let ids = [SYSTEM_LIGHT_ID, SYSTEM_DARK_ID]
            .into_iter()
            .chain(all_themes().iter().map(|theme| theme.id));
        // Hover rows (effort options, Kit menus) pair foreground text with
        // the accent fill: the pair must clear AA in every palette.
        for id in ids {
            let _mode = apply_by_id(id, None, cx);
            let colors = cx.theme().colors;
            let ratio = contrast_ratio(colors.foreground, colors.accent);
            assert!(ratio >= 4.5, "{id}: foreground on accent hover is {ratio:.2}");
        }
    });
}

#[gpui_kit::test]
fn muted_text_holds_crispness_floor_or_palette_ceiling(cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        let ids = [SYSTEM_LIGHT_ID, SYSTEM_DARK_ID]
            .into_iter()
            .chain(all_themes().iter().map(|theme| theme.id));
        for id in ids {
            let _mode = apply_by_id(id, None, cx);
            let colors = cx.theme().colors;
            // Muted text reaches the 5.0 crispness floor unless the palette's
            // own foreground is dimmer — then it holds that ceiling instead of
            // inventing contrast the palette cannot support.
            for (name, surface) in [
                ("canvas", colors.background),
                ("panel", colors.secondary),
                ("group box", colors.group_box),
                ("popover", colors.popover),
            ] {
                let muted = contrast_ratio(colors.muted_foreground, surface);
                let ceiling = contrast_ratio(colors.foreground, surface).min(5.0);
                assert!(muted + 0.01 >= ceiling, "{id}: muted text on {name} is {muted:.2}, ceiling {ceiling:.2}");
            }
        }
    });
}

/// The shimmer contract: the resting glyph tone is dimmer than the label tone
/// it replaces, holds its floor on *every seat these labels use* (not just the
/// canvas), and the sweep always paints brighter than the text it crosses.
///
/// The per-seat loop is the assertion that matters: anchoring the derivation to
/// the canvas passes a canvas-only check while landing near 2.7:1 on the
/// popover and panel seats where these labels actually sit.
#[gpui_kit::test]
fn shimmer_resting_text_dims_while_the_sweep_stays_bright(cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        for id in [SYSTEM_LIGHT_ID, SYSTEM_DARK_ID]
            .into_iter()
            .chain(all_themes().iter().map(|theme| theme.id))
        {
            let _mode = apply_by_id(id, None, cx);
            let colors = cx.theme().colors;
            let tone = colors.muted_foreground;
            let resting = shimmer_resting(&colors, tone);
            // The sweep's bright end is the palette foreground, pinned by the
            // builder; assert against that contract directly.
            let highlight = colors.foreground;

            for (seat, surface) in [
                ("canvas", colors.background),
                ("panel", colors.secondary),
                ("group box", colors.group_box),
                ("popover", colors.popover),
            ] {
                let tone_ratio = contrast_ratio(tone, surface);
                let resting_ratio = contrast_ratio(resting, surface);
                let floor = 3.0_f32.min(tone_ratio);
                assert!(
                    resting_ratio <= tone_ratio + 0.01,
                    "{id}: resting shimmer ({resting_ratio:.2}) outshines its tone ({tone_ratio:.2}) on {seat}"
                );
                assert!(
                    resting_ratio + 0.01 >= floor,
                    "{id}: resting shimmer ({resting_ratio:.2}) fell below the quiet floor ({floor:.2}) on {seat}"
                );
                // The sweep must out-pop the resting text on the seat it sits
                // on. This is what fails when the band resolves toward the
                // surface (the Ciapre bug); comparing the two palette poles
                // instead is true by construction and can never fail.
                assert!(
                    contrast_ratio(highlight, surface) > resting_ratio,
                    "{id}: sweep does not outshine the resting text on {seat}"
                );
            }
        }
    });
}

/// A tone already at or below the floor passes through untouched: dimming
/// cannot buy contrast the palette does not have.
#[test]
fn shimmer_resting_leaves_a_sub_floor_tone_alone() {
    let dim: Hsla = rgb(0x3A3A3A).into();
    let colors = ThemeColor {
        popover: rgb(0x181818).into(),
        ..ThemeColor::default()
    };
    assert_eq!(shimmer_resting(&colors, dim), dim);
}

/// sRGB-channel composite of `fg` at `alpha` over an opaque `bg`, mirroring
/// `translucent_sidebar_surface`'s convention.
fn composite_over(fg: Hsla, alpha: f32, bg: Hsla) -> Hsla {
    let top = gpui_kit::Rgba::from(fg);
    let bottom = gpui_kit::Rgba::from(bg);
    Hsla::from(gpui_kit::Rgba {
        r: top.r * alpha + bottom.r * (1.0 - alpha),
        g: top.g * alpha + bottom.g * (1.0 - alpha),
        b: top.b * alpha + bottom.b * (1.0 - alpha),
        a: 1.0,
    })
}

/// Glass primaries composite their accent fill over the canvas seat: the
/// theme's primary text must hold AA on that composite in every palette and
/// across the whole contrast range, using the independent `composite_over`
/// oracle rather than the production blend. The rest fill also keeps the
/// accent's hue (opacity only dims it) and stays inside the glass alpha band.
#[gpui_kit::test]
fn glass_primary_rest_holds_aa_on_the_canvas_seat(cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        for id in [SYSTEM_LIGHT_ID, SYSTEM_DARK_ID]
            .into_iter()
            .chain(all_themes().iter().map(|theme| theme.id))
        {
            for amount in [0, 40, 100] {
                let _mode = apply_with_contrast(id, amount, None, cx);
                let colors = cx.theme().colors;
                let rest = glass_primary_rest(&colors);
                assert!(
                    (0.60..=1.0).contains(&rest.a),
                    "{id} at {amount}: glass alpha {a} left the 0.60-1.00 band",
                    a = rest.a
                );
                let composite = composite_over(colors.button_primary, rest.a, colors.background);
                let ratio = contrast_ratio(colors.button_primary_foreground, composite);
                assert!(ratio >= 4.5, "{id} at {amount}: glass primary text is {ratio:.2}");
                let rest_rgb = gpui_kit::Rgba::from(rest);
                let accent_rgb = gpui_kit::Rgba::from(colors.button_primary);
                assert_eq!(
                    (rest_rgb.r, rest_rgb.g, rest_rgb.b),
                    (accent_rgb.r, accent_rgb.g, accent_rgb.b),
                    "{id} at {amount}: glass must dim the accent, not shift its hue"
                );
            }
        }
    });
}

/// Popover seat matrix: same AA, hue, and band contract as the canvas row,
/// composited over `popover` for dialog commits.
#[gpui_kit::test]
fn glass_primary_rest_holds_aa_on_the_popover_seat(cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        for id in [SYSTEM_LIGHT_ID, SYSTEM_DARK_ID]
            .into_iter()
            .chain(all_themes().iter().map(|theme| theme.id))
        {
            for amount in [0, 40, 100] {
                let _mode = apply_with_contrast(id, amount, None, cx);
                let colors = cx.theme().colors;
                let rest = glass_primary_rest_on_popover(&colors);
                assert!(
                    (0.60..=1.0).contains(&rest.a),
                    "{id} at {amount}: glass alpha {a} left the 0.60-1.00 band",
                    a = rest.a
                );
                let composite = composite_over(colors.button_primary, rest.a, colors.popover);
                let ratio = contrast_ratio(colors.button_primary_foreground, composite);
                assert!(ratio >= 4.5, "{id} at {amount}: glass primary text is {ratio:.2}");
                let rest_rgb = gpui_kit::Rgba::from(rest);
                let accent_rgb = gpui_kit::Rgba::from(colors.button_primary);
                assert_eq!(
                    (rest_rgb.r, rest_rgb.g, rest_rgb.b),
                    (accent_rgb.r, accent_rgb.g, accent_rgb.b),
                    "{id} at {amount}: glass must dim the accent, not shift its hue"
                );
            }
        }
    });
}

/// Secondary seat matrix: same AA, hue, and band contract as the canvas row,
/// composited over `secondary` for settings-surface commits.
#[gpui_kit::test]
fn glass_primary_rest_holds_aa_on_the_secondary_seat(cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        for id in [SYSTEM_LIGHT_ID, SYSTEM_DARK_ID]
            .into_iter()
            .chain(all_themes().iter().map(|theme| theme.id))
        {
            for amount in [0, 40, 100] {
                let _mode = apply_with_contrast(id, amount, None, cx);
                let colors = cx.theme().colors;
                let rest = glass_primary_rest_on_secondary(&colors);
                assert!(
                    (0.60..=1.0).contains(&rest.a),
                    "{id} at {amount}: glass alpha {a} left the 0.60-1.00 band",
                    a = rest.a
                );
                let composite = composite_over(colors.button_primary, rest.a, colors.secondary);
                let ratio = contrast_ratio(colors.button_primary_foreground, composite);
                assert!(ratio >= 4.5, "{id} at {amount}: glass primary text is {ratio:.2}");
                let rest_rgb = gpui_kit::Rgba::from(rest);
                let accent_rgb = gpui_kit::Rgba::from(colors.button_primary);
                assert_eq!(
                    (rest_rgb.r, rest_rgb.g, rest_rgb.b),
                    (accent_rgb.r, accent_rgb.g, accent_rgb.b),
                    "{id} at {amount}: glass must dim the accent, not shift its hue"
                );
            }
        }
    });
}

/// The secondary recipe's seat assumption: settings saves sit on group-box
/// fills, so `group_box` must equal `secondary` on every theme and contrast
/// amount, else the secondary matrix above guarantees the wrong seat.
#[gpui_kit::test]
fn group_box_matches_secondary_on_every_theme(cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        for id in [SYSTEM_LIGHT_ID, SYSTEM_DARK_ID]
            .into_iter()
            .chain(all_themes().iter().map(|theme| theme.id))
        {
            for amount in [0, 40, 100] {
                let _mode = apply_with_contrast(id, amount, None, cx);
                let colors = cx.theme().colors;
                assert_eq!(
                    gpui_kit::Rgba::from(colors.group_box),
                    gpui_kit::Rgba::from(colors.secondary),
                    "{id} at {amount}: group box must share the secondary seat"
                );
            }
        }
    });
}

/// The pilot must actually show glass on the default theme, not silently
/// sit on the opaque fallback everywhere (which would still pass the AA
/// matrix above while changing nothing visible).
#[gpui_kit::test]
fn glass_primary_engages_translucency_on_the_default_theme(cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        let _mode = apply_by_id(DEFAULT_THEME_ID, None, cx);
        let colors = cx.theme().colors;
        let rest = glass_primary_rest(&colors);
        assert!(rest.a < 1.0, "default theme must show glass, not the opaque fallback");
        let popover = glass_primary_rest_on_popover(&colors);
        assert!(popover.a < 1.0, "default theme must show glass on the popover seat");
        let secondary = glass_primary_rest_on_secondary(&colors);
        assert!(secondary.a < 1.0, "default theme must show glass on the secondary seat");
    });
}

/// Asymmetric fixtures: a zero-headroom palette (white accent, white text on
/// white seats) where no alpha can separate must fall back to the opaque
/// fill on every seat, while a headroom-rich palette (black accent on black
/// seats with white text) must engage translucency at exactly the glass
/// floor on every seat.
#[test]
fn glass_primary_rest_degrades_to_opaque_without_headroom() {
    let white: Hsla = rgb(0xffffff).into();
    let black: Hsla = rgb(0x000000).into();
    let stuck = ThemeColor {
        background: white,
        popover: white,
        secondary: white,
        button_primary: white,
        button_primary_foreground: white,
        ..ThemeColor::default()
    };
    assert_eq!(glass_primary_rest(&stuck).a, 1.0, "no alpha can separate white on white");
    assert_eq!(glass_primary_rest_on_popover(&stuck).a, 1.0, "the popover seat falls back with its canvas sibling");
    assert_eq!(
        glass_primary_rest_on_secondary(&stuck).a,
        1.0,
        "the secondary seat falls back with its canvas sibling"
    );
    let roomy = ThemeColor {
        background: black,
        popover: black,
        secondary: black,
        button_primary: black,
        button_primary_foreground: white,
        ..ThemeColor::default()
    };
    for (name, rest) in [
        ("canvas", glass_primary_rest(&roomy)),
        ("popover", glass_primary_rest_on_popover(&roomy)),
        ("secondary", glass_primary_rest_on_secondary(&roomy)),
    ] {
        assert!((rest.a - 0.60).abs() < 1e-6, "{name} headroom must engage the glass floor, got {}", rest.a);
    }
}

/// The disabled wash is Kit's own disabled-primary formula
/// (`button_primary` at 0.15), pinned so a disabled glass button reads
/// exactly like a disabled opaque one.
#[test]
fn glass_disabled_wash_matches_kit_disabled_primary() {
    let colors = ThemeColor {
        button_primary: rgb(0x3b82f6).into(),
        ..ThemeColor::default()
    };
    let wash = glass_disabled_wash(&colors);
    assert!((wash.a - 0.15).abs() < 1e-6, "wash must sit at 0.15 alpha, got {}", wash.a);
    let wash_rgb = gpui_kit::Rgba::from(wash);
    let accent_rgb = gpui_kit::Rgba::from(colors.button_primary);
    assert_eq!(
        (wash_rgb.r, wash_rgb.g, wash_rgb.b),
        (accent_rgb.r, accent_rgb.g, accent_rgb.b),
        "wash must dim the accent, not shift its hue"
    );
}

/// The Recommended pill paints `muted_text` over the choice-row washes
/// (checked 8%, hover 4% primary on popover): the small pill text must
/// hold AA on both seats. The Tag outline default (`muted_foreground`)
/// drops below the floor there, which is why the row overrides it.
#[gpui_kit::test]
fn recommended_pill_holds_contrast_on_washed_rows(cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        for id in [SYSTEM_LIGHT_ID, SYSTEM_DARK_ID]
            .into_iter()
            .chain(all_themes().iter().map(|theme| theme.id))
        {
            for amount in [0, 40, 100] {
                let _mode = apply_with_contrast(id, amount, None, cx);
                let colors = cx.theme().colors;
                for (seat, alpha) in [("checked", 0.08), ("hover", 0.04)] {
                    let surface = composite_over(colors.primary, alpha, colors.popover);
                    let ratio = contrast_ratio(muted_text(&colors), surface);
                    assert!(ratio >= 4.5, "{id} at {amount}: pill on {seat} wash is {ratio:.2}");
                }
            }
        }
    });
}
