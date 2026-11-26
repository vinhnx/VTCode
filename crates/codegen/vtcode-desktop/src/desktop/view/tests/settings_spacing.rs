use super::super::{row_description, settings_group_box, settings_group_box_untitled, settings_row};
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
            .child(
                div().id("probe-gap-group").test_support().child(
                    settings_group_box("Probe group", &colors)
                        .child(div().id("probe-gap-first").test_support().w_full().h(px(12.)))
                        .child(div().id("probe-gap-second").test_support().w_full().h(px(12.))),
                ),
            )
            .child(
                div().id("probe-padding-group").test_support().child(
                    settings_group_box_untitled(&colors)
                        .id("probe-padding-group-surface")
                        .child(div().id("probe-padding-child").test_support().w_full().h(px(12.))),
                ),
            )
            .child(
                div().id("probe-footer-group").test_support().child(
                    settings_group_box_untitled(&colors)
                        .id("probe-footer-group-surface")
                        .title(div().id("probe-footer-title").test_support().child("Footnote group"))
                        .child(div().id("probe-footer-body").test_support().w_full().h(px(12.)))
                        .footer(div().id("probe-footer-note").test_support().child("Supporting copy")),
                ),
            )
            .child(div().id("probe-align-row").test_support().child(settings_row(
                "Interface language",
                "Applies immediately",
                div().id("probe-align-control").test_support().w_16().h_8(),
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
fn settings_row_title_outweighs_description(cx: &mut TestAppContext) {
    let window = open_spacing_probe(cx);
    // A field label and its help text must not read as the same tier: the
    // title is one size step larger so hierarchy survives at both the
    // smallest and largest UI base sizes, not just the default.
    for base_size in [14., 24.] {
        cx.update(|cx| {
            Theme::update(cx, |theme| {
                theme.font_size = px(base_size);
            });
        });
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let title = window.find("probe-title").bounds().size.height;
            let description = window.find("probe-description").bounds().size.height;
            assert!(
                title > description,
                "row title ({title:?}) must render taller than its description ({description:?}) at {base_size}px"
            );
        })
        .unwrap();
    }
}

#[gpui_kit::test]
fn settings_group_separates_rows_by_gap_4_air(cx: &mut TestAppContext) {
    let window = open_spacing_probe(cx);
    // Asymmetric extremes: the Kit Settings rhythm (gap_4 = 1rem) must hold
    // at the smallest and largest UI base sizes, not just the default.
    for base_size in [14., 24.] {
        cx.update(|cx| {
            Theme::update(cx, |theme| {
                theme.font_size = px(base_size);
            });
        });
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let first = window.find("probe-gap-first").bounds();
            let second = window.find("probe-gap-second").bounds();
            let gap = second.origin.y - (first.origin.y + first.size.height);
            assert!(
                (gap - px(base_size)).abs() <= px(1.),
                "rows separate by gap_4 air ({base_size}px) without dividers: gap={gap:?} first={first:?} second={second:?}"
            );
        })
        .unwrap();
    }
}

#[gpui_kit::test]
fn settings_group_pads_content_evenly_at_p_6(cx: &mut TestAppContext) {
    let window = open_spacing_probe(cx);
    // Titled and untitled variants share one content-style owner, so pinning
    // the untitled surface pins both. Untitled has no title row, so the
    // wrapper bounds equal the surface bounds.
    for base_size in [14., 24.] {
        cx.update(|cx| {
            Theme::update(cx, |theme| {
                theme.font_size = px(base_size);
            });
        });
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let group = window.find("probe-padding-group").bounds();
            let child = window.find("probe-padding-child").bounds();
            // p_6 = 1.5rem per side on both axes: the visible band stays even
            // (concentric) and airier than the gap_4 row rhythm.
            let pad = px(base_size * 1.5);
            let group_right = group.origin.x + group.size.width;
            let group_bottom = group.origin.y + group.size.height;
            for (edge, actual) in [
                ("leading", child.origin.x - group.origin.x),
                ("trailing", group_right - (child.origin.x + child.size.width)),
                ("top", child.origin.y - group.origin.y),
                ("bottom", group_bottom - (child.origin.y + child.size.height)),
            ] {
                assert!(
                    (actual - pad).abs() <= px(1.),
                    "section keeps an even {edge} band at {base_size}px: actual={actual:?} want={pad:?} child={child:?} group={group:?}"
                );
            }
        })
        .unwrap();
    }
}

