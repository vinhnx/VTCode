use criterion::{Criterion, black_box, criterion_group, criterion_main};
use vtcode_core::tools::rate_limiter::{RateLimiter, RateLimiterConfig};

pub fn rate_limiter_benchmark(c: &mut Criterion) {
    let config = RateLimiterConfig {
        per_sec: 100,
        burst: 200,
    };

    c.bench_function("rate_limiter_acquire", |b| {
        let mut limiter = RateLimiter::new_with_config(config);
        b.iter(|| {
            // Benchmark token acquisition
            let _ = black_box(limiter.acquire("tool_name"));
        })
    });
}

fn simulated_tool_outcome_clone(c: &mut Criterion) {
    // Simulate the ToolPipelineOutcome structure
    #[allow(dead_code)]
    struct ExecutionStatus {
        output: Option<String>,
        stdout: Option<String>,
        modified_files: Vec<String>,
    }

    #[allow(dead_code)]
    struct PipelineOutcome {
        status: ExecutionStatus,
        stdout: Option<String>,
        modified_files: Vec<String>,
    }

    let big_string = "x".repeat(10000); // 10KB
    let modified = vec![
        "file1.rs".to_string(),
        "file2.rs".to_string(),
        "file3.rs".to_string(),
    ];

    c.bench_function("outcome_double_clone", |b| {
        b.iter(|| {
            // Old way: double clone
            let output = Some(big_string.clone());
            let stdout = Some(big_string.clone());
            let mod_files = modified.clone();

            let _outcome = PipelineOutcome {
                status: ExecutionStatus {
                    output: output.clone(),
                    stdout: stdout.clone(),
                    modified_files: mod_files.clone(),
                },
                stdout,
                modified_files: mod_files,
            };
        })
    });

    c.bench_function("outcome_single_clone", |b| {
        b.iter(|| {
            // New way: single clone
            let output = Some(big_string.clone());
            let stdout = Some(big_string.clone());
            let mod_files = modified.clone();

            let stdout_copy = stdout.clone();
            let mod_files_copy = mod_files.clone();

            let _outcome = PipelineOutcome {
                status: ExecutionStatus {
                    output,
                    stdout,
                    modified_files: mod_files,
                },
                stdout: stdout_copy,
                modified_files: mod_files_copy,
            };
        })
    });
}

criterion_group!(
    benches,
    rate_limiter_benchmark,
    simulated_tool_outcome_clone
);
criterion_main!(benches);
