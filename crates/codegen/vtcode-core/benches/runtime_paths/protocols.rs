use std::{hint::black_box, sync::Arc, time::Duration};

use criterion::{BenchmarkId, Criterion};
use tokio::runtime::Runtime;
use vtcode_a2a::{
    agent_card::AgentCard,
    client::A2aClient,
    rpc::MessageSendParams,
    server::{A2aServerState, create_router},
    task_manager::TaskManager,
    types::Message,
};
use vtcode_config::mcp::McpClientConfig;
use vtcode_mcp::{DetailLevel, McpClient, McpToolExecutor, ToolDiscovery};

use super::LoopbackPeer;

pub(super) fn benchmarks(c: &mut Criterion) {
    let runtime = Runtime::new().expect("protocol runtime");
    let mut group = c.benchmark_group("a2a_loopback");
    for history_length in [8, 64, 512] {
        let state = A2aServerState::new_with_auth_token(
            TaskManager::new(),
            AgentCard::vtcode_default("http://127.0.0.1"),
            "benchmark-token",
        )
        .expect("authenticated server");
        let peer = LoopbackPeer::start(&runtime, create_router(state));
        let client = A2aClient::new(&peer.url)
            .expect("A2A client")
            .with_bearer_token("benchmark-token");
        let task_id = runtime.block_on(async {
            let first = client
                .send_message(MessageSendParams::new(Message::user_text("first")))
                .await
                .expect("new task");
            for index in 1..history_length {
                let _ = client
                    .send_message(
                        MessageSendParams::new(Message::user_text(format!("message-{index}: {}", "x".repeat(8_192))))
                            .with_task_id(&first.id),
                    )
                    .await
                    .expect("history message");
            }
            first.id
        });
        let params: vtcode_a2a::rpc::ListTasksParams =
            serde_json::from_value(serde_json::json!({"historyLength": 1})).expect("listing params");
        let _ = group.bench_function(BenchmarkId::new("list_one_history", history_length), |b| {
            b.iter(|| {
                runtime.block_on(async {
                    let result = client.list_tasks(Some(params.clone())).await.expect("list tasks");
                    let history = result
                        .get("tasks")
                        .and_then(|tasks| tasks.get(0))
                        .and_then(|task| task.get("history"))
                        .and_then(serde_json::Value::as_array)
                        .expect("listed history");
                    assert_eq!(history.len(), 1);
                    black_box(result)
                })
            })
        });
        let _ = group.bench_function(BenchmarkId::new("get_default_history", history_length), |b| {
            b.iter(|| {
                runtime.block_on(async {
                    let task = client.get_task(task_id.clone()).await.expect("get task");
                    // The public get endpoint defaults to an empty history.
                    assert!(task.history.is_empty());
                    black_box(task)
                })
            })
        });
        let _ = group.bench_function(BenchmarkId::new("discovery", history_length), |b| {
            b.iter(|| black_box(runtime.block_on(client.agent_card()).expect("agent card")))
        });
    }
    group.finish();

    let state = A2aServerState::new_with_auth_token(
        TaskManager::new(),
        AgentCard::vtcode_default("http://127.0.0.1"),
        "benchmark-token",
    )
    .expect("slow authenticated server");
    let router = create_router(state).layer(axum::middleware::from_fn(
        |request: axum::extract::Request, next: axum::middleware::Next| async move {
            if request.method() == axum::http::Method::POST {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            next.run(request).await
        },
    ));
    let peer = LoopbackPeer::start(&runtime, router);
    let client = A2aClient::new(&peer.url)
        .expect("slow client")
        .with_bearer_token("benchmark-token");
    let task = runtime
        .block_on(client.send_message(MessageSendParams::new(Message::user_text("slow fixture"))))
        .expect("slow fixture task");
    let _ = c.bench_function("a2a_boundaries/slow_request", |b| {
        b.iter(|| black_box(runtime.block_on(client.get_task(task.id.clone())).expect("slow task")))
    });
    let _ = c.bench_function("a2a_boundaries/cancel_and_recover", |b| {
        b.iter(|| {
            runtime.block_on(async {
                assert!(
                    tokio::time::timeout(Duration::from_millis(1), client.get_task(task.id.clone()))
                        .await
                        .is_err()
                );
                black_box(client.agent_card().await.expect("healthy request after cancellation"))
            })
        })
    });

    let peer_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("benches/runtime_paths/mcp_peer.py");
    let python = which::which("python3").expect("python3 for local MCP peer");
    let mut group = c.benchmark_group("mcp_stdio");
    for tool_count in [8, 128, 1_024] {
        let config: McpClientConfig = serde_json::from_value(serde_json::json!({
            "enabled": true,
            "providers": [{"name": "fixture", "command": python, "args": [peer_path, tool_count.to_string()], "transport": "stdio", "enabled": true}],
        })).expect("MCP fixture config");
        let mut client = McpClient::new(config);
        runtime.block_on(client.initialize()).expect("MCP handshake");
        let client = Arc::new(client);
        let tools = runtime.block_on(client.list_mcp_tools()).expect("initial discovery");
        assert_eq!(tools.len(), tool_count);
        let discovery = ToolDiscovery::new(client.clone());
        let _ = group.bench_function(BenchmarkId::new("cached_list", tool_count), |b| {
            b.iter(|| black_box(runtime.block_on(client.list_mcp_tools()).expect("cached discovery")))
        });
        let _ = group.bench_function(BenchmarkId::new("search", tool_count), |b| {
            b.iter(|| {
                let results = runtime
                    .block_on(discovery.search_tools("widget", DetailLevel::NameOnly))
                    .expect("search");
                assert_eq!(results.len(), 5);
                black_box(results)
            })
        });
        let args = serde_json::json!({"text": "asymmetric fixture"});
        let _ = group.bench_function(BenchmarkId::new("request", tool_count), |b| {
            b.iter(|| {
                black_box(
                    runtime
                        .block_on(client.execute_mcp_tool("widget_0000", &args))
                        .expect("MCP request"),
                )
            })
        });
        runtime.block_on(client.shutdown()).expect("stop MCP fixture peer");
    }
    group.finish();

    let config: McpClientConfig = serde_json::from_value(serde_json::json!({
        "enabled": true,
        "providers": [{"name": "fixture", "command": python, "args": [peer_path, "8", "5"], "transport": "stdio", "enabled": true}],
    })).expect("slow MCP fixture config");
    let mut client = McpClient::new(config.clone());
    runtime.block_on(client.initialize()).expect("slow MCP handshake");
    let args = serde_json::json!({"text": "slow asymmetric fixture"});
    let _ = c.bench_function("mcp_boundaries/slow_request", |b| {
        b.iter(|| {
            black_box(
                runtime
                    .block_on(client.execute_mcp_tool("widget_0000", &args))
                    .expect("slow echo"),
            )
        })
    });
    let _ = c.bench_function("mcp_boundaries/32_in_flight", |b| {
        b.iter(|| {
            runtime.block_on(async {
                let results = futures::future::join_all((0..32).map(|index| {
                    let client = &client;
                    async move {
                        let text = format!("request-{index}");
                        let result = client
                            .execute_mcp_tool("widget_0000", &serde_json::json!({"text": text}))
                            .await
                            .expect("concurrent echo");
                        assert_eq!(
                            result
                                .get("content")
                                .and_then(|content| content.get(0))
                                .and_then(|part| part.get("text"))
                                .and_then(serde_json::Value::as_str),
                            Some(text.as_str())
                        );
                        result
                    }
                }))
                .await;
                assert_eq!(results.len(), 32);
                black_box(results)
            })
        })
    });
    let _ = c.bench_function("mcp_boundaries/cancel_and_recover", |b| {
        b.iter(|| {
            runtime.block_on(async {
                assert!(
                    tokio::time::timeout(Duration::from_millis(1), client.execute_mcp_tool("widget_0000", &args))
                        .await
                        .is_err()
                );
                black_box(
                    client
                        .execute_mcp_tool("widget_0000", &args)
                        .await
                        .expect("healthy MCP request after cancellation"),
                )
            })
        })
    });
    runtime.block_on(client.shutdown()).expect("stop slow peer");
    // A disconnected peer must return an error rather than leave the caller waiting.
    let _ = c.bench_function("mcp_boundaries/disconnect", |b| {
        b.iter_batched(
            || {
                let mut client = McpClient::new(config.clone());
                runtime.block_on(client.initialize()).expect("disconnect MCP handshake");
                client
            },
            |client| {
                runtime.block_on(async {
                    let result = tokio::time::timeout(
                        Duration::from_secs(3),
                        client.execute_mcp_tool("widget_0000", &serde_json::json!({"text": "disconnect"})),
                    )
                    .await
                    .expect("disconnect notification deadline");
                    assert!(result.is_err());
                    client.shutdown().await.expect("stop disconnected peer");
                })
            },
            criterion::BatchSize::PerIteration,
        )
    });
}
