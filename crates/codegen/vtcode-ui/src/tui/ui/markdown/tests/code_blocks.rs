use super::*;

#[test]
fn test_markdown_code_block_with_language_renders_line_numbers() {
    let markdown = "```rust\nfn main() {}\n```\n";
    let lines = render_markdown(markdown);
    let text_lines = lines_to_text(&lines);
    let code_line = text_lines
        .iter()
        .find(|line| line.contains("fn main() {}"))
        .expect("code line exists");
    assert!(code_line.contains("  1  "));
}
#[test]
fn test_markdown_code_block_trailing_newline_has_no_phantom_line_number() {
    let markdown = "```rust\nfn first() {}\nfn second() {}\n```\n";
    let lines = render_markdown_highlighted(markdown);

    let code_lines = lines
        .iter()
        .filter(|line| {
            let text = line.segments.iter().map(|segment| segment.text.as_str()).collect::<String>();
            text.contains("fn first() {}") || text.contains("fn second() {}")
        })
        .count();
    assert_eq!(code_lines, 2);

    assert!(
        lines
            .iter()
            .any(|line| line.segments.iter().any(|segment| segment.text == "  2  ")),
        "second source line should have a gutter"
    );
    assert!(
        lines
            .iter()
            .all(|line| line.segments.iter().all(|segment| segment.text != "  3  ")),
        "trailing newline should not create a phantom gutter"
    );
}
#[test]
fn test_markdown_code_block_leading_whitespace_uses_code_style() {
    let markdown = "```rust\nlet value = 1;\n    return value;\n```\n";
    let lines = render_markdown_highlighted(markdown);

    let code_line = lines
        .iter()
        .find(|line| {
            line.segments
                .iter()
                .map(|segment| segment.text.as_str())
                .collect::<String>()
                .contains("return value")
        })
        .expect("code line exists");
    let gutter_style = code_line.segments.first().expect("gutter segment exists").style;
    let first_code_segment = code_line
        .segments
        .iter()
        .find(|segment| segment.text == "    ")
        .expect("leading whitespace segment exists");

    assert_ne!(first_code_segment.style, gutter_style);
}
#[test]
fn test_markdown_code_block_omitted_line_gutter_uses_source_line_numbers() {
    let markdown = "```rust\n\
line 1\n\
line 2\n\
… [+70 lines omitted; use read_file with offset/limit (1-indexed line numbers) for full content]\n\
tail line\n\
```\n";
    let lines = render_markdown(markdown);
    let text_lines = lines_to_text(&lines);

    let omitted_line = text_lines
        .iter()
        .find(|line| line.contains("lines omitted"))
        .expect("omitted line exists");
    assert!(omitted_line.contains("3-72  "), "omitted line should render source range, got: {omitted_line}");

    let tail_line = text_lines
        .iter()
        .find(|line| line.contains("tail line"))
        .expect("tail line exists");
    assert!(tail_line.contains("73  "), "tail line should continue from omitted range, got: {tail_line}");
}
#[test]
fn test_markdown_code_block_without_language_skips_line_numbers() {
    let markdown = "```\nfn main() {}\n```\n";
    let lines = render_markdown(markdown);
    let text_lines = lines_to_text(&lines);
    let code_line = text_lines
        .iter()
        .find(|line| line.contains("fn main() {}"))
        .expect("code line exists");
    assert!(!code_line.contains("  1  "));
}
#[test]
fn muted_code_block_recedes_toward_the_theme_background() {
    let markdown = "```rust\nfn main() { let answer = 42; }\n```\n";
    let theme_styles = theme::active_styles();
    let highlight_config = SyntaxHighlightingConfig::default();

    let primary = render_markdown_to_lines_with_options(
        markdown,
        Style::default(),
        &theme_styles,
        Some(&highlight_config),
        RenderMarkdownOptions {
            code_block_weight: CodeBlockWeight::Primary,
            ..Default::default()
        },
    );
    let muted = render_markdown_to_lines_with_options(
        markdown,
        Style::default(),
        &theme_styles,
        Some(&highlight_config),
        RenderMarkdownOptions {
            code_block_weight: CodeBlockWeight::Muted,
            ..Default::default()
        },
    );

    let primary_colors = code_foreground_colors(&primary);
    let muted_colors = code_foreground_colors(&muted);
    assert!(!primary_colors.is_empty(), "the code block must render foreground colors");
    assert_eq!(primary_colors.len(), muted_colors.len(), "muting must not change the segment count");
    let anstyle::Color::Rgb(background) = theme_styles.background else {
        return;
    };
    for (primary, muted) in primary_colors.iter().zip(muted_colors.iter()) {
        assert_ne!(primary, muted, "muted code must recede from the primary palette");
        let (anstyle::Color::Rgb(primary_rgb), anstyle::Color::Rgb(muted_rgb)) = (*primary, *muted) else {
            continue;
        };
        assert!(
            crate::theme::contrast_ratio(muted_rgb, background) < crate::theme::contrast_ratio(primary_rgb, background),
            "muted code must move toward the background"
        );
    }
}
#[test]
fn muted_code_block_keeps_the_theme_contrast_floor() {
    let markdown = "```rust\nfn main() { let answer = 42; }\n```\n";
    let theme_styles = theme::active_styles();
    let min_contrast = crate::theme::get_minimum_contrast();

    let muted = render_markdown_to_lines_with_options(
        markdown,
        Style::default(),
        &theme_styles,
        Some(&SyntaxHighlightingConfig::default()),
        RenderMarkdownOptions {
            code_block_weight: CodeBlockWeight::Muted,
            ..Default::default()
        },
    );

    let anstyle::Color::Rgb(background) = theme_styles.background else {
        return;
    };
    // Every segment of a muted code line — the gutter and the body — must
    // clear the floor; the gutter is new chrome in the muted block.
    for line in muted.iter().filter(|line| {
        line.segments
            .iter()
            .map(|segment| segment.text.as_str())
            .collect::<String>()
            .contains("fn main()")
    }) {
        for segment in &line.segments {
            let Some(anstyle::Color::Rgb(rgb)) = segment.style.get_fg_color() else {
                continue;
            };
            assert!(
                crate::theme::contrast_ratio(rgb, background) >= min_contrast,
                "muted code must stay legible: {rgb:?} on {background:?}"
            );
        }
    }
}
#[test]
fn muted_code_block_keeps_syntax_highlighting_distinguishable() {
    let markdown = "```rust\nfn main() { let answer = 42; }\n```\n";
    let theme_styles = theme::active_styles();
    let highlight_config = SyntaxHighlightingConfig::default();

    let muted = render_markdown_to_lines_with_options(
        markdown,
        Style::default(),
        &theme_styles,
        Some(&highlight_config),
        RenderMarkdownOptions {
            code_block_weight: CodeBlockWeight::Muted,
            ..Default::default()
        },
    );

    let distinct = code_foreground_colors(&muted)
        .into_iter()
        .collect::<std::collections::HashSet<_>>();
    assert!(distinct.len() >= 2, "muted code must stay syntax highlighted, got {distinct:?}");
}
