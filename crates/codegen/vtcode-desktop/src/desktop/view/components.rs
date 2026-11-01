//! Shared stateless element builders reused by every desktop screen.
//!
//! Surface tiers: workspace shell and large overlays (palette, config editor,
//! diff review) use `rounded_xl`; sidebar and inspector panels `rounded_lg`;
//! nested cards and compact dialogs `rounded_md`; tiny inset boxes
//! `rounded_sm`; pills only for intentional pill shapes. Panel toolbar rows
//! share [`panel_header`]; short navigation labels share [`eyebrow_label`],
//! settings section headings keep sentence case via [`section_label`] and
//! [`group_box_title`] one size step above body text. Interactive controls
//! use the pointing hand
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

/// Animated status label for an owned loading state. The Kit shimmer inherits
/// typography and color from the call-site parent and renders static text
/// under reduced motion, so callers only style the row, never the motion.
/// Loop sites pass an owned domain-derived id (never a visible index), so
/// repeated rows keep stable animation identity across reordering.
pub(super) fn shimmer_label(id: impl Into<gpui_kit::ElementId>, text: impl Into<gpui_kit::SharedString>) -> AnyElement {
    let id = id.into();
    div()
        .id(id.clone())
        .test_support()
        .child(ShimmerText::new(text.into()).id(id))
        .into_any_element()
}

/// Pending assistant row pinned below the transcript scroller while a turn
/// runs with no content yet. It lives outside the measured list on purpose:
/// scroller geometry never churns when the row appears or clears. The label
/// reuses the owned turn-phase status, so it tracks
/// submitting/running/cancelling with no extra copy.
pub(super) fn turn_pending_row(status: String, content_width: f32, cx: &App) -> AnyElement {
    div()
        .id("turn-pending")
        .test_support()
        .role(gpui_kit::Role::Status)
        .aria_label(status.clone())
        .w_full()
        .flex()
        .justify_center()
        .child(
            div()
                .w_full()
                .max_w(px(content_width))
                .min_w_0()
                .flex()
                .flex_col()
                .gap_2()
                .px_3()
                .py_2()
                .text_sm()
                .text_color(cx.theme().colors.muted_foreground)
                .child(shimmer_label("turn-pending-label", status)),
        )
        .into_any_element()
}

/// Shared segmented tab treatment at the ecosystem-default medium size. Dense
/// panels chain `.small()` with a justification instead of shrinking the
/// shared default.
pub(super) fn desktop_tab_bar(id: impl Into<gpui_kit::ElementId>) -> TabBar {
    TabBar::new(id).segmented().w_full()
}

/// Sentence-case settings section heading: one rem step above body text at
/// label-grade foreground with semibold weight.
pub(super) fn section_label(
    text: impl Into<gpui_kit::SharedString>,
    colors: &gpui_kit::component::theme::ThemeColor,
) -> impl IntoElement {
    div()
        .text_base()
        .font_semibold()
        .text_color(themes::label_foreground(colors))
        .child(text.into())
}

