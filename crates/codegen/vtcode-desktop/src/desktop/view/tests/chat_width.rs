use super::*;
use crate::desktop::preferences::ChatWidth;
use crate::desktop::view::settings::SettingsTab;

#[test]
fn appearance_search_finds_the_chat_width_setting() {
    let keywords = SettingsTab::Appearance.search_keywords();
    assert!(keywords.contains(&"chat width"), "Settings search must surface Chat width: {keywords:?}");
    assert!(keywords.contains(&"width"), "a short query must reach the Appearance page: {keywords:?}");
}

#[gpui_kit::test]
fn chat_width_preset_controls_the_column_cap(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop(cx);
    view.update(cx, |view, cx| {
        // A fresh install keeps the comfortable cap.
        assert_eq!(view.content_width(), 768.0);

        view.set_chat_width(ChatWidth::Wide, cx);
        assert_eq!(view.content_width(), 1200.0);

        view.set_chat_width(ChatWidth::FullExtend, cx);
        assert!(view.content_width() > 10_000.0, "full extend is effectively unbounded");

        view.set_chat_width(ChatWidth::Comfortable, cx);
        assert_eq!(view.content_width(), 768.0);
    });
}

#[gpui_kit::test]
fn chat_width_in_zen_keeps_comfortable_narrow_but_honors_wide_and_full(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop(cx);
    view.update(cx, |view, cx| {
        view.zen_mode = true;
        assert_eq!(view.content_width(), 680.0);

        view.set_chat_width(ChatWidth::Wide, cx);
        assert_eq!(view.content_width(), 1200.0);

        view.set_chat_width(ChatWidth::FullExtend, cx);
        assert!(view.content_width() > 10_000.0);
    });
}

#[gpui_kit::test]
fn full_extend_widens_the_rendered_column(cx: &mut TestAppContext) {
    let (window, view) = open_desktop_sized(cx, 1400., 900.);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.sidebar_open = false;
            view.inspector_open = false;
            view.zen_mode = false;
            view.set_chat_width(ChatWidth::Comfortable, cx);
        });
        window.render_frame(cx);
        let comfortable = window.find("message-composer-frame").bounds().size.width;

        view.update(cx, |view, cx| view.set_chat_width(ChatWidth::FullExtend, cx));
        window.render_frame(cx);
        let full = window.find("message-composer-frame").bounds().size.width;
        assert!(
            full > comfortable + px(300.),
            "full extend fills the conversation area: full={full:?} comfortable={comfortable:?}"
        );
    })
    .unwrap();
}
