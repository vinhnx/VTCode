use std::hint::black_box;
use std::sync::Arc;

use criterion::Criterion;
use criterion::criterion_group;
use criterion::criterion_main;
use tokio::runtime::Runtime;
use tokio::sync::RwLock;
use vtcode_core::core::agent::harness_kernel::{
    HarnessRequestPlanInput, PreparedToolBatch, PreparedToolCall, build_harness_request_plan,
};
use vtcode_core::llm::provider::{Message, ToolChoice, ToolDefinition};
use vtcode_core::tools::registry::SessionToolCatalogState;

fn sample_tool(name: &str) -> ToolDefinition {
    ToolDefinition::function(
        name.to_string(),
        format!("Tool {name}"),
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" }
            }
        }),
    )
}

fn sample_tools(count: usize) -> Arc<Vec<ToolDefinition>> {
    Arc::new(
        (0..count)
            .map(|index| sample_tool(&format!("tool_{index}")))
            .collect(),
    )
}

fn sample_messages(count: usize) -> Vec<Message> {
    (0..count)
        .map(|index| Message::user(format!("message {index}")))
        .collect()
}

fn request_plan_benchmark(c: &mut Criterion) {
    let tools = sample_tools(24);
    let messages = sample_messages(32);

    c.bench_function("agent_harness_request_plan_with_tools", |b| {
        b.iter(|| {
            black_box(build_harness_request_plan(HarnessRequestPlanInput {
                messages: messages.clone(),
                system_prompt: "System prompt\n[Runtime Context]\nturn=12".to_string(),
                tools: Some(Arc::clone(&tools)),
                model: "gpt-5".to_string(),
                max_tokens: Some(2000),
                temperature: Some(0.7),
                stream: true,
                tool_choice: Some(ToolChoice::auto()),
                parallel_tool_config: None,
                reasoning_effort: None,
                verbosity: None,
                metadata: None,
                context_management: None,
                previous_response_id: Some("resp_123".to_string()),
                prompt_cache_key: Some("session:test".to_string()),
                prompt_cache_profile: None,
                tool_catalog_hash: None,
            }))
        })
    });
}

fn prepared_batch_planning_benchmark(c: &mut Criterion) {
    let calls: Vec<PreparedToolCall> = (0..48)
        .map(|index| {
            let readonly = index % 5 != 0;
            PreparedToolCall::new(
                format!("tool_{index}"),
                readonly,
                readonly,
                serde_json::json!({ "path": format!("src/file_{index}.rs") }),
            )
        })
        .collect();

    c.bench_function("agent_harness_prepared_batch_plan", |b| {
        b.iter(|| black_box(PreparedToolBatch::plan(calls.clone(), true)))
    });
}

fn tool_catalog_projection_benchmark(c: &mut Criterion) {
    let runtime = Runtime::new().expect("criterion tokio runtime");
    let state = Arc::new(SessionToolCatalogState::new());
    let tools = Arc::new(RwLock::new((*sample_tools(32)).clone()));

    runtime.block_on(async {
        let _ = state
            .filtered_snapshot_with_stats(&tools, true, false)
            .await;
    });

    c.bench_function("agent_harness_tool_catalog_cache_hit", |b| {
        b.iter(|| {
            let state = Arc::clone(&state);
            let tools = Arc::clone(&tools);
            runtime.block_on(async move {
                black_box(
                    state
                        .filtered_snapshot_with_stats(&tools, true, false)
                        .await,
                )
            })
        })
    });

    c.bench_function("agent_harness_tool_catalog_cache_miss", |b| {
        b.iter(|| {
            let state = Arc::clone(&state);
            let tools = Arc::clone(&tools);
            runtime.block_on(async move {
                state.note_explicit_refresh("benchmark");
                black_box(
                    state
                        .filtered_snapshot_with_stats(&tools, true, false)
                        .await,
                )
            })
        })
    });
}

criterion_group!(
    benches,
    request_plan_benchmark,
    prepared_batch_planning_benchmark,
    tool_catalog_projection_benchmark
);
criterion_main!(benches);
