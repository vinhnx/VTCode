use std::sync::Once;

use gpui_kit::component as gpui_component;

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
    BrazilianPortuguese,
    Korean,
    Dutch,
}

impl UiLanguage {
    pub(super) const ALL: [Self; 11] = [
        Self::English,
        Self::Vietnamese,
        Self::Spanish,
        Self::French,
        Self::Japanese,
        Self::German,
        Self::Italian,
        Self::SimplifiedChinese,
        Self::BrazilianPortuguese,
        Self::Korean,
        Self::Dutch,
    ];

    pub(super) fn from_saved(value: Option<&str>) -> Self {
        match value.map(str::trim) {
            Some("vi") => Self::Vietnamese,
            Some("es") => Self::Spanish,
            Some("fr") => Self::French,
            Some("ja") => Self::Japanese,
            Some("de") => Self::German,
            Some("it") => Self::Italian,
            Some("zh-CN") => Self::SimplifiedChinese,
            Some("pt-BR") => Self::BrazilianPortuguese,
            Some("ko") => Self::Korean,
            Some("nl") => Self::Dutch,
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
            Self::German => "de",
            Self::Italian => "it",
            Self::SimplifiedChinese => "zh-CN",
            Self::BrazilianPortuguese => "pt-BR",
            Self::Korean => "ko",
            Self::Dutch => "nl",
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
            Self::BrazilianPortuguese => "Português (Brasil)",
            Self::Korean => "한국어",
            Self::Dutch => "Nederlands",
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
        for (locale, title, copy, select_all, remove) in [
            ("es", "Configuración", "Copiar", "Seleccionar todo", "Quitar"),
            ("fr", "Réglages", "Copier", "Tout sélectionner", "Supprimer"),
            ("ja", "設定", "コピー", "すべて選択", "削除"),
            ("de", "Einstellungen", "Kopieren", "Alles auswählen", "Entfernen"),
            ("it", "Impostazioni", "Copia", "Seleziona tutto", "Rimuovi"),
            ("zh-CN", "设置", "复制", "全选", "移除"),
            ("pt-BR", "Configurações", "Copiar", "Selecionar tudo", "Remover"),
            ("ko", "설정", "복사", "모두 선택", "제거"),
            ("nl", "Instellingen", "Kopiëren", "Alles selecteren", "Verwijderen"),
        ] {
            assert_eq!(rust_i18n::t!("app.settings.title", locale = locale).to_string(), title);
            assert_eq!(rust_i18n::t!("gpui_component.Input.Copy", locale = locale).to_string(), copy);
            assert_eq!(rust_i18n::t!("gpui_component.Input.Select All", locale = locale).to_string(), select_all);
            assert_eq!(rust_i18n::t!("gpui_component.Attachment.Remove", locale = locale).to_string(), remove);
            let history = rust_i18n::t!("app.activity.history.notice", locale = locale, count = 42).to_string();
            assert!(history.contains("42"), "{locale} must interpolate counts");
        }
    }

    #[test]
    fn settings_surface_translations_cover_every_supported_language() {
        let expected = [
            (
                "en",
                "Light",
                "Edit the Light appearance profile",
                "Edit the Light appearance profile (selected)",
                "Display",
                "Connected",
            ),
            (
                "vi",
                "Sáng",
                "Chỉnh cấu hình giao diện Sáng",
                "Chỉnh cấu hình giao diện Sáng (đã chọn)",
                "Hiển thị",
                "Đã kết nối",
            ),
            (
                "es",
                "Claro",
                "Editar el perfil de apariencia Claro",
                "Editar el perfil de apariencia Claro (seleccionado)",
                "Pantalla",
                "Conectado",
            ),
            (
                "fr",
                "Clair",
                "Modifier le profil d’apparence Clair",
                "Modifier le profil d’apparence Clair (sélectionné)",
                "Affichage",
                "Connecté",
            ),
            (
                "ja",
                "ライト",
                "ライト外観プロファイルを編集",
                "選択中のライト外観プロファイルを編集",
                "ディスプレイ",
                "接続済み",
            ),
            (
                "de",
                "Hell",
                "Das Erscheinungsbild-Profil „Hell“ bearbeiten",
                "Das Erscheinungsbild-Profil „Hell“ bearbeiten (ausgewählt)",
                "Anzeige",
                "Verbunden",
            ),
            (
                "it",
                "Chiaro",
                "Modifica il profilo di aspetto Chiaro",
                "Modifica il profilo di aspetto Chiaro (selezionato)",
                "Schermo",
                "Connesso",
            ),
            ("zh-CN", "浅色", "编辑 浅色 外观配置", "编辑 浅色 外观配置（已选中）", "显示", "已连接"),
            (
                "pt-BR",
                "Claro",
                "Editar o perfil de aparência Claro",
                "Editar o perfil de aparência Claro (selecionado)",
                "Exibição",
                "Conectado",
            ),
            (
                "ko",
                "라이트",
                "라이트 모양 프로필 편집",
                "라이트 모양 프로필 편집 (선택됨)",
                "디스플레이",
                "연결됨",
            ),
            (
                "nl",
                "Licht",
                "Het profiel voor de weergave Licht bewerken",
                "Het profiel voor de weergave Licht bewerken (geselecteerd)",
                "Weergave",
                "Verbonden",
            ),
        ];

        for (locale, light, edit_profile, edit_profile_selected, display, connected) in expected {
            assert_eq!(UiLanguage::from_saved(Some(locale)).locale(), locale);
            assert_eq!(
                rust_i18n::t!("app.settings.appearance.edit_profile", locale = locale, profile = light).to_string(),
                edit_profile,
                "profile action must be localized in {locale}"
            );
            assert_eq!(
                rust_i18n::t!("app.settings.appearance.edit_profile_selected", locale = locale, profile = light)
                    .to_string(),
                edit_profile_selected,
                "selected profile state must be localized in {locale}"
            );
            assert_eq!(rust_i18n::t!("app.settings.accessibility.display", locale = locale).to_string(), display);
            assert_eq!(rust_i18n::t!("app.settings.connections.connected", locale = locale).to_string(), connected);
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
            ("de", "Guten Morgen", "Guten Tag", "Guten Abend"),
            ("it", "Buongiorno", "Buon pomeriggio", "Buonasera"),
            ("zh-CN", "早上好", "下午好", "晚上好"),
            ("pt-BR", "Bom dia", "Boa tarde", "Boa noite"),
            ("ko", "좋은 아침입니다", "안녕하세요", "좋은 저녁입니다"),
            ("nl", "Goedemorgen", "Goedemiddag", "Goedenavond"),
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
