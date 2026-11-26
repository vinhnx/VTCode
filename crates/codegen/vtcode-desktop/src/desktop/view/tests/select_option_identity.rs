//! Rendered contract for the shared searchable-select option row: identity is
//! a namespace plus the option's domain key, never its (translated) label.
//! See the GPUI Kit RenderOnce guide's identity and ownership rules.

use std::cell::Cell;

use super::*;
use gpui_kit::{Context, IntoElement, ParentElement, Render, Window, div};

/// Two rows with the same visible text but different domain keys, mounted as
/// value elements the way a real `Select` mounts its option views. The title
/// is interior-mutable so the test can swap presentation without remounting.
struct SelectOptionIdentityProbe {
    title: Cell<&'static str>,
}

impl Render for SelectOptionIdentityProbe {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let title = gpui_kit::SharedString::from(self.title.get());
        div()
            .flex()
            .flex_col()
            .size_full()
            .child(crate::desktop::accessible_select_option("probe-option", "alpha", title.clone()))
            .child(crate::desktop::accessible_select_option("probe-option", "beta", title))
    }
}

fn open_probe(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<SelectOptionIdentityProbe>) {
    cx.update(gpui_kit::init);
    cx.update(|cx| {
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    point(px(0.), px(0.)),
                    size(px(640.), px(480.)),
                ))),
                ..Default::default()
            },
            cx,
            |_, cx| cx.new(|_| SelectOptionIdentityProbe { title: Cell::new("Same label") }),
        )
        .expect("open option identity probe window")
    })
}

#[gpui_kit::test]
fn select_option_identity_follows_the_domain_key_not_the_label(cx: &mut TestAppContext) {
    let (window, probe) = open_probe(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let alpha = window.find(option_element_id("probe-option", "alpha"));
        let beta = window.find(option_element_id("probe-option", "beta"));
        assert_eq!(alpha.label(), Some("Same label"));
        assert_eq!(beta.label(), Some("Same label"));
        assert_ne!(alpha.bounds().origin, beta.bounds().origin, "two domain keys must be two rows");
        assert!(
            window.try_find(option_element_id("probe-option", "Same label")).is_none(),
            "the painted label must never become the element key"
        );
    })
    .unwrap();

    // A locale switch changes only presentation. Both domain ids still resolve
    // with the new text, so keyed interaction state survives the re-render.
    probe.update(cx, |probe, _| probe.title.set("Libellé identique"));
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find(option_element_id("probe-option", "alpha")).label(), Some("Libellé identique"));
        assert_eq!(window.find(option_element_id("probe-option", "beta")).label(), Some("Libellé identique"));
    })
    .unwrap();
}
