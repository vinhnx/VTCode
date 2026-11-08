//! Kit `Settings` host for the settings surface on wide viewports.
//!
//! V1 hosts each pane builder verbatim inside one Element item per page:
//! the Kit sidebar, search box, and page chrome come from the framework
//! while pane content keeps its tested builders. Finer per-group
//! decomposition happens page by page afterwards; narrow viewports keep
//! the legacy stacked navigation and content.

use super::appearance::AppearanceSettingsProps;
use super::configuration_render::{CONFIG_WIDE_VIEWPORT, ConfigPageProps};
use super::settings::SettingsTab;
use super::settings_accessibility::AccessibilitySettingsProps;
use super::settings_connections::ConnectionsSettingsProps;
use super::settings_notifications::NotificationSettingsProps;
use super::settings_vtcode::VtcodeSettingsProps;
use super::shortcuts_render::ShortcutSettingsProps;
use super::storage::StorageSettingsProps;
use super::*;
use gpui_kit::component::setting::{RenderOptions, SelectIndex, SettingGroup, SettingItem, SettingPage, Settings};

/// Search keywords for the CLI configuration page. Tab pages reuse their
/// palette keywords; the config entry has no tab so it carries a small
/// static set (the fuzzy settings palette stays the deep index).
const CONFIG_PAGE_KEYWORDS: &[&str] = &["configuration", "config", "cli", "setting", "settings"];

/// A v1 tab page: one Element item hosting the pane builder. Title, icon,
/// and keywords come from the tab so the Kit sidebar cannot drift from the
/// navigation model. The brand V mark has no Kit `IconName` equivalent, so
/// VT Code renders it through `vtcode_nav_mark()` while the other tabs use
/// their catalog glyph.
///
/// Both branches carry [`nav_entry_icon_size`] explicitly: Kit's sidebar row
/// sets `text_sm` (0.875rem) and an unsized `Icon` inherits that text size, so
/// without the explicit box the wide (default) layout would paint 14px glyphs
/// while the narrow legacy rail paints the shared 16px baseline.
fn kit_page_icon(tab: SettingsTab) -> Icon {
    match tab.icon() {
        Some(name) => Icon::new(name),
        None => vtcode_nav_mark(),
    }
    .size(nav_entry_icon_size())
}

fn kit_single_item_page<R, E>(tab: SettingsTab, render: R) -> SettingPage
where
    R: Fn(&RenderOptions, &mut Window, &mut App) -> E + 'static,
    E: IntoElement,
{
    SettingPage::new(tab.label())
        .icon(kit_page_icon(tab))
        .group(SettingGroup::new().item(SettingItem::render(render).keywords(tab.search_keywords().iter().copied())))
}

