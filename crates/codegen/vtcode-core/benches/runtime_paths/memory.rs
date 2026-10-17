use std::hint::black_box;

use criterion::{BatchSize, BenchmarkId, Criterion, Throughput};
use vtcode_exec_events::{
    AgentMessageItem, ItemCompletedEvent, ThreadEvent, ThreadItem, ThreadItemDetails, TurnCompletedEvent,
    TurnStartedEvent, Usage,
};

pub(super) fn fixtures(count: usize, payload_bytes: usize) -> Vec<ThreadEvent> {
    let mut events = vec![ThreadEvent::TurnStarted(TurnStartedEvent::default())];
    for index in 0..count {
        events.push(ThreadEvent::ItemCompleted(ItemCompletedEvent {
            item: ThreadItem {
                id: format!("event-{index}"),
                context: None,
                details: ThreadItemDetails::AgentMessage(AgentMessageItem {
                    text: format!("message-{index}: {}", "x".repeat(payload_bytes)),
                }),
            },
        }));
    }
    events.push(ThreadEvent::TurnCompleted(TurnCompletedEvent {
        completed_at: None,
        usage: Usage::default(),
        in_progress_exec_sessions: Vec::new(),
    }));
    events
}

pub(super) fn benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("session_store");
    for (count, payload_bytes) in [(32, 128), (512, 1_024), (4_096, 8_192)] {
        let events = fixtures(count, payload_bytes);
        let fixture_bytes = events
            .iter()
            .map(|event| serde_json::to_vec(event).expect("fixture event").len())
            .sum::<usize>();
        let _ = group.throughput(Throughput::Bytes(fixture_bytes as u64));
        let _ = group.bench_function(BenchmarkId::new("append_flush", count), |b| {
            b.iter_batched(
                || {
                    let workspace = tempfile::tempdir().expect("memory workspace");
                    let log = vtcode_memory::open(workspace.path(), "fixture", 0).expect("new log");
                    (workspace, log)
                },
                |(workspace, log)| {
                    for event in &events {
                        log.append(event).expect("append");
                    }
                    log.flush().expect("durable flush");
                    assert_eq!(log.event_count(), events.len() as u64);
                    let _ = black_box(log.manifest());
                    (workspace, log)
                },
                BatchSize::PerIteration,
            )
        });

        let workspace = tempfile::tempdir().expect("replay workspace");
        let directory = vtcode_memory::session_directory(workspace.path(), "fixture");
        {
            let log = vtcode_memory::open(workspace.path(), "fixture", 0).expect("new log");
            for event in &events {
                log.append(event).expect("append");
            }
            log.flush().expect("flush");
        }
        let _ = group.bench_function(BenchmarkId::new("reopen", count), |b| {
            b.iter(|| {
                let log = vtcode_memory::open(workspace.path(), "fixture", 0).expect("reopen");
                assert_eq!(log.event_count(), events.len() as u64);
                black_box(log)
            })
        });
        let _ = group.bench_function(BenchmarkId::new("replay", count), |b| {
            b.iter_batched(
                || vtcode_memory::open(workspace.path(), "fixture", 0).expect("reopen"),
                |log| {
                    let mut visited = 0;
                    let _ = log
                        .visit_snapshot(|offset, bytes| {
                            let _ = black_box((offset, bytes));
                            visited += 1;
                        })
                        .expect("replay");
                    assert_eq!(visited, events.len());
                    log
                },
                BatchSize::PerIteration,
            )
        });
        let _ = group.bench_function(BenchmarkId::new("index_rebuild", count), |b| {
            b.iter_batched(
                || std::fs::write(directory.join("index/turns.json"), b"invalid").expect("invalidate derived index"),
                |()| {
                    let log = vtcode_memory::open(workspace.path(), "fixture", 0).expect("rebuild");
                    assert_eq!(log.event_count(), events.len() as u64);
                    black_box(log)
                },
                BatchSize::PerIteration,
            )
        });
    }
    group.finish();
}
