use std::{hint::black_box, sync::Arc, time::Duration};

use criterion::{BenchmarkId, Criterion};
use serde_json::{Value, json};
use tokio::{runtime::Runtime, sync::oneshot, time::Instant};
use vtcode_core::tools::{cache::FILE_CACHE, file_ops::FileOpsTool, grep_file::GrepSearchManager, traits::Tool};

struct ListingFixture {
    _workspace: tempfile::TempDir,
    tool: FileOpsTool,
    args: Value,
    expected_total: usize,
}

impl ListingFixture {
    fn new(entries: usize, selective: bool) -> Self {
        let workspace = tempfile::tempdir().expect("listing workspace");
        let directory = workspace.path().join("listing");
        std::fs::create_dir(&directory).expect("listing directory");
        for index in 0..entries {
            let hidden = if index % 8 == 0 { "." } else { "" };
            let extension = if index % 4 == 1 { "rs" } else { "txt" };
            let path = directory.join(format!("{hidden}entry_{index:04}.{extension}"));
            if index % 4 == 2 {
                std::fs::create_dir(path).expect("child directory");
            } else {
                std::fs::write(path, "fixture\n").expect("listing file");
            }
        }
        let tool = FileOpsTool::new(
            workspace.path().to_path_buf(),
            Arc::new(GrepSearchManager::new(workspace.path().to_path_buf())),
        );
        Self {
            _workspace: workspace,
            tool,
            args: json!({
                "path": "listing", "mode": "list", "max_items": entries,
                "per_page": 20, "include_hidden": false,
                "pattern": if selective { "*.rs" } else { "*" }
            }),
            expected_total: if selective { entries / 4 } else { entries - entries / 8 },
        }
    }

    async fn list(&self) -> Value {
        let result = self.tool.execute(self.args.clone()).await.expect("public listing entrypoint");
        assert_eq!(result.get("total"), Some(&json!(self.expected_total)));
        assert_eq!(result.get("count"), Some(&json!(self.expected_total.min(20))));
        result
    }
}

struct ListingMeasurement {
    latency: Duration,
    max_wake_delay: Duration,
    timer_ticks: usize,
}

// Arm a separate timer before the listing. Even short cache hits get one tick.
// Subsequent deadlines start at the previous wake, avoiding catch-up bursts.
async fn measure(fixture: Option<&ListingFixture>) -> ListingMeasurement {
    let (armed_tx, armed_rx) = oneshot::channel();
    let (done_tx, mut done_rx) = oneshot::channel();
    let timer = tokio::spawn(async move {
        let mut deadline = Instant::now() + Duration::from_millis(1);
        armed_tx.send(()).expect("arm timer");
        tokio::time::sleep_until(deadline).await;
        let mut max_delay = Instant::now().saturating_duration_since(deadline);
        let mut ticks = 1;
        loop {
            deadline = Instant::now() + Duration::from_millis(1);
            tokio::select! {
                biased;
                _ = tokio::time::sleep_until(deadline) => {
                    max_delay = max_delay.max(Instant::now().saturating_duration_since(deadline));
                    ticks += 1;
                }
                _ = &mut done_rx => break,
            }
        }
        (max_delay, ticks)
    });
    armed_rx.await.expect("timer armed");
    let start = Instant::now();
    if let Some(fixture) = fixture {
        drop(black_box(fixture.list().await));
    } else {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let latency = start.elapsed();
    done_tx.send(()).expect("stop timer");
    let (max_wake_delay, timer_ticks) = timer.await.expect("timer task");
    ListingMeasurement { latency, max_wake_delay, timer_ticks }
}

pub(super) fn benchmarks(c: &mut Criterion) {
    let raw_samples = std::env::var("VTCODE_FILE_LISTING_SAMPLES")
        .ok()
        .map(|value| value.parse::<usize>().expect("positive sample count"));
    assert!(raw_samples != Some(0));
    let mut group = c.benchmark_group("file_listing");
    for current_thread in [true, false] {
        let runtime = if current_thread {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("current-thread runtime")
        } else {
            Runtime::new().expect("normal multithread runtime")
        };
        let runtime_name = if current_thread {
            "current_thread"
        } else {
            "multithread"
        };
        if let Some(samples) = raw_samples {
            for sample in 0..samples {
                let measurement = runtime.block_on(measure(None));
                println!(
                    "{}",
                    json!({"runtime": runtime_name, "case": "idle", "sample": sample,
                    "wake_ns": measurement.max_wake_delay.as_nanos(), "ticks": measurement.timer_ticks})
                );
            }
        }
        for entries in [8, 256, 4_096] {
            for selective in [false, true] {
                let fixture = ListingFixture::new(entries, selective);
                for cache_hit in [false, true] {
                    runtime.block_on(FILE_CACHE.clear());
                    drop(runtime.block_on(fixture.list()));
                    let case = format!(
                        "{runtime_name}/{}{}",
                        if selective { "selective_" } else { "visible_" },
                        if cache_hit { "hit" } else { "miss" }
                    );
                    // Prime threads and metadata before either measurement mode.
                    for _ in 0..5 {
                        if !cache_hit {
                            runtime.block_on(FILE_CACHE.clear());
                        }
                        let _ = runtime.block_on(measure(Some(&fixture)));
                    }
                    if let Some(samples) = raw_samples {
                        for sample in 0..samples {
                            if !cache_hit {
                                runtime.block_on(FILE_CACHE.clear());
                            }
                            let measurement = runtime.block_on(measure(Some(&fixture)));
                            println!(
                                "{}",
                                json!({"runtime": runtime_name, "case": case, "entries": entries,
                                "sample": sample, "latency_ns": measurement.latency.as_nanos(),
                                "wake_ns": measurement.max_wake_delay.as_nanos(), "ticks": measurement.timer_ticks})
                            );
                        }
                    } else {
                        for wake_delay in [false, true] {
                            let metric = if wake_delay { "wake_delay" } else { "latency" };
                            let _ = group.bench_function(BenchmarkId::new(format!("{case}/{metric}"), entries), |b| {
                                b.iter_custom(|iterations| {
                                    let mut total = Duration::ZERO;
                                    for _ in 0..iterations {
                                        // Cache clearing and timer setup are outside the measured listing latency.
                                        if !cache_hit {
                                            runtime.block_on(FILE_CACHE.clear());
                                        }
                                        let measurement = runtime.block_on(measure(Some(&fixture)));
                                        total += if wake_delay {
                                            measurement.max_wake_delay
                                        } else {
                                            measurement.latency
                                        };
                                    }
                                    total
                                });
                            });
                        }
                    }
                }
            }
        }
    }
    group.finish();
}