#[gpui_kit::test]
fn settings_group_footer_sits_below_the_surface_on_the_title_spine(cx: &mut TestAppContext) {
    let window = open_spacing_probe(cx);
    // Kit renders a GroupBox footer outside the filled surface: it shares the
    // title's leading edge, sits one gap_2 (0.5rem) under the surface, and is
    // not inset by `content_style` padding. This is the contract the
    // `card_border_style` comment relies on for title/footer seats; pin it at
    // both rem extremes so a future styler cannot silently move the caption
    // inside the padded body.
    for base_size in [14., 24.] {
        cx.update(|cx| {
            Theme::update(cx, |theme| {
                theme.font_size = px(base_size);
            });
        });
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let title = window.find("probe-footer-title").bounds();
            let body = window.find("probe-footer-body").bounds();
            let note = window.find("probe-footer-note").bounds();

            // p_6 on `content_style` insets only the body; the footer keeps the
            // title/surface leading edge, so the body starts one p_6 to its right.
            let pad = px(base_size * 1.5);
            assert!(
                (body.origin.x - (note.origin.x + pad)).abs() <= px(1.),
                "body carries the p_6 inset but the footer stays on the spine: body={body:?} note={note:?}"
            );
            assert!(
                (note.origin.x - title.origin.x).abs() <= px(1.),
                "footer shares the title's leading edge, not the body's: title={title:?} note={note:?}"
            );

            // The surface bottom is one p_6 below the body; the footer starts
            // one gap_2 (0.5rem) after that, outside the surface entirely.
            let gap = note.origin.y - (body.origin.y + body.size.height + pad);
            let expected = px(base_size * 0.5);
            assert!(
                (gap - expected).abs() <= px(1.),
                "footer sits {expected:?} below the surface at {base_size}px: gap={gap:?} body={body:?} note={note:?}"
            );
        })
        .unwrap();
    }
}

