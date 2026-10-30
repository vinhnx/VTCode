//! Shared stateless element builders reused by every desktop screen.
//!
//! Surface tiers: workspace shell and large overlays (palette, config editor,
//! diff review) use `rounded_xl`; sidebar and inspector panels `rounded_lg`;
//! nested cards and compact dialogs `rounded_md`; tiny inset boxes
//! `rounded_sm`; pills only for intentional pill shapes. Panel toolbar rows
//! share [`panel_header`]; short
//! navigation labels share [`eyebrow_label`], settings labels keep sentence
//! case via [`section_label`]. Interactive controls use the pointing hand
//! through [`desktop_button`]/[`DesktopButtonExt::desktop_disabled`]; rows
//! that act as buttons keep their own cursor. Chain `.lock_scroll_axis()`
//! before `.test_support()`: the test wrapper hides that method.

use super::*;

pub(super) fn accessible_group_box(
    id: &'static str,
    label: impl Into<gpui_kit::SharedString>,
    group: GroupBox,
) -> AnyElement {
    div()
        .id(id)
        .test_support()
        .role(gpui_kit::Role::Group)
        .aria_label(label)
        .w_full()
        .min_w_0()
        .flex_shrink_0()
        .child(group.id(format!("{id}-surface")))
        .into_any_element()
}

pub(super) fn skeleton_line(id: &'static str, width: Pixels, cx: &App) -> AnyElement {
    let placeholder = if cx.reduce_motion() {
        div()
            .w(width)
            .max_w_full()
            .h_4()
            .rounded_md()
            .bg(cx.theme().skeleton)
            .into_any_element()
    } else {
        Skeleton::new().w(width).max_w_full().h_4().rounded_md().into_any_element()
    };

    div()
        .id(id)
        .test_support()
        .w(width)
        .max_w_full()
        .h_4()
        .child(placeholder)
        .into_any_element()
}

/// Shared segmented tab treatment at the ecosystem-default medium size. Dense
/// panels chain `.small()` with a justification instead of shrinking the
/// shared default.
pub(super) fn desktop_tab_bar(id: impl Into<gpui_kit::ElementId>) -> TabBar {
    TabBar::new(id).segmented().w_full()
}

pub(super) fn section_label(
    text: impl Into<gpui_kit::SharedString>,
    colors: &gpui_kit::component::theme::ThemeColor,
) -> impl IntoElement {
    div()
        .text_xs()
        .font_semibold()
        .text_color(themes::label_foreground(colors))
        .child(text.into())
}

/// Eyebrow section label: the single owner of the ALL-CAPS short-section
/// style used by the sidebar, the utilities pane, and popover group headers.
/// Callers pass natural-case text; the builder applies the casing once, so no
/// call site hand-rolls `.to_uppercase()` or ships pre-uppercased literals.
/// Settings and config surfaces keep sentence-case [`section_label`].
pub(super) fn eyebrow_label(
    text: impl AsRef<str>,
    colors: &gpui_kit::component::theme::ThemeColor,
) -> impl IntoElement {
    section_label(text.as_ref().to_uppercase(), colors)
}

/// One panel toolbar row: leading content pinned left, trailing actions
/// pinned right, on the shared panel content inset. Inspector tabs and review
/// surfaces share this instead of hand-rolling
/// `flex justify_between gap_2 px_3 py_2` headers. Rows inside an already
/// padded group box keep their own unpadded flex row.
pub(super) fn panel_header(leading: impl IntoElement, trailing: impl IntoElement) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_2()
        .px_3()
        .py_2()
        .child(leading)
        .child(trailing)
}

/// Pre-configured fill group box for settings sections. Callers chain
/// `.child(...)`/`.footer(...)` as before. The local-service group keeps its
/// bespoke `.outline()` chain; it is the only visible variant.
pub(super) fn settings_group_box(
    title: impl Into<gpui_kit::SharedString>,
    colors: &gpui_kit::component::theme::ThemeColor,
) -> GroupBox {
    GroupBox::new().flex_shrink_0().fill().title(group_box_title(title, colors))
}