impl DesktopView {
    /// Kit-hosted settings content. The remount id carries the selected
    /// page so view-driven selection (tabs, palette, deep links, tab-cycle
    /// actions) pushes into Kit's internal state. Kit-internal sidebar
    /// clicks navigate Kit only, so the Storage item fetches its summary
    /// on display; the view/Kit selection indexes still diverge after a
    /// Kit click until upstream on-select lands, and WebMCP status
    /// refreshes on the view-driven path plus backend events and its
    /// manual refresh.
    ///
    /// Pages follow [`SettingsTab::ALL`] by construction: the match is
    /// exhaustive, so a reordered or added tab cannot silently desync the
    /// Kit order from [`DesktopView::settings_page_index`]. The CLI
    /// configuration entry has no tab and stays last by contract.
    pub(super) fn render_settings_kit(&self, cx: &mut Context<Self>) -> AnyElement {
        let page_ix = self.settings_page_index();
        let view = cx.entity();

        let mut pages = Vec::with_capacity(SettingsTab::PAGE_COUNT);
        for tab in SettingsTab::ALL {
            let page_view = view.clone();
            let page = match tab {
                SettingsTab::VtCode => {
                    let props = VtcodeSettingsProps::capture(self);
                    kit_single_item_page(tab, move |_: &RenderOptions, _: &mut Window, cx: &mut App| {
                        Self::render_vtcode_settings_content(props.clone(), page_view.clone(), cx)
                    })
                }
                SettingsTab::Appearance => {
                    let props = AppearanceSettingsProps::capture(self);
                    kit_single_item_page(tab, move |_: &RenderOptions, _: &mut Window, cx: &mut App| {
                        Self::render_appearance_settings_content(props.clone(), page_view.clone(), cx)
                    })
                }
                SettingsTab::Accessibility => {
                    let props = AccessibilitySettingsProps::capture(self);
                    kit_single_item_page(tab, move |_: &RenderOptions, _: &mut Window, cx: &mut App| {
                        Self::render_accessibility_settings_content(props.clone(), page_view.clone(), cx)
                    })
                }
                SettingsTab::Shortcuts => {
                    let props = ShortcutSettingsProps::capture(self);
                    kit_single_item_page(tab, move |_: &RenderOptions, window: &mut Window, cx: &mut App| {
                        Self::render_shortcuts_content(props.clone(), page_view.clone(), window, cx)
                    })
                }
                SettingsTab::Connections => {
                    let props = ConnectionsSettingsProps::capture(self);
                    kit_single_item_page(tab, move |_: &RenderOptions, _: &mut Window, cx: &mut App| {
                        Self::render_connections_settings_content(props.clone(), page_view.clone(), cx)
                    })
                }
                SettingsTab::Notifications => {
                    let props = NotificationSettingsProps::capture(self);
                    kit_single_item_page(tab, move |_: &RenderOptions, _: &mut Window, cx: &mut App| {
                        Self::render_notification_settings_content(props.clone(), page_view.clone(), cx)
                    })
                }
                SettingsTab::Storage => {
                    let props = StorageSettingsProps::capture(self);
                    kit_single_item_page(tab, move |_: &RenderOptions, _: &mut Window, cx: &mut App| {
                        // Fetch-on-display: Kit sidebar clicks bypass
                        // view-driven selection, so without this the summary
                        // would never load on that path. Kit renders only
                        // the active page, so this fires exactly when
                        // Storage shows. Guarded one-shot (missing summary,
                        // no in-flight request); errored state waits for the
                        // manual retry instead of refetching every render.
                        page_view.update(cx, |view, cx| {
                            if view.storage.error.is_none() {
                                view.maybe_request_storage_summary(cx);
                            }
                        });
                        Self::render_storage_settings_content(props.clone(), page_view.clone(), cx)
                    })
                }
                SettingsTab::About => {
                    kit_single_item_page(tab, move |_: &RenderOptions, _: &mut Window, cx: &mut App| {
                        Self::render_about_settings_content(page_view.clone(), cx)
                    })
                }
            };
            pages.push(page);
        }

        let config_props = ConfigPageProps::capture(self);
        let config_view = view.clone();
        pages.push(
            SettingPage::new(rust_i18n::t!("app.configuration.page_title").to_string())
                .icon(IconName::Settings2)
                .group(
                    SettingGroup::new().item(
                        SettingItem::render(move |_: &RenderOptions, window: &mut Window, cx: &mut App| {
                            let wide = window.viewport_size().width >= px(CONFIG_WIDE_VIEWPORT);
                            Self::render_cli_config_page_content(config_props.clone(), config_view.clone(), wide, cx)
                        })
                        .keywords(CONFIG_PAGE_KEYWORDS.iter().copied()),
                    ),
                ),
        );

        Settings::new(format!("settings-kit-{page_ix}"))
            .pages(pages)
            .default_selected_index(SelectIndex { page_ix, group_ix: None })
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::component::theme::Theme;
    use gpui_kit::test::TestWindowExt;
    use gpui_kit::{
        AnyWindowHandle, Bounds, Context, Render, TestAppContext, TestSupportExt, Window, WindowBounds, WindowOptions,
        div,
    };

    #[test]
    fn kit_pages_carry_search_keywords() {
        for tab in SettingsTab::ALL {
            assert!(!tab.search_keywords().is_empty(), "Kit page `{}` needs search keywords", tab.label());
        }
        assert!(!CONFIG_PAGE_KEYWORDS.is_empty(), "config page needs search keywords");
    }

    /// Probe mounting the Kit page glyphs so the shared rail box is asserted on
    /// rendered geometry. `Icon` is `RenderOnce` with no id, so each glyph sits
    /// in an identified wrapper that owns the measured box.
    struct KitGlyphProbe;

    impl Render for KitGlyphProbe {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            // Row containers mirror Kit's sidebar row (`.text_sm()`), the
            // condition that shrinks an unsized `Icon` to 14px. Each identified
            // wrapper is a `flex_shrink_0` item, so it shrink-wraps the glyph
            // instead of stretching to the window width.
            div()
                .flex()
                .flex_col()
                .size_full()
                .child(
                    div().flex().text_sm().child(
                        div()
                            .id("probe-kit-glyph-catalog")
                            .test_support()
                            .flex_shrink_0()
                            .child(kit_page_icon(SettingsTab::Appearance)),
                    ),
                )
                .child(
                    div().flex().text_sm().child(
                        div()
                            .id("probe-kit-glyph-mark")
                            .test_support()
                            .flex_shrink_0()
                            .child(kit_page_icon(SettingsTab::VtCode)),
                    ),
                )
        }
    }

    fn open_kit_glyph_probe(cx: &mut TestAppContext) -> AnyWindowHandle {
        cx.update(gpui_kit::init);
        let (window, _probe) = cx
            .update(|cx| {
                gpui_kit::open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                            point(px(0.), px(0.)),
                            size(px(640.), px(480.)),
                        ))),
                        ..Default::default()
                    },
                    cx,
                    |_, cx| cx.new(|_| KitGlyphProbe),
                )
            })
            .expect("open Kit glyph probe window");
        window
    }

    /// The Kit sidebar row is `text_sm` (0.875rem); an unsized `Icon` inherits
    /// that 14px text size. Both glyph branches must instead paint the shared
    /// 16px app baseline and keep growing with the UI font size setting, or the
    /// wide (default) layout drifts below the narrow legacy rail.
    #[gpui_kit::test]
    fn kit_page_glyphs_keep_the_shared_rail_box(cx: &mut TestAppContext) {
        let window = open_kit_glyph_probe(cx);
        for base_size in [14., 24.] {
            cx.update(|cx| {
                Theme::update(cx, |theme| {
                    theme.font_size = px(base_size);
                });
            });
            cx.update_window(window, |_, window, cx| {
                window.render_frame(cx);
                for id in ["probe-kit-glyph-catalog", "probe-kit-glyph-mark"] {
                    let box_ = window.find(id).bounds();
                    let expected = base_size * 1.0;
                    assert_eq!(
                        box_.size,
                        size(px(expected), px(expected)),
                        "{id} must paint the font-relative 1rem rail box at {base_size}px: {box_:?}"
                    );
                }
            })
            .unwrap();
        }
    }
}
