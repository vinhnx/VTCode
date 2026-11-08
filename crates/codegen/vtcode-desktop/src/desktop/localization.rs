use std::sync::Once;

use gpui_kit::SharedString;
use gpui_kit::component as gpui_component;
use gpui_kit::component::IndexPath;
use gpui_kit::component::searchable_list::SearchableListItem;
use gpui_kit::component::select::{SearchableVec, SelectState};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum UiLanguage {
    English,
    Vietnamese,
    Spanish,
    French,
    Japanese,
    German,
    Italian,
    SimplifiedChinese,
    TraditionalChineseTaiwan,
    TraditionalChineseHongKong,
    BrazilianPortuguese,
    Korean,
    Dutch,
    Russian,
    Indonesian,
    Turkish,
    Polish,
    Hindi,
    Ukrainian,
    Swedish,
}

impl UiLanguage {
    pub(super) const ALL: [Self; 20] = [
        Self::English,
        Self::Vietnamese,
        Self::Spanish,
        Self::French,
        Self::Japanese,
        Self::German,
        Self::Italian,
        Self::SimplifiedChinese,
        Self::TraditionalChineseTaiwan,
        Self::TraditionalChineseHongKong,
        Self::BrazilianPortuguese,
        Self::Korean,
        Self::Dutch,
        Self::Russian,
        Self::Indonesian,
        Self::Turkish,
        Self::Polish,
        Self::Hindi,
        Self::Ukrainian,
        Self::Swedish,
    ];

    pub(super) fn from_saved(value: Option<&str>) -> Self {
        let Some(value) = value.map(str::trim) else {
            return Self::English;
        };
        let locale = value.replace('_', "-");
        if locale
            .split('-')
            .any(|part| part.is_empty() || !part.chars().all(|ch| ch.is_ascii_alphanumeric()))
        {
            return Self::English;
        }
        Self::ALL
            .into_iter()
            .find(|language| language.locale().eq_ignore_ascii_case(&locale))
            .or_else(|| {
                let base = locale.split('-').next()?;
                Self::ALL
                    .into_iter()
                    .find(|language| language.locale().len() == 2 && language.locale().eq_ignore_ascii_case(base))
            })
            .unwrap_or(Self::English)
    }

    pub(super) const fn locale(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::Vietnamese => "vi",
            Self::Spanish => "es",
            Self::French => "fr",
            Self::Japanese => "ja",
            Self::German => "de",
            Self::Italian => "it",
            Self::SimplifiedChinese => "zh-CN",
            Self::TraditionalChineseTaiwan => "zh-TW",
            Self::TraditionalChineseHongKong => "zh-HK",
            Self::BrazilianPortuguese => "pt-BR",
            Self::Korean => "ko",
            Self::Dutch => "nl",
            Self::Russian => "ru",
            Self::Indonesian => "id",
            Self::Turkish => "tr",
            Self::Polish => "pl",
            Self::Hindi => "hi",
            Self::Ukrainian => "uk",
            Self::Swedish => "sv",
        }
    }

    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::Vietnamese => "Tiếng Việt",
            Self::Spanish => "Español",
            Self::French => "Français",
            Self::Japanese => "日本語",
            Self::German => "Deutsch",
            Self::Italian => "Italiano",
            Self::SimplifiedChinese => "简体中文",
            Self::TraditionalChineseTaiwan => "繁體中文（台灣）",
            Self::TraditionalChineseHongKong => "繁體中文（香港）",
            Self::BrazilianPortuguese => "Português (Brasil)",
            Self::Korean => "한국어",
            Self::Dutch => "Nederlands",
            Self::Russian => "Русский",
            Self::Indonesian => "Bahasa Indonesia",
            Self::Turkish => "Türkçe",
            Self::Polish => "Polski",
            Self::Hindi => "हिन्दी",
            Self::Ukrainian => "Українська",
            Self::Swedish => "Svenska",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct UiLanguageOption(UiLanguage);

impl UiLanguageOption {
    pub(super) fn all() -> SearchableVec<Self> {
        SearchableVec::new(UiLanguage::ALL.map(Self).to_vec())
    }

    pub(super) fn selected_index(language: UiLanguage) -> Option<IndexPath> {
        UiLanguage::ALL
            .iter()
            .position(|option| *option == language)
            .map(IndexPath::new)
    }
}

impl SearchableListItem for UiLanguageOption {
    type Value = UiLanguage;

    fn title(&self) -> SharedString {
        self.0.label().into()
    }

    fn render(&self, _: &mut gpui_kit::Window, _: &mut gpui_kit::App) -> impl gpui_kit::IntoElement {
        super::accessible_select_option(format!("language-option-{}", self.0.locale()), self.title())
    }

    fn value(&self) -> &Self::Value {
        &self.0
    }

    fn matches(&self, query: &str) -> bool {
        let query = query.trim().to_lowercase();
        self.0.label().to_lowercase().contains(&query) || self.0.locale().to_lowercase().contains(&query)
    }
}

pub(super) type UiLanguageSelectState = SelectState<SearchableVec<UiLanguageOption>>;

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
mod tests;
#[cfg(test)]
mod tests_appearance;
#[cfg(test)]
mod tests_browser;
#[cfg(test)]
mod tests_changelog;
#[cfg(test)]
mod tests_composer;
#[cfg(test)]
mod tests_configuration;
#[cfg(test)]
mod tests_inspector;
#[cfg(test)]
mod tests_locales;
#[cfg(test)]
mod tests_misc;
#[cfg(test)]
mod tests_navigation;
#[cfg(test)]
mod tests_service;
#[cfg(test)]
mod tests_session;
#[cfg(test)]
mod tests_shortcuts;
#[cfg(test)]
mod tests_transcript;
#[cfg(test)]
mod tests_untranslated;
#[cfg(test)]
mod tests_webmcp;
#[cfg(test)]
mod tests_workspace;
