use std::{hint::black_box, sync::Arc};

use criterion::{BenchmarkId, Criterion};
use futures::future::join_all;
use tokio::runtime::Runtime;
use vtcode_core::{
    config::constants::tools,
    core::agent::state::normalize_history_for_request_shared,
    llm::provider::{Message, ToolCall},
    tool_policy::ToolPolicy,
    tools::registry::ToolRegistry,
};

pub(super) fn benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("request_history");
    for turns in [8, 128, 2_048] {
        let messages: Arc<Vec<Message>> = Arc::new(
            (0..turns)
                .flat_map(|index| {
                    let id = format!("call-{index}");
                    [
                        Message::user(format!("question-{index}")),
                        Message::assistant_with_tools(
                            String::new(),
                            vec![ToolCall::function(
                                id.clone(),
                                "code_search".to_string(),
                                "{\"query\":\"widget\"}".to_string(),
                            )],
                        ),
                        Message::tool_response(id, format!("asymmetric-result-{index}")),
                    ]
                })
                .collect(),
        );
        let _ = group.bench_function(BenchmarkId::new("clean", turns), |b| {
            b.iter(|| {
                let normalized = normalize_history_for_request_shared(Arc::clone(&messages));
                assert!(Arc::ptr_eq(&normalized, &messages));
                black_box(normalized)
            })
        });
        let mut incomplete = messages.as_ref().clone();
        drop(incomplete.pop());
        let incomplete = Arc::new(incomplete);
        let _ = group.bench_function(BenchmarkId::new("missing_result", turns), |b| {
            b.iter(|| {
                let normalized = normalize_history_for_request_shared(Arc::clone(&incomplete));
                assert_eq!(normalized.len(), messages.len());
                assert_eq!(
                    normalized.last().expect("cancellation result").tool_call_id.as_deref(),
                    Some(format!("call-{}", turns - 1).as_str())
                );
                black_box(normalized)
            })
        });
    }
    group.finish();

    let runtime = Runtime::new().expect("tool runtime");
    let workspace = tempfile::tempdir().expect("tool workspace");
    let registry = Arc::new(runtime.block_on(ToolRegistry::new(workspace.path().to_path_buf())));
    runtime.block_on(async {
        registry
            .set_tool_policy(tools::READ_FILE, ToolPolicy::Allow)
            .await
            .expect("admit fixture tool");
    });
    let args = (0..8)
        .map(|ordinal| {
            let name = format!("file-{ordinal}.txt");
            std::fs::write(workspace.path().join(&name), format!("fixture-value-{ordinal}\n")).expect("fixture file");
            serde_json::json!({"path": name})
        })
        .collect::<Vec<_>>();
    let mut group = c.benchmark_group("admitted_tool_dispatch");
    for parallel in [false, true] {
        let _ = group.bench_function(if parallel { "parallel" } else { "sequential" }, |b| {
            b.iter(|| {
                runtime.block_on(async {
                    let results = if parallel {
                        join_all(args.iter().map(|args| registry.execute_tool_ref(tools::READ_FILE, args))).await
                    } else {
                        let mut results = Vec::new();
                        for args in &args {
                            results.push(registry.execute_tool_ref(tools::READ_FILE, args).await);
                        }
                        results
                    };
                    for (ordinal, result) in results.iter().enumerate() {
                        assert!(
                            result
                                .as_ref()
                                .expect("admitted fixture call")
                                .to_string()
                                .contains(&format!("fixture-value-{ordinal}"))
                        );
                    }
                    black_box(results)
                })
            })
        });
    }
    let _ = group.bench_function("spawned_parallel", |b| {
        b.iter(|| {
            runtime.block_on(async {
                let task_handles = args.iter().map(|args| {
                    let registry = Arc::clone(&registry);
                    let args = args.clone();
                    tokio::spawn(async move { registry.execute_tool_ref(tools::READ_FILE, &args).await })
                });
                let results = join_all(task_handles).await;
                for (ordinal, result) in results.iter().enumerate() {
                    assert!(
                        result
                            .as_ref()
                            .expect("fixture task join")
                            .as_ref()
                            .expect("spawned admitted call")
                            .to_string()
                            .contains(&format!("fixture-value-{ordinal}"))
                    );
                }
                black_box(results)
            })
        })
    });
    group.finish();
}
