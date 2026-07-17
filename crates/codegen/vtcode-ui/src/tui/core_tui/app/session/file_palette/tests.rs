use std::path::{Path, PathBuf};

use super::*;

#[test]
fn test_extract_file_reference_at_symbol() {
    let input = "@";
    let result = extract_file_reference(input, 1);
    assert_eq!(result, Some((0, 1, String::new())));
}

#[test]
fn test_extract_file_reference_with_path() {
    let input = "@src/main.rs";
    let result = extract_file_reference(input, 12);
    assert_eq!(result, Some((0, 12, "src/main.rs".to_owned())));
}

#[test]
fn test_extract_file_reference_mid_word() {
    let input = "@src/main.rs";
    let result = extract_file_reference(input, 5);
    assert_eq!(result, Some((0, 12, "src/main.rs".to_owned())));
}

#[test]
fn test_extract_file_reference_with_text_before() {
    let input = "check @src/main.rs for errors";
    let result = extract_file_reference(input, 18);
    assert_eq!(result, Some((6, 18, "src/main.rs".to_owned())));
}

#[test]
fn test_no_file_reference() {
    let input = "no reference here";
    let result = extract_file_reference(input, 5);
    assert_eq!(result, None);
}

#[test]
fn test_npm_scoped_package_no_trigger() {
    let input = "npx -y @openai/codex@latest";
    let result = extract_file_reference(input, 17);
    assert_eq!(result, None);
}

#[test]
fn test_npm_scoped_package_variant() {
    let input = "npm install @scope/package@1.0.0";
    let result = extract_file_reference(input, 22);
    assert_eq!(result, None);
}

#[test]
fn test_npm_install_scoped() {
    let input = "npm i @types/node";
    let result = extract_file_reference(input, 11);
    assert_eq!(result, None);
}

#[test]
fn test_yarn_scoped_package() {
    let input = "yarn add @babel/core";
    let result = extract_file_reference(input, 14);
    assert_eq!(result, None);
}

#[test]
fn test_pnpm_scoped_package() {
    let input = "pnpm install @vitejs/plugin-vue";
    let result = extract_file_reference(input, 23);
    assert_eq!(result, None);
}

#[test]
fn test_valid_file_path_with_at() {
    let input = "@src/main.rs";
    let result = extract_file_reference(input, 12);
    assert_eq!(result, Some((0, 12, "src/main.rs".to_owned())));
}

#[test]
fn test_valid_at_path_in_text() {
    let input = "check @./src/components/Button.tsx";
    let result = extract_file_reference(input, 34);
    assert_eq!(
        result,
        Some((6, 34, "./src/components/Button.tsx".to_owned()))
    );
}

#[test]
fn test_absolute_at_path() {
    let input = "see @/etc/config.txt";
    let result = extract_file_reference(input, 20);
    assert_eq!(result, Some((4, 20, "/etc/config.txt".to_owned())));
}

#[test]
fn test_bare_identifier_in_conversation() {
    let input = "choose @files and do something";
    let result = extract_file_reference(input, 13);
    assert_eq!(result, Some((7, 13, "files".to_owned())));
}

#[test]
fn test_bare_identifier_config() {
    let input = "edit @config";
    let result = extract_file_reference(input, 12);
    assert_eq!(result, Some((5, 12, "config".to_owned())));
}

#[test]
fn test_bare_identifier_rejected_in_npm() {
    let input = "npm i @types";
    let result = extract_file_reference(input, 12);
    assert_eq!(result, None);
}

#[test]
fn test_no_false_positive_with_a() {
    let input = "a";
    let result = extract_file_reference(input, 1);
    assert_eq!(result, None);

    let input = "write a function";
    let result = extract_file_reference(input, 7);
    assert_eq!(result, None);
}

#[test]
fn test_browse_listing_at_root() {
    let mut palette = FilePalette::new(PathBuf::from("/workspace"));
    let files: Vec<String> = (0..50).map(|i| format!("/workspace/file{i}.rs")).collect();
    palette.load_files(files);

    // Browse mode at the root shows every top-level file; nothing is collapsed.
    assert!(!palette.is_search_mode());
    assert_eq!(palette.total_items(), 50);
    assert!(palette.list_entries().iter().all(|e| !e.is_dir));
}

