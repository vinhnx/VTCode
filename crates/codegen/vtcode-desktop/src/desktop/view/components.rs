//! Shared stateless element builders reused by every desktop screen.
//!
//! Surface tiers from the shared theme baseline (`md` 8px, `lg` 12px): workspace
//! shell and large overlays `rounded_xl`; sidebar/inspector `rounded_lg`;
//! nested cards/dialogs `rounded_md`; tiny insets `rounded_sm`; pills only
//! for intentional pill shapes. Panel toolbar rows
//! share [`panel_header`]; short navigation labels share [`eyebrow_label`],
//! settings section headings keep sentence case via [`section_label`] and
//! [`group_box_title`] one size step above body text. Interactive controls
//! use the pointing hand
//! through the button baseline in [`buttons`]/[`DesktopButtonExt::desktop_disabled`]; rows
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

/// Inline error text for errors that arrive after a request. The danger color
/// is only the visual cue; the same string is the `Alert` node's accessible
/// name, so assistive technology can discover the failure and recover without
/// relying on hue or on a neighboring label. One owner with the shared error
/// size prevents a new error site from shipping color-only.
pub(super) fn accessible_error(
    id: impl Into<gpui_kit::ElementId>,
    message: impl Into<gpui_kit::SharedString>,
    colors: &gpui_kit::component::theme::ThemeColor,
) -> AnyElement {
    let message = message.into();
    div()
        .id(id)
        .test_support()
        .role(gpui_kit::Role::Alert)
        .aria_label(message.clone())
        .text_sm()
        .text_color(colors.danger)
        .child(message)
        .into_any_element()
}

/// The `(resting, highlight)` pair every shimmer label uses.
///
/// One owner so a label cannot pick a tone in one place and let Kit default the
/// band in another: the two policies used to disagree, which painted a sweep
/// darker than the text it crossed on palettes where muted and foreground are
/// close (Ciapre: 0.097 apart). Under reduced motion both halves collapse to
/// `tone`, because the static label must stay at full readable contrast.
fn shimmer_tones(tone: gpui_kit::Hsla, cx: &App) -> (gpui_kit::Hsla, gpui_kit::Hsla) {
    if cx.reduce_motion() {
        return (tone, tone);
    }
    let colors = cx.theme().colors;
    (themes::shimmer_resting(&colors, tone), colors.foreground)
}

/// Animated status label for an owned loading state. Typography comes from the
/// call-site parent; this builder owns the color pair (see [`shimmer_tones`])
/// AND the reduced-motion fallback, so callers pass their semantic tone and
/// never style the motion. Loop sites pass an owned domain-derived id (never a
/// visible index), so repeated rows keep stable animation identity across
/// reordering.
pub(super) fn shimmer_label(
    id: impl Into<gpui_kit::ElementId>,
    text: impl Into<gpui_kit::SharedString>,
    tone: gpui_kit::Hsla,
    cx: &App,
) -> AnyElement {
    let id = id.into();
    let (resting, highlight) = shimmer_tones(tone, cx);
    div()
        .id(id.clone())
        .test_support()
        .child(
            ShimmerText::new(text.into())
                .id(id)
                .text_color(resting)
                .highlight_color(highlight),
        )
        .into_any_element()
}

/// Full-width loading row for an owned loading state: a shimmer marker
/// inside the caller's test identity wrapper (`Marker` cannot carry
/// `.test_support()` itself). Same color contract as [`shimmer_label`]. Only
/// full rows come here; inline embeddings, shrink-wrapped centered labels, and
/// one-off typography (e.g. `text_xs` rows) keep `shimmer_label`.
pub(super) fn loading_marker_row(
    id: impl Into<gpui_kit::ElementId>,
    text: impl Into<gpui_kit::SharedString>,
    tone: gpui_kit::Hsla,
    cx: &App,
) -> AnyElement {
    loading_marker_row_inner(id, text, tone, None, cx)
}

/// Full-width loading row with an explicit leading spinner, for rows
/// that pair a spinner with shimmer text. The caller owns the spinner's
/// exact size and color; the text tone is the row tone.
pub(super) fn loading_marker_row_with_spinner(
    id: impl Into<gpui_kit::ElementId>,
    text: impl Into<gpui_kit::SharedString>,
    spinner: Spinner,
    tone: gpui_kit::Hsla,
    cx: &App,
) -> AnyElement {
    loading_marker_row_inner(id, text, tone, Some((spinner, tone)), cx)
}