/// One settings row: title and description stacked left, a control pinned
/// right. The row wraps so the control drops below the text in narrow sheets.
pub(super) fn settings_row(
    title: impl Into<gpui_kit::SharedString>,
    description: impl Into<gpui_kit::SharedString>,
    control: impl IntoElement,
    colors: &gpui_kit::component::theme::ThemeColor,
) -> impl IntoElement {
    div()
        .flex()
        .flex_wrap()
        .items_center()
        .justify_between()
        .gap_3()
        .child(
            div()
                .min_w_0()
                .flex_1()
                .child(row_title(title, colors))
                .child(row_description(description, colors)),
        )
        .child(control)
}

/// Section headings for Kit group boxes, whose default title renders in muted
/// metadata text. Headings stay at primary foreground with semibold weight so
/// they read above body text instead of below it.
pub(super) fn group_box_title(
    title: impl Into<gpui_kit::SharedString>,
    colors: &gpui_kit::component::theme::ThemeColor,
) -> impl IntoElement {
    div()
        .text_sm()
        .font_semibold()
        .text_color(colors.foreground)
        .child(title.into())
}

/// Settings-row title: one step below group headings, medium weight at
/// primary foreground so it reads above its description.
pub(super) fn row_title(
    text: impl Into<gpui_kit::SharedString>,
    colors: &gpui_kit::component::theme::ThemeColor,
) -> impl IntoElement {
    div().text_sm().font_medium().text_color(colors.foreground).child(text.into())
}

/// Settings-row description: same size as the title, set in the brighter
/// secondary ramp so small muted copy stays readable on every fill.
pub(super) fn row_description(
    text: impl Into<gpui_kit::SharedString>,
    colors: &gpui_kit::component::theme::ThemeColor,
) -> impl IntoElement {
    div().text_sm().text_color(themes::muted_text(colors)).child(text.into())
}

/// 1px separator between settings rows, drawn in the theme border token with
/// breathing room so it reads as a quiet hairline rather than a rule.
pub(super) fn row_divider(colors: &gpui_kit::component::theme::ThemeColor) -> impl IntoElement {
    div().w_full().py_1().child(div().h(px(1.)).w_full().bg(colors.border))
}

/// Trailing settings control pairing a right-aligned label with a switch.
///
/// The fixed-width lane keeps every row's toggle on one trailing spine while
/// the labels right-align against it, so rows with different label lengths
/// still scan as one column. The lane never exceeds the row width; longer
/// labels wrap instead of crowding the toggle. The switch keeps its accessible
/// name and owns activation; the visible label is descriptive text.
pub(super) fn settings_switch(
    id: impl Into<gpui_kit::ElementId>,
    label: impl Into<gpui_kit::SharedString>,
    checked: bool,
    on_click: impl Fn(&bool, &mut Window, &mut App) + 'static,
    colors: &gpui_kit::component::theme::ThemeColor,
) -> impl IntoElement {
    let label = label.into();
    div()
        .w_64()
        .max_w_full()
        .flex()
        .items_center()
        .justify_end()
        .gap_2()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_right()
                .text_sm()
                .text_color(colors.foreground)
                .child(label.clone()),
        )
        .child(
            gpui_kit::component::switch::Switch::new(id)
                .flex_shrink_0()
                .checked(checked)
                .accessibility_label(label)
                .on_click(on_click),
        )
}

/// Quiet full-width break between conversation turns on the canvas. The
/// single-sided rhythm keeps the row compact: preceding rows already carry
/// bottom padding, and user bubbles must start within 16px of their column.
pub(super) fn turn_divider(colors: &gpui_kit::component::theme::ThemeColor) -> impl IntoElement {
    div()
        .w_full()
        .py_1()
        .child(div().h(px(1.)).bg(themes::hairline(colors.background, colors.foreground)))
}

pub(super) fn navigation_label(
    label: String,
    icon: IconName,
    selected: bool,
    accent: gpui_kit::Hsla,
) -> impl IntoElement {
    div()
        .w_full()
        .min_w_0()
        .flex()
        .items_center()
        .gap_2()
        .child(Icon::new(icon).size_4().when(selected, |icon| icon.text_color(accent)))
        .child(div().flex_1().min_w_0().truncate().text_left().child(label))
        // The status lane is always reserved; selection only repaints it, so
        // the label truncation point never moves between rows.
        .child(
            div()
                .size(px(5.))
                .rounded_full()
                .bg(if selected { accent } else { accent.opacity(0.) }),
        )
}

