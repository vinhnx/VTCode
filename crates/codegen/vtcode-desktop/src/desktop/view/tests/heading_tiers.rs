use super::*;
use gpui_kit::component::theme::Theme;
use gpui_kit::prelude::*;
use gpui_kit::{Context, IntoElement, Pixels, Render, TestSupportExt, Window, div};

/// Probe mounting the shared heading builders with identical copy so the tier
/// contract is asserted on rendered geometry rather than builder source.
struct HeadingTierProbe;

impl Render for HeadingTierProbe {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors;
        div()
            .flex()
            .flex_col()
            .size_full()
            .child(
                div()
                    .id("probe-section")
                    .test_support()
                    .child(section_label("Appearance settings", &colors)),
            )
            .child(
                div()
                    .id("probe-group")
                    .test_support()
                    .child(group_box_title("Appearance settings", &colors)),
            )
            .child(
                div()
                    .id("probe-row")
                    .test_support()
                    .child(row_title("Appearance settings", &colors)),
            )
            .child(
                div()
                    .id("probe-eyebrow")
                    .test_support()
                    .child(eyebrow_label("Appearance settings", &colors)),
            )
            .child(
                div()
                    .id("probe-row-upper")
                    .test_support()
                    .child(row_title("APPEARANCE SETTINGS", &colors)),
            )
            .child(
                div()
                    .id("probe-content-title")
                    .test_support()
                    .child(content_page_title("Appearance settings", &colors)),
            )
    }
}

fn open_probe(cx: &mut TestAppContext) -> AnyWindowHandle {
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
            |_, cx| cx.new(|_| HeadingTierProbe),
        )
        .expect("open probe window")
    });
    window
}

#[gpui_kit::test]
fn section_headings_render_one_step_above_body_text(cx: &mut TestAppContext) {
    let window = open_probe(cx);
    // Asymmetric extremes: the smallest and largest UI base sizes must both
    // preserve the tier ordering, not just the default.
    let mut previous_section: Option<Pixels> = None;
    for base_size in [14., 24.] {
        cx.update(|cx| {
            Theme::update(cx, |theme| {
                theme.font_size = px(base_size);
            });
        });
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let section = window.find("probe-section").bounds().size.height;
            let group = window.find("probe-group").bounds().size.height;
            let row = window.find("probe-row").bounds().size.height;
            let eyebrow = window.find("probe-eyebrow").bounds().size.height;
            let row_upper = window.find("probe-row-upper").bounds().size.height;
            let content_title = window.find("probe-content-title").bounds().size.height;
            assert!(
                section > row,
                "section heading ({section:?}) must render taller than body text ({row:?}) at {base_size}px"
            );
            assert_eq!(group, section, "group-box titles share the section tier at {base_size}px");
            assert_eq!(eyebrow, row_upper, "eyebrows stay at body size at {base_size}px");
            assert!(
                content_title > section,
                "content page title ({content_title:?}) must render one step above section headings ({section:?}) at {base_size}px"
            );
            if let Some(previous) = previous_section {
                assert!(
                    section > previous,
                    "headings must scale with the UI base size ({previous:?} at 14px vs {section:?} at 24px)"
                );
            }
            previous_section = Some(section);
        })
        .unwrap();
    }
}
