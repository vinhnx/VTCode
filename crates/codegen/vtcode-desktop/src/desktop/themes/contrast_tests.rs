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
