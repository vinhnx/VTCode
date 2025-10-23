use criterion::{Criterion, black_box, criterion_group, criterion_main};
use vtcode_core::TreeSitterAnalyzer;
use vtcode_core::tools::tree_sitter::LanguageSupport;

/// Benchmark tree-sitter parsing performance
fn benchmark_tree_sitter_parsing(c: &mut Criterion) {
    let mut group = c.benchmark_group("tree_sitter");

    // Create a simple test code snippet
    let test_code = r#"
    fn main() {
        println!("Hello, world!");
        let x = 42;
        if x > 0 {
            println!("Positive number");
        }
    }
    "#;

    group.bench_function("parse_rust_code", |b| {
        b.iter(|| {
            let mut analyzer = TreeSitterAnalyzer::new().expect("Failed to create analyzer");
            let tree = analyzer.parse(test_code, LanguageSupport::Rust);
            black_box(tree.is_ok())
        })
    });

    group.bench_function("extract_symbols", |b| {
        b.iter(|| {
            let mut analyzer = TreeSitterAnalyzer::new().expect("Failed to create analyzer");
            if let Ok(tree) = analyzer.parse(test_code, LanguageSupport::Rust) {
                let symbols = analyzer.extract_symbols(&tree, test_code, LanguageSupport::Rust);
                black_box(symbols.is_ok())
            } else {
                black_box(false)
            }
        })
    });

    group.finish();
}

criterion_group!(benches, benchmark_tree_sitter_parsing);
criterion_main!(benches);
