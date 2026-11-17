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
                    .id("probe-panel")
                    .test_support()
                    .child(panel_title("Appearance settings", &colors)),
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
                    .id("probe-description")
                    .test_support()
                    .child(row_description("Appearance settings", &colors)),
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
fn label_ladder_keeps_every_tier_distinct(cx: &mut TestAppContext) {
    let window = open_probe(cx);
    // Smallest, default, and largest UI base sizes: no two neighbouring roles
    // may collapse into one size, and the ladder must scale as a whole.
    let mut previous_page: Option<Pixels> = None;
    for base_size in [14., 16., 24.] {
        cx.update(|cx| {
            Theme::update(cx, |theme| {
                theme.font_size = px(base_size);
            });
        });
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let page = window.find("probe-content-title").bounds().size.height;
            let panel = window.find("probe-panel").bounds().size.height;
            let section = window.find("probe-section").bounds().size.height;
            let group = window.find("probe-group").bounds().size.height;
            let row = window.find("probe-row").bounds().size.height;
            let eyebrow = window.find("probe-eyebrow").bounds().size.height;
            let description = window.find("probe-description").bounds().size.height;
            assert!(page > panel, "page ({page:?}) > panel ({panel:?}) at {base_size}px");
            assert!(
                panel > section,
                "panel title ({panel:?}) > section heading ({section:?}) at {base_size}px"
            );
            assert_eq!(group, section, "group-box titles share the section tier at {base_size}px");
            assert!(
                section > row,
                "section heading ({section:?}) > row label ({row:?}) at {base_size}px"
            );
            assert!(row > eyebrow, "row label ({row:?}) > eyebrow ({eyebrow:?}) at {base_size}px");
            assert_eq!(
                eyebrow, description,
                "eyebrows and help text share the micro tier at {base_size}px"
            );
            if let Some(previous) = previous_page {
                assert!(
                    page > previous,
                    "the ladder scales with the UI base size ({previous:?} at the smaller base vs {page:?} at {base_size}px)"
                );
            }
            previous_page = Some(page);
        })
        .unwrap();
    }
}
