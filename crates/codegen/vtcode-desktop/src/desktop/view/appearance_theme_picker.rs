//! Theme picker sections for the appearance settings: swatch buttons and
//! per-mode groups with their own virtualized scrollers.
//!
//! Each mode row hosts its virtual list in a small child view ([`ThemeGrid`])
//! rather than in `DesktopView`, with a weak back-reference to avoid an entity
//! cycle. The virtual list mounts only the visible swatch window per frame;
//! GPUI's scroller notifies `window.current_view()` on wheel ticks, which is
//! now the grid, though `mark_view_dirty` still dirties ancestor views. The
//! per-swatch `hairline` cost is separately cached in `themes/hairline.rs`.

use super::*;

/// Swatch button box: matches the fixed button below. Cells add the
/// inter-item gap so consecutive swatches land exactly as the previous
/// flex `gap_2` row laid them (112px steps at the default scale); buttons
/// are fixed px, so the table is static.
const SWATCH_CELL_WIDTH: f32 = 104.0;
const SWATCH_CELL_GAP: f32 = 8.0;
const SWATCH_CELL_HEIGHT: f32 = 112.0;
/// One swatch plus its trailing gap: the row's uniform step. Chevron paging
/// and scroll-into-view math share it so the pixels cannot drift.
pub(super) const THEME_SWATCH_STRIDE: f32 = SWATCH_CELL_WIDTH + SWATCH_CELL_GAP;

/// Size table for one mode's row: uniform cells, so only the visible
/// window builds per frame. Lengths match [`themes::picker_entries_for`]
/// by construction (same count source), and the item closure still indexes
/// defensively. Rebuilt per render: uniform math over a static count is
/// noise next to one swatch build, and `Rc` is not `Sync` for a static.
fn swatch_sizes(mode: ThemeMode) -> Rc<Vec<Size<Pixels>>> {
    let count = themes::picker_entries_for(mode).len();
    Rc::new(
        (0..count)
            .map(|ix| {
                let width = if ix + 1 == count {
                    SWATCH_CELL_WIDTH
                } else {
                    THEME_SWATCH_STRIDE
                };
                size(px(width), px(SWATCH_CELL_HEIGHT))
            })
            .collect(),
    )
}

/// The slice of live view state a swatch needs to decide its active/profile
/// badges. Copied out of `DesktopView` before each item is built so the
/// virtual list's item closure never holds a read guard across a mutable
/// `App` borrow.
#[derive(Clone)]
pub(super) struct ThemeSelection {
    theme_id: String,
    separate_profiles: bool,
    light_theme: Option<String>,
    dark_theme: Option<String>,
}

impl ThemeSelection {
    fn capture(view: &DesktopView) -> Self {
        Self {
            theme_id: view.theme_id.clone(),
            separate_profiles: view.preferences.separate_theme_profiles,
            light_theme: view.preferences.light_theme.clone(),
            dark_theme: view.preferences.dark_theme.clone(),
        }
    }
}

/// One virtualized theme row, hosted in its own view so wheel-driven repaints
/// stay off the rest of the desktop. The retained scroll handle is shared
/// with `DesktopView` (chevrons, tests, and the other profile's row use it).
/// The `WeakEntity` back-reference avoids an entity cycle: the view owns this
/// grid, so the grid must not own the view back.
pub(super) struct ThemeGrid {
    mode: ThemeMode,
    scroll: VirtualListScrollHandle,
    view: WeakEntity<DesktopView>,
}

impl ThemeGrid {
    pub(super) fn new(mode: ThemeMode, scroll: VirtualListScrollHandle, view: &Entity<DesktopView>) -> Self {
        Self { mode, scroll, view: view.downgrade() }
    }
}

impl Render for ThemeGrid {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mode = self.mode;
        let light = mode == ThemeMode::Light;
        let label = if light {
            rust_i18n::t!("app.settings.appearance.light").to_string()
        } else {
            rust_i18n::t!("app.settings.appearance.dark").to_string()
        };
        let entries = themes::picker_entries_for(mode);
        let counts = themes::picker_label_counts();
        let list_id = if light { "theme-list-light" } else { "theme-list-dark" };
        let scroller_id = if light {
            "theme-scroller-light"
        } else {
            "theme-scroller-dark"
        };
        let scroll = self.scroll.clone();

        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .text_sm()
                    .font_semibold()
                    .text_color(themes::label_foreground(&cx.theme().colors))
                    .child(label.clone()),
            )
            .child(
                div()
                    .id(list_id)
                    .test_support()
                    .role(gpui_kit::Role::Group)
                    .aria_label(label)
                    .w_full()
                    .min_w_0()
                    .flex_shrink_0()
                    .h(px(128.))
                    .relative()
                    .child(
                        h_virtual_list(cx.entity(), scroller_id, swatch_sizes(mode), move |grid, range, _, cx| {
                            let Some(desktop) = grid.view.upgrade() else {
                                return Vec::new();
                            };
                            let selection = ThemeSelection::capture(desktop.read(cx));
                            range
                                .filter_map(|ix| {
                                    let entry = entries.get(ix).copied()?;
                                    let duplicate = counts.get(entry.label()).copied().unwrap_or_default() > 1;
                                    Some(DesktopView::render_theme_swatch_content(
                                        entry, duplicate, &selection, &desktop, cx,
                                    ))
                                })
                                .collect::<Vec<_>>()
                        })
                        .track_scroll(&scroll),
                    )
                    .child(ScrollableMask::vertical_wheel(&scroll))
                    .child(Scrollbar::horizontal(&scroll)),
            )
    }
}

