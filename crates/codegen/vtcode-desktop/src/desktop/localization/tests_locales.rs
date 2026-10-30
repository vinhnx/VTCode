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

fn assert_not_trailing_space(name: &str, index: usize, line: &str, close: usize) {
    let trailing = line
        .get(..close)
        .is_some_and(|head| head.chars().next_back().is_some_and(|ch| ch == ' ' || ch == '\t'));
    assert!(!trailing, "{name}:{} ends a value with whitespace", index + 1);
}
