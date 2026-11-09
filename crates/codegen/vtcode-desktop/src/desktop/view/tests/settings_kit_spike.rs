//! Time-boxed spike: prove the Kit `Settings` bridging patterns the adoption
//! plan depends on, with a minimal probe view (deliberate fixture, no
//! production wiring).
//!
//! One probe per gate question: Entity-bridged mutation from a render
//! closure, id-remount programmatic selection, group scroll targeting,
//! embedded search over multilingual keywords, sidebar row observability,
//! Element reset handlers, stateful controls inside Element items, narrow
//! geometry, and multi-page scale. A failing probe is a gate input that
//! reshapes or stops the adoption, not a regression.

use super::*;
use gpui_kit::{
    Context, FocusHandle, InteractiveElement, IntoElement, ParentElement, Render, TestSupportExt, Window,
    component::{
        button::Button,
        setting::{SelectIndex, SettingGroup, SettingItem, SettingPage, Settings},
        slider::{Slider, SliderState},
    },
    div,
};

/// Minimal `Settings` host. `selection`/`group` drive both the remount id
/// and `default_selected_index`, mirroring the planned adapter: every
/// programmatic navigation dimension must be part of the id, because Kit
/// keys its internal state by element id and only reads the default once.
struct SpikeState {
    selection: usize,
    group: Option<usize>,
    counter: usize,
    dirty: bool,
    reset_count: usize,
    slider: Entity<SliderState>,
    slider_focus: FocusHandle,
    many: bool,
}

impl SpikeState {
    fn new(cx: &mut Context<Self>) -> Self {
        Self {
            selection: 0,
            group: None,
            counter: 0,
            dirty: false,
            reset_count: 0,
            slider: cx.new(|_| SliderState::new().min(0.).max(100.).step(1.).default_value(25.)),
            slider_focus: cx.focus_handle(),
            many: false,
        }
    }

    fn many(cx: &mut Context<Self>) -> Self {
        Self { many: true, ..Self::new(cx) }
    }

    /// Counter row: the display value is cloned at render time (a render
    /// closure cannot re-borrow the rendering entity), while the button
    /// mutates through the captured handle outside render.
    fn counter_item(&self, handle: Entity<Self>) -> SettingItem {
        let counter = self.counter;
        SettingItem::render(move |_, _, _| {
            let handle = handle.clone();
            div()
                .id("spike-counter-wrap")
                .test_support()
                .flex()
                .flex_col()
                .gap_2()
                .child(div().id("spike-counter").test_support().child(format!("Count: {counter}")))
                .child(Button::new("spike-increment").label("Increment").on_click(
                    move |_, _: &mut Window, cx: &mut gpui_kit::App| {
                        handle.update(cx, |this, cx| {
                            this.counter += 1;
                            cx.notify();
                        });
                    },
                ))
        })
        .keywords(["general", "counter"])
    }

    /// Stateful control inside an Element item, mirroring the production
    /// contrast slider: entity-backed state plus an app-level key handler.
    fn slider_item(&self) -> SettingItem {
        let slider = self.slider.clone();
        let slider_keys = slider.clone();
        let focus = self.slider_focus.clone();
        SettingItem::render(move |_, _, _| {
            let keys = slider_keys.clone();
            let mouse_focus = focus.clone();
            div()
                .id("spike-slider-wrap")
                .test_support()
                .track_focus(&focus)
                .capture_any_mouse_down(move |event, window, cx| {
                    if event.button == MouseButton::Left {
                        mouse_focus.focus(window, cx);
                    }
                })
                .on_key_down(move |event, window, cx| {
                    if event.keystroke.modifiers.control
                        || event.keystroke.modifiers.alt
                        || event.keystroke.modifiers.platform
                        || event.keystroke.modifiers.function
                    {
                        return;
                    }
                    let delta = match event.keystroke.key.as_str() {
                        "left" | "down" => -1.0,
                        "right" | "up" => 1.0,
                        _ => return,
                    };
                    cx.stop_propagation();
                    keys.update(cx, |state, cx| {
                        let value = (state.value().start() + delta).clamp(0.0, 100.0);
                        state.set_value(value, window, cx);
                    });
                })
                .child(Slider::new(&slider).w(px(180.)))
        })
        .keywords(["volume", "slider"])
    }

