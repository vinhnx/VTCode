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
        view.update(cx, |view, cx| view.apply_theme("solarized-light".into(), window, cx));
        window.render_frame(cx);
        let language = view.read(cx).composer.read(cx).language_name().to_string();
        assert_eq!(language, super::super::composer_highlight::PROMPT_LANGUAGE);
    })
    .unwrap();
}
