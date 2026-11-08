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
/// navigation model. The brand V mark has no Kit `Icon` equivalent, so
/// VtCode keeps a neutral agent glyph meanwhile.
fn kit_single_item_page<R, E>(tab: SettingsTab, render: R) -> SettingPage
where
    R: Fn(&RenderOptions, &mut Window, &mut App) -> E + 'static,
    E: IntoElement,
{
    SettingPage::new(tab.label())
        .icon(tab.icon().unwrap_or(IconName::Bot))
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
                SettingsTab::VtCode => {
                    let props = VtcodeSettingsProps::capture(self);
                    kit_single_item_page(tab, move |_: &RenderOptions, _: &mut Window, cx: &mut App| {
                        Self::render_vtcode_settings_content(props.clone(), page_view.clone(), cx)
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

    #[test]
    fn kit_pages_carry_search_keywords() {
        for tab in SettingsTab::ALL {
            assert!(!tab.search_keywords().is_empty(), "Kit page `{}` needs search keywords", tab.label());
        }
        assert!(!CONFIG_PAGE_KEYWORDS.is_empty(), "config page needs search keywords");
    }
}
