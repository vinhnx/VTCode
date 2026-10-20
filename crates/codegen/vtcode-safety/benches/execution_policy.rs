//! Policy-layer deduplication and token-prefix matching benchmarks.

use criterion::{BatchSize, BenchmarkId, Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use vtcode_safety::exec_policy::{Decision, Policy, PrefixRule};

fn execution_policy(c: &mut Criterion) {
    let mut group = c.benchmark_group("policy_layers");
    let _ = group.sample_size(20);
    let _ = group.warm_up_time(std::time::Duration::from_secs(1));
    let _ = group.measurement_time(std::time::Duration::from_secs(2));
    for count in [16, 512, 4096] {
        for duplicates in [false, true] {
            let rules: Vec<_> = (0..count)
                .map(|index| {
                    let key = if duplicates { index % 16 } else { index };
                    PrefixRule::new(vec!["tool".into(), format!("arg-{key}")], Decision::Allow)
                })
                .collect();
            let _ = group.bench_with_input(
                BenchmarkId::new(if duplicates { "duplicates" } else { "unique" }, count),
                &rules,
                |b, rules| {
                    b.iter_batched(
                        || (Policy::empty(), rules.clone()),
                        |(mut policy, layer)| {
                            policy.prepend_layer(black_box(layer));
                            black_box(policy)
                        },
                        BatchSize::SmallInput,
                    );
                },
            );
        }
    }
    group.finish();
    let rule = PrefixRule::new(vec!["cargo".into(), "build".into()], Decision::Allow);
    let command = vec!["cargo".into(), "build".into(), "--release".into()];
    let _ = c.bench_function("nonempty_prefix_match", |b| {
        b.iter(|| black_box(rule.matches(black_box(&command))));
    });
}

criterion_group!(benches, execution_policy);
criterion_main!(benches);