fn loading_marker_row_inner(
    id: impl Into<gpui_kit::ElementId>,
    text: impl Into<gpui_kit::SharedString>,
    tone: gpui_kit::Hsla,
    leading: Option<(Spinner, gpui_kit::Hsla)>,
    cx: &App,
) -> AnyElement {
    let (resting, highlight) = shimmer_tones(tone, cx);
    // `Marker` builds its own `ShimmerText`, so the band arrives through the
    // shimmer style and the glyph tone through `MarkerContent` (which is
    // `Styled`). Setting either on the outer marker alone would be dropped.
    let mut marker = Marker::new()
        .loading(true)
        .with_loading_style(MarkerLoadingStyle::Shimmer)
        .with_shimmer_style(ShimmerStyle::new().highlight_color(highlight));
    if let Some((spinner, spinner_tone)) = leading {
        marker = marker.icon(MarkerIcon::new().child(spinner)).text_color(spinner_tone);
    }
    div()
        .id(id)
        .test_support()
        .child(marker.content(MarkerContent::new().text(text).text_color(resting)))
        .into_any_element()
}

/// Turn activity shimmer pinned below the transcript scroller while a turn
/// runs; idle hides it. It lives outside the measured list on purpose:
/// scroller geometry never churns when the row appears or clears. The label
/// reuses the owned turn status, which tracks the newest started item
/// (`turn_activity_status`) on top of the submitting/running/cancelling
/// phases, so the thread names what the turn is doing until it completes.
/// The edge inset lives on the outer wrapper (mirroring the scroller's row
/// wrapper), so the label text starts on the transcript spine instead of
/// one step inside it.
pub(super) fn turn_pending_row(status: String, content_width: f32, cx: &App) -> AnyElement {
    div()
        .id("turn-pending")
        .test_support()
        .role(gpui_kit::Role::Status)
        .aria_label(status.clone())
        .w_full()
        .flex()
        .justify_center()
        .px_3()
        .child(
            div()
                .w_full()
                .max_w(px(content_width))
                .min_w_0()
                .flex()
                .flex_col()
                .gap_2()
                .py_2()
                .text_sm()
                .text_color(cx.theme().colors.muted_foreground)
                .child(loading_marker_row("turn-pending-label", status, cx.theme().colors.muted_foreground, cx)),
        )
        .into_any_element()
}

/// Label tiers, largest first: page `2xl`, panel `lg`, section/group `base`,
/// row `sm`, eyebrow/help `xs` (see `docs/development/desktop-workspace.md`).
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

/// Content page title: the largest interface label, naming the visible
/// settings page. Rendered once as the first child of the scroll content so
/// scroll geometry is unchanged; the panel header keeps its own smaller tier.
pub(super) fn content_page_title(
    text: impl Into<gpui_kit::SharedString>,
    colors: &gpui_kit::component::theme::ThemeColor,
) -> impl IntoElement {
    div()
        .text_2xl()
        .font_semibold()
        .text_color(colors.foreground)
        .child(text.into())
}

/// Panel / dialog header title: one step below the page title and above
/// section headings, so the chrome never competes with the page it contains.
pub(super) fn panel_title(
    text: impl Into<gpui_kit::SharedString>,
    colors: &gpui_kit::component::theme::ThemeColor,
) -> impl IntoElement {
    div().text_lg().font_semibold().text_color(colors.foreground).child(text.into())
}