    fn text_item(id: &'static str, text: &str, keywords: &[&'static str]) -> SettingItem {
        let text = text.to_string();
        SettingItem::render(move |_, _, _| div().id(id).test_support().child(text.clone())).keywords(keywords.to_vec())
    }

    /// Tall first group so the second group starts below the fold: group
    /// scroll targeting must actually move content to pass.
    fn colors_item() -> SettingItem {
        SettingItem::render(|_, _, _| {
            div()
                .id("spike-colors")
                .test_support()
                .flex()
                .flex_col()
                .gap_2()
                .child("Colors body")
                .child(div().h(px(420.)).w_full())
        })
        .keywords(["theme colors", "màu sắc"])
    }

    fn pages(&self, handle: Entity<Self>) -> Vec<SettingPage> {
        if self.many {
            return (0..12)
                .map(|ix| {
                    let title = if ix == 11 {
                        "Section 11 with a much longer title".to_string()
                    } else {
                        format!("Section {ix}")
                    };
                    let label = title.clone();
                    SettingPage::new(title).group(SettingGroup::new().item(SettingItem::render(move |_, _, _| {
                        div().id(format!("spike-p{ix}")).test_support().child(label.clone())
                    })))
                })
                .collect();
        }
        let dirty = self.dirty;
        let reset_handle = handle.clone();
        vec![
            SettingPage::new("General").group(
                SettingGroup::new()
                    .title("Basics")
                    .items(vec![self.counter_item(handle), self.slider_item()]),
            ),
            SettingPage::new("Appearance & Themes")
                .icon(super::super::components::vtcode_nav_mark())
                .default_open(true)
                .groups(vec![
                    SettingGroup::new().title("Colors").item(Self::colors_item()),
                    SettingGroup::new().title("Fonts").item(Self::text_item(
                        "spike-fonts",
                        "Fonts body",
                        &["theme font"],
                    )),
                ]),
            SettingPage::new("About").resettable(true).group(SettingGroup::new().item(
                Self::text_item("spike-about", "About body", &["version", "phiên bản"]).on_reset(
                    move |_: &gpui_kit::App| dirty,
                    move |_: &mut Window, cx: &mut gpui_kit::App| {
                        reset_handle.update(cx, |this, cx| {
                            this.dirty = false;
                            this.reset_count += 1;
                            cx.notify();
                        });
                    },
                ),
            )),
        ]
    }
}

impl Render for SpikeState {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let handle = cx.entity();
        let pages = self.pages(handle);
        Settings::new(format!("spike-settings-{}-{:?}", self.selection, self.group))
            .default_selected_index(SelectIndex { page_ix: self.selection, group_ix: self.group })
            .pages(pages)
    }
}

fn open_spike(cx: &mut TestAppContext, width: f32, height: f32, many: bool) -> (AnyWindowHandle, Entity<SpikeState>) {
    cx.update(gpui_kit::init);
    cx.update(|cx| {
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    point(px(0.), px(0.)),
                    size(px(width), px(height)),
                ))),
                ..Default::default()
            },
            cx,
            |_, cx| {
                if many {
                    cx.new(SpikeState::many)
                } else {
                    cx.new(SpikeState::new)
                }
            },
        )
        .expect("open spike window")
    })
}

/// Entity-bridged mutation: a real click inside a render closure updates
/// view state through the captured handle, repeatedly.
#[gpui_kit::test]
fn spike_counter_increments_through_bridged_closure(cx: &mut TestAppContext) {
    let (window, view) = open_spike(cx, 640., 480., false);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("spike-increment", cx);
    })
    .unwrap();
    cx.update(|cx| assert_eq!(view.read(cx).counter, 1, "first click increments"));
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("spike-increment", cx);
    })
    .unwrap();
    cx.update(|cx| assert_eq!(view.read(cx).counter, 2, "the bridge survives re-renders"));
}

/// Programmatic selection via id remount, both directions with skips:
/// presence and absence on every hop.
#[gpui_kit::test]
fn spike_remount_selects_pages_programmatically(cx: &mut TestAppContext) {
    let (window, view) = open_spike(cx, 640., 480., false);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("spike-counter").is_some(), "page 0 mounts on first render");
        assert!(window.try_find("spike-colors").is_none(), "page 1 stays unmounted while page 0 is selected");
        view.update(cx, |view, cx| {
            view.selection = 2;
            cx.notify();
        });
        window.render_frame(cx);
        window.render_frame(cx);
        assert!(window.try_find("spike-about").is_some(), "remount skips forward onto page 2");
        assert!(window.try_find("spike-counter").is_none(), "page 0 unmounts after the remount");
        view.update(cx, |view, cx| {
            view.selection = 1;
            cx.notify();
        });
        window.render_frame(cx);
        window.render_frame(cx);
        assert!(window.try_find("spike-colors").is_some(), "remount lands back on page 1");
        assert!(window.try_find("spike-about").is_none(), "page 2 unmounts after the remount");
    })
    .unwrap();
}