/// Embed the shared artwork so packaged applications never depend on checkout paths.
pub(super) fn brand_logo(size: f32) -> impl IntoElement {
    static LOGO: std::sync::OnceLock<Arc<gpui_kit::Image>> = std::sync::OnceLock::new();
    let image = LOGO.get_or_init(|| {
        Arc::new(gpui_kit::Image::from_bytes(
            gpui_kit::ImageFormat::Png,
            include_bytes!("../../../../../../resources/icons/vtcode-profile-120.png").to_vec(),
        ))
    });
    gpui_kit::img(image.clone()).w(px(size)).h(px(size)).rounded_md()
}

pub(super) fn welcome_brand_mark(size: f32, color: gpui_kit::Hsla) -> impl IntoElement {
    gpui_kit::svg()
        .data(include_bytes!("../../../../../../resources/icons/vtcode-mark.svg"))
        .size(px(size))
        .text_color(color)
        .id("welcome-brand-mark")
        .test_support()
}

pub(in super::super) fn warning_alert(
    id: &'static str,
    message: impl Into<gpui_kit::SharedString>,
    colors: &gpui_kit::component::theme::ThemeColor,
) -> AnyElement {
    status_alert(id, message, colors, colors.warning)
}

pub(super) fn error_alert(
    id: &'static str,
    message: impl Into<gpui_kit::SharedString>,
    colors: &gpui_kit::component::theme::ThemeColor,
) -> AnyElement {
    status_alert(id, message, colors, colors.danger)
}

fn status_alert(
    id: &'static str,
    message: impl Into<gpui_kit::SharedString>,
    colors: &gpui_kit::component::theme::ThemeColor,
    accent: gpui_kit::Hsla,
) -> AnyElement {
    let message = message.into();
    div()
        .id(id)
        .test_support()
        .role(gpui_kit::Role::Alert)
        .aria_label(message.clone())
        .w_full()
        .px_3()
        .py_2()
        .rounded_md()
        .border_1()
        .border_color(accent)
        .bg(colors.secondary)
        .text_sm()
        .text_color(colors.secondary_foreground)
        .child(message)
        .into_any_element()
}

pub(super) fn empty_state(
    title: impl Into<gpui_kit::SharedString>,
    description: impl Into<gpui_kit::SharedString>,
    icon: IconName,
) -> EmptyState {
    let title = title.into();
    EmptyState::new().header(
        EmptyHeader::new()
            .media(EmptyMedia::new().child(if matches!(icon, IconName::Bot) {
                brand_logo(40.).into_any_element()
            } else {
                Icon::new(icon).size_6().into_any_element()
            }))
            .title(EmptyTitle::new().text_lg().font_semibold().child(title))
            .description(EmptyDescription::new().child(description.into())),
    )
}

/// Page-level empty state centered in the remaining space. Returns the `Div`
/// so callers can chain trailing content (a trailing action also needs
/// `.gap_3()`); the base carries no gap. Status-role empty states with their
/// own accessible wiring (inspector, workspace review) stay bespoke.
pub(super) fn centered_empty_state(
    title: impl Into<gpui_kit::SharedString>,
    description: impl Into<gpui_kit::SharedString>,
    icon: IconName,
) -> gpui_kit::Div {
    div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .child(empty_state(title, description, icon))
}

/// Usable height for a settings-sheet page: the viewport minus the sheet
/// chrome and the font-scaled title row, with 8px beneath it so only the
/// content region scrolls.
pub(super) fn sheet_panel_height(window: &Window, cx: &App) -> Pixels {
    let sheet_header_height = cx.theme().font_size * 2.5;
    (window.viewport_size().height - cx.theme().sheet.margin_top - sheet_header_height - px(8.)).max(px(0.))
}

