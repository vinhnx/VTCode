//! Shared assertions for the localization test modules.
//!
//! Every `tests*.rs` module checks the same contract (non-empty everywhere,
//! translated everywhere except pinned cognates), so the helper lives here in
//! the common parent instead of being copy-pasted per test.

use super::UiLanguage;

/// Every key resolves to a non-empty translation in all 20 locales, and every
/// non-English translation differs from English unless it is listed in
/// `identical` (correct cognates asserted equal so stale entries fail).
#[track_caller]
pub(super) fn assert_localized(keys: &[&str], identical: &[(&str, &str)]) {
    for language in UiLanguage::ALL {
        let locale = language.locale();
        for key in keys {
            let key: &str = key;
            let value = rust_i18n::t!(key, locale = locale).to_string();
            assert!(!value.is_empty(), "{locale}/{key} is empty");
            if identical.contains(&(locale, key)) {
                assert_eq!(
                    value,
                    rust_i18n::t!(key, locale = "en").to_string(),
                    "{locale}/{key} must stay identical to English"
                );
            } else if language != UiLanguage::English {
                assert_ne!(value, rust_i18n::t!(key, locale = "en").to_string(), "{locale}/{key} fell back to English");
            }
        }
    }
}
