#![allow(
    missing_docs,
    clippy::expect_used,
    reason = "Intentional benchmark-only suppression."
)]
#![expect(
    unused_results,
    reason = "Benchmark setup and criterion group methods are invoked for their measurement side effects."
)]

//! Micro-benchmark for the shell-command classifier hot path.
//!
//! A single `validate_shell_script` call parses the same script with the bash
//! grammar several times across `contains_dynamic_find_syntax`,
//! `validate_redirection_paths`, and `parse_shell_commands`. The shared parsed
//! tree cache turns those repeats into cache hits; this bench makes a
//! regression back to per-pass re-parsing visible rather than silent (the same
//! "measure the fix" discipline behind the clippy `nonstandard_macro_braces`
//! speedup).

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use vtcode_safety::command_safety::shell_parser::parse_shell_commands;
use vtcode_safety::command_safety::validate_shell_script;

/// Commands that exercise the compound / pipeline / redirection paths that
/// trigger multiple parses inside one validation.
const SCRIPTS: &[&str] = &[
    "git status",
    "cargo check > build.log 2>&1",
    "for f in src/*.rs; do grep -n pattern \"$f\" | head -5; done",
    "cat manifest.toml | grep -n section | sort | uniq -c > out.txt",
];

fn bench_shell_classification(c: &mut Criterion) {
    let mut group = c.benchmark_group("shell_classification");

    for script in SCRIPTS {
        group.bench_function(format!("validate:{script}"), |b| {
            b.iter(|| {
                let result = validate_shell_script(black_box(script));
                assert!(result.is_ok(), "fixture must stay valid: {script}");
            });
        });
    }

    group.bench_function("parse_repeated_same_script", |b| {
        let script = SCRIPTS.get(2).copied().expect("loop fixture present");
        b.iter(|| {
            let commands = parse_shell_commands(black_box(script)).expect("fixture parses");
            assert!(!commands.is_empty());
        });
    });

    group.finish();
}

criterion_group!(benches, bench_shell_classification);
criterion_main!(benches);
