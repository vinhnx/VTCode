use anstyle::Style;
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use std::hint::black_box;
use vtcode_tui::ui::markdown::{RenderMarkdownOptions, render_markdown_to_lines_with_options};
use vtcode_tui::ui::theme;

fn short_assistant_markdown() -> String {
    "## Summary\n\
    - Fixed markdown rendering performance hot paths\n\
    - Preserved output behavior\n\
    \n\
    ```rust\n\
    fn main() {\n\
        println!(\"hello\");\n\
    }\n\
    ```\n\
    "
    .to_string()
}

fn large_mixed_markdown() -> String {
    let mut out = String::new();
    out.push_str("# VT Code Session Report\n\n");
    for i in 0..80 {
        out.push_str(&format!("## Section {i}\n"));
        out.push_str("- item one\n- item two\n- item three\n\n");
        out.push_str("[docs](https://example.com/docs) and [src/main.rs](/tmp/src/main.rs:42)\n\n");
        out.push_str("| Col A | Col B | Col C |\n|---|---|---|\n| 1 | 2 | 3 |\n| a | b | c |\n\n");
        out.push_str("```diff\n@@ -1 +1 @@\n- old value\n+ new value\n```\n\n");
    }
    out
}

fn nested_list_blockquote_markdown() -> String {
    let mut out = String::new();
    for i in 0..220 {
        out.push_str(&format!(
            "> - item {i}\n>   - nested {i}\n>     - deep {i}\n>       continuation line {i}\n"
        ));
    }
    out
}

fn large_fenced_code_block(language: Option<&str>) -> String {
    let fence = language.unwrap_or("");
    let mut out = format!("```{fence}\n");
    for i in 0..1800 {
        out.push_str(&format!("let value_{i} = compute({i});\n"));
    }
    out.push_str("```\n");
    out
}

fn markdown_render_benchmark(c: &mut Criterion) {
    let styles = theme::active_styles();
    let cases: Vec<(&str, String)> = vec![
        ("short_assistant", short_assistant_markdown()),
        ("large_mixed", large_mixed_markdown()),
        ("nested_list_blockquote", nested_list_blockquote_markdown()),
        (
            "large_code_with_language",
            large_fenced_code_block(Some("rust")),
        ),
        ("large_code_without_language", large_fenced_code_block(None)),
    ];

    let mut group = c.benchmark_group("markdown_render");
    for (name, input) in &cases {
        group.throughput(Throughput::Bytes(input.len() as u64));
        group.bench_with_input(BenchmarkId::from_parameter(*name), input, |b, source| {
            b.iter(|| {
                let lines = render_markdown_to_lines_with_options(
                    black_box(source.as_str()),
                    Style::default(),
                    &styles,
                    None,
                    RenderMarkdownOptions::default(),
                );
                black_box(lines.len())
            });
        });
    }
    group.finish();
}

criterion_group!(benches, markdown_render_benchmark);
criterion_main!(benches);
