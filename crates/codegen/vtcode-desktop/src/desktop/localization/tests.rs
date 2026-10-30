use super::{UiLanguage, UiLanguageOption};

#[test]
fn menu_bar_copy_is_localized_for_every_language() {
    for language in UiLanguage::ALL {
        let locale = language.locale();
        for key in [
            "app.settings.menu_bar.title",
            "app.settings.menu_bar.description",
            "app.settings.menu_bar.unavailable",
            "app.settings.menu_bar.show",
            "app.settings.menu_bar.quit",
        ] {
            let text = rust_i18n::t!(key, locale = locale).to_string();
            assert!(!text.is_empty(), "{locale}/{key}");
            if language != UiLanguage::English {
                assert_ne!(text, rust_i18n::t!(key, locale = "en").to_string(), "{locale}/{key}");
            }
        }
    }
}

#[test]
fn storage_copy_is_localized_for_every_language() {
    const KEYS: [&str; 31] = [
        "app.settings.tab.storage",
        "app.settings.storage.device_title",
        "app.settings.storage.device_description",
        "app.settings.storage.used_by",
        "app.settings.storage.refresh",
        "app.settings.storage.loading",
        "app.settings.storage.retry",
        "app.settings.storage.sessions",
        "app.settings.storage.worktrees",
        "app.settings.storage.memory",
        "app.settings.storage.other",
        "app.settings.storage.usage",
        "app.settings.storage.cleanup_title",
        "app.settings.storage.cleanup_description",
        "app.settings.storage.review",
        "app.settings.storage.worktrees_title",
        "app.settings.storage.worktrees_description",
        "app.settings.storage.manage",
        "app.settings.storage.back",
        "app.settings.storage.no_sessions",
        "app.settings.storage.no_worktrees",
        "app.settings.storage.current_session",
        "app.settings.storage.dirty",
        "app.settings.storage.delete",
        "app.settings.storage.remove",
        "app.settings.storage.delete_title",
        "app.settings.storage.delete_detail",
        "app.settings.storage.remove_title",
        "app.settings.storage.remove_detail",
        "app.settings.storage.remove_dirty_detail",
        "app.settings.storage.cancel",
    ];
    // Legitimate cognates: correct translations spelled like the English source.
    const COGNATES: [(&str, &str); 7] = [
        ("fr", "app.settings.storage.sessions"),
        ("es", "app.settings.storage.worktrees"),
        ("fr", "app.settings.storage.worktrees"),
        ("de", "app.settings.storage.worktrees"),
        ("pt-BR", "app.settings.storage.worktrees"),
        ("nl", "app.settings.storage.worktrees"),
        ("sv", "app.settings.storage.worktrees"),
    ];
    for language in UiLanguage::ALL {
        let locale = language.locale();
        for key in KEYS {
            let text = rust_i18n::t!(key, locale = locale).to_string();
            assert!(!text.is_empty(), "{locale}/{key}");
            if language != UiLanguage::English && !COGNATES.contains(&(locale, key)) {
                assert_ne!(text, rust_i18n::t!(key, locale = "en").to_string(), "{locale}/{key}");
            }
        }
    }
    for language in UiLanguage::ALL {
        let locale = language.locale();
        let used = rust_i18n::t!("app.settings.storage.used_by", locale = locale, size = "1 GB").to_string();
        assert!(used.contains("1 GB"), "{locale}");
        let title = rust_i18n::t!("app.settings.storage.delete_title", locale = locale, name = "demo").to_string();
        assert!(title.contains("demo"), "{locale}");
        let usage = rust_i18n::t!("app.settings.storage.usage", locale = locale, breakdown = "demo").to_string();
        assert!(usage.contains("demo"), "{locale}");
        let removed = rust_i18n::t!("app.settings.storage.remove_title", locale = locale, name = "demo").to_string();
        assert!(removed.contains("demo"), "{locale}");
    }
}

#[test]
fn contrast_description_is_localized_for_every_language() {
    for language in UiLanguage::ALL {
        let locale = language.locale();
        let label = rust_i18n::t!("app.settings.appearance.contrast", locale = locale).to_string();
        let description = rust_i18n::t!("app.settings.appearance.contrast_description", locale = locale).to_string();
        assert!(!label.is_empty() && !description.is_empty(), "{locale}");
        if language != UiLanguage::English {
            assert_ne!(description, "Increase text contrast while preserving the selected theme", "{locale}");
        }
    }
}

#[test]
fn translucent_sidebar_label_is_localized_for_every_language() {
    for language in UiLanguage::ALL {
        let value =
            rust_i18n::t!("app.settings.appearance.translucent_sidebar", locale = language.locale()).to_string();
        assert!(!value.is_empty());
        if language != UiLanguage::English {
            assert_ne!(value, "Translucent sidebar", "{}", language.locale());
        }
    }
}