/// Group targeting, pinned as characterized: a fresh mount keeps the top
/// offset even with an initial group index — Kit only scrolls on
/// in-container group clicks and filter-driven selection changes. Section
/// deep links therefore land on the page until Kit honors initial groups
/// (upstream ask); change this probe when it does. The second half proves
/// Kit's own scroll path works once mounted.
#[gpui_kit::test]
fn spike_remount_scrolls_to_group(cx: &mut TestAppContext) {
    let (window, view) = open_spike(cx, 640., 480., false);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        view.update(cx, |view, cx| {
            view.selection = 1;
            view.group = None;
            cx.notify();
        });
        window.render_frame(cx);
        window.render_frame(cx);
        let before = window.try_find("spike-fonts").map(|snap| snap.visible());
        assert_ne!(before, Some(true), "fonts starts out of view: {before:?}");
        view.update(cx, |view, cx| {
            view.group = Some(1);
            cx.notify();
        });
        window.render_frame(cx);
        window.render_frame(cx);
        let after_remount = window.try_find("spike-fonts").map(|snap| snap.visible());
        assert_ne!(after_remount, Some(true), "fresh mount keeps the top offset despite group_ix=1: {after_remount:?}");
        assert!(window.try_find("0-1-1").is_some(), "the targeted group row mounts in the sidebar");
        window.click("0-1-1", cx);
        window.render_frame(cx);
        assert!(
            window.try_find("spike-fonts").is_some_and(|snap| snap.visible()),
            "in-container group click scrolls fonts into view"
        );
    })
    .unwrap();
}

/// Embedded search: first Tab reaches it, queries match titles and
/// multilingual keywords case-insensitively but accent-sensitively, an
/// empty result keeps the selection for when results return.
#[gpui_kit::test]
fn spike_search_filters_by_title_and_multilingual_keywords(cx: &mut TestAppContext) {
    let (window, view) = open_spike(cx, 640., 480., false);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        // The embedded search has no stable id, so drive it empirically:
        // seed focus explicitly (Tab-from-void goes nowhere and clicks do
        // not focus Kit buttons headless), then Tab through stops typing
        // a marker query until the filter effect shows. Typing into
        // buttons or the slider wrapper is dropped harmlessly, and the
        // clearing backspaces are no-ops there too.
        view.read(cx).slider_focus.clone().focus(window, cx);
        let mut found = None;
        let mut trace = Vec::new();
        for stop in 0..40 {
            if stop > 0 {
                window.press("tab", cx);
            }
            let focus = (
                window.try_find("spike-increment").and_then(|snap| snap.focused()),
                window.try_find("spike-slider-wrap").and_then(|snap| snap.focused()),
            );
            window.input("màu", cx);
            window.render_frame(cx);
            if window.try_find("spike-colors").is_some_and(|snap| snap.visible()) {
                found = Some(stop);
                break;
            }
            trace.push((stop, focus));
            for _ in 0.."màu".chars().count() {
                window.press("backspace", cx);
            }
            window.render_frame(cx);
        }
        assert!(found.is_some(), "typing reached the embedded search within 40 tab stops; focus trace: {trace:?}");
        assert!(
            window.try_find("spike-colors").is_some_and(|snap| snap.visible()),
            "accented keyword query finds the colors item (search stop: {found:?})"
        );
        assert!(window.try_find("spike-counter").is_none(), "page 0 leaves the view once page 1 matches");
        for _ in 0.."màu".chars().count() {
            window.press("backspace", cx);
        }
        window.render_frame(cx);
        assert!(
            window.try_find("spike-colors").is_some_and(|snap| snap.visible()),
            "clearing keeps page 1 instead of restoring page 0"
        );
        window.input("mau", cx);
        window.render_frame(cx);
        assert!(window.try_find("spike-colors").is_none(), "unaccented query must not match the accented keyword");
        for _ in 0.."mau".chars().count() {
            window.press("backspace", cx);
        }
        window.input("MÀU", cx);
        window.render_frame(cx);
        assert!(
            window.try_find("spike-colors").is_some_and(|snap| snap.visible()),
            "matching folds case but keeps accents"
        );
        for _ in 0.."MÀU".chars().count() {
            window.press("backspace", cx);
        }
        window.input("zzz-no-match", cx);
        window.render_frame(cx);
        assert!(
            window.try_find("spike-counter").is_none()
                && window.try_find("spike-colors").is_none()
                && window.try_find("spike-about").is_none(),
            "no match shows no page content"
        );
        for _ in 0.."zzz-no-match".chars().count() {
            window.press("backspace", cx);
        }
        window.render_frame(cx);
        assert!(
            window.try_find("spike-colors").is_some_and(|snap| snap.visible()),
            "the retained selection restores page 1 after clearing"
        );
    })
    .unwrap();
}

