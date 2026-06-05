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
    // Should NOT trigger file picker on npm scoped packages
    let input = "npx -y @openai/codex@latest";
    let result = extract_file_reference(input, 17); // cursor at @openai
    assert_eq!(result, None);
}

#[test]
fn test_npm_scoped_package_variant() {
    // npm install @scope/package@version
    let input = "npm install @scope/package@1.0.0";
    let result = extract_file_reference(input, 22); // cursor at @scope
    assert_eq!(result, None);
}

#[test]
fn test_npm_install_scoped() {
    // npm i @types/node
    let input = "npm i @types/node";
    let result = extract_file_reference(input, 11); // cursor after @
    assert_eq!(result, None);
}

#[test]
fn test_yarn_scoped_package() {
    // yarn add @babel/core
    let input = "yarn add @babel/core";
    let result = extract_file_reference(input, 14); // cursor at @babel
    assert_eq!(result, None);
}

#[test]
fn test_pnpm_scoped_package() {
    // pnpm install @vitejs/plugin-vue
    let input = "pnpm install @vitejs/plugin-vue";
    let result = extract_file_reference(input, 23); // cursor at @vitejs
    assert_eq!(result, None);
}

#[test]
fn test_valid_file_path_with_at() {
    // Valid file path like @src/main.rs should work
    let input = "@src/main.rs";
    let result = extract_file_reference(input, 12);
    assert_eq!(result, Some((0, 12, "src/main.rs".to_owned())));
}

#[test]
fn test_valid_at_path_in_text() {
    // @./relative/path should work
    let input = "check @./src/components/Button.tsx";
    let result = extract_file_reference(input, 34);
    assert_eq!(
        result,
        Some((6, 34, "./src/components/Button.tsx".to_owned()))
    );
}

#[test]
fn test_absolute_at_path() {
    // @/absolute/path should work
    let input = "see @/etc/config.txt";
    let result = extract_file_reference(input, 20);
    assert_eq!(result, Some((4, 20, "/etc/config.txt".to_owned())));
}

#[test]
fn test_bare_identifier_in_conversation() {
    // In normal conversation, @files should trigger picker
    let input = "choose @files and do something";
    let result = extract_file_reference(input, 13); // cursor after "files"
    assert_eq!(result, Some((7, 13, "files".to_owned())));
}

#[test]
fn test_bare_identifier_config() {
    // @config in conversation context should work
    let input = "edit @config";
    let result = extract_file_reference(input, 12); // cursor at end
    assert_eq!(result, Some((5, 12, "config".to_owned())));
}

#[test]
fn test_bare_identifier_rejected_in_npm() {
    // But in npm context, bare identifier is rejected
    let input = "npm i @types";
    let result = extract_file_reference(input, 12); // cursor at end
    assert_eq!(result, None);
}

#[test]
fn test_no_false_positive_with_a() {
    // Should NOT trigger on standalone "a" without @
    let input = "a";
    let result = extract_file_reference(input, 1);
    assert_eq!(result, None);

    // Should NOT trigger on "a" in middle of text
    let input = "write a function";
    let result = extract_file_reference(input, 7); // cursor after "a"
    assert_eq!(result, None);
}

#[test]
fn test_tree_structure() {
    let mut palette = FilePalette::new(PathBuf::from("/workspace"));
    let files: Vec<String> = (0..50).map(|i| format!("file{}.rs", i)).collect();
    palette.load_files(files);

    // Tree has no pagination — all items in one page
    assert_eq!(palette.total_pages(), 1);
    assert_eq!(palette.current_page_number(), 1);
    assert!(!palette.has_more_items());

    // All top-level files should be visible as tree groups
    assert_eq!(palette.tree_groups().len(), 50);
    assert_eq!(palette.total_items(), 50);
}

#[test]
fn test_filtering() {
    let mut palette = FilePalette::new(PathBuf::from("/workspace"));
    let files = vec![
        "src/main.rs".to_owned(),
        "src/lib.rs".to_owned(),
        "tests/test.rs".to_owned(),
        "README.md".to_owned(),
    ];
    palette.load_files(files);

    assert_eq!(palette.total_items(), 4);

    palette.set_filter("src".to_owned());
    assert_eq!(palette.total_items(), 2);

    palette.set_filter("main".to_owned());
    assert_eq!(palette.total_items(), 1);
}