#[test]
fn appearance_and_cli_sync_copy_is_localized_for_every_language() {
    for language in UiLanguage::ALL {
        let locale = language.locale();
        for key in [
            "app.settings.appearance.status_bar",
            "app.settings.appearance.reduce_motion",
            "app.settings.appearance.reduce_motion_description",
            "app.settings.cli_sync.group",
            "app.settings.cli_sync.sync_theme",
            "app.settings.cli_sync.reload_settings",
            "app.settings.cli_sync.use_theme",
            "app.settings.cli_sync.following",
            "app.settings.cli_sync.desktop_override",
            "app.settings.cli_sync.disconnected",
            "app.settings.cli_sync.theme_unavailable",
        ] {
            let value = rust_i18n::t!(key, locale = locale).to_string();
            assert!(!value.is_empty(), "{locale}/{key}");
            if language != UiLanguage::English {
                assert_ne!(value, rust_i18n::t!(key, locale = "en").to_string(), "{locale}/{key}");
            }
        }
        let status = rust_i18n::t!(
            "app.settings.cli_sync.theme_status",
            locale = locale,
            theme = "demo-theme",
            mode = "demo-mode"
        )
        .to_string();
        assert!(status.contains("demo-theme"), "{locale}");
        assert!(status.contains("demo-mode"), "{locale}");
    }
}

#[test]
fn license_settings_copy_is_translated_and_interpolates_errors() {
    for language in UiLanguage::ALL {
        let locale = language.locale();
        for key in [
            "app.settings.licenses.title",
            "app.settings.licenses.description",
            "app.settings.licenses.view",
        ] {
            let value = rust_i18n::t!(key, locale = locale).to_string();
            assert!(!value.is_empty(), "{locale}/{key}");
            if language != UiLanguage::English {
                assert_ne!(value, rust_i18n::t!(key, locale = "en").to_string(), "{locale}/{key}");
            }
        }
        let error = rust_i18n::t!("app.settings.licenses.error", locale = locale, error = "example-error").to_string();
        assert!(error.contains("example-error"), "{locale}: {error}");
        if language != UiLanguage::English {
            assert_ne!(error, "Could not open third-party notices: example-error", "{locale}");
        }
    }
}
use gpui_kit::component::searchable_list::SearchableListItem;

#[test]
fn saved_locale_values_are_validated_and_unknown_values_fall_back_to_english() {
    assert_eq!(UiLanguage::from_saved(None), UiLanguage::English);
    assert_eq!(UiLanguage::from_saved(Some("vi")), UiLanguage::Vietnamese);
    assert_eq!(UiLanguage::from_saved(Some(" vi ")), UiLanguage::Vietnamese);
    let available_locales = rust_i18n::available_locales!();
    for language in UiLanguage::ALL {
        assert_eq!(UiLanguage::from_saved(Some(language.locale())), language);
        assert!(
            available_locales.iter().any(|locale| locale.as_ref() == language.locale()),
            "{} is not present in the compiled catalog",
            language.locale()
        );
    }
    assert_eq!(UiLanguage::from_saved(Some("xx")), UiLanguage::English);
    assert_eq!(UiLanguage::from_saved(Some("fr-CA")), UiLanguage::French);
    assert_eq!(UiLanguage::from_saved(Some("ES_mx")), UiLanguage::Spanish);
    assert_eq!(UiLanguage::from_saved(Some("zh_hk")), UiLanguage::TraditionalChineseHongKong);
    assert_eq!(UiLanguage::from_saved(Some("pt-PT")), UiLanguage::English);
    assert_eq!(UiLanguage::from_saved(Some("fr-")), UiLanguage::English);
    assert_eq!(UiLanguage::from_saved(Some("sv-SE")), UiLanguage::Swedish);
    assert_eq!(rust_i18n::t!("app.settings.title", locale = "sv").to_string(), "Inställningar");
    assert_eq!(rust_i18n::t!("gpui_component.Input.Copy", locale = "sv").to_string(), "Kopiera");
}

#[test]
fn language_options_match_native_names_and_locale_codes() {
    let hindi = UiLanguageOption(UiLanguage::Hindi);
    assert!(hindi.matches("हिन्दी"));
    assert!(hindi.matches("HI"));
    let ukrainian = UiLanguageOption(UiLanguage::Ukrainian);
    assert!(ukrainian.matches("Українська"));
    assert!(ukrainian.matches("UK"));
    assert!(!ukrainian.matches("Swedish"));
    assert_eq!(UiLanguageOption::selected_index(UiLanguage::Hindi).map(|index| index.row), Some(17));
    assert_eq!(UiLanguageOption::selected_index(UiLanguage::Ukrainian).map(|index| index.row), Some(18));
}

