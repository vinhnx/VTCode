//! `vtcode bench-allocator` — measure allocator RSS behavior under a bursty,
//! sparse Tokio workload.
//!
//! Reproduces the pattern from the mimalloc-vs-jemalloc analysis: bursts of many
//! short-lived tasks (each allocating a payload `Bytes` + a `Vec<String>` of
//! tokens) executed across a work-stealing Tokio runtime, with idle gaps
//! between bursts. The diagnostic signature is whether RSS returns toward the
//! baseline after the burst settles and workers go idle (jemalloc) or stays
//! pinned near peak (mimalloc/glibc).

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use vtcode_commons::memory::{sample_peak_rss_mb, sample_rss_mb};

use vtcode_core::cli::args::BenchAllocatorArgs;

/// A single short-lived leaf task: a tiny clone plus an I/O wait. The bulk
/// allocations (payload + token vector) live in the parent event, mirroring the
/// article's workload where each event owns a 4KB payload and up to 1000 tokens.
async fn leaf_task(data: Vec<u8>, token: String) {
    // Simulate outbound I/O latency.
    tokio::time::sleep(Duration::from_millis(1)).await;
    let _ = data.len() + token.len();
}

fn make_payload(n: usize) -> Vec<u8> {
    // Use a large-ish buffer to defeat small-allocation caching and exercise
    // real heap regions.
    std::iter::repeat_n(0x5Au8, n).collect()
}

/// Run one burst: spawn `events` events behind a `Semaphore` cap. Each event
/// allocates a payload + a vector of `tokens_per_task` tokens, then fans out
/// one short-lived leaf task per token across the work-stealing runtime.
async fn run_burst(events: usize, tokens_per_task: usize, payload_bytes: usize) -> Result<()> {
    let semaphore = Arc::new(Semaphore::new(events));
    let mut events_set: JoinSet<()> = JoinSet::new();

    for _ in 0..events {
        let permit = semaphore.clone().acquire_owned().await?;
        events_set.spawn(async move {
            let _permit = permit;
            let payload = make_payload(payload_bytes);
            let tokens: Vec<String> = (0..tokens_per_task)
                .map(|i| format!("token-{i}-{:064}", i))
                .collect();
            let mut tasks: JoinSet<()> = JoinSet::new();
            for token in &tokens {
                let token = token.clone();
                let data = payload.clone();
                tasks.spawn(async move {
                    leaf_task(data, token).await;
                });
            }
            while let Some(res) = tasks.join_next().await {
                let _ = res;
            }
        });
    }

    while let Some(res) = events_set.join_next().await {
        res?;
    }
    Ok(())
}

pub async fn handle_bench_allocator_command(args: BenchAllocatorArgs) -> Result<()> {
    let BenchAllocatorArgs {
        bursts,
        concurrency,
        tokens_per_task,
        idle_seconds,
        payload_bytes,
    } = args;

    eprintln!(
        "bench-allocator: bursts={bursts} concurrency={concurrency} tokens/task={tokens_per_task} \
         payload={payload_bytes}B idle={idle_seconds}s",
    );
    eprintln!("allocator: {}", allocator_name());

    // Warm up so the runtime and any one-time allocations settle.
    tokio::time::sleep(Duration::from_millis(500)).await;
    let baseline = sample_rss_mb();
    eprintln!("baseline RSS: {baseline:.1} MB\n");

    println!("burst | peak_rss_mb | post_burst_mb | post_idle_mb | retained_after_idle_mb");
    println!("------+-------------+---------------+--------------+------------------------");

    for b in 1..=bursts {
        // Sample peak RSS concurrently while the burst runs.
        let poll_handle = tokio::spawn(async move {
            sample_peak_rss_mb(Duration::from_secs(60), Duration::from_millis(25))
        });
        run_burst(concurrency, tokens_per_task, payload_bytes).await?;
        let peak = poll_handle.await?;
        let post_burst = sample_rss_mb();

        // Let Tokio workers go idle between bursts — this is the condition that
        // triggers allocator RSS pinning for mimalloc/glibc.
        tokio::time::sleep(Duration::from_secs(idle_seconds)).await;
        let post_idle = sample_rss_mb();
        let retained = post_idle - baseline;

        println!(
            "{b:>5} | {peak:>11.1} | {post_burst:>13.1} | {post_idle:>12.1} | {retained:>22.1}",
        );
    }

    let final_rss = sample_rss_mb();
    println!("\nfinal RSS: {final_rss:.1} MB (baseline {baseline:.1} MB)");
    let pct = (final_rss / baseline - 1.0) * 100.0;
    if final_rss > baseline * 1.5 {
        if cfg!(target_os = "linux") {
            eprintln!(
                "VERDICT: RSS stayed pinned ~{pct:.0}% above baseline after idle — jemalloc is \
                 active but not reclaiming. Tune MALLOC_CONF (background_thread:true, \
                 dirty_decay_ms:10000, muzzy_decay_ms:0)."
            );
        } else {
            eprintln!(
                "VERDICT: RSS stayed pinned ~{pct:.0}% above baseline after idle — expected on \
                 macOS: jemalloc's background_thread is unsupported here, so switching would not \
                 help. Linux builds use jemalloc by default and reclaim memory."
            );
        }
    } else {
        eprintln!("VERDICT: RSS returned near baseline after idle — allocator reclaims memory.");
    }
    Ok(())
}

fn allocator_name() -> &'static str {
    if cfg!(feature = "allocator-jemalloc") {
        "jemalloc"
    } else {
        "mimalloc"
    }
}
