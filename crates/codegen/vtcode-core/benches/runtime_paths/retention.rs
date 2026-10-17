use std::hint::black_box;

use criterion::{BatchSize, BenchmarkId, Criterion};
use vtcode_exec_events::{ThreadCompletedEvent, ThreadCompletionSubtype, ThreadEvent, Usage};

use super::memory::fixtures;

pub(super) fn benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("session_retention");
    for count in [8, 64, 256] {
        let _ = group.bench_function(BenchmarkId::from_parameter(count), |b| {
            b.iter_batched(
                || {
                    let workspace = tempfile::tempdir().expect("retention workspace");
                    for index in 0..count {
                        let id = format!("retention-{index}");
                        let log = vtcode_memory::open(workspace.path(), &id, 0).expect("retention log");
                        for event in fixtures(1, 128) {
                            log.append(&event).expect("retention turn");
                        }
                        log.append(&ThreadEvent::ThreadCompleted(Box::new(ThreadCompletedEvent {
                            completed_at: None,
                            thread_id: id.clone(),
                            session_id: id,
                            subtype: ThreadCompletionSubtype::Success,
                            outcome_code: "success".into(),
                            result: None,
                            stop_reason: None,
                            usage: Usage::default(),
                            total_cost_usd: None,
                            num_turns: 1,
                        })))
                        .expect("complete retention log");
                        log.flush().expect("durable retention setup");
                    }
                    workspace
                },
                |workspace| {
                    let removed = vtcode_memory::apply_retention_preserving(
                        workspace.path(),
                        vtcode_memory::RetentionPolicy { max_sessions: count / 2, max_age_days: 30 },
                        Some("retention-0"),
                    )
                    .expect("retention");
                    assert_eq!(removed, count - 1 - count / 2);
                    assert!(vtcode_memory::session_directory(workspace.path(), "retention-0").exists());
                    black_box((workspace, removed))
                },
                BatchSize::PerIteration,
            )
        });
    }
    group.finish();
}