#[test]
fn app_catalog_covers_language_controls_and_kit_overrides() {
    assert_eq!(rust_i18n::t!("app.settings.title", locale = "en").to_string(), "Settings");
    assert_eq!(rust_i18n::t!("app.settings.title", locale = "vi").to_string(), "Cài đặt");
    assert_eq!(rust_i18n::t!("app.settings.visual_style", locale = "en").to_string(), "Visual style");
    assert_eq!(rust_i18n::t!("app.settings.visual_style", locale = "vi").to_string(), "Phong cách giao diện");
    assert_eq!(
        rust_i18n::t!("app.session.preparing_new_thread", locale = "en").to_string(),
        "Preparing a new thread…"
    );
    assert_eq!(
        rust_i18n::t!("app.session.preparing_new_thread", locale = "vi").to_string(),
        "Đang chuẩn bị một cuộc hội thoại mới…"
    );
    assert_eq!(rust_i18n::t!("gpui_component.Input.Copy", locale = "vi").to_string(), "Sao chép");
    for (locale, title, visual_style, copy, select_all, remove) in [
        ("es", "Configuración", "Estilo visual", "Copiar", "Seleccionar todo", "Quitar"),
        ("fr", "Réglages", "Style visuel", "Copier", "Tout sélectionner", "Supprimer"),
        ("ja", "設定", "ビジュアルスタイル", "コピー", "すべて選択", "削除"),
        ("de", "Einstellungen", "Visueller Stil", "Kopieren", "Alles auswählen", "Entfernen"),
        ("it", "Impostazioni", "Stile visivo", "Copia", "Seleziona tutto", "Rimuovi"),
        ("zh-CN", "设置", "视觉风格", "复制", "全选", "移除"),
        ("zh-TW", "設定", "視覺風格", "複製", "全選", "移除"),
        ("zh-HK", "設定", "視覺風格", "複製", "全選", "移除"),
        ("pt-BR", "Configurações", "Estilo visual", "Copiar", "Selecionar tudo", "Remover"),
        ("ko", "설정", "시각적 스타일", "복사", "모두 선택", "제거"),
        ("nl", "Instellingen", "Visuele stijl", "Kopiëren", "Alles selecteren", "Verwijderen"),
        ("ru", "Настройки", "Визуальный стиль", "Копировать", "Выделить всё", "Удалить"),
        ("id", "Pengaturan", "Gaya visual", "Salin", "Pilih Semua", "Hapus"),
        ("tr", "Ayarlar", "Görsel stil", "Kopyala", "Tümünü Seç", "Kaldır"),
        ("pl", "Ustawienia", "Styl wizualny", "Kopiuj", "Zaznacz wszystko", "Usuń"),
        ("hi", "सेटिंग्स", "दृश्य शैली", "कॉपी करें", "सभी चुनें", "हटाएँ"),
        ("uk", "Налаштування", "Візуальний стиль", "Копіювати", "Вибрати все", "Видалити"),
    ] {
        assert_eq!(rust_i18n::t!("app.settings.title", locale = locale).to_string(), title);
        assert_eq!(rust_i18n::t!("app.settings.visual_style", locale = locale).to_string(), visual_style);
        assert_eq!(rust_i18n::t!("gpui_component.Input.Copy", locale = locale).to_string(), copy);
        assert_eq!(rust_i18n::t!("gpui_component.Input.Select All", locale = locale).to_string(), select_all);
        assert_eq!(rust_i18n::t!("gpui_component.Attachment.Remove", locale = locale).to_string(), remove);
        let history = rust_i18n::t!("app.activity.history.notice", locale = locale, count = 42).to_string();
        assert!(history.contains("42"), "{locale} must interpolate counts");
    }
    assert_eq!(rust_i18n::t!("gpui_component.Attachment.Retry", locale = "uk").to_string(), "Повторити");
}

#[test]
fn notification_settings_copy_covers_every_language() {
    assert_eq!(rust_i18n::t!("app.settings.tab.notifications", locale = "en").to_string(), "Notifications");
    assert_eq!(rust_i18n::t!("app.settings.tab.notifications", locale = "vi").to_string(), "Thông báo");
    assert_eq!(
        rust_i18n::t!("app.settings.notifications.enable", locale = "en").to_string(),
        "Enable notifications"
    );
    assert_eq!(rust_i18n::t!("app.settings.notifications.enable", locale = "vi").to_string(), "Bật thông báo");
    assert_eq!(
        rust_i18n::t!("app.settings.notifications.turn_completion.title", locale = "en").to_string(),
        "Turn completion notifications"
    );
    assert_eq!(
        rust_i18n::t!("app.settings.notifications.turn_completion.title", locale = "vi").to_string(),
        "Tác vụ agent hoàn thành"
    );
    assert_eq!(
        rust_i18n::t!("app.settings.notifications.permission", locale = "en").to_string(),
        "Enable permission notifications"
    );
    assert_eq!(
        rust_i18n::t!("app.settings.notifications.permission", locale = "vi").to_string(),
        "Bật thông báo xin phép"
    );
    assert_eq!(
        rust_i18n::t!("app.settings.notifications.up_to_date", locale = "en").to_string(),
        "You're up to date"
    );
    assert_eq!(
        rust_i18n::t!("app.settings.notifications.up_to_date", locale = "vi").to_string(),
        "Bạn đang dùng bản mới nhất"
    );

    macro_rules! assert_catalog_key {
        ($key:literal) => {
            for language in UiLanguage::ALL {
                let locale = language.locale();
                let translated = rust_i18n::t!($key, locale = locale).to_string();
                assert!(!translated.is_empty(), "{locale}/{} is empty", $key);
                // French "Notifications" is a genuine cognate of the
                // English group/tab title, not an untranslated entry.
                let cognate = locale == "fr"
                    && ($key == "app.settings.tab.notifications" || $key == "app.settings.notifications.group");
                if language != UiLanguage::English && !cognate {
                    assert_ne!(
                        translated,
                        rust_i18n::t!($key, locale = "en").to_string(),
                        "{locale}/{} must be translated",
                        $key
                    );
                }
            }
        };
    }

    assert_catalog_key!("app.settings.tab.notifications");
    assert_catalog_key!("app.settings.notifications.group");
    assert_catalog_key!("app.settings.notifications.enable");
    assert_catalog_key!("app.settings.notifications.enable_description");
    assert_catalog_key!("app.settings.notifications.turn_completion.title");
    assert_catalog_key!("app.settings.notifications.turn_completion.description");
    assert_catalog_key!("app.settings.notifications.turn_completion.always");
    assert_catalog_key!("app.settings.notifications.turn_completion.unfocused");
    assert_catalog_key!("app.settings.notifications.turn_completion.never");
    assert_catalog_key!("app.settings.notifications.permission");
    assert_catalog_key!("app.settings.notifications.permission_description");
    assert_catalog_key!("app.settings.notifications.questions");
    assert_catalog_key!("app.settings.notifications.questions_description");
    assert_catalog_key!("app.settings.notifications.updates");
    assert_catalog_key!("app.settings.notifications.updates_description");
    assert_catalog_key!("app.settings.notifications.updates_status");
    assert_catalog_key!("app.settings.notifications.check_for_updates");
    assert_catalog_key!("app.settings.notifications.up_to_date");
    assert_catalog_key!("app.settings.notifications.update_available");
    assert_catalog_key!("app.settings.notifications.status_unavailable");
}