#[gpui_kit::test]
fn settings_row_pins_control_to_title_frame(cx: &mut TestAppContext) {
    let window = open_spacing_probe(cx);
    // Two-line text block (title + description) against a shorter rem-based
    // control: top alignment pins the control to the title frame instead of
    // centering it on the help text.
    for base_size in [14., 24.] {
        cx.update(|cx| {
            Theme::update(cx, |theme| {
                theme.font_size = px(base_size);
            });
        });
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let row = window.find("probe-align-row").bounds();
            let control = window.find("probe-align-control").bounds();
            assert!(
                (control.origin.y - row.origin.y).abs() <= px(1.),
                "control pins to the title frame, not the text-block center, at {base_size}px: row={row:?} control={control:?}"
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
fn status_bar_long_labels_shrink_instead_of_pushing_siblings_out(cx: &mut TestAppContext) {
    // GPUI's flex layout already releases a truncating label's minimum width
    // inside the status bar's `min_w_0` rows, so long workspace/branch/model
    // copy ellipsizes instead of swallowing its siblings. Pin that contract at
    // both font-size extremes: a future tier change must not reintroduce the
    // CSS-style nowrap overflow the style guide warns about.
    let (window, view) = open_desktop_sized(cx, 600., 560.);
    view.update(cx, |view, cx| {
        view.preferences.show_status_bar = true;
        view.workspace = Some(PathBuf::from(format!("/projects/{}", "w".repeat(160))));
        view.git_branch = Some("feature/an-extremely-long-branch-name-that-never-ends".to_string());
        let mut config = runtime_options_config();
        config.provider = "provider-with-a-very-long-name".to_string();
        config.model = "model-with-a-very-long-name".to_string();
        view.configuration = Some(config);
        cx.notify();
    });
    for base_size in [14., 24.] {
        cx.update(|cx| {
            Theme::update(cx, |theme| {
                theme.font_size = px(base_size);
            });
        });
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let bar = window.find("status-bar").bounds();
            let bar_right = bar.origin.x + bar.size.width;
            for id in [
                "status-bar-workspace",
                "status-bar-branch",
                "status-bar-sandbox",
                "status-bar-provider-model",
            ] {
                let bounds = window.find(id).bounds();
                assert!(
                    bounds.origin.x >= bar.origin.x - px(1.)
                        && bounds.origin.x + bounds.size.width <= bar_right + px(1.),
                    "{id} must shrink inside the status bar at {base_size}px: bounds={bounds:?}, bar={bar:?}"
                );
            }
        })
        .unwrap();
    }
}

#[gpui_kit::test]
fn settings_panel_spans_the_center_container_while_other_routes_stay_inset(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_settings(window, cx));
        window.render_frame(cx);
        let panel = window.find("settings-panel").bounds();
        let gutter = window.find("route-page-gutter").bounds();
        assert_eq!(panel.origin.x, gutter.origin.x, "settings reaches the leading center edge: {panel:?} {gutter:?}");
        assert_eq!(panel.right(), gutter.right(), "settings reaches the trailing center edge: {panel:?} {gutter:?}");

        // Asymmetric comparison: the shared centered container must still
        // inset the other routes, so the full-width change is settings-only.
        view.update(cx, |view, cx| view.open_thread_browser(window, cx));
        window.render_frame(cx);
        let browser = window.find("thread-browser").bounds();
        assert!(browser.origin.x > gutter.origin.x, "browser keeps its leading inset: {browser:?} {gutter:?}");
        assert!(browser.right() < gutter.right(), "browser keeps its trailing inset: {browser:?} {gutter:?}");
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
            // Wide viewports host the Kit container: the sidebar rows and
            // the full-width typography group proxy the legacy nav and
            // content edges (Kit exposes no stable full-height content
            // id, so the bottom inset has no equivalent here).
            let sidebar_row = window.find("0-0").bounds();
            let group = window.find("appearance-typography-group").bounds();
            let panel_right = panel.origin.x + panel.size.width;
            // Panel insets at `px_4` (1rem): sidebar and content sit one
            // panel step inside the panel.
            let inset = base_size - 1.;
            assert!(
                sidebar_row.origin.x - panel.origin.x >= px(inset),
                "sidebar keeps a panel-scale leading inset at {base_size}px: row={sidebar_row:?} panel={panel:?}"
            );
            assert!(
                title.origin.x - panel.origin.x >= px(inset),
                "title keeps a panel-scale leading inset at {base_size}px: title={title:?} panel={panel:?}"
            );
            assert!(
                panel_right - (group.origin.x + group.size.width) >= px(inset),
                "content keeps a panel-scale trailing inset at {base_size}px: group={group:?} panel={panel:?}"
            );
            // The Kit sidebar sits beside the hosted content instead of
            // overlapping it.
            assert!(
                sidebar_row.origin.x + sidebar_row.size.width <= group.origin.x + px(1.),
                "sidebar sits beside content at {base_size}px: row={sidebar_row:?} group={group:?}"
            );
            // Section surfaces pad horizontal at `px_6` (1.5rem): the full-
            // width code preview pins both edges of the air inside the card.
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

/// A UI font-size change rescales the window rem, so the Kit surface radius
/// tokens must be rebuilt in the same `Theme::update` that sets the font size.
/// Otherwise Kit popovers keep the old pixel radii while the app surfaces
/// around them zoom, breaking the concentric tier contract at 14px and 24px.
#[gpui_kit::test]
fn typography_changes_rescale_kit_radius_tokens(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop_sized(cx, 1200., 800.);
    for size in [14_u8, 16, 24] {
        view.update(cx, |view, cx| {
            view.preferences.ui_font_size = Some(size);
            view.apply_typography(cx);
        });
        cx.update(|cx| {
            let theme = cx.theme();
            let rem = px(f32::from(size));
            assert_eq!(theme.font_size, rem, "font size at {size}px");
            assert_eq!(theme.radius, gpui_kit::rems(0.5).to_pixels(rem), "base radius at {size}px");
            assert_eq!(theme.radius_lg, gpui_kit::rems(0.75).to_pixels(rem), "large radius at {size}px");
        });
    }
}
