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
fn test_pagination() {
    let mut palette = FilePalette::new(PathBuf::from("/workspace"));
    let files: Vec<String> = (0..50).map(|i| format!("file{}.rs", i)).collect();
    palette.load_files(files);

    // With PAGE_SIZE=20, 50 files = 3 pages (20 + 20 + 10)
    assert_eq!(palette.total_pages(), 3);
    assert_eq!(palette.current_page_number(), 1);
    assert_eq!(palette.current_page_items().len(), 20);
    assert!(palette.has_more_items());

    palette.page_down();
    assert_eq!(palette.current_page_number(), 2);
    assert_eq!(palette.current_page_items().len(), 20);
    assert!(palette.has_more_items());

    palette.page_down();
    assert_eq!(palette.current_page_number(), 3);
    assert_eq!(palette.current_page_items().len(), 10);
    assert!(!palette.has_more_items());
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

    // Exact filename match should rank highest
    let items = palette.current_page_items();
    assert_eq!(items[0].1.relative_path, "main.rs");
}

#[test]
fn test_has_files() {
    let mut palette = FilePalette::new(PathBuf::from("/workspace"));
    assert!(!palette.has_files());

    palette.load_files(vec!["file.rs".to_owned()]);
    assert!(palette.has_files());
}

#[test]
fn test_circular_navigation() {
    let mut palette = FilePalette::new(PathBuf::from("/workspace"));
    let files = vec!["a.rs".to_owned(), "b.rs".to_owned(), "c.rs".to_owned()];
    palette.load_files(files);

    // At first item, up should wrap to last
    assert_eq!(palette.selected_index, 0);
    palette.move_selection_up();
    assert_eq!(palette.selected_index, 2);

    // At last item, down should wrap to first
    palette.move_selection_down();
    assert_eq!(palette.selected_index, 0);
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
fn test_sorting_directories_first_alphabetical() {
    let mut palette = FilePalette::new(PathBuf::from("/workspace"));

    // Create test entries directly (bypassing filesystem checks)
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

    // Apply sorting manually
    palette
        .all_files
        .sort_by(|a, b| match (a.is_dir, b.is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a
                .relative_path
                .to_lowercase()
                .cmp(&b.relative_path.to_lowercase()),
        });
    palette.filtered_files = palette.all_files.clone();

    // Directories should come first, then files
    // Within each group, alphabetically sorted (case-insensitive)
    let items = palette.current_page_items();

    // First items should be directories (alphabetically: lib, src, tests)
    assert!(items[0].1.is_dir);
    assert_eq!(items[0].1.relative_path, "lib");
    assert!(items[1].1.is_dir);
    assert_eq!(items[1].1.relative_path, "src");
    assert!(items[2].1.is_dir);
    assert_eq!(items[2].1.relative_path, "tests");

    // Then files, alphabetically (Apple.txt, banana.txt, zebra.txt)
    assert!(!items[3].1.is_dir);
    assert_eq!(items[3].1.relative_path, "Apple.txt");
    assert!(!items[4].1.is_dir);
    assert_eq!(items[4].1.relative_path, "banana.txt");
    assert!(!items[5].1.is_dir);
    assert_eq!(items[5].1.relative_path, "zebra.txt");
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
fn test_filtering_maintains_directory_priority() {
    let mut palette = FilePalette::new(PathBuf::from("/workspace"));

    // Create test entries with both directories and files
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

    // Filter for "src" - should match directories and files
    palette.set_filter("src".to_owned());

    // Directories should still be first even after filtering
    let items = palette.current_page_items();
    let dir_count = items.iter().filter(|(_, entry, _)| entry.is_dir).count();
    let file_count = items.iter().filter(|(_, entry, _)| !entry.is_dir).count();

    if dir_count > 0 && file_count > 0 {
        // Find first directory and first file
        let first_dir_idx = items.iter().position(|(_, entry, _)| entry.is_dir).unwrap();
        let first_file_idx = items
            .iter()
            .position(|(_, entry, _)| !entry.is_dir)
            .unwrap();

        // First directory should come before first file
        assert!(
            first_dir_idx < first_file_idx,
            "Directories should appear before files even after filtering"
        );
    }
}
