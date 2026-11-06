use std::time::Duration;

use gpui_kit::component::theme::MotionTokens;

use super::*;

/// Transparent brand mark: exactly the stroke-only V path, no tile rect.
fn is_transparent_mark(svg: &str) -> bool {
    svg.starts_with("<svg")
        && svg.matches("<path").count() == 1
        && svg.contains("fill=\"none\"")
        && !svg.contains("fill=\"#")
}

#[test]
fn vtcode_nav_mark_is_the_stroke_only_v_on_transparency() {
    let mark = std::str::from_utf8(VTCODE_NAV_MARK_SVG).expect("nav mark must be UTF-8 SVG");
    assert!(is_transparent_mark(mark), "nav mark must be the stroke-only V on transparency");
    // The full tile carries an opaque background rect and must never
    // stand in for the nav mark: this arm proves the check discriminates.
    let tile = std::str::from_utf8(include_bytes!("../../../../../../../resources/icons/vtcode.svg"))
        .expect("tile must be UTF-8 SVG");
    assert!(!is_transparent_mark(tile), "opaque tile must fail the nav-mark check");
}

#[test]
fn button_tiers_keep_the_pointing_hand_cursor() {
    for mut button in [
        desktop_default_button("tier-cursor-default"),
        desktop_tertiary_button("tier-cursor-tertiary"),
        desktop_primary_button("tier-cursor-primary"),
        desktop_danger_button("tier-cursor-danger"),
    ] {
        assert_eq!(
            button.style().mouse_cursor,
            Some(CursorStyle::PointingHand),
            "every tier delegates to desktop_button"
        );
    }
}

#[test]
fn desktop_elevated_pins_an_instance_shadow_while_plain_buttons_stay_flat() {
    let mut plain = desktop_primary_button("tier-shadow-plain");
    assert!(plain.style().box_shadow.is_none(), "in-layer primaries carry no instance shadow");

    let mut elevated = desktop_primary_button("tier-shadow-elevated").desktop_elevated();
    let shadows = elevated
        .style()
        .box_shadow
        .as_ref()
        .expect("elevated buttons set an instance shadow");
    assert_eq!(shadows.len(), 1, "elevation is one xs shadow layer");
}

#[gpui_kit::test]
fn card_border_style_pins_a_1px_theme_border(cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        let colors = cx.theme().colors;
        let style = card_border_style(&colors);
        assert_eq!(style.border_color, Some(soft_edge_color(&colors)));
        let widths = style.border_widths;
        for width in [widths.top, widths.left, widths.bottom, widths.right] {
            assert_eq!(width, Some(gpui_kit::AbsoluteLength::Pixels(px(1.))));
        }
    });
}

#[test]
fn severity_marker_style_maps_each_level_to_its_own_tone() {
    use gpui_kit::component::theme::ThemeColor;

    let sentinel = |h: f32| gpui_kit::Hsla { h, s: 0.5, l: 0.5, a: 1.0 };
    let white = gpui_kit::Hsla { h: 0.0, s: 0.0, l: 1.0, a: 1.0 };
    let black = gpui_kit::Hsla { h: 0.0, s: 0.0, l: 0.0, a: 1.0 };
    let colors = ThemeColor {
        danger: sentinel(0.0),
        warning: sentinel(0.33),
        foreground: white,
        popover: black,
        ..Default::default()
    };
    let error = severity_marker_style(NoticeSeverity::Error, &colors);
    let warning = severity_marker_style(NoticeSeverity::Warning, &colors);
    let info = severity_marker_style(NoticeSeverity::Info, &colors);

    assert!(matches!(error.icon, IconName::CircleAlert), "errors keep the alert icon");
    assert!(matches!(warning.icon, IconName::TriangleAlert), "warnings keep the triangle icon");
    assert!(matches!(info.icon, IconName::Info), "info keeps the info icon");
    assert_eq!(error.tone, colors.danger, "errors read the danger token");
    assert_eq!(warning.tone, colors.warning, "warnings keep their own tone, not danger");
    assert_eq!(info.tone, themes::muted_text(&colors), "info delegates to the muted text derivation");
    assert!(
        themes::contrast_ratio(info.tone, colors.popover) >= 4.5,
        "info tone holds AA against its surface, unlike a raw token pick"
    );
    assert!(error.semibold_title, "only errors earn semibold");
    assert!(!warning.semibold_title && !info.semibold_title, "icon + tone already carry severity");
    assert_eq!(error.role, gpui_kit::Role::Alert, "errors announce as alerts");
    assert_eq!(warning.role, gpui_kit::Role::Status, "warnings announce as status");
    assert_eq!(info.role, gpui_kit::Role::Status, "info announces as status");
    assert_eq!(error.status_label, "error");
    assert_eq!(warning.status_label, "warning");
    assert_eq!(info.status_label, "info");
    assert_ne!(error.tone, warning.tone, "error and warning tones must not collapse");
    assert_ne!(warning.tone, info.tone, "warning and info tones must not collapse");
    assert_ne!(error.tone, info.tone, "error and info tones must not collapse");
}

/// The shimmer builder owns one `(resting, highlight)` pair and the
/// reduced-motion fallback, so a caller cannot dim a static label below AA or
/// hand Kit a band that out-paints the text it crosses.
#[gpui_kit::test]
fn shimmer_tones_dim_while_animating_and_hold_the_tone_when_static(cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        let tone = cx.theme().colors.muted_foreground;

        cx.set_reduce_motion(false);
        let (resting, highlight) = shimmer_tones(tone, cx);
        assert_eq!(highlight, cx.theme().colors.foreground, "the sweep pins to the palette foreground");
        assert!(
            themes::contrast_ratio(resting, cx.theme().colors.background)
                < themes::contrast_ratio(tone, cx.theme().colors.background),
            "animated resting text must read quieter than its own tone"
        );

        cx.set_reduce_motion(true);
        let (resting, highlight) = shimmer_tones(tone, cx);
        assert_eq!(resting, tone, "reduced motion keeps the full readable tone");
        assert_eq!(highlight, tone, "a static label has no bright band to preserve");
    });
}

/// Motion-policy ratchet: every duration token our fades consume must stay
/// under the 300ms UI-animation ceiling, and the shared enter curve must
/// stay ease-out-shaped (front-loaded progress). Pins the upstream contract
/// `zen_fade_animation` (normal) and the palette fade (fast) depend on;
/// `duration_slow` is intentionally unpinned (nothing consumes it).
#[test]
fn shared_motion_tokens_stay_within_ui_animation_policy() {
    let motion = MotionTokens::default();
    for (name, duration) in [
        ("duration_fast", motion.duration_fast),
        ("duration_normal", motion.duration_normal),
    ] {
        assert!(duration <= Duration::from_millis(300), "{name} must stay under the 300ms UI-animation ceiling");
    }
    assert!(
        motion.easing_enter.sample(0.25) > 0.25 && motion.easing_enter.sample(0.5) > 0.5,
        "enter curve must stay ease-out-shaped for instant feedback"
    );
}
