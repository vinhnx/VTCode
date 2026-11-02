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

#[gpui_kit::test]
fn settings_container_and_sections_keep_horizontal_air(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_settings(window, cx));
        window.render_frame(cx);
    })
    .unwrap();
    // Asymmetric extremes: panel insets, divider gaps, and section padding
    // are rem-based, so they must hold at the smallest and largest UI base
    // sizes, not just the default.
    for base_size in [14., 24.] {
        cx.update(|cx| {
            Theme::update(cx, |theme| {
                theme.font_size = px(base_size);
            });
        });
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let panel = window.find("settings-panel").bounds();
            let title = window.find("settings-title").bounds();
            let navigation = window.find("settings-navigation").bounds();
            let divider = window.find("settings-pane-divider").bounds();
            let content = window.find("settings-content").bounds();
            let panel_right = panel.origin.x + panel.size.width;
            let panel_bottom = panel.origin.y + panel.size.height;
            // Panel insets at `px_4` (1rem): nav and content sit one panel
            // step inside the panel on every side.
            let inset = base_size - 1.;
            assert!(
                navigation.origin.x - panel.origin.x >= px(inset),
                "nav keeps a panel-scale leading inset at {base_size}px: navigation={navigation:?} panel={panel:?}"
            );
            assert!(
                panel_right - (content.origin.x + content.size.width) >= px(inset),
                "content keeps a panel-scale trailing inset at {base_size}px: content={content:?} panel={panel:?}"
            );
            assert!(
                panel_bottom - (content.origin.y + content.size.height) >= px(inset),
                "content keeps a panel-scale bottom inset at {base_size}px: content={content:?} panel={panel:?}"
            );
            // Divider gaps at `gap_2` (0.5rem) per side: the nav/content
            // separation reads at panel scale instead of hairline-tight.
            let flank = base_size / 2. - 1.;
            assert!(
                divider.origin.x - (navigation.origin.x + navigation.size.width) >= px(flank),
                "divider keeps air from the nav at {base_size}px: navigation={navigation:?} divider={divider:?}"
            );
            assert!(
                content.origin.x - (divider.origin.x + divider.size.width) >= px(flank),
                "content keeps air from the divider at {base_size}px: divider={divider:?} content={content:?}"
            );
            // Title shares the body spine: header `px_3` plus the title's
            // own `pl_1` equals body `px_4`, so both start on one edge.
            assert!(
                (title.origin.x - navigation.origin.x).abs() <= px(1.),
                "title aligns to the nav spine at {base_size}px: title={title:?} navigation={navigation:?}"
            );
            // Section surfaces pad horizontal at `px_6` (1.5rem): the full-
            // width code preview pins both edges of the air inside the card.
            let group = window.find("appearance-typography-group").bounds();
            let preview = window.find("appearance-code-preview").bounds();
            let group_right = group.origin.x + group.size.width;
            let air = base_size * 1.5 - 1.;
            assert!(
                preview.origin.x - group.origin.x >= px(air),
                "section keeps leading air at {base_size}px: preview={preview:?} group={group:?}"
            );
            assert!(
                group_right - (preview.origin.x + preview.size.width) >= px(air),
                "section keeps trailing air at {base_size}px: preview={preview:?} group={group:?}"
            );
        })
        .unwrap();
    }
}
