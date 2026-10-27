use std::sync::Once;

use gpui_kit::component as gpui_component;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum UiLanguage {
    English,
    Vietnamese,
    Spanish,
    French,
    Japanese,
}

impl UiLanguage {
    pub(super) const ALL: [Self; 5] = [
        Self::English,
        Self::Vietnamese,
        Self::Spanish,
        Self::French,
        Self::Japanese,
    ];

    pub(super) fn from_saved(value: Option<&str>) -> Self {
        match value.map(str::trim) {
            Some("vi") => Self::Vietnamese,
            Some("es") => Self::Spanish,
            Some("fr") => Self::French,
            Some("ja") => Self::Japanese,
            _ => Self::English,
        }
    }

    pub(super) const fn locale(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::Vietnamese => "vi",
            Self::Spanish => "es",
            Self::French => "fr",
            Self::Japanese => "ja",
        }
    }

    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::Vietnamese => "Tiếng Việt",
            Self::Spanish => "Español",
            Self::French => "Français",
            Self::Japanese => "日本語",
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
        for language in UiLanguage::ALL {
            assert_eq!(UiLanguage::from_saved(Some(language.locale())), language);
        }
        assert_eq!(UiLanguage::from_saved(Some("xx")), UiLanguage::English);
    }

    #[test]
    fn app_catalog_covers_language_controls_and_kit_overrides() {
        assert_eq!(rust_i18n::t!("app.settings.title", locale = "en").to_string(), "Settings");
        assert_eq!(rust_i18n::t!("app.settings.title", locale = "vi").to_string(), "Cài đặt");
        assert_eq!(
            rust_i18n::t!("app.session.preparing_new_thread", locale = "en").to_string(),
            "Preparing a new thread…"
        );
        assert_eq!(
            rust_i18n::t!("app.session.preparing_new_thread", locale = "vi").to_string(),
            "Đang chuẩn bị một cuộc hội thoại mới…"
        );
        assert_eq!(rust_i18n::t!("gpui_component.Input.Copy", locale = "vi").to_string(), "Sao chép");
        for (locale, title, copy) in [
            ("es", "Configuración", "Copiar"),
            ("fr", "Réglages", "Copier"),
            ("ja", "設定", "コピー"),
        ] {
            assert_eq!(rust_i18n::t!("app.settings.title", locale = locale).to_string(), title);
            assert_eq!(rust_i18n::t!("gpui_component.Input.Copy", locale = locale).to_string(), copy);
            let history = rust_i18n::t!("app.activity.history.notice", locale = locale, count = 42).to_string();
            assert!(history.contains("42"), "{locale} must interpolate counts");
        }
    }

    #[test]
    fn welcome_copy_is_translated_and_actions_remain_distinct() {
        for (locale, morning, afternoon, evening) in [
            ("en", "Good morning", "Good afternoon", "Good evening"),
            ("vi", "Chào buổi sáng", "Chào buổi chiều", "Chào buổi tối"),
            ("es", "Buenos días", "Buenas tardes", "Buenas noches"),
            ("fr", "Bonjour", "Bon après-midi", "Bonsoir"),
            ("ja", "おはようございます", "こんにちは", "こんばんは"),
        ] {
            assert_eq!(rust_i18n::t!("app.welcome.greeting.morning", locale = locale).to_string(), morning);
            assert_eq!(rust_i18n::t!("app.welcome.greeting.afternoon", locale = locale).to_string(), afternoon);
            assert_eq!(rust_i18n::t!("app.welcome.greeting.evening", locale = locale).to_string(), evening);

            let actions = [
                rust_i18n::t!("app.welcome.action.build", locale = locale).to_string(),
                rust_i18n::t!("app.welcome.action.improve", locale = locale).to_string(),
                rust_i18n::t!("app.welcome.action.explore", locale = locale).to_string(),
            ];
            let workspace_actions = [
                rust_i18n::t!("app.welcome.action.build_in_workspace", locale = locale).to_string(),
                rust_i18n::t!("app.welcome.action.improve_in_workspace", locale = locale).to_string(),
                rust_i18n::t!("app.welcome.action.explore_in_workspace", locale = locale).to_string(),
            ];
            let calls_to_action = [
                rust_i18n::t!("app.welcome.cta.choose_workspace", locale = locale).to_string(),
                rust_i18n::t!("app.welcome.cta.open_project", locale = locale).to_string(),
                rust_i18n::t!("app.welcome.cta.select_folder", locale = locale).to_string(),
            ];
            for first in 0..actions.len() {
                for second in first + 1..actions.len() {
                    assert_ne!(actions[first], actions[second]);
                    assert_ne!(workspace_actions[first], workspace_actions[second]);
                    assert_ne!(calls_to_action[first], calls_to_action[second]);
                }
            }
        }
    }
}