#[test]
fn transcript_and_configuration_copy_are_localized_for_every_language() {
    macro_rules! assert_catalog_key {
        ($key:literal) => {
            for language in UiLanguage::ALL {
                let locale = language.locale();
                let translated = rust_i18n::t!($key, locale = locale).to_string();
                assert!(!translated.is_empty(), "{locale}/{} is empty", $key);
                if language != UiLanguage::English {
                    assert_ne!(
                        translated,
                        rust_i18n::t!($key, locale = "en").to_string(),
                        "{locale}/{} fell back to English",
                        $key
                    );
                }
            }
        };
    }

    assert_catalog_key!("app.palette.title.configuration");
    assert_catalog_key!("app.transcript.history_limit_reached");
    assert_catalog_key!("app.transcript.load_earlier_tooltip");
    assert_catalog_key!("app.transcript.compact_boundary_title");
    assert_catalog_key!("app.transcript.compact_boundary_body");
    assert_catalog_key!("app.transcript.context_reset_title");
    assert_catalog_key!("app.transcript.context_reset_body");

    for language in UiLanguage::ALL {
        let locale = language.locale();
        let compacted = rust_i18n::t!(
            "app.transcript.compact_boundary_body",
            locale = locale,
            original = 120,
            compacted = 8,
            trigger = "auto",
            mode = "local"
        )
        .to_string();
        for expected in ["120", "8", "auto", "local"] {
            assert!(compacted.contains(expected), "{locale} dropped {expected}");
        }
        let reset = rust_i18n::t!(
            "app.transcript.context_reset_body",
            locale = locale,
            trigger = "plan approval",
            previous = 87
        )
        .to_string();
        for expected in ["plan approval", "87"] {
            assert!(reset.contains(expected), "{locale} dropped {expected}");
        }
    }
}

#[test]
fn runtime_control_recovery_copy_is_localized_for_every_language() {
    macro_rules! assert_catalog_key {
        ($key:literal) => {
            for language in UiLanguage::ALL {
                let locale = language.locale();
                let translated = rust_i18n::t!($key, locale = locale).to_string();
                assert!(!translated.is_empty(), "{locale}/{} is empty", $key);
                if language != UiLanguage::English {
                    assert_ne!(
                        translated,
                        rust_i18n::t!($key, locale = "en").to_string(),
                        "{locale}/{} fell back to English",
                        $key
                    );
                }
            }
        };
    }

    assert_catalog_key!("app.errors.fast_processing_unavailable");
    assert_catalog_key!("app.errors.model_selection_runtime_changing");
    assert_catalog_key!("app.errors.runtime_options_unavailable");
    assert_catalog_key!("app.errors.desktop_command_unavailable");
    assert_catalog_key!("app.errors.request_queue_unavailable");
}

#[test]
fn webmcp_notice_copy_is_translated_for_every_language() {
    macro_rules! assert_catalog_key {
        ($key:literal) => {
            for language in UiLanguage::ALL {
                let locale = language.locale();
                let translated = rust_i18n::t!($key, locale = locale).to_string();
                assert!(!translated.is_empty(), "{locale}/{} is empty", $key);
                if language != UiLanguage::English {
                    assert_ne!(
                        translated,
                        rust_i18n::t!($key, locale = "en").to_string(),
                        "{locale}/{} fell back to English",
                        $key
                    );
                }
            }
        };
    }

    assert_catalog_key!("app.webmcp_notice.commands_help");
    assert_catalog_key!("app.webmcp_notice.disconnected");
    assert_catalog_key!("app.webmcp_notice.invalid_status");
    assert_catalog_key!("app.webmcp_notice.pairing_created");
    assert_catalog_key!("app.webmcp_notice.invalid_pairing");
    assert_catalog_key!("app.webmcp_notice.stopped");
}

