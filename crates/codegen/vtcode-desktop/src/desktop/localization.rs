use std::sync::Once;

use gpui_kit::component as gpui_component;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum UiLanguage {
    English,
    Vietnamese,
}

impl UiLanguage {
    pub(super) const ALL: [Self; 2] = [Self::English, Self::Vietnamese];

    pub(super) fn from_saved(value: Option<&str>) -> Self {
        match value.map(str::trim) {
            Some("vi") => Self::Vietnamese,
            _ => Self::English,
        }
    }

    pub(super) const fn locale(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::Vietnamese => "vi",
        }
    }

    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::Vietnamese => "Tiếng Việt",
        }
    }
}

static KIT_TRANSLATIONS: Once = Once::new();

/// Register app-owned overrides for GPUI Kit once, then keep both catalogs on
/// the same locale. `rust-i18n` locale changes are global and do not notify GPUI.
pub(super) fn initialize(language: UiLanguage) {
    KIT_TRANSLATIONS.call_once(|| rust_i18n::extend!(gpui_component));
    set_language(language);
}

pub(super) fn set_language(language: UiLanguage) {
    rust_i18n::set_locale(language.locale());
    gpui_component::set_locale(language.locale());
}

#[cfg(test)]
mod tests {
    use super::UiLanguage;

    #[test]
    fn saved_locale_values_are_validated_and_unknown_values_fall_back_to_english() {
        assert_eq!(UiLanguage::from_saved(None), UiLanguage::English);
        assert_eq!(UiLanguage::from_saved(Some("vi")), UiLanguage::Vietnamese);
        assert_eq!(UiLanguage::from_saved(Some(" vi ")), UiLanguage::Vietnamese);
        assert_eq!(UiLanguage::from_saved(Some("xx")), UiLanguage::English);
    }

    #[test]
    fn app_catalog_covers_language_controls_and_kit_overrides() {
        assert_eq!(rust_i18n::t!("app.settings.title", locale = "en").to_string(), "Settings");
        assert_eq!(rust_i18n::t!("app.settings.title", locale = "vi").to_string(), "Cài đặt");
        assert_eq!(rust_i18n::t!("gpui_component.Input.Copy", locale = "vi").to_string(), "Sao chép");
    }
}