/// Scrollable settings-sheet content column: axis-locked vertical scroll
/// with a themed scrollbar.
pub(super) fn scroll_column(id: &'static str, scroll: &ScrollHandle, child: impl IntoElement) -> impl IntoElement {
    scroll_column_inner(id, scroll, child, false)
}

/// Scrollable content column whose children grow (flex column): used where
/// `flex_1` children such as centered empty states must fill the height.
pub(super) fn scroll_column_flex(id: &'static str, scroll: &ScrollHandle, child: impl IntoElement) -> impl IntoElement {
    scroll_column_inner(id, scroll, child, true)
}

fn scroll_column_inner(
    id: &'static str,
    scroll: &ScrollHandle,
    child: impl IntoElement,
    flex: bool,
) -> impl IntoElement {
    div()
        .id(id)
        .flex_1()
        .min_h_0()
        .min_w_0()
        .lock_scroll_axis()
        .test_support()
        .when(flex, |column| column.flex().flex_col())
        .track_scroll(scroll)
        .overflow_y_scroll()
        .vertical_scrollbar(scroll)
        .pr_2()
        .child(child)
}

/// Interactive desktop buttons use a pointing hand; disabled buttons keep the arrow.
pub(super) fn desktop_button(id: impl Into<gpui_kit::ElementId>) -> Button {
    Button::new(id).tab_stop(true).cursor_pointer()
}

/// The single zen toggle: one title-bar trailing slot shared by both modes.
/// Enter and exit share id, size, variant, and position; only the icon paint
/// and label change, so the button never shifts when toggled.
pub(super) fn zen_mode_toggle(zen_mode: bool, faded: gpui_kit::Hsla, focus: &FocusHandle) -> Button {
    let label = if zen_mode { "Exit Zen mode" } else { "Enter Zen mode" };
    let icon = if zen_mode {
        Icon::new(IconName::Leaf).text_color(faded)
    } else {
        Icon::new(IconName::Leaf)
    };
    desktop_button(ZEN_MODE_TOGGLE_ELEMENT_ID)
        .icon(icon)
        .ghost()
        .small()
        .accessibility_label(label)
        .tooltip_with_action(label, &ToggleZenMode, Some(key_context::DESKTOP))
        .dispatches_desktop_action(ToggleZenMode, focus.clone())
}

/// Opacity transition for elements appearing with a zen toggle, using the
/// theme duration and enter curve. Pair it with a stable animation id per
/// mount point; state-key the id when the element persists across modes so
/// each toggle replays the fade. `with_animation` renders the end state
/// statically under reduced motion, so no manual gate is needed.
pub(super) fn zen_fade_animation(theme: &Theme) -> Animation {
    let motion = theme.motion_tokens();
    let easing = motion.easing_enter.clone();
    Animation::new(motion.duration_normal).with_easing(move |delta| easing.sample(delta))
}

pub(super) trait DesktopButtonExt {
    fn desktop_disabled(self, disabled: bool) -> Self;
    fn dispatches_desktop_action(self, action: impl gpui_kit::Action + 'static, focus: FocusHandle) -> Self;
    /// Selected-option treatment: pressed-state metadata plus a secondary
    /// fill when selected, quiet ghost otherwise. Only for options whose
    /// selected paint is the default base + secondary fill; ghost-base
    /// (theme profiles, diff layout) and label-only (reduced motion)
    /// selections keep their bespoke chains.
    fn desktop_selected(self, selected: bool, colors: &gpui_kit::component::theme::ThemeColor) -> Self;
}

impl DesktopButtonExt for Button {
    fn desktop_disabled(self, disabled: bool) -> Self {
        self.disabled(disabled).cursor(if disabled {
            CursorStyle::Arrow
        } else {
            CursorStyle::PointingHand
        })
    }

    fn desktop_selected(self, selected: bool, colors: &gpui_kit::component::theme::ThemeColor) -> Self {
        self.toggled(selected)
            .when(selected, |button| button.bg(colors.secondary).text_color(colors.foreground))
            .when(!selected, |button| button.ghost())
    }

    fn dispatches_desktop_action(self, action: impl gpui_kit::Action + 'static, focus: FocusHandle) -> Self {
        self.on_click(move |_, window, cx| focus.dispatch_action(&action, window, cx))
    }
}