#[test]
fn terminal_handoff_copy_is_translated_for_every_language() {
    assert_eq!(
        rust_i18n::t!("app.settings.connections.terminal", locale = "en").to_string(),
        "Continue in terminal"
    );
    assert_eq!(
        rust_i18n::t!("app.settings.connections.terminal", locale = "vi").to_string(),
        "Tiếp tục trong terminal"
    );
    for language in UiLanguage::ALL {
        let locale = language.locale();
        let translated = rust_i18n::t!("app.settings.connections.terminal", locale = locale).to_string();
        assert!(!translated.is_empty(), "{locale}/terminal is empty");
        if language != UiLanguage::English {
            assert_ne!(
                translated,
                rust_i18n::t!("app.settings.connections.terminal", locale = "en").to_string(),
                "{locale}/terminal fell back to English"
            );
        }
    }
}

#[test]
fn workspace_and_composer_copy_covers_every_supported_language() {
    let expected = [
        ("en", "Workspace", "New thread", "Add files and more @", "Type a request…"),
        ("vi", "Không gian làm việc", "Cuộc hội thoại mới", "Thêm tệp và nội dung khác @", "Nhập yêu cầu…"),
        ("es", "Espacio de trabajo", "Nuevo hilo", "Añadir archivos y más @", "Escribe una solicitud…"),
        (
            "fr",
            "Espace de travail",
            "Nouvelle conversation",
            "Ajouter des fichiers et plus @",
            "Saisissez une demande…",
        ),
        ("ja", "ワークスペース", "新しいスレッド", "ファイルなどを追加 @", "リクエストを入力…"),
        ("de", "Arbeitsbereich", "Neuer Chat", "Dateien und mehr hinzufügen @", "Anfrage eingeben…"),
        ("it", "Spazio di lavoro", "Nuova conversazione", "Aggiungi file e altro @", "Digita una richiesta…"),
        ("zh-CN", "工作区", "新对话", "添加文件等 @", "输入请求…"),
        ("zh-TW", "工作區", "新對話", "新增檔案等 @", "輸入請求…"),
        ("zh-HK", "工作區", "新對話", "新增檔案等 @", "輸入請求…"),
        (
            "pt-BR",
            "Espaço de trabalho",
            "Nova conversa",
            "Adicionar arquivos e mais @",
            "Digite uma solicitação…",
        ),
        ("ko", "작업 공간", "새 대화", "파일 및 추가 항목 @", "요청을 입력하세요…"),
        ("nl", "Werkruimte", "Nieuwe thread", "Bestanden en meer toevoegen @", "Typ een verzoek…"),
        ("ru", "Рабочее пространство", "Новый диалог", "Добавить файлы и другое @", "Введите запрос…"),
        ("id", "Ruang kerja", "Percakapan baru", "Tambahkan file dan lainnya @", "Ketik permintaan…"),
        ("tr", "Çalışma alanı", "Yeni sohbet", "Dosya ve daha fazlasını ekle @", "Bir istek yazın…"),
        ("pl", "Obszar roboczy", "Nowy wątek", "Dodaj pliki i więcej @", "Wpisz prośbę…"),
        ("hi", "वर्कस्पेस", "नया थ्रेड", "फ़ाइलें और अन्य चीज़ें जोड़ें @", "एक अनुरोध लिखें…"),
        ("uk", "Робочий простір", "Нова розмова", "Додати файли та інше @", "Введіть запит…"),
    ];

    for (locale, workspace, new_thread, add_context, placeholder) in expected {
        assert_eq!(rust_i18n::t!("app.workspace.section", locale = locale).to_string(), workspace);
        assert_eq!(rust_i18n::t!("app.navigation.new_thread", locale = locale).to_string(), new_thread);
        assert_eq!(rust_i18n::t!("app.composer.add_context", locale = locale).to_string(), add_context);
        assert_eq!(rust_i18n::t!("app.composer.input_placeholder", locale = locale).to_string(), placeholder);
        let current_workspace =
            rust_i18n::t!("app.workspace.current_label", locale = locale, workspace = "vtcode").to_string();
        assert!(current_workspace.starts_with("vtcode"), "workspace name must interpolate in {locale}");
    }
}

/// The desktop composer placeholder tracks the CLI bootstrap prompt.
///
/// The CLI appends terminal shortcut hints that do not apply to the
/// desktop, so the desktop keeps only the shared lead text.
#[test]
fn composer_placeholder_matches_cli_bootstrap_lead() {
    let desktop = rust_i18n::t!("app.composer.input_placeholder", locale = "en").to_string();
    assert!(
        vtcode_config::constants::ui::CHAT_INPUT_PLACEHOLDER_BOOTSTRAP.starts_with(desktop.as_str()),
        "desktop placeholder {desktop:?} drifted from CLI bootstrap {:?}",
        vtcode_config::constants::ui::CHAT_INPUT_PLACEHOLDER_BOOTSTRAP
    );
}

