use super::*;

#[test]
fn named_options_match_positional_constructors() {
    let options = DiffRendererOptions {
        show_line_numbers: true,
        context_lines: 3,
        use_colors: false,
    };
    let from_named = DiffRenderer::with_options(options);
    let from_positional = DiffRenderer::new(true, 3, false);
    assert_eq!(
        from_named.render_diff(&from_positional.generate_diff("a", "b", "f")),
        from_positional.render_diff(&from_positional.generate_diff("a", "b", "f"))
    );

    let chat_named = DiffChatRenderer::with_options(options);
    let preview_named = chat_named.render_file_change(Path::new("f"), "a", "b");
    let chat_positional = DiffChatRenderer::new(true, 3, false);
    let preview_positional = chat_positional.render_file_change(Path::new("f"), "a", "b");
    assert_eq!(preview_named, preview_positional);

    let config = GitColorConfig::default();
    let git_named = DiffRenderer::with_git_config_options(options, &config);
    let git_positional = DiffRenderer::with_git_config(true, 3, false, &config);
    assert_eq!(
        git_named.render_diff(&git_positional.generate_diff("a", "b", "f")),
        git_positional.render_diff(&git_positional.generate_diff("a", "b", "f"))
    );
}

#[test]
fn test_suppression_check_no_suppression() {
    let renderer = DiffChatRenderer::new(true, 3, false);
    let changes = vec![
        ("file1.rs".to_owned(), "old1".to_owned(), "new1".to_owned()),
        ("file2.rs".to_owned(), "old2".to_owned(), "new2".to_owned()),
    ];

    let check = renderer.check_suppression(&changes);
    assert!(!check.should_suppress);
    assert!(check.reason.is_none());
    assert_eq!(check.file_count, 2);
}

#[test]
fn test_suppression_check_too_many_files() {
    let renderer = DiffChatRenderer::new(true, 3, false);
    // Create more files than MAX_INLINE_DIFF_FILES
    let mut changes = Vec::new();
    for i in 0..(diff_constants::MAX_INLINE_DIFF_FILES + 5) {
        changes.push((format!("file{i}.rs"), format!("old{i}"), format!("new{i}")));
    }

    let check = renderer.check_suppression(&changes);
    assert!(check.should_suppress);
    assert!(check.reason.is_some());
    assert!(check.reason.as_ref().unwrap().contains("Too many files"));
}

#[test]
fn test_suppression_check_large_single_file() {
    let renderer = DiffChatRenderer::new(true, 3, false);

    // Create a file with many changes
    let old_content: String = (0..300).map(|i| format!("old line {i}\n")).collect();
    let new_content: String = (0..300).map(|i| format!("new line {i}\n")).collect();

    let changes = vec![("large_file.rs".to_owned(), old_content, new_content)];

    let check = renderer.check_suppression(&changes);
    assert!(check.should_suppress);
    assert!(check.reason.is_some());
}

#[test]
fn test_render_suppressed_summary() {
    let renderer = DiffChatRenderer::new(true, 3, false);
    let file_stats = vec![
        FileChangeStats {
            path: "file1.rs".to_owned(),
            additions: 20,
            deletions: 10,
        },
        FileChangeStats {
            path: "file2.rs".to_owned(),
            additions: 30,
            deletions: 20,
        },
    ];
    let check = DiffSuppressionCheck::suppressed("Test reason".to_string(), 5, 100, 50, 30, file_stats);

    let output = renderer.render_suppressed_summary(&check);
    assert!(output.contains(diff_constants::SUPPRESSION_MESSAGE));
    assert!(output.contains("5 file(s) changed"));
    assert!(output.contains("50")); // additions
    assert!(output.contains("30")); // deletions
    assert!(output.contains("Test reason"));
    assert!(output.contains("file1.rs"));
    assert!(output.contains("file2.rs"));
}

#[test]
fn test_render_multiple_changes_uses_bounded_preview() {
    let renderer = DiffChatRenderer::new(true, 3, false);

    let old = (0..300).map(|i| format!("old-{i}\n")).collect::<String>();
    let new = (0..300).map(|i| format!("new-{i}\n")).collect::<String>();
    let changes = vec![("large.rs".to_owned(), old, new)];

    let output = renderer.render_multiple_changes(changes);
    assert!(!output.contains(diff_constants::SUPPRESSION_MESSAGE));
    assert!(output.contains("old-0"));
    assert!(output.contains("new-299"));
    assert!(output.contains("lines omitted"));
}

#[test]
fn test_render_multiple_changes_without_suppression() {
    let renderer = DiffChatRenderer::new(true, 3, false);
    let changes = vec![
        ("file1.rs".to_string(), "a".to_string(), "b".to_string()),
        ("file2.rs".to_string(), "c".to_string(), "d".to_string()),
    ];

    let output = renderer.render_multiple_changes(changes);
    assert!(output.contains("Multiple File Changes"));
    assert!(!output.contains(diff_constants::SUPPRESSION_MESSAGE));
}

#[test]
fn test_render_file_change_includes_summary_and_hunk_header() {
    let renderer = DiffChatRenderer::new(true, 3, false);
    let output = renderer.render_file_change(Path::new("file.rs"), "a\nb\n", "a\nc\n");

    assert!(output.contains("▸ Edit file.rs (+1 -1)"));
    assert!(output.contains("@@ -1 +1 @@"));
}

#[test]
fn test_render_file_change_keeps_blank_added_line_spacing() {
    let renderer = DiffChatRenderer::new(false, 3, false);
    let output = renderer.render_file_change(
        Path::new("Cargo.toml"),
        "[package]\n[dependencies]\n",
        "[package]\n[dependencies]\n\n[workspace]\n",
    );

    assert!(output.contains("\n+ \n+ [workspace]\n"));
}

#[test]
fn test_render_file_change_normalizes_carriage_return_line_endings() {
    let renderer = DiffChatRenderer::new(false, 3, false);
    let output = renderer.render_file_change(Path::new("file.rs"), "old\r\n", "new\r\n");

    assert!(!output.contains('\r'));
    assert!(output.contains("- old\n+ new\n"));
}

#[test]
fn test_render_file_change_bounds_large_preview() {
    let renderer = DiffChatRenderer::new(false, 3, false);
    let old = (0..300).map(|index| format!("old-{index}\n")).collect::<String>();
    let new = (0..300).map(|index| format!("new-{index}\n")).collect::<String>();

    let output = renderer.render_file_change(Path::new("large.rs"), &old, &new);

    assert!(output.contains("old-0"));
    assert!(output.contains("new-299"));
    assert!(output.contains("lines omitted"));
    assert!(output.lines().count() <= diff_constants::MAX_PREVIEW_LINES + 1);
}
