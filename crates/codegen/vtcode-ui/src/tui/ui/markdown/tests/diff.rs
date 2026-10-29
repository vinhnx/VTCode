use super::*;

#[test]
fn markdown_diff_code_block_stores_line_background() {
    let markdown = "```diff\n@@ -1 +1 @@\n- old\n+ new\n context\n```\n";
    let lines = render_markdown(markdown);

    let joined = |line: &MarkdownLine| line.segments.iter().map(|seg| seg.text.as_str()).collect::<String>();
    let added_line = lines
        .iter()
        .find(|line| joined(line).contains("+ new"))
        .expect("added line exists");
    assert_eq!(added_line.line_background.is_some(), diff_backgrounds_enabled());
    assert!(added_line.segments.iter().any(|seg| seg.style.get_fg_color().is_some()));
    assert_eq!(added_line.segments.iter().all(|seg| seg.style.get_bg_color().is_some()), diff_backgrounds_enabled());
    assert_eq!(added_line.segments[0].text, "+");
    assert!(!added_line.segments.iter().any(|seg| seg.text.contains('│')));

    let removed_line = lines
        .iter()
        .find(|line| joined(line).contains("- old"))
        .expect("removed line exists");
    assert_eq!(removed_line.line_background.is_some(), diff_backgrounds_enabled());
    assert!(removed_line.segments.iter().any(|seg| seg.style.get_fg_color().is_some()));
    assert_eq!(
        removed_line.segments.iter().all(|seg| seg.style.get_bg_color().is_some()),
        diff_backgrounds_enabled()
    );
    assert_eq!(removed_line.segments[0].text, "-");

    let context_line = lines
        .iter()
        .find(|line| joined(line).contains("context"))
        .expect("context line exists");
    assert!(context_line.line_background.is_none());
    assert!(context_line.segments.iter().all(|seg| seg.style.get_bg_color().is_none()));
}
#[test]
fn markdown_diff_truecolor_has_row_tint_and_stronger_word_chip() {
    let context =
        diff_render_style_context_for(DiffTheme::Dark, DiffColorLevel::TrueColor, DiffScopeBackgroundRgbs::default());
    let lines = render_diff_code_block_with_context(
        "@@ -1 +1 @@\n-const value = 1;\n+const value = 2;\n context",
        &theme::active_styles(),
        Style::default(),
        &[],
        context,
    );
    let joined = |line: &MarkdownLine| line.segments.iter().map(|segment| segment.text.as_str()).collect::<String>();
    let added = lines.iter().find(|line| joined(line).contains("value = 2")).expect("added row");
    let removed = lines
        .iter()
        .find(|line| joined(line).contains("value = 1"))
        .expect("removed row");
    let context_line = lines.iter().find(|line| joined(line).contains("context")).expect("context row");

    let add_row = anstyle::Color::Rgb(anstyle::RgbColor(20, 58, 45));
    let add_word = anstyle::Color::Rgb(anstyle::RgbColor(36, 100, 70));
    let del_row = anstyle::Color::Rgb(anstyle::RgbColor(70, 38, 42));
    let del_word = anstyle::Color::Rgb(anstyle::RgbColor(140, 52, 58));

    assert_eq!(added.line_background, Some(add_row));
    assert_eq!(removed.line_background, Some(del_row));
    assert!(
        added
            .segments
            .iter()
            .any(|segment| segment.style.get_bg_color() == Some(add_word))
    );
    assert!(
        removed
            .segments
            .iter()
            .any(|segment| segment.style.get_bg_color() == Some(del_word))
    );
    assert!(
        added
            .segments
            .iter()
            .any(|segment| segment.style.get_bg_color() == Some(add_row))
    );
    assert!(
        removed
            .segments
            .iter()
            .any(|segment| segment.style.get_bg_color() == Some(del_row))
    );
    let added_ansi = added
        .segments
        .iter()
        .map(|segment| segment.style.render().to_string())
        .collect::<String>();
    assert!(added_ansi.contains("48;2;20;58;45"), "row tint must render as ANSI background");
    assert!(added_ansi.contains("48;2;36;100;70"), "word tint must render as ANSI background");
    assert!(context_line.line_background.is_none());
    assert!(
        context_line
            .segments
            .iter()
            .all(|segment| segment.style.get_bg_color().is_none())
    );
}
#[test]
fn markdown_diff_ansi16_is_background_free() {
    let context =
        diff_render_style_context_for(DiffTheme::Dark, DiffColorLevel::Ansi16, DiffScopeBackgroundRgbs::default());
    let lines = render_diff_code_block_with_context(
        "@@ -1 +1 @@\n-old\n+new",
        &theme::active_styles(),
        Style::default(),
        &[],
        context,
    );
    assert!(lines.iter().all(|line| line.line_background.is_none()));
    assert!(
        lines
            .iter()
            .flat_map(|line| line.segments.iter())
            .all(|segment| segment.style.get_bg_color().is_none())
    );
}
#[test]
fn markdown_diff_ansi16_keeps_the_diff_foreground_through_syntax_segments() {
    let context =
        diff_render_style_context_for(DiffTheme::Dark, DiffColorLevel::Ansi16, DiffScopeBackgroundRgbs::default());
    let lines = render_diff_code_block_with_context(
        "--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1 +1 @@\n- fn old() {}\n+ fn new() {}",
        &theme::active_styles(),
        Style::default(),
        &[],
        context,
    );
    let added = lines
        .iter()
        .find(|line| lines_to_text(std::slice::from_ref(line))[0].contains("fn new()"))
        .expect("added row");
    assert!(
        added
            .segments
            .iter()
            .skip(1)
            .all(|segment| segment.style.get_fg_color().is_some()),
        "ANSI16 diff bodies must retain a foreground on every syntax segment"
    );
}
#[test]
fn markdown_diff_empty_add_delete_rows_keep_the_capability_fallback() {
    let markdown = "```diff\n@@ -1 +1 @@\n+\n- \n```\n";
    let lines = render_markdown(markdown);
    let joined = |line: &MarkdownLine| line.segments.iter().map(|seg| seg.text.as_str()).collect::<String>();
    // Exclude the `• Diff …` summary row: it carries `+N`/`-N` count text but
    // is header metadata, not a tinted add/del body row.
    let diff_rows: Vec<_> = lines
        .iter()
        .filter(|line| {
            let text = joined(line);
            let trimmed = text.trim_start();
            !trimmed.starts_with("• Diff ")
                && (trimmed.contains('+') || trimmed.contains('-') || trimmed.contains("@@"))
        })
        .collect();
    assert!(diff_rows.len() >= 2, "empty + and - rows must render: {diff_rows:?}");
    for line in diff_rows {
        let text = joined(line);
        if text.trim_start().starts_with("@@") {
            assert!(line.line_background.is_none(), "hunk metadata must stay un-tinted");
            assert!(line.segments.iter().all(|seg| seg.style.get_bg_color().is_none()));
        } else {
            assert_eq!(line.line_background.is_some(), diff_backgrounds_enabled());
            assert_eq!(
                line.segments.iter().all(|seg| seg.style.get_bg_color().is_some()),
                diff_backgrounds_enabled(),
                "empty row must have no background holes"
            );
        }
    }
}
#[test]
fn markdown_diff_header_styles_are_classified() {
    let markdown = "```diff\ndiff --git a/main.rs b/main.rs\nindex 1111111..2222222 100644\n--- a/main.rs\n+++ b/main.rs\n@@ -1 +1 @@\n```\n";
    let lines = render_markdown(markdown);

    let text_and_style = |needle: &str| {
        lines
            .iter()
            .flat_map(|line| line.segments.iter())
            .find(|segment| segment.text.contains(needle))
            .map(|segment| segment.style)
            .unwrap_or_else(|| panic!("{needle} segment exists"))
    };
    let metadata = anstyle::Color::Ansi(anstyle::AnsiColor::BrightBlack);
    let cyan = anstyle::Color::Ansi(anstyle::AnsiColor::Cyan);
    let detected_level = DiffColorLevel::detect();
    let detected_theme = DiffTheme::detect();
    let deletion = diff_del_fg(detected_theme, detected_level);
    let insertion = diff_add_fg(detected_theme, detected_level);

    assert_eq!(text_and_style("diff --git").get_fg_color(), Some(metadata));
    assert_eq!(text_and_style("index 1111111").get_fg_color(), Some(metadata));
    assert_eq!(text_and_style("--- a/main.rs").get_fg_color(), Some(deletion));
    assert_eq!(text_and_style("+++ b/main.rs").get_fg_color(), Some(insertion));
    assert_eq!(text_and_style("@@ -1 +1 @@").get_fg_color(), Some(cyan));
    for needle in ["--- a/main.rs", "+++ b/main.rs", "@@ -1 +1 @@"] {
        assert_eq!(text_and_style(needle).get_bg_color(), None, "{needle} must be background-free");
    }
}
#[test]
fn markdown_diff_preserves_hunk_range_counts() {
    // Regression: a partial hunk must keep its counts; `@@ -65,19 +64,0 @@`
    // collapsed to `@@ -65 +64 @@` reads as a one-line change.
    let markdown = "```diff\n@@ -65,19 +64,0 @@\n-removed\n```\n";
    let lines = render_markdown(markdown);
    let text = lines_to_text(&lines);
    assert!(
        text.iter().any(|line| line.trim() == "@@ -65,19 +64,0 @@"),
        "hunk header must keep range counts, got: {text:?}"
    );
}
#[test]
fn markdown_diff_lines_use_row_tints_and_strong_markers() {
    let markdown = "```diff\n-const value = 1;\n+const value = 2;\n```\n";
    let lines = render_markdown(markdown);

    let added_line = lines
        .iter()
        .find(|line| lines_to_text(std::slice::from_ref(line))[0].contains("const value = 2;"))
        .expect("added line exists");
    let removed_line = lines
        .iter()
        .find(|line| lines_to_text(std::slice::from_ref(line))[0].contains("const value = 1;"))
        .expect("removed line exists");

    // Marker-only reference shape: sign + body, without a second gutter.
    assert_eq!(added_line.segments[0].text, "+");
    assert!(!added_line.segments.iter().any(|seg| seg.text.contains('│')));
    assert_eq!(added_line.line_background.is_some(), diff_backgrounds_enabled());
    assert_eq!(removed_line.line_background.is_some(), diff_backgrounds_enabled());
    // The sign stays bold for scannability.
    assert!(added_line.segments[0].style.get_effects().contains(anstyle::Effects::BOLD));
    // Color-capable output paints every segment; ANSI16 remains foreground-only.
    assert_eq!(added_line.segments.iter().all(|seg| seg.style.get_bg_color().is_some()), diff_backgrounds_enabled());
    assert_eq!(
        removed_line.segments.iter().all(|seg| seg.style.get_bg_color().is_some()),
        diff_backgrounds_enabled()
    );
}
#[test]
fn diff_style_helpers_use_row_tint_and_ansi16_fallback() {
    let truecolor = DiffScopeBackgroundRgbs::default();
    for theme in [DiffTheme::Light, DiffTheme::Dark] {
        let context = diff_render_style_context_for(theme, DiffColorLevel::TrueColor, truecolor);
        for kind in [DiffLineType::Insert, DiffLineType::Delete] {
            let style = style_content_ansi(kind, context);
            let marker = style_sign_ansi(kind, context);
            assert!(style.get_bg_color().is_some());
            assert_eq!(marker.get_bg_color(), style.get_bg_color());
            assert!(marker.get_fg_color().is_some());
            assert_eq!(style.get_fg_color(), None);
        }
    }

    let ansi16 = diff_render_style_context_for(DiffTheme::Dark, DiffColorLevel::Ansi16, truecolor);
    assert!(style_content_ansi(DiffLineType::Insert, ansi16).get_bg_color().is_none());
    assert!(style_sign_ansi(DiffLineType::Insert, ansi16).get_bg_color().is_none());
}
#[test]
fn test_markdown_unlabeled_diff_code_block_detects_diff() {
    let markdown = "```\n@@ -1 +1 @@\n- old\n+ new\n```\n";
    let lines = render_markdown_to_lines(markdown, Style::default(), &theme::active_styles(), None);
    let added_line = lines
        .iter()
        .find(|line| {
            line.segments
                .iter()
                .map(|seg| seg.text.as_str())
                .collect::<String>()
                .contains("new")
        })
        .expect("added line exists");
    assert_eq!(added_line.line_background.is_some(), diff_backgrounds_enabled());
    assert!(added_line.segments.iter().any(|seg| seg.style.get_fg_color().is_some()));
    assert_eq!(added_line.segments.iter().all(|seg| seg.style.get_bg_color().is_some()), diff_backgrounds_enabled());
}
#[test]
fn test_diff_blank_line_renders_placeholder_space() {
    let segments = render_diff_content_segments("", Some("toml"), Style::default());

    assert_eq!(segments.len(), 1);
    assert_eq!(segments[0].text, " ");
}
#[test]
fn markdown_diff_code_block_styles_additions_deletions_and_hunk_headers() {
    let markdown = "```diff\n--- a/main.rs\n+++ b/main.rs\n@@ -1 +1 @@\n-const value = 1;\n+const value = 2;\n```\n";
    let lines = render_markdown(markdown);

    let joined = |line: &MarkdownLine| line.segments.iter().map(|seg| seg.text.as_str()).collect::<String>();
    let added_line = lines
        .iter()
        .find(|line| joined(line).contains("const value = 2;"))
        .expect("added line exists");
    assert_eq!(added_line.line_background.is_some(), diff_backgrounds_enabled());

    let removed_line = lines
        .iter()
        .find(|line| joined(line).contains("const value = 1;"))
        .expect("removed line exists");
    assert_eq!(removed_line.line_background.is_some(), diff_backgrounds_enabled());

    let header_line = lines
        .iter()
        .find(|line| joined(line).contains("@@ -1 +1 @@"))
        .expect("hunk header exists");
    assert_eq!(header_line.segments[0].style.get_fg_color(), Some(anstyle::Color::Ansi(anstyle::AnsiColor::Cyan)));
    assert!(header_line.line_background.is_none());

    let old_header = lines
        .iter()
        .find(|line| joined(line).contains("--- a/main.rs"))
        .expect("old file header exists");
    assert!(old_header.line_background.is_none());

    let new_header = lines
        .iter()
        .find(|line| joined(line).contains("+++ b/main.rs"))
        .expect("new file header exists");
    assert!(new_header.line_background.is_none());
}
#[test]
fn markdown_diff_rows_use_full_backgrounds_without_holes() {
    let markdown = "```diff\n--- a/main.rs\n+++ b/main.rs\n@@ -1 +1 @@\n-const value = 1;\n+const value = 2;\n```\n";
    let lines = render_markdown(markdown);

    for needle in [
        "const value = 2;",
        "const value = 1;",
        "--- a/main.rs",
        "+++ b/main.rs",
        "@@ -1 +1 @@",
    ] {
        let line = lines
            .iter()
            .find(|line| {
                line.segments
                    .iter()
                    .map(|seg| seg.text.as_str())
                    .collect::<String>()
                    .contains(needle)
            })
            .unwrap_or_else(|| panic!("{needle} line exists"));
        let is_metadata = needle.starts_with("--- ") || needle.starts_with("+++ ") || needle.starts_with("@@");
        assert_eq!(
            line.line_background.is_some(),
            !is_metadata && diff_backgrounds_enabled(),
            "row bg state in {needle}"
        );
        assert!(!line.segments.is_empty());
        for segment in &line.segments {
            assert_eq!(
                segment.style.get_bg_color().is_some(),
                !is_metadata && diff_backgrounds_enabled(),
                "bg hole in {needle}"
            );
        }
        assert!(line.segments.iter().any(|seg| seg.style.get_fg_color().is_some()), "missing fg in {needle}");
    }
}
#[test]
fn markdown_diff_body_preserves_syntax_and_row_backgrounds() {
    let markdown = "```diff\ndiff --git a/main.rs b/main.rs\n--- a/main.rs\n+++ b/main.rs\n@@ -1 +1 @@\n-fn old() {}\n+fn new() {}\n```\n";
    let lines = render_markdown(markdown);

    let find_body = |needle: &str| {
        lines
            .iter()
            .find(|line| {
                line.segments
                    .iter()
                    .map(|seg| seg.text.as_str())
                    .collect::<String>()
                    .contains(needle)
            })
            .unwrap_or_else(|| panic!("{needle} line exists"))
    };
    let added_line = find_body("fn new()");
    let removed_line = find_body("fn old()");

    // Marker + body, with no second gutter and no unpainted row holes.
    assert!(added_line.segments.len() >= 2, "added body needs marker + body");
    assert!(removed_line.segments.len() >= 2, "removed body needs marker + body");
    assert!(
        added_line
            .segments
            .iter()
            .skip(1)
            .all(|segment| { segment.style.get_bg_color().is_some() == diff_backgrounds_enabled() })
    );
    assert!(
        removed_line
            .segments
            .iter()
            .skip(1)
            .all(|segment| { segment.style.get_bg_color().is_some() == diff_backgrounds_enabled() })
    );
    assert_eq!(added_line.line_background.is_some(), diff_backgrounds_enabled());
    assert_eq!(removed_line.line_background.is_some(), diff_backgrounds_enabled());
}
#[test]
fn markdown_apply_patch_headers_provide_syntax_language_for_diff_bodies() {
    let context =
        diff_render_style_context_for(DiffTheme::Dark, DiffColorLevel::TrueColor, DiffScopeBackgroundRgbs::default());
    let lines = render_diff_code_block_with_context(
        "*** Begin Patch\n*** Update File: src/main.rs\n@@\n- fn old() {}\n+ fn new() {}\n*** End Patch",
        &theme::active_styles(),
        Style::default(),
        &[],
        context,
    );
    let added = lines
        .iter()
        .find(|line| lines_to_text(std::slice::from_ref(line))[0].contains("fn new()"))
        .expect("added apply_patch body exists");
    assert!(
        added
            .segments
            .iter()
            .skip(1)
            .any(|segment| segment.style.get_fg_color().is_some()),
        "the .rs apply_patch body should retain syntax foregrounds"
    );
}
#[test]
fn markdown_diff_body_without_path_stays_solid() {
    let markdown = "```diff\n@@ -1 +1 @@\n-fn old() {}\n+fn new() {}\n```\n";
    let lines = render_markdown(markdown);

    let added_line = lines
        .iter()
        .find(|line| {
            line.segments
                .iter()
                .map(|seg| seg.text.as_str())
                .collect::<String>()
                .contains("fn new()")
        })
        .expect("added line exists");
    // No file header: marker + body (word chips may split the body).
    assert!(added_line.segments.len() >= 2);
    assert_eq!(added_line.segments[0].text, "+");
}
#[test]
fn markdown_diff_prose_file_body_uses_row_tint_without_syntax_split() {
    let markdown = "```diff\ndiff --git a/README.md b/README.md\n--- a/README.md\n+++ b/README.md\n@@ -1 +1 @@\n-old **bold** text\n+new **bold** text\n```\n";
    let lines = render_markdown(markdown);

    let added_line = lines
        .iter()
        .find(|line| {
            line.segments
                .iter()
                .map(|seg| seg.text.as_str())
                .collect::<String>()
                .contains("new **bold** text")
        })
        .expect("added line exists");
    // Marker + body, with prose kept as one body segment.
    assert!(added_line.segments.len() >= 2, "prose body: marker + body");
    assert_eq!(added_line.line_background.is_some(), diff_backgrounds_enabled());
}
#[test]
fn test_diff_prose_content_skips_syntax_highlighting() {
    let fallback = Style::new().fg_color(Some(anstyle::Color::Ansi(anstyle::AnsiColor::BrightGreen)));
    for hint in [Some("md"), Some("markdown"), Some("txt")] {
        let segments = render_diff_content_segments("- **Agent-first**: prose `code`", hint, fallback);
        assert_eq!(segments.len(), 1, "prose hint {hint:?} must stay solid");
        assert_eq!(segments[0].text, "- **Agent-first**: prose `code`");
        assert_eq!(segments[0].style, fallback);
    }
}
#[test]
fn test_diff_unknown_language_stays_solid_fallback() {
    let fallback = Style::new().fg_color(Some(anstyle::Color::Ansi(anstyle::AnsiColor::BrightGreen)));
    for hint in [None, Some(""), Some("xyzunknown123")] {
        let segments = render_diff_content_segments("fn new() {}", hint, fallback);
        assert_eq!(segments.len(), 1, "unknown hint {hint:?} must stay solid");
        assert_eq!(segments[0].style, fallback);
    }
}
#[test]
fn markdown_unlabeled_minimal_hunk_detects_diff() {
    let markdown = "```\n@@\n pub fn demo() {\n  -    old();\n  +    new();\n }\n```\n";
    let lines = render_markdown_to_lines(markdown, Style::default(), &theme::active_styles(), None);
    let header_segment = lines
        .iter()
        .flat_map(|line| line.segments.iter())
        .find(|seg| seg.text.trim() == "@@")
        .expect("hunk header exists");
    assert_eq!(header_segment.style.get_fg_color(), Some(anstyle::Color::Ansi(anstyle::AnsiColor::Cyan)));

    let joined = |line: &MarkdownLine| line.segments.iter().map(|seg| seg.text.as_str()).collect::<String>();
    let removed_segment = lines
        .iter()
        .find(|line| joined(line).contains("old();"))
        .expect("removed line exists");
    assert_eq!(removed_segment.line_background.is_some(), diff_backgrounds_enabled());

    let added_segment = lines
        .iter()
        .find(|line| joined(line).contains("new();"))
        .expect("added line exists");
    assert_eq!(added_segment.line_background.is_some(), diff_backgrounds_enabled());
}
#[test]
fn test_highlight_line_for_diff_strips_background_colors() {
    let segments =
        highlight_line_for_diff("let changed = true;", Some("rust")).expect("highlighting should return segments");
    assert!(segments.iter().all(|(style, _)| style.get_bg_color().is_none()));
}