/// Eyebrow section label: the single owner of the ALL-CAPS short-section
/// style used by the sidebar, the utilities pane, and popover group headers.
/// Callers pass natural-case text; the builder applies the casing once, so no
/// call site hand-rolls `.to_uppercase()` or ships pre-uppercased literals.
/// Eyebrows stay at body size; settings and config surfaces keep sentence-case
/// [`section_label`].
pub(super) fn eyebrow_label(
    text: impl AsRef<str>,
    colors: &gpui_kit::component::theme::ThemeColor,
) -> impl IntoElement {
    div()
        .text_sm()
        .font_semibold()
        .text_color(themes::label_foreground(colors))
        .child(gpui_kit::SharedString::from(text.as_ref().to_uppercase()))
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

/// Pre-configured bordered card for settings sections: the fill token plus the
/// soft 1px theme border, with the title above the surface. Callers chain
/// `.child(...)`/`.footer(...)` as before.
pub(super) fn settings_group_box(
    title: impl Into<gpui_kit::SharedString>,
    colors: &gpui_kit::component::theme::ThemeColor,
) -> GroupBox {
    GroupBox::new()
        .flex_shrink_0()
        .fill()
        .content_style(card_border_style(colors))
        .title(group_box_title(title, colors))
}

/// 1px theme-border outline for a filled Kit `GroupBox` surface, giving cards
/// the soft Codex-style outline. Applied through `content_style` so the title
/// and footer keep their unboxed seats above and below the surface.
pub(super) fn card_border_style(colors: &gpui_kit::component::theme::ThemeColor) -> gpui_kit::StyleRefinement {
    gpui_kit::StyleRefinement::default()
        .border_1()
        .border_color(soft_edge_color(colors))
}

/// Softened edge tone shared by card outlines and dividers: the theme
/// border at reduced opacity so boundaries read as quiet hairlines rather
/// than rules. One value for every surface — tune here, not per surface.
pub(super) fn soft_edge_color(colors: &gpui_kit::component::theme::ThemeColor) -> gpui_kit::Hsla {
    colors.border.opacity(0.55)
}

/// One settings row: title and description stacked left, a control pinned
/// right. The row wraps so the control drops below the text in narrow pages.
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
/// metadata text. Headings stay one size step above body text at primary
/// foreground with semibold weight so they read above body text instead of
/// below it.
pub(super) fn group_box_title(
    title: impl Into<gpui_kit::SharedString>,
    colors: &gpui_kit::component::theme::ThemeColor,
) -> impl IntoElement {
    div()
        .text_base()
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

/// 1px separator between settings rows, drawn in the softened edge tone
/// with breathing room so it reads as a quiet hairline rather than a rule.
pub(super) fn row_divider(colors: &gpui_kit::component::theme::ThemeColor) -> impl IntoElement {
    div()
        .w_full()
        .py_1()
        .child(div().h(px(1.)).w_full().bg(soft_edge_color(colors)))
}

/// 1px hairline between panes: vertical in side-by-side bodies, horizontal in
/// stacked ones. The parent's gap supplies the breathing room; the line owns
/// the boundary alone so adjacent regions never draw it twice. The stable id
/// keeps pane geometry testable.
pub(super) fn pane_divider(
    id: impl Into<gpui_kit::ElementId>,
    vertical: bool,
    colors: &gpui_kit::component::theme::ThemeColor,
) -> AnyElement {
    let tone = soft_edge_color(colors);
    if vertical {
        div()
            .id(id)
            .test_support()
            .w(px(1.))
            .flex_shrink_0()
            .self_stretch()
            .bg(tone)
            .into_any_element()
    } else {
        div()
            .id(id)
            .test_support()
            .h(px(1.))
            .w_full()
            .flex_shrink_0()
            .bg(tone)
            .into_any_element()
    }
}

/// Navigation group header: an eyebrow label for a sidebar-like rail. Both the
/// settings navigation and the configuration group rail are navigation chrome,
/// not content sections, so they share this instead of sentence-case section
/// labels. The inset matches small-button rails; medium-button labels start
/// one step further in, as in the utilities pane.
pub(super) fn nav_group_header(
    id: impl Into<gpui_kit::ElementId>,
    text: impl AsRef<str>,
    colors: &gpui_kit::component::theme::ThemeColor,
) -> AnyElement {
    div()
        .id(id)
        .test_support()
        .flex_shrink_0()
        .px_2()
        .child(eyebrow_label(text, colors))
        .into_any_element()
}

/// Bordered section group for diff panes: quiet 1px `border`-token
/// containment at the nested-card tier, so the file list and the diff read
/// as two grouped panes with [`row_divider`] between them. Returns `Div`
/// so callers can chain sizing.
pub(super) fn diff_pane_group(
    child: impl IntoElement,
    colors: &gpui_kit::component::theme::ThemeColor,
) -> gpui_kit::Div {
    div()
        .w_full()
        .flex()
        .flex_col()
        .min_h_0()
        .rounded_md()
        .border_1()
        .border_color(colors.border)
        .child(child)
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

impl DesktopView {
    /// Whether route pages must reserve the traffic-light zone themselves:
    /// windowed zen hides the toolbar that otherwise owns the clearance,
    /// while fullscreen hides the lights so nothing is reserved there.
    pub(super) fn route_reserves_light_clearance(&self, window: &Window) -> bool {
        self.zen_mode && !window.is_fullscreen()
    }
}

/// Route container shared by the settings, browser, and review pages:
/// centers the max-width panel in the content slot beneath the persistent
/// toolbar, which owns the traffic-light clearance. Windowed zen hides the
/// toolbar (`top_clearance`), so the page reserves the light zone itself and
/// the panel — navigation column included — starts below the lights instead
/// of floating them over its corner. Titles align to the content spine in
/// every mode; no horizontal dodge is needed once the lights clear the panel
/// vertically.
pub(super) fn route_page(top_clearance: bool, panel: impl IntoElement) -> impl IntoElement {
    div()
        .flex_1()
        .min_w_0()
        .h_full()
        .px_6()
        .pb_6()
        .when(top_clearance, |page| page.pt(px(ZEN_ROUTE_TOP_CLEARANCE)))
        .flex()
        .justify_center()
        .child(div().w_full().h_full().max_w(px(1500.)).child(panel))
}

/// Shared scrim behind every inline modal (palette, review, browser, file
/// preview, config editor, settings): the canvas at 85% so the background
/// fades while staying faintly legible. One value for all six surfaces —
/// tune here, not per surface.
pub(super) fn modal_scrim(colors: &gpui_kit::component::theme::ThemeColor) -> gpui_kit::Hsla {
    colors.background.opacity(0.85)
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
            include_bytes!("../../../../../../resources/icons/vtcode-app-dynamic-512.png").to_vec(),
        ))
    });
    img(image.clone()).w(px(size)).h(px(size)).rounded_md()
}

