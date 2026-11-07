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

/// Pending assistant row pinned below the transcript scroller while a turn
/// runs with no content yet. It lives outside the measured list on purpose:
/// scroller geometry never churns when the row appears or clears. The label
/// reuses the owned turn-phase status, so it tracks
/// submitting/running/cancelling with no extra copy. The edge inset lives on
/// the outer wrapper (mirroring the scroller's row wrapper), so the label
/// text starts on the transcript spine instead of one step inside it.
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

/// Shared folder tab treatment at the ecosystem-default medium size: the
/// selected tab joins the canvas content below it, so callers on canvas place
/// the bar flush above that content (the picker keeps its stack gap — its
/// popover surface never joins anyway). No width cap: Kit truncates labels
/// only when a `max_width` is set, which this builder never does — overflow
/// scrolls, and callers with many tabs chain `.menu(true)` for the overflow
/// dropdown.
pub(super) fn desktop_folder_tab_bar(id: impl Into<gpui_kit::ElementId>) -> TabBar {
    TabBar::new(id).folder().w_full()
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

/// Content page title: one step above section headings, naming the visible
/// settings page. Rendered once as the first child of the scroll content so
/// scroll geometry is unchanged; the panel header keeps the stable dialog
/// identity.
pub(super) fn content_page_title(
    text: impl Into<gpui_kit::SharedString>,
    colors: &gpui_kit::component::theme::ThemeColor,
) -> impl IntoElement {
    div().text_xl().font_semibold().text_color(colors.foreground).child(text.into())
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
        .border_color(soft_edge_color(colors))
}

/// Softened edge tone shared by card outlines and dividers: the theme
/// border at reduced opacity so boundaries read as quiet hairlines rather
/// than rules. One value for every surface — tune here, not per surface.
pub(super) fn soft_edge_color(colors: &gpui_kit::component::theme::ThemeColor) -> gpui_kit::Hsla {
    colors.border.opacity(0.55)
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

/// 1px separator for non-settings surfaces (navigation chrome, inspector
/// lists), drawn in the softened edge tone with breathing room so it reads
/// as a quiet hairline rather than a rule. Settings groups separate rows by
/// `gap_4` air instead (see [`settings_group_box`]).
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

/// Navigation entry content: a small icon pinned to a truncating label on
/// one spine. Shared by the settings navigation and the configuration group
/// rail (Kit `Button` centers its built-in icon/label pair, so rails use
/// this custom child instead). The icon is decorative; the owning button
/// carries the accessible name. The leading mark is any `Icon`, so a Lucide
/// `IconName` and brand SVG bytes share one size/color path and align.
pub(super) fn nav_entry_content(
    label_id: impl Into<gpui_kit::ElementId>,
    icon: impl Into<Icon>,
    label: String,
) -> AnyElement {
    div()
        .w_full()
        .min_w_0()
        .flex()
        .items_center()
        .gap_2()
        .child(Icon::new(icon).small())
        .child(div().id(label_id).test_support().flex_1().min_w_0().truncate().child(label))
        .into_any_element()
}

/// Brand V mark shared by the welcome empty states and the settings
/// navigation. GPUI paints SVGs as tinted monochrome sprites, so the mark
/// follows the surrounding text color in every theme.
const VTCODE_NAV_MARK_SVG: &[u8] = include_bytes!("../../../../../../resources/icons/vtcode-mark.svg");

/// Settings-nav leading mark for the VT Code entry: the brand V at the same
/// small size and inherited color as the Lucide sibling rows.
pub(super) fn vtcode_nav_mark() -> Icon {
    Icon::default().data(VTCODE_NAV_MARK_SVG)
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
        .border_color(colors.border)
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
        .on_click(on_click)
}

/// Route container shared by the settings, browser, and review pages:
/// centers the max-width panel in the content slot. The toolbar (normal
/// mode) or the zen drag strip (windowed zen) owns the traffic-light
/// clearance above the slot, so the page adds none itself. Titles align
/// to the content spine in every mode; no horizontal dodge is needed once
/// the lights clear the panel vertically.
pub(super) fn route_page(panel: impl IntoElement) -> impl IntoElement {
    div()
        .flex_1()
        .min_w_0()
        .h_full()
        .px_6()
        .pb_6()
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
        .child(
            div()
                .font_semibold()
                .text_lg()
                .text_color(colors.foreground)
                .child(title.into()),
        )
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

/// Brand mark for small in-app renders (welcome empty states, about page),
/// embedded so packaged applications never depend on checkout paths. Uses
/// the display-sized 128px art, not the 512px Dock art: the small renders
/// would minify 8x through a mipmap-less sampler and show stair-stepped
/// edges (see [`app_icon::preview_bytes`]).
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

pub(super) fn welcome_brand_mark(size: f32, color: gpui_kit::Hsla) -> impl IntoElement {
    gpui_kit::svg()
        .data(VTCODE_NAV_MARK_SVG)
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

/// Severity-resolved presentation for a status marker row: icon, tone,
/// title weight, accessibility role, and the stable status token used in
/// the details toggle label. Single dispatch for every severity-driven
/// choice, so callers resolve once and render from the result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SeverityMarkerStyle {
    pub(super) icon: IconName,
    pub(super) tone: gpui_kit::Hsla,
    pub(super) semibold_title: bool,
    pub(super) role: gpui_kit::Role,
    pub(super) status_label: &'static str,
}

/// Desktop severity mapping for status rows: errors announce as alerts in
/// the danger tone, warnings keep their own warning tone and status role,
/// info stays quiet muted text. Only errors earn semibold: icon + tone
/// already carry severity.
pub(super) fn severity_marker_style(
    severity: NoticeSeverity,
    colors: &gpui_kit::component::theme::ThemeColor,
) -> SeverityMarkerStyle {
    match severity {
        NoticeSeverity::Error => SeverityMarkerStyle {
            icon: IconName::CircleAlert,
            tone: colors.danger,
            semibold_title: true,
            role: gpui_kit::Role::Alert,
            status_label: "error",
        },
        NoticeSeverity::Warning => SeverityMarkerStyle {
            icon: IconName::TriangleAlert,
            tone: colors.warning,
            semibold_title: false,
            role: gpui_kit::Role::Status,
            status_label: "warning",
        },
        NoticeSeverity::Info => SeverityMarkerStyle {
            icon: IconName::Info,
            tone: themes::muted_text(colors),
            semibold_title: false,
            role: gpui_kit::Role::Status,
            status_label: "info",
        },
    }
}

/// Status-marker row for transcript severity notices: one icon/title header
/// line in severity colors, no message chrome. Callers keep test identity,
/// roles, and aria labels on their own wrapper: `Marker` is `RenderOnce`
/// and cannot carry `.test_support()`. Extra trailing children (e.g. the
/// Show/Hide details toggle) compose as `Marker` siblings; the content
/// slot holds `flex_1` so they dock at the trailing edge. The leading
/// inset matches a small button's `px_2`, so notice titles start on the
/// same spine as group and tool headers (inset + [`transcript_icon_slot`]
/// + `gap_2`).
pub(super) fn severity_marker(style: &SeverityMarkerStyle, title: impl Into<gpui_kit::SharedString>) -> Marker {
    Marker::new()
        .items_center()
        .pl_2()
        .text_color(style.tone)
        .icon(MarkerIcon::new().child(Icon::new(style.icon).small().text_color(style.tone)))
        .content(
            MarkerContent::new()
                .text(title)
                .flex_1()
                .when(style.semibold_title, |content| content.font_semibold()),
        )
}

/// Fixed leading slot shared by transcript header rows: the same `size_4`
/// centered box `MarkerIcon` renders, so group and tool titles start on
/// the notice spine. Always rendered, even when empty, so optional
/// content (the running spinner) never shifts the title.
pub(super) fn transcript_icon_slot(child: impl IntoElement) -> gpui_kit::Div {
    div().size_4().flex_none().flex().items_center().justify_center().child(child)
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

/// Ordinary visible action: a filled push button for commands that deserve a
/// control frame (dialog secondaries, Save/Cancel pairs, labeled toolbar
/// actions). Kit's Default variant paints the themed button fill with an input
/// hairline; rails, nav rows, and icon-only toolbar actions stay on
/// [`desktop_button`] + `.ghost()` instead.
pub(super) fn desktop_default_button(id: impl Into<gpui_kit::ElementId>) -> Button {
    desktop_button(id)
}

/// Decision-area default commit: the one emphasized action per view, normally
/// the Enter action. Paints the brand accent fill; never use for destructive
/// actions even when destruction is the most likely choice.
pub(super) fn desktop_primary_button(id: impl Into<gpui_kit::ElementId>) -> Button {
    desktop_button(id).primary()
}

/// Destructive commitment: irreversible or data-losing actions only. Pair with
/// an explicit confirmation for consequential work.
pub(super) fn desktop_danger_button(id: impl Into<gpui_kit::ElementId>) -> Button {
    desktop_button(id).danger()
}

/// Quiet header dismiss control: ghost X icon with an accessible name and a
/// matching tooltip. Callers add sizing (usually `.small()`), disabled
/// state, and the click handler. Icon-only X buttons without a tooltip
/// (tab/attachment closers) and action-bound tooltips stay manual.
pub(super) fn desktop_dismiss_button(
    id: impl Into<gpui_kit::ElementId>,
    label: impl Into<gpui_kit::SharedString>,
) -> Button {
    let label = label.into();
    desktop_button(id)
        .icon(IconName::X)
        .accessibility_label(label.clone())
        .tooltip(label)
        .ghost()
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
    /// Floating-layer elevation for a decision action that sits over content
    /// (the composer send and its running-state cluster). In-layer actions
    /// (dialog commits) take no shadow: the dialog already carries the
    /// elevation. Gate at the call site with `.when(enabled, ...)` so
    /// disabled buttons stay flat: Kit replays the instance style inside its
    /// disabled arm, after its own shadow reset.
    fn desktop_elevated(self) -> Self;
    /// Selected-option treatment: pressed-state metadata plus a stronger
    /// interaction fill when selected, quiet ghost otherwise. Every call site
    /// sits on a `secondary`-equivalent surface (settings panel, group-box fill), where
    /// a `secondary` fill would be pixel-identical to the background, so the
    /// highlight uses the readable pressed fill. Ghost-base
    /// (diff layout) and label-only (reduced motion)
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
            .selected(selected)
            .ghost()
            .when(selected, |button| button.bg(colors.button_active).text_color(colors.foreground))
    }

    fn dispatches_desktop_action(self, action: impl gpui_kit::Action + 'static, focus: FocusHandle) -> Self {
        self.on_click(move |_, window, cx| focus.dispatch_action(&action, window, cx))
    }

    fn desktop_elevated(self) -> Self {
        self.shadow_xs()
    }
}

#[cfg(test)]
mod tests;