#[test]
fn test_browse_shows_subdirectories() {
    let mut palette = FilePalette::new(PathBuf::from("/workspace"));
    palette.load_files(vec![
        "/workspace/src/main.rs".to_owned(),
        "/workspace/src/lib.rs".to_owned(),
        "/workspace/README.md".to_owned(),
    ]);

    // Root shows the directory plus the top-level file (dirs sorted first).
    let entries = palette.list_entries();
    assert_eq!(entries.len(), 2);
    assert!(entries[0].is_dir && entries[0].display_name == "src/");
    assert!(!entries[1].is_dir && entries[1].relative_path == "README.md");

    // Descending reveals the files inside the subdirectory (plus the `..` parent).
    palette.enter_selected_dir();
    let entries = palette.list_entries();
    assert_eq!(entries.len(), 3);
    assert!(entries.iter().any(|e| e.is_parent));
    assert!(
        entries
            .iter()
            .filter(|e| !e.is_parent)
            .all(|e| e.relative_path.starts_with("src/"))
    );
}

#[test]
fn test_enter_dir_and_go_up() {
    let mut palette = FilePalette::new(PathBuf::from("/workspace"));
    palette.load_files(vec!["/workspace/a/b/c.rs".to_owned()]);

    // Each ancestor directory is navigable, even when it holds no direct files.
    assert_eq!(palette.list_entries().len(), 1);
    assert!(palette.list_entries()[0].is_dir);

    palette.enter_selected_dir();
    assert_eq!(palette.breadcrumb(), "/a");
    palette.enter_selected_dir();
    assert_eq!(palette.breadcrumb(), "/a/b");
    palette.enter_selected_dir();
    assert_eq!(palette.breadcrumb(), "/a/b");

    // The leaf file is now visible.
    assert!(
        palette
            .list_entries()
            .iter()
            .any(|e| e.relative_path == "a/b/c.rs")
    );

    // Ascending walks back up the tree (the final enter was on a file, so the
    // directory is unchanged).
    palette.go_up();
    assert_eq!(palette.breadcrumb(), "/a");
    palette.go_up();
    assert_eq!(palette.breadcrumb(), "/");
    palette.go_up();
    assert_eq!(palette.breadcrumb(), "/");
}

#[test]
fn test_go_up_reselects_entered_directory() {
    let mut palette = FilePalette::new(PathBuf::from("/workspace"));
    palette.load_files(vec![
        "/workspace/src/a.rs".to_owned(),
        "/workspace/src/b.rs".to_owned(),
    ]);

    palette.enter_selected_dir(); // into src/
    palette.go_up(); // back to root
    let selected = palette.get_selected();
    assert!(selected.is_some_and(|e| e.display_name == "src/"));
}

#[test]
fn test_browse_excludes_hidden_directories() {
    let mut palette = FilePalette::new(PathBuf::from("/workspace"));
    palette.load_files(vec![
        "/workspace/.secret/key".to_owned(),
        "/workspace/src/main.rs".to_owned(),
    ]);

    let entries = palette.list_entries();
    assert!(!entries.iter().any(|e| e.display_name == ".secret/"));
    assert!(entries.iter().any(|e| e.display_name == "src/"));
}

#[test]
fn test_search_filtering() {
    let mut palette = FilePalette::new(PathBuf::from("/workspace"));
    palette.load_files(vec![
        "/workspace/src/main.rs".to_owned(),
        "/workspace/src/lib.rs".to_owned(),
        "/workspace/tests/test.rs".to_owned(),
        "/workspace/README.md".to_owned(),
    ]);

    // Browse mode at root: two directories + one file.
    assert_eq!(palette.total_items(), 3);

    palette.set_filter("src".to_owned());
    assert!(palette.is_search_mode());
    assert_eq!(palette.total_items(), 2);

    palette.set_filter("main".to_owned());
    assert_eq!(palette.total_items(), 1);
}

#[test]
fn test_smart_ranking() {
    let mut palette = FilePalette::new(PathBuf::from("/workspace"));
    palette.load_files(vec![
        "src/main.rs".to_owned(),
        "src/domain/main_handler.rs".to_owned(),
        "tests/main_test.rs".to_owned(),
        "main.rs".to_owned(),
    ]);

    palette.set_filter("main".to_owned());

    // All files containing "main" should be found.
    assert_eq!(palette.total_items(), 4);
}

#[test]
fn test_exact_match_ranked_first() {
    let mut palette = FilePalette::new(PathBuf::from("/workspace"));
    palette.load_files(vec![
        "src/domain/main_handler.rs".to_owned(),
        "src/utils/helpers.rs".to_owned(),
        "tests/main_test.rs".to_owned(),
        "main.rs".to_owned(),
        "src/main.rs".to_owned(),
    ]);

    // Exact filename match "main.rs" should rank above partial matches.
    palette.set_filter("main.rs".to_owned());
    let filtered = &palette.filtered_files;
    assert_eq!(filtered[0].relative_path, "main.rs");
    assert_eq!(filtered[1].relative_path, "src/main.rs");

    // Exact full-path match should rank first.
    palette.set_filter("src/main.rs".to_owned());
    let filtered = &palette.filtered_files;
    assert_eq!(filtered[0].relative_path, "src/main.rs");

    // Filename prefix match should rank above substring matches.
    palette.set_filter("main".to_owned());
    let filtered = &palette.filtered_files;
    assert_eq!(filtered[0].relative_path, "main.rs");

    // Top-level files should rank above nested files with the same name.
    palette.set_filter("main".to_owned());
    let filtered = &palette.filtered_files;
    let top_level_positions: Vec<usize> = filtered
        .iter()
        .enumerate()
        .filter(|(_, e)| !e.relative_path.contains('/'))
        .map(|(i, _)| i)
        .collect();
    let nested_positions: Vec<usize> = filtered
        .iter()
        .enumerate()
        .filter(|(_, e)| e.relative_path.contains('/'))
        .map(|(i, _)| i)
        .collect();
    assert!(
        top_level_positions.iter().max().unwrap_or(&0)
            < nested_positions.iter().min().unwrap_or(&usize::MAX)
    );
}

