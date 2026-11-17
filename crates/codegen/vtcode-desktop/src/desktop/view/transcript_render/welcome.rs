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
    colors: gpui_kit::component::theme::ThemeColor,
) -> impl IntoElement {
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
                        .text_size(gpui_kit::rems(1.75))
                        .font_weight(gpui_kit::FontWeight::NORMAL)
                        .text_color(colors.foreground)
                        .child(title),
                ),
        )
        .size_full()
}
