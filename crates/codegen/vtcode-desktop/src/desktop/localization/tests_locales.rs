use std::collections::HashSet;

/// Locale data ships as one slim file per namespace. Every file must carry
/// `_version: 2` on its first line: without it rust-i18n parses the file as
/// v1 with the filename as the locale, silently dropping every key it holds.
/// The per-key localization tests prove the multi-file merge resolves; this
/// test pins the layout so the split cannot silently regress.
#[test]
fn locale_files_stay_slim_versioned_and_grouped() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("locales");
    let mut entries = std::fs::read_dir(&dir)
        .expect("locales dir exists")
        .collect::<Result<Vec<_>, _>>()
        .expect("locales dir is readable");
    entries.sort_by_key(|entry| entry.file_name());
    let mut namespaces = HashSet::new();
    let mut files = 0;
    for entry in entries {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("yml") {
            continue;
        }
        files += 1;
        let name = entry.file_name().to_string_lossy().into_owned();
        let content = std::fs::read_to_string(&path).expect("locale file is readable");
        assert!(content.lines().count() <= 1000, "{name} exceeds the slim-file ceiling; split the namespace");
        assert!(content.starts_with("_version: 2\n"), "{name} must start with `_version: 2`");
        // No value may end with whitespace: a raw newline inside a
        // double-quoted flow scalar folds to a space and silently corrupts
        // notices that terminate with `\n` (see `truncation_notice`).
        for (index, line) in content.lines().enumerate() {
            for (close, _) in line.match_indices("\",") {
                assert_not_trailing_space(&name, index, line, close);
            }
            for (close, _) in line.match_indices("\"}") {
                assert_not_trailing_space(&name, index, line, close);
            }
        }
        if name == "gpui_component.yml" {
            assert!(content.contains("\ngpui_component:\n"), "{name} owns gpui_component");
            continue;
        }
        let namespace = name.strip_prefix("app.").and_then(|stem| stem.strip_suffix(".yml"));
        let Some(namespace) = namespace else {
            panic!("{name} must be named `app.<namespace>.yml`");
        };
        assert!(
            content.contains(&format!("\n  {namespace}:\n")),
            "{name} must own exactly the `app.{namespace}` subtree"
        );
        assert!(namespaces.insert(namespace.to_string()), "duplicate namespace {namespace}");
    }
    assert!(files > 1, "locales must stay split, one file per namespace");
}

/// Yield the contents of each double-quoted value on a line, honouring `\"`.
/// `split('"')` breaks inside escaped quotes and skips part of values such as
/// `"No shortcuts match \"%{query}\"."`, hiding copy from the checks below.
fn quoted_values(line: &str) -> impl Iterator<Item = &str> {
    let mut rest = line;
    std::iter::from_fn(move || {
        let open = rest.find('"')?;
        let after_open = rest.get(open + 1..).expect("ASCII quote ends on a char boundary");
        let mut escaped = false;
        for (ix, ch) in after_open.char_indices() {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                let value = after_open.get(..ix).expect("char_indices yields char boundaries");
                rest = after_open.get(ix + 1..).expect("ASCII quote ends on a char boundary");
                return Some(value);
            }
        }
        rest = "";
        None
    })
}

/// UI copy uses one ellipsis character (U+2026), never three ASCII periods.
/// Three periods render as a cramped run and silently disagree with every
/// other translation of the same key.
#[test]
fn locale_values_use_a_single_ellipsis_character() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("locales");
    let mut entries = std::fs::read_dir(&dir)
        .expect("locales dir exists")
        .collect::<Result<Vec<_>, _>>()
        .expect("locales dir is readable");
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("yml") {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let content = std::fs::read_to_string(&path).expect("locale file is readable");
        for (line_ix, line) in content.lines().enumerate() {
            for value in quoted_values(line) {
                assert!(!value.contains("..."), "{name}:{} uses three periods instead of `…`", line_ix + 1);
            }
        }
    }
}

/// The guard must see copy inside escaped quotes, not just the fragments a
/// naive `split('"')` would check (see `app.shortcuts.yml::no_match`).
#[test]
fn locale_ellipsis_scan_sees_text_inside_escaped_quotes() {
    let line = r#"    no_match: {en: "No shortcuts match \"foo...\"."}"#;
    let values: Vec<_> = quoted_values(line).collect();
    assert_eq!(values, vec![r#"No shortcuts match \"foo...\"."#]);
    assert!(values[0].contains("..."), "the scan must reach a run inside an escaped quote");
}

fn assert_not_trailing_space(name: &str, index: usize, line: &str, close: usize) {
    let trailing = line
        .get(..close)
        .is_some_and(|head| head.chars().next_back().is_some_and(|ch| ch == ' ' || ch == '\t'));
    assert!(!trailing, "{name}:{} ends a value with whitespace", index + 1);
}

#[test]
fn composer_cli_tooltip_covers_each_supported_locale() {
    use super::UiLanguage;

    let composer = include_str!("../../../locales/app.composer.yml");
    let translations = composer
        .lines()
        .find_map(|line| line.strip_prefix("    connect_cli_tooltip: {"))
        .expect("composer locale entry");
    assert_eq!(translations.matches(": \"").count(), UiLanguage::ALL.len());
    for language in UiLanguage::ALL {
        let locale = language.locale();
        assert!(translations.contains(&format!("{locale}: \"")), "missing {locale} translation for the CLI tooltip");
        let translated = rust_i18n::t!("app.composer.connect_cli_tooltip", locale = locale);
        assert!(!translated.is_empty() && translated != "app.composer.connect_cli_tooltip");
    }
}
