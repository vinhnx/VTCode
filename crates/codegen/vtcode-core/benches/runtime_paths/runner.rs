use std::{hint::black_box, time::Duration};

use criterion::{BenchmarkId, Criterion, Throughput};
use tokio::runtime::Runtime;
use vtcode_bash_runner::pipe::{PipeSpawnOptions, spawn_process_with_options};
use vtcode_core::{
    config::PtyConfig,
    tools::pty::{PtyCommandRequest, PtyManager, PtySize},
};

pub(super) fn benchmarks(c: &mut Criterion) {
    let runtime = Runtime::new().expect("runner runtime");
    let workspace = tempfile::tempdir().expect("runner workspace");
    let workspace_root = vtcode_commons::paths::canonicalize(workspace.path()).expect("canonical runner workspace");
    let mut group = c.benchmark_group("runner_output");
    for bytes in [1_024, 65_536, 1_048_576] {
        let fixture = workspace_root.join(format!("output-{bytes}.txt"));
        let payload = "line with output\n".repeat(bytes / 17);
        std::fs::write(&fixture, &payload).expect("runner fixture");
        let _ = group.throughput(Throughput::Bytes(payload.len() as u64));
        for slow in [false, true] {
            let _ =
                group.bench_function(BenchmarkId::new(if slow { "pipe_slow_consumer" } else { "pipe" }, bytes), |b| {
                    b.iter(|| {
                        runtime.block_on(async {
                            let mut process = spawn_process_with_options(
                                PipeSpawnOptions::new("/bin/cat", &workspace_root)
                                    .args([fixture.to_string_lossy().into_owned()])
                                    .lossless_output(true),
                            )
                            .await
                            .expect("spawn pipe");
                            let mut output = Vec::new();
                            while let Some(chunk) = process.reliable_output_rx.recv().await {
                                output.extend_from_slice(&chunk);
                                if slow {
                                    tokio::time::sleep(Duration::from_millis(1)).await;
                                }
                            }
                            assert_eq!(process.exit_rx.await.expect("exit"), 0);
                            assert_eq!(output, payload.as_bytes());
                            black_box(output)
                        })
                    })
                });
        }
        let manager = PtyManager::new(workspace_root.clone(), PtyConfig::default());
        let _ = group.bench_function(BenchmarkId::new("pty_bounded_preview", bytes), |b| {
            b.iter(|| {
                runtime.block_on(async {
                    let result = manager
                        .run_command(PtyCommandRequest {
                            command: vec![
                                "/bin/sh".to_string(),
                                "-c".to_string(),
                                "cat \"$1\"; printf 'END-MARKER\\n'".to_string(),
                                "fixture".to_string(),
                                fixture.to_string_lossy().into_owned(),
                            ],
                            working_dir: workspace_root.clone(),
                            timeout: Duration::from_secs(10),
                            size: PtySize::default(),
                            max_tokens: Some(1_024),
                            output_callback: None,
                        })
                        .await
                        .expect("PTY burst");
                    assert_eq!(result.exit_code, 0);
                    assert!(!result.output.is_empty());
                    assert!(result.output.len() <= 4_096 + "\n[... truncated by max_tokens ...]".len());
                    black_box(result)
                })
            })
        });
    }
    group.finish();

    let _ = c.bench_function("runner_cancellation/spawn_terminate_reap", |b| {
        b.iter(|| {
            runtime.block_on(async {
                let mut process = spawn_process_with_options(
                    PipeSpawnOptions::new("/bin/sleep", &workspace_root)
                        .args(["30"])
                        .lossless_output(true),
                )
                .await
                .expect("spawn cancellation fixture");
                // Keep the wait task alive so termination includes the actual
                // exit notification; terminate() also aborts that task.
                process.session.terminate_process();
                let code = tokio::time::timeout(Duration::from_secs(3), async {
                    while let Some(chunk) = process.reliable_output_rx.recv().await {
                        assert!(chunk.is_empty());
                    }
                    process.exit_rx.await.expect("cancelled child exit notification")
                })
                .await
                .expect("reap cancelled child");
                assert_ne!(code, 0);
                black_box(code)
            })
        })
    });
}
