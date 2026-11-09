//! Theme picker sections for the appearance settings: swatch buttons and
//! per-mode groups with their own scrollers.

use super::appearance::AppearanceSettingsProps;
use super::*;

impl DesktopView {
    /// One theme entry in the settings picker: a color preview chip that
    /// applies live on click.
    pub(super) fn render_theme_swatch_content(
        entry: themes::SwatchEntry,
        duplicate: bool,
        props: &AppearanceSettingsProps,
        view: Entity<Self>,
        cx: &mut App,
    ) -> AnyElement {
        let (background, foreground) = entry.preview();
        let entry_id = entry.id();
        let label = if duplicate {
            format!("{entry_id} · {}", entry.label())
        } else {
            entry.label().to_owned()
        };
        let active = props.theme_id == entry_id;
        let light_profile = props.separate_theme_profiles && props.light_theme.as_deref() == Some(entry_id.as_str());
        let dark_profile = props.separate_theme_profiles && props.dark_theme.as_deref() == Some(entry_id.as_str());
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
            .w(px(104.))
            .flex_shrink_0()
            .h(px(112.))
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

    /// One mode group inside the theme picker: a subordinate header plus its
    /// own horizontal swatch scroller. Headers sit one type step below the
    /// section title so Light/Dark read as groups under Themes.
    pub(super) fn render_theme_group_content(
        mode: ThemeMode,
        counts: &HashMap<&'static str, usize>,
        props: &AppearanceSettingsProps,
        view: Entity<Self>,
        cx: &mut App,
    ) -> AnyElement {
        let light = mode == ThemeMode::Light;
        let label = if light {
            rust_i18n::t!("app.settings.appearance.light").to_string()
        } else {
            rust_i18n::t!("app.settings.appearance.dark").to_string()
        };
        let swatches = themes::picker_swatches_for(mode)
            .map(|entry| {
                Self::render_theme_swatch_content(
                    entry,
                    counts.get(entry.label()).copied().unwrap_or_default() > 1,
                    props,
                    view.clone(),
                    cx,
                )
            })
            .collect::<Vec<_>>();
        let scroll = if light {
            &props.theme_scroll_light
        } else {
            &props.theme_scroll_dark
        };
        let list_id = if light { "theme-list-light" } else { "theme-list-dark" };
        let scroller_id = if light {
            "theme-scroller-light"
        } else {
            "theme-scroller-dark"
        };
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
                        div()
                            .id(scroller_id)
                            .lock_scroll_axis()
                            .size_full()
                            .overflow_x_scroll()
                            .track_scroll(scroll)
                            .flex()
                            .items_start()
                            .gap_2()
                            .children(swatches)
                            .test_support(),
                    )
                    .child(ScrollableMask::vertical_wheel(scroll))
                    .child(Scrollbar::horizontal(scroll)),
            )
            .into_any_element()
    }
}