#[test]
fn workspace_review_surface_is_translated_and_interpolates_values() {
    macro_rules! assert_translated {
        ($locale:expr, $key:literal) => {{
            let translated = rust_i18n::t!($key, locale = $locale).to_string();
            assert!(!translated.is_empty(), "{}/{} is empty", $locale, $key);
            if $locale != "en" {
                let english = rust_i18n::t!($key, locale = "en").to_string();
                assert_ne!(translated, english, "{}/{} fell back to English", $locale, $key);
            }
        }};
    }

    for language in UiLanguage::ALL {
        let locale = language.locale();
        assert_translated!(locale, "app.workspace_review.review_working_tree");
        assert_translated!(locale, "app.workspace_review.title");
        assert_translated!(locale, "app.workspace_review.description");
        assert_translated!(locale, "app.workspace_review.session_title");
        assert_translated!(locale, "app.workspace_review.session_description");
        assert_translated!(locale, "app.workspace_review.close_review");
        assert_translated!(locale, "app.workspace_review.expanded_in_inspector");
        assert_translated!(locale, "app.workspace_review.folder_optional");
        assert_translated!(locale, "app.workspace_review.include_untracked");
        assert_translated!(locale, "app.workspace_review.refresh_tooltip");
        assert_translated!(locale, "app.workspace_review.refreshing");
        assert_translated!(locale, "app.workspace_review.load_changes");
        assert_translated!(locale, "app.workspace_review.refresh");
        assert_translated!(locale, "app.workspace_review.diff_layout");
        assert_translated!(locale, "app.workspace_review.unified");
        assert_translated!(locale, "app.workspace_review.split");
        assert_translated!(locale, "app.workspace_review.copy_preview");
        assert_translated!(locale, "app.workspace_review.reading_git_changes");
        assert_translated!(locale, "app.workspace_review.reading_git_changes_progress");
        assert_translated!(locale, "app.workspace_review.capturing_snapshot");
        assert_translated!(locale, "app.workspace_review.refreshing_status");
        assert_translated!(locale, "app.workspace_review.changed_files_summary");
        assert_translated!(locale, "app.workspace_review.changed_files_count");
        assert_translated!(locale, "app.workspace_review.added_deleted_description");
        assert_translated!(locale, "app.workspace_review.file_position");
        assert_translated!(locale, "app.workspace_review.previous_file");
        assert_translated!(locale, "app.workspace_review.choose_changed_file");
        assert_translated!(locale, "app.workspace_review.next_file");
        assert_translated!(locale, "app.workspace_review.git_review_unavailable");
        assert_translated!(locale, "app.workspace_review.choose_what_to_review");
        assert_translated!(locale, "app.workspace_review.choose_what_to_review_hint");
        assert_translated!(locale, "app.workspace_review.folder_value");
        assert_translated!(locale, "app.workspace_review.refresh_failed");
        assert_translated!(locale, "app.workspace_review.repository_unavailable_hint");
        assert_translated!(locale, "app.workspace_review.no_previewable_changes");
        assert_translated!(locale, "app.workspace_review.no_tracked_changes");
        assert_translated!(locale, "app.workspace_review.no_previewable_changes_hint");
        assert_translated!(locale, "app.workspace_review.tracked_files_clean_hint");
        assert_translated!(locale, "app.workspace_review.preview_incomplete");
        assert_translated!(locale, "app.workspace_review.preview_truncated");
        assert_translated!(locale, "app.workspace_review.structured_preview_unavailable");
        assert_translated!(locale, "app.workspace_review.changed_files");
        assert_translated!(locale, "app.workspace_review.files");
        assert_translated!(locale, "app.errors.request_queue_unavailable");

        let summary = rust_i18n::t!(
            "app.workspace_review.changed_files_summary",
            locale = locale,
            count = 8,
            files = rust_i18n::t!("app.workspace_review.files", locale = locale),
            additions = 13,
            deletions = 4
        )
        .to_string();
        assert!(summary.contains("8"), "count was not interpolated in {locale}");
        assert!(summary.contains("+13"), "additions were not interpolated in {locale}");
        assert!(summary.contains("−4"), "deletions were not interpolated in {locale}");
        if locale == "pl" {
            assert!(summary.contains("plików"), "file label was not interpolated in {locale}");
        }

        let file_position = rust_i18n::t!(
            "app.workspace_review.file_position",
            locale = locale,
            current = 2,
            total = 5,
            path = "src/main.rs"
        )
        .to_string();
        assert!(file_position.contains("src/main.rs"), "path was not interpolated in {locale}");
        let refresh_error =
            rust_i18n::t!("app.workspace_review.refresh_failed", locale = locale, error = "repository unavailable")
                .to_string();
        assert!(refresh_error.contains("repository unavailable"), "diagnostic was lost in {locale}");
    }

    for locale in ["tr", "pl", "hi", "uk"] {
        let folder = rust_i18n::t!("app.workspace_review.folder_value", locale = locale, folder = "src").to_string();
        assert!(folder.contains("src"), "folder path was not interpolated in {locale}");
    }
}

