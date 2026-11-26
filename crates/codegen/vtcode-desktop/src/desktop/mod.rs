mod app_icon;
mod assets;
mod build_info;
mod diff_preferences;
mod files;
mod inspector;
mod localization;
mod markdown;
mod notifications;
mod output_sections;
mod output_tail;
mod preferences;
mod project_tags;
mod startup;
mod strings;
mod terminal_process;
mod terminal_program_status;
mod terminal_screen;
mod terminal_tab;
mod themes;
mod transcript;
mod transport;
mod view;

pub(crate) use startup::{SharedUrlInbox, UrlInbox, parse_startup_link};
pub(crate) use view::run;

/// Typed identity for one option row: namespace plus domain key. Keeping this
/// in one place lets callers and rendered tests name the same element without
/// rebuilding a formatted string that could drift from the component's id.
pub(crate) fn option_element_id(
    namespace: &'static str,
    key: impl Into<gpui_kit::SharedString>,
) -> gpui_kit::ElementId {
    (gpui_kit::ElementId::from(namespace), key.into()).into()
}

/// One searchable-select dropdown row: a stable, domain-keyed identity and the
/// same text for painting and assistive technology.
///
/// The id is split into a namespace plus the option's domain key rather than a
/// formatted string. A translated `title` is presentation only — switching
/// locale must keep the same element (and its keyed interaction state), so the
/// label is deliberately never part of the identity. Callers pass the option's
/// own id (never its visible index), so inserting or reordering options cannot
/// move identity either.
fn accessible_select_option(
    namespace: &'static str,
    key: impl Into<gpui_kit::SharedString>,
    title: gpui_kit::SharedString,
) -> impl gpui_kit::IntoElement {
    use gpui_kit::{InteractiveElement, ParentElement, StatefulInteractiveElement, TestSupportExt};
    gpui_kit::div()
        .id(option_element_id(namespace, key))
        .test_support()
        .role(gpui_kit::Role::Label)
        .aria_label(title.clone())
        .child(title)
}
