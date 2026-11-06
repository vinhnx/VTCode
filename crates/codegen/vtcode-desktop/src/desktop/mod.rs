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
mod startup;
mod strings;
mod themes;
mod transcript;
mod transport;
mod view;

pub(crate) use startup::{SharedUrlInbox, UrlInbox, parse_startup_link};
pub(crate) use view::run;

fn accessible_select_option(id: String, title: gpui_kit::SharedString) -> impl gpui_kit::IntoElement {
    use gpui_kit::{InteractiveElement, ParentElement, StatefulInteractiveElement, TestSupportExt};
    gpui_kit::div()
        .id(id)
        .test_support()
        .role(gpui_kit::Role::Label)
        .aria_label(title.clone())
        .child(title)
}