impl DesktopView {
    /// One theme entry in the settings picker: a color preview chip that
    /// applies live on click. Reads the live view (not captured props) so
    /// the virtualized row's item closure stays a pure function of the
    /// visible index.
    pub(super) fn render_theme_swatch_content(
        entry: themes::SwatchEntry,
        duplicate: bool,
        selection: &ThemeSelection,
        view: &Entity<Self>,
        cx: &mut App,
    ) -> AnyElement {
        let (background, foreground) = entry.preview();
        let entry_id = entry.id();
        let label = if duplicate {
            format!("{entry_id} · {}", entry.label())
        } else {
            entry.label().to_owned()
        };
        let active = selection.theme_id == entry_id;
        let separate = selection.separate_profiles;
        let light_profile = separate && selection.light_theme.as_deref() == Some(entry_id.as_str());
        let dark_profile = separate && selection.dark_theme.as_deref() == Some(entry_id.as_str());
        let badge = if active {
            "active"
        } else if light_profile && dark_profile {
            "light and dark profiles"
        } else if light_profile {
            "light profile"
        } else if dark_profile {
            "dark profile"
        } else {
            ""
        };
        // Only the active theme gets the filled highlight; profile themes keep a
        // semibold label so a light/dark pair never reads as doubly selected.
        // Profile assignments also get a visible caption: semibold alone is too
        // quiet to explain which swatch a light/dark profile points at.
        let badged = !badge.is_empty();
        let mut caption = Vec::new();
        if light_profile {
            caption.push(rust_i18n::t!("app.settings.appearance.light").to_string());
        }
        if dark_profile {
            caption.push(rust_i18n::t!("app.settings.appearance.dark").to_string());
        }
        let profile_caption = (!caption.is_empty()).then(|| caption.join(" · "));
        let id = format!("theme-{entry_id}");
        let swatch = div()
            .w_full()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .h(px(40.))
                    .rounded_md()
                    .bg(background)
                    .border_1()
                    .border_color(themes::hairline(background, cx.theme().colors.foreground))
                    .p_1()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .when_some(entry.accent(), |chip, accent| chip.child(div().h(px(6.)).rounded_sm().bg(accent)))
                    .child(div().h(px(4.)).w_3_4().rounded_sm().bg(foreground))
                    .child(div().h(px(4.)).w_1_2().rounded_sm().bg(foreground.opacity(0.6))),
            )
            .child(
                div()
                    .text_xs()
                    .truncate()
                    .when(badged, |label| label.font_semibold())
                    .child(label.clone()),
            )
            .child(
                div()
                    .id(format!("theme-{entry_id}-profile"))
                    .test_support()
                    .h(px(20.))
                    .text_xs()
                    .truncate()
                    .text_color(cx.theme().colors.muted_foreground)
                    .when_some(profile_caption, |lane, caption| {
                        lane.role(gpui_kit::Role::Label).aria_label(caption.clone()).child(caption)
                    }),
            );
        desktop_button(id)
            .accessibility_label(if badged { format!("{label} ({badge})") } else { label })
            .toggled(active)
            .ghost()
            .w(px(SWATCH_CELL_WIDTH))
            .flex_shrink_0()
            .h(px(SWATCH_CELL_HEIGHT))
            .p_2()
            .when(active, |button| button.bg(cx.theme().colors.popover))
            .rounded_lg()
            .on_click({
                let view = view.clone();
                move |_, window: &mut Window, cx: &mut App| {
                    view.update(cx, |view, cx| {
                        view.apply_theme(entry.id(), window, cx);
                    });
                }
            })
            .child(swatch)
            .into_any_element()
    }

    /// Host one mode's grid child view. The header and virtual scroller live
    /// in [`ThemeGrid`]; the handle stays on `DesktopView` so chevron paging
    /// and tests keep one scroll position per mode.
    pub(super) fn render_theme_group_content(grid: &Entity<ThemeGrid>) -> AnyElement {
        grid.clone().into_any_element()
    }
}
