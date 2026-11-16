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