#[test]
fn workspace_file_browser_and_preview_copy_covers_every_language() {
    macro_rules! assert_translated {
        ($locale:expr, $key:literal) => {{
            let translated = rust_i18n::t!($key, locale = $locale).to_string();
            assert!(!translated.is_empty(), "{}/{} is empty", $locale, $key);
            if $locale != "en" && !($locale == "it" && $key == "app.files.file_accessibility") {
                let english = rust_i18n::t!($key, locale = "en").to_string();
                assert_ne!(translated, english, "{}/{} fell back to English", $locale, $key);
            }
        }};
    }

    for language in UiLanguage::ALL {
        let locale = language.locale();
        assert_translated!(locale, "app.files.workspace_files");
        assert_translated!(locale, "app.files.focus_tree");
        assert_translated!(locale, "app.files.refresh");
        assert_translated!(locale, "app.files.refresh_tooltip");
        assert_translated!(locale, "app.files.scanning_description");
        assert_translated!(locale, "app.files.file_accessibility");
        assert_translated!(locale, "app.files.folder_accessibility");
        assert_translated!(locale, "app.files.preview_unavailable");
        assert_translated!(locale, "app.file_preview.accessibility_title");
        assert_translated!(locale, "app.file_preview.accessibility_description");
        assert_translated!(locale, "app.file_preview.open_in_editor");
        assert_translated!(locale, "app.file_preview.copy");
        assert_translated!(locale, "app.file_preview.close");
        assert_translated!(locale, "app.file_preview.close_tooltip");
        assert_translated!(locale, "app.file_preview.truncated");
        assert_translated!(locale, "app.file_preview.editor_error");

        let file_label =
            rust_i18n::t!("app.files.file_accessibility", locale = locale, path = "src/main.rs").to_string();
        assert!(file_label.contains("src/main.rs"), "file path was not interpolated in {locale}");
        let preview_title =
            rust_i18n::t!("app.file_preview.accessibility_title", locale = locale, path = "src/main.rs").to_string();
        assert!(preview_title.contains("src/main.rs"), "preview path was not interpolated in {locale}");
        let truncated = rust_i18n::t!("app.file_preview.truncated", locale = locale, size = "64 KiB").to_string();
        assert!(truncated.contains("64 KiB"), "truncation limit was not interpolated in {locale}");
        let editor_error =
            rust_i18n::t!("app.file_preview.editor_error", locale = locale, error = "editor unavailable").to_string();
        assert!(editor_error.contains("editor unavailable"), "editor error was lost in {locale}");
    }
}