#[test]
fn test_smart_ranking() {
    let mut palette = FilePalette::new(PathBuf::from("/workspace"));
    let files = vec![
        "src/main.rs".to_owned(),
        "src/domain/main_handler.rs".to_owned(),
        "tests/main_test.rs".to_owned(),
        "main.rs".to_owned(),
    ];
    palette.load_files(files);

    palette.set_filter("main".to_owned());

    // All files containing "main" should be found
    assert_eq!(palette.total_items(), 4);

    // Tree groups should exist for each top-level directory + top-level files
    let groups = palette.tree_groups();
    assert!(!groups.is_empty());
}

#[test]
fn test_has_files() {
    let mut palette = FilePalette::new(PathBuf::from("/workspace"));
    assert!(!palette.has_files());

    palette.load_files(vec!["file.rs".to_owned()]);
    assert!(palette.has_files());
}

#[test]
fn test_tree_navigation_no_wrapping() {
    let mut palette = FilePalette::new(PathBuf::from("/workspace"));
    let files = vec!["a.rs".to_owned(), "b.rs".to_owned(), "c.rs".to_owned()];
    palette.load_files(files);

    // Tree navigation does not wrap — at first item, up stays at first
    assert_eq!(palette.selected_index(), Some(0));
    palette.move_selection_up();
    assert_eq!(palette.selected_index(), Some(0));

    // Move to last item
    palette.move_to_last();
    let last = palette.selected_index().unwrap();
    palette.move_selection_down();
    assert_eq!(palette.selected_index(), Some(last));
}