/// Appearance-picker preview tile for one icon variant. Images are decoded
/// once and shared; the `Dynamic` tile shows the split tile glyph.
pub(super) fn app_icon_preview(preference: AppIconPreference, size: f32) -> impl IntoElement {
    static PREVIEWS: std::sync::OnceLock<[Arc<gpui_kit::Image>; 5]> = std::sync::OnceLock::new();
    let previews = PREVIEWS.get_or_init(|| {
        AppIconPreference::ALL.map(|variant| {
            Arc::new(gpui_kit::Image::from_bytes(gpui_kit::ImageFormat::Png, app_icon::preview_bytes(variant).to_vec()))
        })
    });
    let [dynamic, light, dark, clear, tinted] = previews;
    let image = match preference {
        AppIconPreference::Dynamic | AppIconPreference::Unknown => dynamic,
        AppIconPreference::Light => light,
        AppIconPreference::Dark => dark,
        AppIconPreference::Clear => clear,
        AppIconPreference::Tinted => tinted,
    };
    img(image.clone()).w(px(size)).h(px(size)).rounded_md()
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

/// Short display name for a workspace path: the final component, or the
/// full path when it has none.
pub(super) fn workspace_short_name(path: &str) -> &str {
    Path::new(path).file_name().and_then(|name| name.to_str()).unwrap_or(path)
}

/// Display title for a session row: the archived title, or a stable
/// fallback derived from the session id when no title was persisted.
pub(super) fn session_display_title(session_id: &str, title: Option<&str>) -> String {
    title.map(str::to_owned).unwrap_or_else(|| {
        let short = session_id.chars().take(8).collect::<String>();
        rust_i18n::t!(i18n::SESSION_FALLBACK_TITLE, id = short.as_str()).to_string()
    })
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

/// Scrollable settings content column: axis-locked vertical scroll
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

/// The single zen toggle: docked in the title bar normally, floating above
/// the content in zen mode. Enter and exit share id, size, and variant; only
/// the icon paint, label, and mount point change.
pub(super) fn zen_mode_toggle(zen_mode: bool, faded: gpui_kit::Hsla, focus: &FocusHandle) -> Button {
    let label = if zen_mode {
        rust_i18n::t!("app.navigation.exit_zen").to_string()
    } else {
        rust_i18n::t!("app.navigation.enter_zen").to_string()
    };
    let icon = if zen_mode {
        Icon::new(IconName::Leaf).text_color(faded)
    } else {
        Icon::new(IconName::Leaf)
    };
    desktop_button(ZEN_MODE_TOGGLE_ELEMENT_ID)
        .icon(icon)
        .ghost()
        .small()
        .accessibility_label(label.clone())
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
    /// Selected-option treatment: pressed-state metadata plus a popover
    /// fill when selected, quiet ghost otherwise. Every call site sits on a
    /// `secondary`-equivalent surface (settings panel, group-box fill), where
    /// a `secondary` fill would be pixel-identical to the background, so the
    /// highlight uses `popover` like the settings navigation. Ghost-base
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
            .ghost()
            .when(selected, |button| button.bg(colors.popover).text_color(colors.foreground))
    }

    fn dispatches_desktop_action(self, action: impl gpui_kit::Action + 'static, focus: FocusHandle) -> Self {
        self.on_click(move |_, window, cx| focus.dispatch_action(&action, window, cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[gpui_kit::test]
    fn card_border_style_pins_a_1px_theme_border(cx: &mut gpui_kit::TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            let colors = cx.theme().colors;
            let style = card_border_style(&colors);
            assert_eq!(style.border_color, Some(soft_edge_color(&colors)));
            let widths = style.border_widths;
            for width in [widths.top, widths.left, widths.bottom, widths.right] {
                assert_eq!(width, Some(gpui_kit::AbsoluteLength::Pixels(px(1.))));
            }
        });
    }
}
