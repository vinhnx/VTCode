//! Installed-font selection reuses the retained Kit command palette.

use super::{DesktopView, PaletteAction, PaletteKind, command, desktop_button};
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::theme::ActiveTheme;
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, Context, Window, div};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FontTarget {
    Ui,
    Code,
}

pub(super) fn installed_fonts(cx: &App) -> &'static [String] {
    static FONTS: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    FONTS.get_or_init(|| {
        let mut fonts = cx.text_system().all_font_names();
        fonts.retain(|name| {
            !name.trim().is_empty()
                && name.len() <= 128
                && !name.starts_with('.')
                && !name.chars().any(char::is_control)
        });
        fonts.sort_unstable();
        fonts.dedup();
        fonts
    })
}

pub(super) fn resolve_font(saved: Option<&str>, fallback: &str, installed: &[String]) -> String {
    saved
        .filter(|name| installed.iter().any(|font| font == name))
        .unwrap_or(fallback)
        .to_owned()
}

pub(super) fn default_code_font(cx: &App) -> gpui_kit::SharedString {
    static DEFAULT: std::sync::OnceLock<gpui_kit::SharedString> = std::sync::OnceLock::new();
    DEFAULT
        .get_or_init(|| {
            installed_fonts(cx)
                .iter()
                .find(|name| name.as_str() == "SF Mono")
                .map_or_else(|| cx.theme().mono_font_family.clone(), |name| name.clone().into())
        })
        .clone()
}

impl DesktopView {
    pub(super) fn open_font_palette(&mut self, target: FontTarget, window: &mut Window, cx: &mut Context<Self>) {
        self.close_settings(window, cx);
        let default_label = match target {
            FontTarget::Ui if cfg!(target_os = "macos") => "System default · SF Pro".to_owned(),
            FontTarget::Ui => "System default".to_owned(),
            FontTarget::Code => format!("System default · {}", self.default_code_font),
        };
        let mut entries = vec![command(
            default_label,
            PaletteAction::SelectFont { target, family: None },
        )];
        entries.extend(
            installed_fonts(cx).iter().map(|family| {
                command(family.clone(), PaletteAction::SelectFont { target, family: Some(family.clone()) })
            }),
        );
        self.show_palette(PaletteKind::Fonts(target), vec![("Installed fonts".into(), entries)], window, cx);
    }

    pub(super) fn select_font(&mut self, target: FontTarget, family: Option<String>, cx: &mut Context<Self>) {
        if family.as_ref().is_some_and(|family| !installed_fonts(cx).contains(family)) {
            return;
        }
        match target {
            FontTarget::Ui => self.preferences.ui_font_family = family,
            FontTarget::Code => self.preferences.code_font_family = family,
        }
        self.apply_typography(cx);
        self.save_preferences();
    }

    pub(super) fn render_font_family(&self, target: FontTarget, cx: &Context<Self>) -> AnyElement {
        let (id, label, family) = match target {
            FontTarget::Ui => ("choose-ui-font", "UI font", &cx.theme().font_family),
            FontTarget::Code => ("choose-code-font", "Code font", &cx.theme().mono_font_family),
        };
        let display = if family.as_str() == ".SystemUIFont" {
            if cfg!(target_os = "macos") {
                "System · SF Pro"
            } else {
                "System"
            }
        } else {
            family.as_str()
        };
        div()
            .flex()
            .w_full()
            .items_center()
            .justify_between()
            .gap_3()
            .py_2()
            .child(label)
            .child(
                desktop_button(id)
                    .label(display.to_string())
                    .accessibility_label(format!("Choose {label}, current: {display}"))
                    .ghost()
                    .on_click(cx.listener(move |view, _, window, cx| view.open_font_palette(target, window, cx))),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::resolve_font;

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
