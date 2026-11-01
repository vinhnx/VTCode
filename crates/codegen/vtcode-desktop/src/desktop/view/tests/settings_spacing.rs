use super::super::{row_description, settings_row};
use super::*;
use gpui_kit::component::theme::Theme;
use gpui_kit::prelude::*;
use gpui_kit::{Context, IntoElement, Render, TestSupportExt, Window, div};

/// Probe mounting one settings row plus its title and description in
/// isolation, so the title/description gap contract is asserted on rendered
/// geometry rather than builder source. Short single-line copy keeps every
/// lane on one line at any probe width.
struct SettingsRowSpacingProbe;

impl Render for SettingsRowSpacingProbe {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors;
        div()
            .flex()
            .flex_col()
            .size_full()
            .child(div().id("probe-row").test_support().child(settings_row(
                "Interface language",
                "Applies immediately",
                div(),
                &colors,
            )))
            .child(
                div()
                    .id("probe-title")
                    .test_support()
                    .child(row_title("Interface language", &colors)),
            )
            .child(
                div()
                    .id("probe-description")
                    .test_support()
                    .child(row_description("Applies immediately", &colors)),
            )
            .child(div().id("probe-row-empty").test_support().child(settings_row(
                "Interface language",
                "",
                div(),
                &colors,
            )))
    }
}

fn open_spacing_probe(cx: &mut TestAppContext) -> AnyWindowHandle {
    cx.update(gpui_kit::init);
    let (window, _probe) = cx.update(|cx| {
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    point(px(0.), px(0.)),
                    size(px(640.), px(480.)),
                ))),
                ..Default::default()
            },
            cx,
            |_, cx| cx.new(|_| SettingsRowSpacingProbe),
        )
        .expect("open spacing probe window")
    });
    window
}

#[gpui_kit::test]
fn settings_row_separates_title_from_description(cx: &mut TestAppContext) {
    let window = open_spacing_probe(cx);
    // Asymmetric extremes: the smallest and largest UI base sizes must both
    // keep a positive gap, not just the default.
    for base_size in [14., 24.] {
        cx.update(|cx| {
            Theme::update(cx, |theme| {
                theme.font_size = px(base_size);
            });
        });
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let row = window.find("probe-row").bounds().size.height;
            let title = window.find("probe-title").bounds().size.height;
            let description = window.find("probe-description").bounds().size.height;
            assert!(
                row > title + description,
                "the row ({row:?}) must be taller than title ({title:?}) + description ({description:?}) at {base_size}px"
            );
            let row_empty = window.find("probe-row-empty").bounds().size.height;
            assert_eq!(
                row_empty, title,
                "an empty description adds no dead gap: row ({row_empty:?}) must equal title ({title:?}) at {base_size}px"
            );
        })
        .unwrap();
    }
}

#[gpui_kit::test]
fn appearance_sections_keep_order_air_and_narrow_containment(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_settings(window, cx));
        window.render_frame(cx);
        // Asymmetric row heights: title-only family rows, title+description
        // size rows, and the code preview must stack in strict order.
        let ui_font = window.find("choose-ui-font").bounds();
        let code_font = window.find("choose-code-font").bounds();
        let ui_size = window.find("font-ui-value").bounds();
        let code_size = window.find("font-code-value").bounds();
        let preview = window.find("appearance-code-preview").bounds();
        for (upper, lower, name) in [
            (ui_font, code_font, "ui-font/code-font"),
            (code_font, ui_size, "code-font/ui-size"),
            (ui_size, code_size, "ui-size/code-size"),
            (code_size, preview, "code-size/preview"),
        ] {
            assert!(
                upper.origin.y + upper.size.height <= lower.origin.y + px(1.),
                "{name} rows must not overlap: upper={upper:?} lower={lower:?}"
            );
        }
        let typography = window.find("appearance-typography-group").bounds();
        let language = window.find("appearance-language-group").bounds();
        let typography_bottom = typography.origin.y + typography.size.height;
        assert!(
            typography_bottom <= language.origin.y + px(1.),
            "typography and language sections must not touch: typography={typography:?} language={language:?}"
        );
        let section_gap = language.origin.y - typography_bottom;
        assert!(section_gap >= px(20.), "sections keep air between them (section-scale gap): gap={section_gap:?}");
    })
    .unwrap();

    // Narrow viewports stack the nav above the content; groups (including
    // the typography code preview) must stay inside the content spine
    // instead of overflowing it.
    cx.simulate_window_resize(window, size(px(600.), px(700.)));
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let content = window.find("settings-content").bounds();
        let content_right = content.origin.x + content.size.width;
        for id in [
            "appearance-typography-group",
            "appearance-language-group",
            "appearance-files-group",
        ] {
            let bounds = window.find(id).bounds();
            assert!(
                bounds.origin.x + bounds.size.width <= content_right + px(1.),
                "{id} must stay inside the narrow content: bounds={bounds:?} content={content:?}"
            );
        }
    })
    .unwrap();
}
