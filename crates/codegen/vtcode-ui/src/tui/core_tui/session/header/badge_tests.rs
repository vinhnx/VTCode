use super::*;

#[test]
fn ultrafast_badge_color_policy_and_bold_is_bright() {
    for bold_is_bright in [false, true] {
        for (colors_enabled, safe_colors_only, foreground, background, reversed) in [
            (true, false, Color::Rgb(0, 0, 0), Color::Rgb(0xD9, 0x9A, 0x4E), false),
            (true, true, Color::Black, Color::Yellow, false),
            (false, false, Color::Reset, Color::Reset, true),
            (false, true, Color::Reset, Color::Reset, true),
        ] {
            let style = ultrafast_badge_style(colors_enabled, safe_colors_only, bold_is_bright);
            let inherited = Style::default()
                .add_modifier(Modifier::DIM | Modifier::BOLD | Modifier::REVERSED)
                .patch(style);
            assert_eq!(inherited.fg, Some(foreground));
            assert_eq!(inherited.bg, Some(background));
            assert_eq!(inherited.add_modifier.contains(Modifier::REVERSED), reversed);
            assert_eq!(inherited.add_modifier.contains(Modifier::BOLD), !bold_is_bright);
            assert!(!inherited.add_modifier.contains(Modifier::DIM));
        }
    }
}