#[test]
fn test_has_files() {
    let mut palette = FilePalette::new(PathBuf::from("/workspace"));
    assert!(!palette.has_files());

    palette.load_files(vec!["file.rs".to_owned()]);
    assert!(palette.has_files());
}

#[test]
fn test_navigation_no_wrapping() {
    let mut palette = FilePalette::new(PathBuf::from("/workspace"));
    let files = vec![
        "/workspace/a.rs".to_owned(),
        "/workspace/b.rs".to_owned(),
        "/workspace/c.rs".to_owned(),
    ];
    palette.load_files(files);

    // Navigation does not wrap — at first item, up stays at first.
    assert_eq!(palette.selected_index(), Some(0));
    palette.move_selection_up();
    assert_eq!(palette.selected_index(), Some(0));

    // Move to last item.
    palette.move_to_last();
    let last = palette.selected_index().unwrap();
    palette.move_selection_down();
    assert_eq!(palette.selected_index(), Some(last));
}

#[test]
fn test_security_filters_sensitive_files() {
    let mut palette = FilePalette::new(PathBuf::from("/workspace"));

    let files = vec![
        "/workspace/src/main.rs".to_owned(),
        "/workspace/.env".to_owned(),
        "/workspace/.env.local".to_owned(),
        "/workspace/.env.production".to_owned(),
        "/workspace/.git/config".to_owned(),
        "/workspace/.gitignore".to_owned(),
        "/workspace/.DS_Store".to_owned(),
        "/workspace/.hidden_file".to_owned(),
        "/workspace/tests/test.rs".to_owned(),
    ];

    palette.load_files(files);

    // Only non-sensitive files should be loaded into the index.
    assert!(
        palette
            .all_files
            .iter()
            .all(|f| !f.relative_path.contains(".env"))
    );
    assert!(
        palette
            .all_files
            .iter()
            .all(|f| !f.relative_path.contains(".git"))
    );
    assert!(
        palette
            .all_files
            .iter()
            .all(|f| !f.relative_path.contains(".DS_Store"))
    );
    assert!(
        palette
            .all_files
            .iter()
            .all(|f| !f.relative_path.starts_with('.'))
    );
}

#[test]
fn test_should_exclude_file() {
    assert!(FilePalette::should_exclude_file(Path::new(
        "/workspace/.env"
    )));
    assert!(FilePalette::should_exclude_file(Path::new(
        "/workspace/.env.local"
    )));
    assert!(FilePalette::should_exclude_file(Path::new(
        "/workspace/.env.production"
    )));

    assert!(FilePalette::should_exclude_file(Path::new(
        "/workspace/.git/config"
    )));
    assert!(FilePalette::should_exclude_file(Path::new(
        "/workspace/project/.git/HEAD"
    )));

    assert!(FilePalette::should_exclude_file(Path::new(
        "/workspace/.hidden"
    )));
    assert!(FilePalette::should_exclude_file(Path::new(
        "/workspace/.DS_Store"
    )));

    assert!(!FilePalette::should_exclude_file(Path::new(
        "/workspace/src/main.rs"
    )));
    assert!(!FilePalette::should_exclude_file(Path::new(
        "/workspace/README.md"
    )));
    assert!(!FilePalette::should_exclude_file(Path::new(
        "/workspace/environment.txt"
    )));
}

#[test]
fn test_simple_fuzzy_match() {
    assert!(FilePalette::simple_fuzzy_match("src/main.rs", "smr").is_some());
    assert!(FilePalette::simple_fuzzy_match("src/main.rs", "src").is_some());
    assert!(FilePalette::simple_fuzzy_match("src/main.rs", "main").is_some());

    assert!(FilePalette::simple_fuzzy_match("src/main.rs", "xyz").is_none());
    assert!(FilePalette::simple_fuzzy_match("src/main.rs", "msr").is_none());
}