/// Sidebar rows are id-observable (`0-<page>`, `0-<page>-<group>`) and
/// switch pages on click. Role queries cannot see them: Kit puts the
/// TreeItem role on the inner unobserved element, so id queries are the
/// production-test path. Page 1 also carries the brand V mark as its
/// icon, so this click proves the custom icon mounts cleanly.
#[gpui_kit::test]
fn spike_sidebar_rows_are_observable_and_clickable(cx: &mut TestAppContext) {
    let (window, _view) = open_spike(cx, 640., 480., false);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        for row in ["0-0", "0-1", "0-2", "0-1-0", "0-1-1"] {
            assert!(window.try_find(row).is_some(), "sidebar row {row} mounts");
        }
        assert!(window.try_find("0-0-0").is_none(), "single-group pages show no group children");
        window.click("0-1", cx);
        window.render_frame(cx);
        assert!(window.try_find("spike-colors").is_some(), "clicking row 0-1 shows page 1");
        assert!(window.try_find("spike-counter").is_none(), "page 0 leaves after the row click");
    })
    .unwrap();
}

/// Element reset handlers: no button while clean, one click runs reset.
#[gpui_kit::test]
fn spike_reset_button_runs_element_reset_handler(cx: &mut TestAppContext) {
    let (window, view) = open_spike(cx, 640., 480., false);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        view.update(cx, |view, cx| {
            view.selection = 2;
            cx.notify();
        });
        window.render_frame(cx);
        window.render_frame(cx);
        assert!(window.try_find("reset").is_none(), "clean page shows no reset button");
        view.update(cx, |view, cx| {
            view.dirty = true;
            cx.notify();
        });
        window.render_frame(cx);
        window.click("reset", cx);
    })
    .unwrap();
    cx.update(|cx| {
        assert_eq!(view.read(cx).reset_count, 1, "reset handler ran once");
        assert!(!view.read(cx).dirty, "reset handler cleared the dirty flag");
    });
}

/// Window-dependent controls keep working inside Element items: focus
/// the slider wrapper, then arrow keys step the retained slider state.
#[gpui_kit::test]
fn spike_slider_keeps_keyboard_control_inside_element(cx: &mut TestAppContext) {
    let (window, view) = open_spike(cx, 640., 480., false);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("spike-slider-wrap", cx);
        let before = view.read(cx).slider.read(cx).value().start();
        window.press("right", cx);
        window.render_frame(cx);
        assert_eq!(
            view.read(cx).slider.read(cx).value().start(),
            before + 1.0,
            "right arrow steps the slider up from {before}"
        );
        window.press("left", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).slider.read(cx).value().start(), before, "left arrow steps it back down");
    })
    .unwrap();
}

/// Narrow geometry: at 700px the sidebar and the content group both fit
/// inside the viewport with a readable content width.
#[gpui_kit::test]
fn spike_narrow_window_fits_sidebar_and_content(cx: &mut TestAppContext) {
    let (window, _view) = open_spike(cx, 700., 800., false);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        // Kit's internal group/item ids are path-only (no test_support),
        // so geometry asserts use the observed nav row and own Element ids.
        let viewport = window.viewport_size();
        let row = window.find("0-0").bounds();
        let wrap = window.find("spike-counter-wrap").bounds();
        assert!(
            row.origin.x >= px(0.) && row.right() <= viewport.width,
            "nav row stays inside a 700px viewport: {row:?} vs {viewport:?}"
        );
        assert!(wrap.right() <= viewport.width, "content stays inside a 700px viewport: {wrap:?} vs {viewport:?}");
        assert!(
            row.right() <= wrap.left() + px(1.),
            "sidebar sits left of content without overlap: {row:?} vs {wrap:?}"
        );
        assert!(viewport.width - wrap.left() >= px(200.), "content keeps a readable width: {wrap:?} vs {viewport:?}");
    })
    .unwrap();
}

/// Config scale: twelve pages mount, remount reaches the last page, and
/// wheel scrolling over the sidebar must reveal the last row. Red on the
/// last leg means Kit rows clip without scroll and the config mapping
/// cannot rely on sidebar group children.
#[gpui_kit::test]
fn spike_many_pages_render_and_remain_reachable(cx: &mut TestAppContext) {
    let (window, view) = open_spike(cx, 1000., 360., true);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        for ix in 0..12 {
            let id = format!("0-{ix}");
            assert!(window.try_find(id.clone()).is_some(), "page row {id} mounts");
        }
        view.update(cx, |view, cx| {
            view.selection = 11;
            cx.notify();
        });
        window.render_frame(cx);
        window.render_frame(cx);
        assert!(window.try_find("spike-p11").is_some_and(|snap| snap.visible()), "remount reaches the last page");
        window.scroll("0-0", gpui_kit::ScrollDelta::Pixels(point(px(0.), px(-2000.))), cx);
        window.render_frame(cx);
        let last = window.try_find("0-11").map(|snap| (snap.visible(), snap.bounds()));
        assert!(last.as_ref().is_some_and(|(visible, _)| *visible), "sidebar scroll reveals the last row: {last:?}");
    })
    .unwrap();
}
