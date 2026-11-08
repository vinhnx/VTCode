//! Installed-font selection reuses the retained Kit command palette.

use super::{DesktopView, PaletteAction, PaletteKind, command, desktop_button, settings_row};
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::theme::ActiveTheme;
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, Context, Entity, Window};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FontTarget {
    Ui,
    Code,
}

pub(super) fn installed_fonts(cx: &App) -> Vec<String> {
    normalize_font_names(cx.text_system().all_font_names())
}

fn normalize_font_names(fonts: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut fonts = fonts
        .into_iter()
        .filter(|name| {
            !name.trim().is_empty()
                && name.len() <= 128
                && !name.starts_with('.')
                && !name.chars().any(char::is_control)
        })
        .collect::<Vec<_>>();
    fonts.sort_unstable();
    fonts.dedup();
    fonts
}

pub(super) fn resolve_font(saved: Option<&str>, fallback: &str, installed: &[String]) -> String {
    saved
        .filter(|name| installed.iter().any(|font| font == name))
        .unwrap_or(fallback)
        .to_owned()
}

pub(super) fn default_code_font(cx: &App) -> gpui_kit::SharedString {
    installed_fonts(cx)
        .into_iter()
        .find(|name| name == "SF Mono")
        .map_or_else(|| cx.theme().mono_font_family.clone(), Into::into)
}

impl DesktopView {
    pub(super) fn open_font_palette(&mut self, target: FontTarget, window: &mut Window, cx: &mut Context<Self>) {
        self.close_settings(window, cx);
        let default_label = match target {
            FontTarget::Ui if cfg!(target_os = "macos") => {
                rust_i18n::t!("app.appearance.system_default_font", font = "SF Pro").to_string()
            }
            FontTarget::Ui => rust_i18n::t!("app.appearance.system_default").to_string(),
            FontTarget::Code => {
                rust_i18n::t!("app.appearance.system_default_font", font = self.default_code_font.as_str()).to_string()
            }
        };
        let installed = installed_fonts(cx);
        let mut entries = vec![command(
            default_label,
            PaletteAction::SelectFont { target, family: None },
        )];
        entries.extend(
            installed.iter().map(|family| {
                command(family.clone(), PaletteAction::SelectFont { target, family: Some(family.clone()) })
            }),
        );
        self.show_palette(
            PaletteKind::Fonts(target),
            vec![(rust_i18n::t!("app.appearance.installed_fonts").to_string(), entries)],
            window,
            cx,
        );
    }

    pub(super) fn select_font(&mut self, target: FontTarget, family: Option<String>, cx: &mut Context<Self>) {
        let installed = installed_fonts(cx);
        if family.as_ref().is_some_and(|family| !installed.contains(family)) {
            return;
        }
        match target {
            FontTarget::Ui => self.preferences.set_ui_font_family(self.theme_mode, family),
            FontTarget::Code => self.preferences.set_code_font_family(self.theme_mode, family),
        }
        self.apply_typography(cx);
        self.save_preferences();
    }

    /// App-context content builder, hostable inside a Kit `SettingItem`
    /// closure. The family display reads the live theme; only the palette
    /// opener needs the view handle.
    pub(super) fn render_font_family_content(target: FontTarget, view: Entity<Self>, cx: &mut App) -> AnyElement {
        let (id, label, family) = match target {
            FontTarget::Ui => {
                ("choose-ui-font", rust_i18n::t!("app.appearance.ui_font").to_string(), &cx.theme().font_family)
            }
            FontTarget::Code => (
                "choose-code-font",
                rust_i18n::t!("app.appearance.code_font").to_string(),
                &cx.theme().mono_font_family,
            ),
        };
        let display = if family.as_str() == ".SystemUIFont" {
            if cfg!(target_os = "macos") {
                rust_i18n::t!("app.appearance.system_pro").to_string()
            } else {
                rust_i18n::t!("app.appearance.system_short").to_string()
            }
        } else {
            family.to_string()
        };
        settings_row(
            label.clone(),
            "",
            desktop_button(id)
                .label(display.to_string())
                .accessibility_label(
                    rust_i18n::t!("app.appearance.choose_current", label = label.as_str(), value = display.as_str())
                        .to_string(),
                )
                .ghost()
                .on_click({
                    let view = view.clone();
                    move |_, window: &mut Window, cx: &mut App| {
                        view.update(cx, |view, cx| view.open_font_palette(target, window, cx));
                    }
                }),
            &cx.theme().colors,
        )
        .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{normalize_font_names, resolve_font};

    #[test]
    fn font_catalog_snapshots_are_normalized_without_hiding_later_fonts() {
        let first = normalize_font_names(vec!["Menlo".into(), "Menlo".into(), ".SystemUIFont".into()]);
        let refreshed = normalize_font_names(vec!["Menlo".into(), "Newly Registered Font".into()]);

        assert_eq!(first, vec!["Menlo"]);
        assert_eq!(refreshed, vec!["Menlo", "Newly Registered Font"]);
    }

    #[test]
    fn missing_or_invalid_fonts_fall_back_without_overriding_independent_choices() {
        let installed = vec!["Menlo".into(), "Helvetica Neue".into()];
        assert_eq!(resolve_font(Some("Helvetica Neue"), ".SystemUIFont", &installed), "Helvetica Neue");
        assert_eq!(resolve_font(Some("Menlo"), "Monaco", &installed), "Menlo");
        for invalid in [None, Some("Removed font"), Some(""), Some("Menlo\n")] {
            assert_eq!(resolve_font(invalid, ".SystemUIFont", &installed), ".SystemUIFont");
        }
    }
}
