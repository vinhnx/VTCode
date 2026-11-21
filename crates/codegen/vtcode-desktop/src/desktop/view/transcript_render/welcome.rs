//! Owned welcome-screen text and the shared greeting scaffold, extracted
//! from the transcript renderer so both empty states stay on one source.

use super::*;

/// Owned welcome-screen text shared by the default and Zen empty states.
pub(super) struct WelcomeHeader {
    pub(super) copy: WelcomeCopy,
    pub(super) greeting: String,
    pub(super) workspace_name: Option<String>,
    pub(super) prompt: String,
}

/// Welcome prelude both empty states render from; single owner so their copy cannot drift.
pub(super) fn welcome_header(view: &DesktopView) -> WelcomeHeader {
    let copy = welcome_copy(view.welcome_copy_index);
    let greeting = welcome_greeting(view.welcome_period);
    let workspace_name = view.workspace_file_name();
    let prompt = welcome_prompt(&greeting, &copy.action, &copy.workspace_action, workspace_name.as_deref());
    WelcomeHeader { copy, greeting, workspace_name, prompt }
}

/// Shared greeting scaffold for the welcome and Zen empty states: one
/// owner for the brand mark, title type, and gap, so the two greetings
/// cannot drift apart in size. Callers pass their own title content
/// (workspace link vs plain text) and wrap the scaffold in their own
/// labelled group.
pub(super) fn greeting_empty_state(
    view: &DesktopView,
    title: impl IntoElement,
    connecting: bool,
    colors: gpui_kit::component::theme::ThemeColor,
    cx: &App,
) -> impl IntoElement {
    let title = div()
        .relative()
        .w_full()
        // Let the final greeting determine the slot height, even if it wraps.
        // The hidden greeting does not paint or expose its workspace action.
        .child(div().when(connecting, |title| title.invisible()).child(title))
        .when(connecting, |title| {
            let label = rust_i18n::t!("app.workspace.starting_service").to_string();
            title.child(
                div()
                    .id("workspace-connecting")
                    .test_support()
                    .role(gpui_kit::Role::Status)
                    .aria_label(label.clone())
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(shimmer_label("workspace-connecting-title", label, colors.foreground, cx)),
            )
        });
    EmptyState::new()
        .header(
            EmptyHeader::new()
                .max_w(px(view.content_width()))
                .gap_6()
                .media(
                    EmptyMedia::new()
                        .mb_0()
                        .child(brand_mark("welcome-brand-mark", 36., colors.primary)),
                )
                .title(
                    EmptyTitle::new()
                        .w_full()
                        .text_size(gpui_kit::rems(1.75))
                        .font_weight(gpui_kit::FontWeight::NORMAL)
                        .text_color(colors.foreground)
                        .child(title),
                ),
        )
        .size_full()
}
