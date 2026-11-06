//! Leading icon slots on ⌘K rows: Commands rows reserve the slot, other
//! palettes stay text-only even for mapped actions.
use super::*;

#[gpui_kit::test]
fn command_palette_rows_render_icon_slots_and_other_palettes_do_not(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_command_palette(window, cx));
        window.render_frame(cx);
        // ToggleZenMode/ToggleTheme render in the fresh baseline (see the
        // shortcut-badge tests); both carry leading icon slots.
        let zen = super::super::PaletteAction::ToggleZenMode;
        let theme = super::super::PaletteAction::ToggleTheme;
        let zen_slot = format!("{}-icon", palette_action_id(&zen));
        let theme_slot = format!("{}-icon", palette_action_id(&theme));
        assert!(window.try_find(zen_slot.clone()).is_some(), "zen row renders its icon slot");
        assert!(window.try_find(theme_slot.clone()).is_some(), "theme row renders its icon slot");
        // Fixed slot: different rows share one width, so labels align.
        let zen_bounds = window.find(zen_slot).bounds();
        let theme_bounds = window.find(theme_slot).bounds();
        assert!(
            (zen_bounds.size.width - theme_bounds.size.width).abs() <= px(1.),
            "icon slots share one width: {zen_bounds:?} vs {theme_bounds:?}"
        );
        // A mapped action in a non-Commands palette renders its row but no
        // slot: gating is by palette kind, not by mapping.
        view.update(cx, |view, cx| {
            view.show_palette(
                super::super::PaletteKind::GoTo,
                vec![(
                    "group".to_string(),
                    vec![super::super::command(
                        "row".to_string(),
                        super::super::PaletteAction::ToggleZenMode,
                    )],
                )],
                window,
                cx,
            );
        });
        window.render_frame(cx);
        let goto_row = palette_action_id(&zen);
        assert!(window.try_find(goto_row.clone()).is_some(), "goto row renders");
        assert!(window.try_find(format!("{goto_row}-icon")).is_none(), "non-Commands rows render no icon slot");
    })
    .unwrap();
}