#[test]
fn command_and_composer_palettes_are_translated_for_every_language() {
    macro_rules! assert_translated {
        ($locale:expr, $key:literal) => {{
            let translated = rust_i18n::t!($key, locale = $locale).to_string();
            assert!(!translated.is_empty(), "{}/{} is empty", $locale, $key);
            if $locale != "en"
                && $key != "app.palette.command.vtcode"
                && !($locale == "fr" && $key == "app.palette.sections.session")
                && !($locale == "fr" && $key == "app.palette.sections.conversation")
            {
                let english = rust_i18n::t!($key, locale = "en").to_string();
                assert_ne!(translated, english, "{}/{} fell back to English", $locale, $key);
            }
        }};
    }

    for language in UiLanguage::ALL {
        let locale = language.locale();
        assert_translated!(locale, "app.palette.title.conversation_search");
        assert_translated!(locale, "app.palette.title.file_mention");
        assert_translated!(locale, "app.palette.title.slash_commands");
        assert_translated!(locale, "app.palette.title.prompt_history");
        assert_translated!(locale, "app.palette.title.permissions");
        assert_translated!(locale, "app.palette.title.modes");
        assert_translated!(locale, "app.palette.title.ui_font");
        assert_translated!(locale, "app.palette.title.code_font");
        assert_translated!(locale, "app.palette.title.configuration");
        assert_translated!(locale, "app.palette.title.changed_file");
        assert_translated!(locale, "app.palette.title.commands");
        assert_translated!(locale, "app.palette.sections.session");
        assert_translated!(locale, "app.palette.sections.view");
        assert_translated!(locale, "app.palette.sections.conversation");
        assert_translated!(locale, "app.palette.sections.vtcode_commands");
        assert_translated!(locale, "app.palette.sections.recent_sessions");
        assert_translated!(locale, "app.palette.sections.files");
        assert_translated!(locale, "app.palette.sections.loaded_messages");
        assert_translated!(locale, "app.palette.sections.loaded_tool_output");
        assert_translated!(locale, "app.palette.sections.this_thread");
        assert_translated!(locale, "app.palette.sections.add");
        assert_translated!(locale, "app.palette.sections.workspace_files");
        assert_translated!(locale, "app.palette.sections.skills");
        assert_translated!(locale, "app.palette.sections.plugins");
        assert_translated!(locale, "app.palette.sections.commands");
        assert_translated!(locale, "app.palette.command.new_session");
        assert_translated!(locale, "app.palette.command.close_session");
        assert_translated!(locale, "app.palette.command.switch_workspace");
        assert_translated!(locale, "app.palette.command.refresh_recent_threads");
        assert_translated!(locale, "app.palette.command.choose_model");
        assert_translated!(locale, "app.palette.command.toggle_inspector");
        assert_translated!(locale, "app.palette.command.toggle_zen_mode");
        assert_translated!(locale, "app.palette.command.switch_dark_theme");
        assert_translated!(locale, "app.palette.command.switch_light_theme");
        assert_translated!(locale, "app.palette.command.find_in_conversation");
        assert_translated!(locale, "app.palette.command.open_session");
        assert_translated!(locale, "app.palette.command.open_file");
        assert_translated!(locale, "app.palette.command.add_files");
        assert_translated!(locale, "app.palette.command.workspace_file");
        assert_translated!(locale, "app.palette.command.vtcode");
        assert_translated!(locale, "app.palette.command.you");
        assert_translated!(locale, "app.palette.command.tool_activity");
        assert_translated!(locale, "app.palette.command.output");
        assert_translated!(locale, "app.palette.command.use_plugin");
        assert_translated!(locale, "app.palette.placeholder.configuration");
        assert_translated!(locale, "app.palette.placeholder.fonts");
        assert_translated!(locale, "app.palette.placeholder.workspace_files");
        assert_translated!(locale, "app.palette.placeholder.commands");
        assert_translated!(locale, "app.palette.placeholder.changed_files");
        assert_translated!(locale, "app.palette.placeholder.conversation");
        assert_translated!(locale, "app.palette.placeholder.prompts");
        assert_translated!(locale, "app.palette.placeholder.modes");
        assert_translated!(locale, "app.palette.placeholder.default");
        assert_translated!(locale, "app.palette.conversation_search_header");
        assert_translated!(locale, "app.palette.empty.no_models");
        assert_translated!(locale, "app.palette.empty.models_reload");
        assert_translated!(locale, "app.palette.empty.no_prompts");
        assert_translated!(locale, "app.palette.empty.prompts_hint");
        assert_translated!(locale, "app.palette.empty.modes_unavailable");
        assert_translated!(locale, "app.palette.empty.modes_reload");
        assert_translated!(locale, "app.palette.empty.no_loaded_match");
        assert_translated!(locale, "app.palette.empty.older_history");
        assert_translated!(locale, "app.palette.empty.no_results");
        assert_translated!(locale, "app.palette.empty.try_different_search");
        assert_translated!(locale, "app.palette.history.loading");
        assert_translated!(locale, "app.palette.history.limit_reached");
        assert_translated!(locale, "app.palette.history.retry");
        assert_translated!(locale, "app.palette.history.load");
        assert_translated!(locale, "app.palette.history.retry_hint");

        for (key, value) in [
            (
                "app.palette.command.open_session",
                rust_i18n::t!("app.palette.command.open_session", locale = locale, label = "Project A").to_string(),
            ),
            (
                "app.palette.command.open_file",
                rust_i18n::t!("app.palette.command.open_file", locale = locale, path = "src/main.rs").to_string(),
            ),
            (
                "app.palette.command.use_plugin",
                rust_i18n::t!("app.palette.command.use_plugin", locale = locale, plugin = "web-tools").to_string(),
            ),
        ] {
            let expected = if key == "app.palette.command.open_session" {
                "Project A"
            } else if key == "app.palette.command.open_file" {
                "src/main.rs"
            } else {
                "web-tools"
            };
            assert!(value.contains(expected), "{key} lost its interpolation in {locale}");
        }
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
        (
            "ru",
            "Светлая",
            "Изменить профиль оформления Светлая",
            "Изменить профиль оформления Светлая (выбран)",
            "Экран",
            "Подключено",
        ),
        (
            "id",
            "Terang",
            "Ubah profil tampilan Terang",
            "Ubah profil tampilan Terang (dipilih)",
            "Tampilan",
            "Tersambung",
        ),
        (
            "uk",
            "Світла",
            "Редагувати профіль оформлення Світла",
            "Редагувати профіль оформлення Світла (вибрано)",
            "Дисплей",
            "Підключено",
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
fn file_destination_copy_is_translated_for_every_language() {
    macro_rules! assert_catalog_key {
        ($key:literal) => {
            for language in UiLanguage::ALL {
                let locale = language.locale();
                let translated = rust_i18n::t!($key, locale = locale).to_string();
                assert!(!translated.is_empty(), "{locale}/{} is empty", $key);
                if language != UiLanguage::English {
                    assert_ne!(
                        translated,
                        rust_i18n::t!($key, locale = "en").to_string(),
                        "{locale}/{} fell back to English",
                        $key
                    );
                }
            }
        };
    }

    assert_catalog_key!("app.settings.files.group");
    assert_catalog_key!("app.settings.files.destination.label");
    assert_catalog_key!("app.settings.files.destination.description");
    assert_catalog_key!("app.settings.files.destination.viewer");
    assert_catalog_key!("app.settings.files.destination.system");
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
        ("ru", "Доброе утро", "Добрый день", "Добрый вечер"),
        ("id", "Selamat pagi", "Selamat siang", "Selamat malam"),
        ("uk", "Доброго ранку", "Добрий день", "Добрий вечір"),
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
