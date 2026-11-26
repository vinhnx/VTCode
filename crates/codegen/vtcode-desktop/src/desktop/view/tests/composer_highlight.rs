use super::super::themes;
use super::*;

#[gpui_kit::test]
fn composer_installs_the_prompt_token_highlighter(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let language = view.read(cx).composer.read(cx).language_name().to_string();
        assert_eq!(language, super::super::composer_highlight::PROMPT_LANGUAGE);
    })
    .unwrap();
    // A theme change must keep the highlighter installed while swapping colors.
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.apply_theme(themes::ThemeId::solarized::LIGHT.into(), window, cx));
        window.render_frame(cx);
        let language = view.read(cx).composer.read(cx).language_name().to_string();
        assert_eq!(language, super::super::composer_highlight::PROMPT_LANGUAGE);
    })
    .unwrap();
}

#[gpui_kit::test]
fn composer_slash_color_follows_contrast_changes(cx: &mut TestAppContext) {
    // Only accent_foreground (the slash-command color) is rewritten by the
    // contrast pass; the other token colors come from stable semantic slots.
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        let before = *view.read(cx).composer_highlight_styles.borrow();
        view.update(cx, |view, cx| view.set_contrast(100.0, window, cx));
        let after = *view.read(cx).composer_highlight_styles.borrow();
        let expected = super::super::composer_highlight::ComposerHighlightStyles::from_colors(&cx.theme().colors);
        assert_ne!(before, after, "contrast must re-derive the slash-command color");
        assert_eq!(after, expected);
    })
    .unwrap();
}

#[gpui_kit::test]
fn composer_token_colors_follow_saved_theme_on_startup(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.preferences.contrast = 100;
            view.load_preferences(window, cx);
        });
        let after = *view.read(cx).composer_highlight_styles.borrow();
        let expected = super::super::composer_highlight::ComposerHighlightStyles::from_colors(&cx.theme().colors);
        assert_eq!(after, expected, "startup restore must seed token colors from the saved palette");
    })
    .unwrap();
}

#[gpui_kit::test]
fn composer_grows_for_soft_wrapped_single_line_drafts(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        let mut measure = |text: &str| {
            view.update(cx, |view, cx| {
                view.composer.update(cx, |input, cx| input.set_value(text, window, cx));
            });
            window.render_frame(cx);
            window.find("message-composer-frame").bounds().size.height
        };
        let short_height = measure("ok");
        // One logical line that wraps to many visual rows at the 720px cap.
        let long_height = measure(&"longword ".repeat(200));
        assert!(
            long_height > short_height,
            "soft-wrapped draft must grow the composer ({short_height:?} vs {long_height:?})"
        );
        assert!(long_height <= short_height * 6, "growth must stay within the six-row cap ({long_height:?})");
    })
    .unwrap();
}

#[gpui_kit::test]
fn composer_expand_toggles_a_tall_edit_surface_without_losing_the_draft(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        let long = "longword ".repeat(200);
        view.update(cx, |view, cx| {
            view.composer.update(cx, |input, cx| input.set_value(long.clone(), window, cx));
        });
        window.render_frame(cx);
        let compact = window.find("message-composer-frame").bounds().size.height;
        let cta = window.find("composer-expand");
        assert_eq!(cta.role(), Some(gpui_kit::Role::Button));
        assert_eq!(cta.label(), Some("Expand composer"));

        window.click("composer-expand", cx);
        window.render_frame(cx);
        let expanded = window.find("message-composer-frame").bounds().size.height;
        assert!(expanded > compact, "expanded composer must be taller: {compact:?} -> {expanded:?}");
        assert!(view.read(cx).composer_expanded);
        assert_eq!(window.find("composer-expand").label(), Some("Collapse composer"));
        assert_eq!(view.read(cx).composer.read(cx).value(), long);

        window.click("composer-expand", cx);
        window.render_frame(cx);
        assert_eq!(window.find("message-composer-frame").bounds().size.height, compact);
        assert!(!view.read(cx).composer_expanded);
    })
    .unwrap();
}