/// Eyebrow section label: owns the ALL-CAPS style for short sidebar,
/// utilities, and popover headers. Callers pass natural case; casing is applied
/// once here. Uppercasing is opaque to most scripts, so localized text keeps
/// its own casing; eyebrows share the micro tier with help text.
pub(super) fn eyebrow_label(
    text: impl AsRef<str>,
    colors: &gpui_kit::component::theme::ThemeColor,
) -> impl IntoElement {
    div()
        .text_xs()
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
///
/// Padding steps up to `p_6` on both axes: the fill matches the panel
/// background, so the hairline alone must carry the card gestalt, and the
/// even band reads as one concentric shape. The boundary (`xl`) stays
/// airier than the gaps between rows (`lg`). Rows separate by the inherited
/// Kit `gap_4` air (the Kit Settings rhythm): do not interleave
/// [`row_divider`] between settings rows.
pub(super) fn settings_group_box(
    title: impl Into<gpui_kit::SharedString>,
    colors: &gpui_kit::component::theme::ThemeColor,
) -> GroupBox {
    settings_group_box_untitled(colors).title(group_box_title(title, colors))
}

/// Untitled variant of [`settings_group_box`] for settings cards whose page
/// header already names the surface (e.g. the curated configuration
/// overview): same fill, border, and `p_6` rhythm, no title row.
pub(super) fn settings_group_box_untitled(colors: &gpui_kit::component::theme::ThemeColor) -> GroupBox {
    GroupBox::new()
        .flex_shrink_0()
        .fill()
        .content_style(card_border_style(colors).p_6())
}

/// 1px theme-border outline for a filled Kit `GroupBox` surface, giving cards
/// the soft Codex-style outline. Applied through `content_style` so the title
/// and footer keep their unboxed seats above and below the surface.
pub(super) fn card_border_style(colors: &gpui_kit::component::theme::ThemeColor) -> gpui_kit::StyleRefinement {
    gpui_kit::StyleRefinement::default()
        .border_1()
        .border_color(themes::soft_edge_color(colors))
}

/// One settings row: title and description stacked left, a control pinned
/// right and top-aligned to the title frame (never centered on the help
/// text). The row wraps so the control drops below the text, still
/// start-aligned, in narrow pages.
pub(super) fn settings_row(
    title: impl Into<gpui_kit::SharedString>,
    description: impl Into<gpui_kit::SharedString>,
    control: impl IntoElement,
    colors: &gpui_kit::component::theme::ThemeColor,
) -> impl IntoElement {
    let description = description.into();
    div()
        .flex()
        .flex_wrap()
        .items_start()
        .justify_between()
        .gap_3()
        .child(
            div()
                .min_w_0()
                .flex_1()
                .flex()
                .flex_col()
                .gap_1()
                .child(row_title(title, colors))
                .when(!description.is_empty(), |text| text.child(row_description(description, colors))),
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

/// Filled status pill for one short scannable state: primary fill when
/// highlighted, secondary otherwise. Shared by the composer
/// processing-tier badge and the Computer Use settings title so the two
/// cannot drift apart. The leading glyph is optional: the tier badge
/// carries its own icon, while the settings title already shows the
/// Computer glyph beside the label.
pub(super) fn status_pill(
    id: &'static str,
    accessible_label: String,
    label: String,
    highlighted: bool,
    icon: Option<IconName>,
    colors: &gpui_kit::component::theme::ThemeColor,
) -> AnyElement {
    div()
        .id(id)
        .test_support()
        .role(gpui_kit::Role::Label)
        .aria_label(accessible_label)
        .flex()
        .flex_shrink_0()
        .items_center()
        .gap_1()
        .px_2()
        .py(px(2.))
        .rounded_full()
        .bg(if highlighted { colors.primary } else { colors.secondary })
        .text_color(if highlighted {
            colors.primary_foreground
        } else {
            colors.foreground
        })
        .text_xs()
        .font_semibold()
        .when_some(icon, |pill, icon| pill.child(Icon::new(icon).size_3()))
        .child(label)
        .into_any_element()
}

/// Settings-row title: one step below group headings, semibold at primary
/// foreground so it reads as the field's label rather than its help text.
/// Weight and size carry the tier, so the description can keep its bright
/// secondary tone without the two lines collapsing into one.
pub(super) fn row_title(
    text: impl Into<gpui_kit::SharedString>,
    colors: &gpui_kit::component::theme::ThemeColor,
) -> impl IntoElement {
    div().text_sm().font_semibold().text_color(colors.foreground).child(text.into())
}

/// Settings-row description: one size below the title, set in the brighter
/// secondary ramp so small muted copy stays readable on every fill. The size
/// step keeps help text from masquerading as the label on the same line.
pub(super) fn row_description(
    text: impl Into<gpui_kit::SharedString>,
    colors: &gpui_kit::component::theme::ThemeColor,
) -> impl IntoElement {
    div().text_xs().text_color(themes::muted_text(colors)).child(text.into())
}

/// 1px separator for non-settings surfaces (navigation chrome, inspector
/// lists), drawn in the softened edge tone with breathing room so it reads
/// as a quiet hairline rather than a rule. Settings groups separate rows by
/// `gap_4` air instead (see [`settings_group_box`]).
pub(super) fn row_divider(colors: &gpui_kit::component::theme::ThemeColor) -> impl IntoElement {
    div()
        .w_full()
        .py_1()
        .child(div().h(px(1.)).w_full().bg(themes::soft_edge_color(colors)))
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
    let tone = themes::soft_edge_color(colors);
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

/// Inspector resize-handle paint: the inspector is a separated card floating
/// on the chrome, so a resting hairline would read as a panel border. An idle
/// handle paints nothing while keeping the kit's 1px geometry, so layout
/// never shifts between states; any pointer engagement delegates to the kit
/// divider, keeping the hover/press/drag pill that advertises the resize
/// affordance. The band, cursor, drag, splitter role, and keyboard handling
/// stay with the handle itself.
pub(super) fn inspector_resize_appearance() -> ResizeHandleRenderer {
    let kit = resize_handle_appearance();
    Rc::new(move |handle: &ResizeHandleContext, window: &mut Window, cx: &mut App| {
        if handle.state() == ResizeHandleState::Idle {
            let idle = div().flex_none().flex().map(|line| match handle.axis() {
                Axis::Horizontal => line.w(px(1.)).h_full(),
                _ => line.h(px(1.)).w_full(),
            });
            Some(idle.into_any_element())
        } else {
            kit(handle, window, cx)
        }
    })
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

/// Rail glyph size shared by Lucide catalog icons and the brand mark.
///
/// One font-relative square (`1rem`, 16px at the default 16px UI font) — the
/// same baseline the app's navigation rows use (`size_4`) — that grows with
/// the UI font size setting, unlike Lucide's fixed `small` tier. Both the
/// Lucide names and the brand SVG render through it, so the settings
/// navigation and the configuration rail keep one glyph box per row and the
/// rows never drift apart as the font size changes.
pub(super) fn nav_entry_icon_size() -> gpui_kit::AbsoluteLength {
    gpui_kit::AbsoluteLength::Rems(gpui_kit::rems(1.0))
}

/// Navigation entry content: an icon pinned to a truncating label on one
/// spine. Shared by the settings navigation and the configuration group
/// rail (Kit `Button` centers its built-in icon/label pair, so rails use
/// this custom child instead). The icon is decorative; the owning button
/// carries the accessible name. The leading mark is any `Icon`, so a Lucide
/// `IconName` and brand SVG bytes share one size/color path and align; both
/// render through [`nav_entry_icon_size`] so a row never mixes glyph sizes.
pub(super) fn nav_entry_content(
    label_id: impl Into<gpui_kit::ElementId>,
    icon: impl Into<Icon>,
    icon_id: impl Into<gpui_kit::ElementId>,
    label: String,
) -> AnyElement {
    div()
        .w_full()
        .min_w_0()
        .flex()
        .items_center()
        .gap_2()
        .child(
            // `Icon` is `RenderOnce` with no stable id, so the sized glyph sits
            // in an identified wrapper: rail alignment tests measure one box
            // for the brand V and every Lucide entry.
            div()
                .id(icon_id)
                .test_support()
                .flex_shrink_0()
                .child(Icon::new(icon).size(nav_entry_icon_size())),
        )
        .child(div().id(label_id).test_support().flex_1().min_w_0().truncate().child(label))
        .into_any_element()
}

/// Brand V mark shared by the welcome empty states and the settings
/// navigation. Uses the monochrome V artwork (`vtcode-mono.svg`); GPUI paints
/// SVGs as tinted monochrome sprites, so the mark follows the surrounding text
/// color in every theme.
const VTCODE_NAV_MARK_SVG: &[u8] = include_bytes!("../../../../../../resources/icons/vtcode-mono.svg");

/// Settings-nav leading mark for the VT Code entry: the brand V at the same
/// [`nav_entry_icon_size`] and inherited color as the Lucide sibling rows.
pub(super) fn vtcode_nav_mark() -> Icon {
    Icon::default().data(VTCODE_NAV_MARK_SVG)
}

/// Icon choice for places that mix catalog glyphs with the VT Code brand V.
/// Keeping the choice typed lets mapping tables stay comparable in tests while
/// the brand mark remains raw SVG data (an `IconName` cannot carry it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum UiIcon {
    Named(IconName),
    BrandV,
}

impl UiIcon {
    pub(super) fn into_icon(self) -> Icon {
        match self {
            Self::Named(name) => Icon::new(name),
            Self::BrandV => vtcode_nav_mark(),
        }
    }
}

impl From<IconName> for UiIcon {
    fn from(icon: IconName) -> Self {
        Self::Named(icon)
    }
}

impl From<UiIcon> for Icon {
    fn from(icon: UiIcon) -> Self {
        icon.into_icon()
    }
}

/// Shared inspector tab container: every inspector tab (Changes, Files,
/// Activity, Output, Utilities) fills the card below the tab rail with the
/// same flex geometry. Returns `Div` so callers chain their stable id,
/// scrolling, padding, and roles: scrolled regions keep edge gutters while
/// headers keep the shared `px_3` spine, so insets stay tab-owned and the
/// card never double-pads its content.
pub(super) fn inspector_tab_body() -> gpui_kit::Div {
    div().w_full().flex_1().min_h_0().flex().flex_col()
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
        .border_color(themes::soft_edge_color(colors))
        .child(child)
}

/// Trailing settings toggle: the row title already names the setting, so the
/// control renders the switch alone and keeps the title as its accessible
/// name (Kit Settings parity: fields inherit their setting's title). The
/// row's trailing pin keeps every toggle on one spine with no label lane.
pub(super) fn settings_switch(
    id: impl Into<gpui_kit::ElementId>,
    label: impl Into<gpui_kit::SharedString>,
    checked: bool,
    on_click: impl Fn(&bool, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    gpui_kit::component::switch::Switch::new(id)
        .flex_shrink_0()
        .checked(checked)
        .accessibility_label(label.into())
        .cursor_pointer()
        .on_click(on_click)
}

mod buttons;
mod route_page;
mod segmented;

pub(in crate::desktop) use buttons::desktop_ghost_button;
pub(super) use buttons::{
    DesktopButtonExt, desktop_danger_button, desktop_dismiss_button, desktop_primary_button, desktop_secondary_button,
    desktop_trigger_pill, zen_mode_toggle,
};
pub(super) use route_page::*;
pub(super) use segmented::*;

/// Shared scrim behind every inline modal (palette, review, browser, file
/// preview, config editor, settings): the canvas at 85% so the background
/// fades while staying faintly legible. One value for all six surfaces —
/// tune here, not per surface.
pub(super) fn modal_scrim(colors: &gpui_kit::component::theme::ThemeColor) -> gpui_kit::Hsla {
    colors.background.opacity(0.85)
}

/// Warning title row shared by the consent modals: an alert icon plus a
/// large semibold title naming the decision. One shape for every consent
/// surface — tune here, not per modal.
pub(super) fn consent_title_row(
    title: impl Into<gpui_kit::SharedString>,
    colors: &gpui_kit::component::theme::ThemeColor,
) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap_3()
        .child(Icon::new(IconName::TriangleAlert).text_color(colors.warning))
        .child(panel_title(title, colors))
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

/// Full-color app-icon art for the Bot empty-state media. The About surfaces
/// deliberately use [`brand_mark`] instead: identity is an ink mark, and the
/// Display art (not the 512px Dock art) stays smooth at small sizes.
pub(super) fn brand_logo(size: f32) -> impl IntoElement {
    static LOGO: std::sync::OnceLock<Arc<gpui_kit::Image>> = std::sync::OnceLock::new();
    let image = LOGO.get_or_init(|| {
        Arc::new(gpui_kit::Image::from_bytes(
            gpui_kit::ImageFormat::Png,
            app_icon::preview_bytes(AppIconPreference::Dynamic).to_vec(),
        ))
    });
    img(image.clone()).w(px(size)).h(px(size))
}

/// Appearance-picker preview tile for one icon variant. Images are decoded
/// once and shared; the `Dynamic` tile shows the split tile glyph. The art
/// is display-sized 128px so the 64px tiles render smooth edges (see
/// [`app_icon::preview_bytes`]).
///
/// No corner clip: the art already carries the macOS squircle with
/// transparency baked in, and `rounded_md` scales with the UI font
/// (`0.375rem`) while the tile inset is fixed — past ~17px UI fonts the
/// circular clip would cut into the squircle and kink the corners.
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
    img(image.clone()).w(px(size)).h(px(size))
}

/// Monochrome brand V tinted to `color`: GPUI tints SVG sprites, so one asset
/// serves every theme (unlike the Dock art in [`brand_logo`]).
pub(super) fn brand_mark(id: impl Into<gpui_kit::ElementId>, size: f32, color: gpui_kit::Hsla) -> impl IntoElement {
    gpui_kit::svg()
        .data(VTCODE_NAV_MARK_SVG)
        .size(px(size))
        .text_color(color)
        .id(id)
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

/// Severity-resolved presentation for a status marker row: tone, marker shape,
/// accessibility role, and the stable status token used in the details toggle
/// label. Single dispatch for every severity-driven choice, so callers resolve
/// once and render from the result. The callout has no icon; a solid/outlined
/// marker keeps severity distinct for color-blind readers alongside the title
/// text and role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SeverityMarkerStyle {
    pub(super) tone: gpui_kit::Hsla,
    pub(super) marker_outlined: bool,
    pub(super) role: gpui_kit::Role,
    pub(super) status_label: &'static str,
}

/// Desktop severity mapping for status rows: errors announce as alerts with a
/// solid danger marker, warnings keep their warning tone with an outlined
/// marker, info stays quiet muted text. The two marker shapes mean severity is
/// never carried by hue alone when a diagnostic title does not name it.
pub(super) fn severity_marker_style(
    severity: NoticeSeverity,
    colors: &gpui_kit::component::theme::ThemeColor,
) -> SeverityMarkerStyle {
    match severity {
        NoticeSeverity::Error => SeverityMarkerStyle {
            tone: colors.danger,
            marker_outlined: false,
            role: gpui_kit::Role::Alert,
            status_label: "error",
        },
        NoticeSeverity::Warning => SeverityMarkerStyle {
            tone: colors.warning,
            marker_outlined: true,
            role: gpui_kit::Role::Status,
            status_label: "warning",
        },
        NoticeSeverity::Info => SeverityMarkerStyle {
            tone: themes::muted_text(colors),
            marker_outlined: false,
            role: gpui_kit::Role::Status,
            status_label: "info",
        },
    }
}

/// Fixed leading slot shared by transcript header rows: the same `size_4`
/// centered box `MarkerIcon` renders, so group and tool titles start on
/// the notice spine. Always rendered, even when empty, so optional
/// content (the running spinner) never shifts the title.
pub(super) fn transcript_icon_slot(child: impl IntoElement) -> gpui_kit::Div {
    div()
        .size(themes::layout::ICON_SLOT)
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .child(child)
}

/// Fixed trailing lane for transcript disclosure chevrons: the same slot
/// box as [`transcript_icon_slot`], tone-agnostic so each header keeps
/// its own color (callers chain `.text_color(..)` when they need one).
pub(super) fn transcript_chevron_lane(expanded: bool) -> gpui_kit::Div {
    transcript_icon_slot(
        Icon::new(if expanded {
            IconName::ChevronDown
        } else {
            IconName::ChevronRight
        })
        .small(),
    )
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
    icon: impl Into<UiIcon>,
) -> EmptyState {
    let title = title.into();
    let icon = icon.into();
    EmptyState::new().header(
        EmptyHeader::new()
            .media(if icon == UiIcon::BrandV {
                EmptyMedia::new().child(brand_logo(40.))
            } else {
                EmptyMedia::new().with_variant(EmptyMediaVariant::Icon).child(icon.into_icon())
            })
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

/// Running-turn marker shared by every thread surface (tab strip, sidebar,
/// browser, compact header): a muted spinner under a caller-owned stable id.
/// The owning button already names the running state for assistive tech, so
/// the marker itself stays decorative.
pub(super) fn running_indicator(
    id: impl Into<gpui_kit::ElementId>,
    colors: &gpui_kit::component::theme::ThemeColor,
) -> AnyElement {
    div()
        .id(id)
        .test_support()
        .flex_shrink_0()
        .child(Spinner::new().xsmall().color(colors.muted_foreground))
        .into_any_element()
}

/// The shared mount fade: every toggle, panel, and popup that appears fades
/// opacity 0→1 on mount through the theme's enter motion. All fades go through
/// `motion::opacity_fade`, so the animation builder and its duration/curve
/// policy have one owner. `with_animation` renders the end state statically
/// under reduced motion and requests no frames once settled, so no manual
/// gate is needed.
pub(super) trait FadeInExt: AnimationExt + Sized {
    /// Apply the shared appearance fade with a stable per-mount id. The
    /// element/id types stay fully qualified so the shared import surface
    /// (`use super::*`) never widens and re-flags existing call sites.
    fn fade_in(self, id: impl Into<gpui_kit::ElementId>, theme: &Theme) -> gpui_kit::AnimationElement<Self>
    where
        Self: Styled,
    {
        // `returning` is the decelerating enter curve; a mount reveal has the
        // normal duration and that enter curve.
        motion::opacity_fade(self, id, theme, false, true)
    }
}

impl<T: AnimationExt + Sized> FadeInExt for T {}

#[cfg(test)]
mod tests;