#[test]
fn test_current_at_token_tracks_tokens_with_second_at() {
    let input = "npx -y @kaeawc/auto-mobile@latest";
    let token_start = input.find("@kaeawc").expect("scoped npm package present");
    let version_at = input
        .rfind("@latest")
        .expect("version suffix present in scoped npm package");
    let test_cases = vec![
        (token_start, "Cursor at leading @"),
        (token_start + 8, "Cursor inside scoped package name"),
        (version_at, "Cursor at version @"),
        (input.len(), "Cursor at end of token"),
    ];

    for (cursor_pos, description) in test_cases {
        let result = extract_file_reference(input, cursor_pos);
        assert_eq!(
            result, None,
            "Failed for case: {description} - input: '{input}', cursor: {cursor_pos}"
        );
    }
}

#[test]
fn test_current_at_token_allows_file_queries_with_second_at() {
    let input = "@icons/icon@2x.png";
    let version_at = input
        .rfind("@2x")
        .expect("second @ in file token should be present");
    let test_cases = vec![
        (0, "Cursor at leading @"),
        (8, "Cursor before second @"),
        (version_at, "Cursor at second @"),
        (input.len(), "Cursor at end of token"),
    ];

    for (cursor_pos, description) in test_cases {
        let result = extract_file_reference(input, cursor_pos);
        assert!(
            result.is_some(),
            "Failed for case: {description} - input: '{input}', cursor: {cursor_pos}"
        );
    }
}

#[test]
fn test_current_at_token_ignores_mid_word_at() {
    let input = "foo@bar";
    let at_pos = input.find('@').expect("@ present");
    let test_cases = vec![
        (at_pos, "Cursor at mid-word @"),
        (input.len(), "Cursor at end of word containing @"),
    ];

    for (cursor_pos, description) in test_cases {
        let result = extract_file_reference(input, cursor_pos);
        assert_eq!(
            result, None,
            "Failed for case: {description} - input: '{input}', cursor: {cursor_pos}"
        );
    }
}

/// In-memory directory tree used to exercise lazy (on-demand) loading without
/// touching the real filesystem.
fn fake_dir_lister() -> DirLister {
    DirLister::new(move |dir: &Path| match dir.display().to_string().as_str() {
        "/workspace" => vec![
            (PathBuf::from("/workspace/src"), true),
            (PathBuf::from("/workspace/README.md"), false),
        ],
        "/workspace/src" => vec![
            (PathBuf::from("/workspace/src/main.rs"), false),
            (PathBuf::from("/workspace/src/lib.rs"), false),
        ],
        _ => Vec::new(),
    })
}

#[test]
fn test_configure_loads_root_lazily_and_navigates_on_demand() {
    let mut palette = FilePalette::new(PathBuf::from("/workspace"));
    palette.configure(PathBuf::from("/workspace"), fake_dir_lister());

    // Root is populated immediately from the lister (no full recursive walk).
    let entries = palette.list_entries();
    assert_eq!(entries.len(), 2);
    assert!(entries[0].is_dir && entries[0].display_name == "src/");
    assert!(!entries[1].is_dir && entries[1].display_name == "README.md");

    // Descending loads the subdirectory's children on demand (plus a `..` parent).
    palette.enter_selected_dir();
    assert_eq!(palette.breadcrumb(), "/src");
    let entries = palette.list_entries();
    assert_eq!(entries.len(), 3);
    assert!(entries[0].is_parent);
    let files: Vec<_> = entries.iter().filter(|e| !e.is_parent).collect();
    assert_eq!(files.len(), 2);
    assert!(files.iter().all(|e| !e.is_dir));
    assert!(files.iter().any(|e| e.relative_path == "src/main.rs"));
    assert!(files.iter().any(|e| e.relative_path == "src/lib.rs"));

    // Ascending returns to the cached root view.
    palette.go_up();
    assert_eq!(palette.breadcrumb(), "/");
    let entries = palette.list_entries();
    assert_eq!(entries.len(), 2);
    assert!(entries[0].is_dir && entries[0].display_name == "src/");
}

#[test]
fn test_search_mode_deferred_until_index_arrives() {
    let mut palette = FilePalette::new(PathBuf::from("/workspace"));
    palette.configure(PathBuf::from("/workspace"), fake_dir_lister());

    // Before the recursive index arrives, searching yields nothing.
    palette.set_filter("main".to_string());
    assert!(palette.is_search_mode());
    assert!(palette.list_entries().is_empty());

    // Once the index is delivered, the same query matches.
    palette.set_search_index(vec![
        "/workspace/src/main.rs".to_owned(),
        "/workspace/src/lib.rs".to_owned(),
        "/workspace/README.md".to_owned(),
    ]);
    palette.set_filter("main".to_string());
    let entries = palette.list_entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].relative_path, "src/main.rs");
}