#[test]
fn test_security_filters_sensitive_files() {
    let mut palette = FilePalette::new(PathBuf::from("/workspace"));

    // Test with sensitive files that should be excluded
    let files = vec![
        "/workspace/src/main.rs".to_owned(),
        "/workspace/.env".to_owned(),            // MUST be excluded
        "/workspace/.env.local".to_owned(),      // MUST be excluded
        "/workspace/.env.production".to_owned(), // MUST be excluded
        "/workspace/.git/config".to_owned(),     // MUST be excluded
        "/workspace/.gitignore".to_owned(),      // MUST be excluded
        "/workspace/.DS_Store".to_owned(),       // MUST be excluded
        "/workspace/.hidden_file".to_owned(),    // MUST be excluded (hidden)
        "/workspace/tests/test.rs".to_owned(),
    ];

    palette.load_files(files);

    // Only non-sensitive files should be loaded
    assert_eq!(palette.total_items(), 2); // Only main.rs and test.rs
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
    // Test exact matches
    assert!(FilePalette::should_exclude_file(Path::new(
        "/workspace/.env"
    )));
    assert!(FilePalette::should_exclude_file(Path::new(
        "/workspace/.env.local"
    )));
    assert!(FilePalette::should_exclude_file(Path::new(
        "/workspace/.env.production"
    )));

    // Test .git directory
    assert!(FilePalette::should_exclude_file(Path::new(
        "/workspace/.git/config"
    )));
    assert!(FilePalette::should_exclude_file(Path::new(
        "/workspace/project/.git/HEAD"
    )));

    // Test hidden files
    assert!(FilePalette::should_exclude_file(Path::new(
        "/workspace/.hidden"
    )));
    assert!(FilePalette::should_exclude_file(Path::new(
        "/workspace/.DS_Store"
    )));

    // Test valid files (should NOT be excluded)
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
fn test_sorting_tree_groups_ascending() {
    let mut palette = FilePalette::new(PathBuf::from("/workspace"));

    palette.all_files = vec![
        FileEntry {
            path: "/workspace/zebra.txt".to_owned(),
            display_name: "zebra.txt".to_owned(),
            relative_path: "zebra.txt".to_owned(),
            is_dir: false,
        },
        FileEntry {
            path: "/workspace/src".to_owned(),
            display_name: "src/".to_owned(),
            relative_path: "src".to_owned(),
            is_dir: true,
        },
        FileEntry {
            path: "/workspace/Apple.txt".to_owned(),
            display_name: "Apple.txt".to_owned(),
            relative_path: "Apple.txt".to_owned(),
            is_dir: false,
        },
        FileEntry {
            path: "/workspace/tests".to_owned(),
            display_name: "tests/".to_owned(),
            relative_path: "tests".to_owned(),
            is_dir: true,
        },
        FileEntry {
            path: "/workspace/banana.txt".to_owned(),
            display_name: "banana.txt".to_owned(),
            relative_path: "banana.txt".to_owned(),
            is_dir: false,
        },
        FileEntry {
            path: "/workspace/lib".to_owned(),
            display_name: "lib/".to_owned(),
            relative_path: "lib".to_owned(),
            is_dir: true,
        },
    ];

    // Sort ascending by name (matching load_files behavior)
    palette.all_files.sort_by(|a, b| {
        a.relative_path
            .to_lowercase()
            .cmp(&b.relative_path.to_lowercase())
    });
    palette.filtered_files = palette.all_files.clone();
    let (groups, group_entries) = FilePalette::build_tree_groups(&palette.filtered_files);
    palette.tree_groups = groups;
    palette.group_entries = group_entries;
    palette.tree_state = TreeState::new(palette.tree_groups.len());

    // Tree groups are directories sorted ascending, then top-level files sorted ascending
    // Directories: lib, src, tests (ascending)
    // Files: Apple.txt, banana.txt, zebra.txt (ascending)
    let groups = palette.tree_groups();
    assert_eq!(groups.len(), 6); // 3 dirs + 3 files

    // Directory groups (sorted asc): lib, src, tests
    assert_eq!(groups[0].header().text(), "lib/");
    assert_eq!(groups[1].header().text(), "src/");
    assert_eq!(groups[2].header().text(), "tests/");

    // File groups (sorted asc): Apple.txt, banana.txt, zebra.txt
    assert_eq!(groups[3].header().text(), "Apple.txt");
    assert_eq!(groups[4].header().text(), "banana.txt");
    assert_eq!(groups[5].header().text(), "zebra.txt");
}

#[test]
fn test_simple_fuzzy_match() {
    // Test basic fuzzy matching
    assert!(FilePalette::simple_fuzzy_match("src/main.rs", "smr").is_some());
    assert!(FilePalette::simple_fuzzy_match("src/main.rs", "src").is_some());
    assert!(FilePalette::simple_fuzzy_match("src/main.rs", "main").is_some());

    // Test non-matches
    assert!(FilePalette::simple_fuzzy_match("src/main.rs", "xyz").is_none());
    assert!(FilePalette::simple_fuzzy_match("src/main.rs", "msr").is_none());
    // Wrong order
}

#[test]
fn test_filtering_preserves_tree_hierarchy() {
    let mut palette = FilePalette::new(PathBuf::from("/workspace"));

    palette.all_files = vec![
        FileEntry {
            path: "/workspace/src".to_owned(),
            display_name: "src/".to_owned(),
            relative_path: "src".to_owned(),
            is_dir: true,
        },
        FileEntry {
            path: "/workspace/src_file.rs".to_owned(),
            display_name: "src_file.rs".to_owned(),
            relative_path: "src_file.rs".to_owned(),
            is_dir: false,
        },
        FileEntry {
            path: "/workspace/tests".to_owned(),
            display_name: "tests/".to_owned(),
            relative_path: "tests".to_owned(),
            is_dir: true,
        },
        FileEntry {
            path: "/workspace/source.txt".to_owned(),
            display_name: "source.txt".to_owned(),
            relative_path: "source.txt".to_owned(),
            is_dir: false,
        },
    ];
    palette.filtered_files = palette.all_files.clone();

    palette.set_filter("src".to_owned());

    // Tree groups should exist — the hierarchy preserves directory structure
    let groups = palette.tree_groups();
    assert!(
        !groups.is_empty(),
        "Tree should have groups after filtering"
    );

    // Total items should reflect all matching files
    assert!(palette.total_items() > 0);
}

#[test]
fn test_current_at_token_tracks_tokens_with_second_at() {
    // Test that scoped npm packages don't trigger file picker
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
    // Test that file paths with @ in them still work
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
    // Test that mid-word @ like foo@bar doesn't trigger
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
