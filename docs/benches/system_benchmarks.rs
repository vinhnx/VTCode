use assert_fs::TempDir;
use criterion::{BatchSize, Criterion, black_box, criterion_group, criterion_main};
use serde_json::json;
use std::env;
use vtcode::StartupContext;
use vtcode_core::cli::args::Cli;
use vtcode_core::config::constants::tools;
use vtcode_core::config::router::{HeuristicSettings, RouterConfig};
use vtcode_core::core::router::{ModelSelector, TaskClass, TaskClassifier};
use vtcode_core::tools::ToolRegistry;

fn benchmark_startup_context(c: &mut Criterion) {
    let workspace = TempDir::new().expect("temp workspace");
    let home_dir = TempDir::new().expect("temp home");

    let original_home = env::var_os("HOME");
    unsafe {
        // SAFETY: benchmarks run single-threaded; environment modifications are restored before returning.
        env::set_var("HOME", home_dir.path());
    }

    let original_api_key = env::var_os("GEMINI_API_KEY");
    unsafe {
        // SAFETY: benchmarks run single-threaded; environment modifications are restored before returning.
        env::set_var("GEMINI_API_KEY", "benchmark-key");
    }

    let mut cli = Cli::default();
    cli.workspace_path = Some(workspace.path().to_path_buf());
    cli.workspace = Some(workspace.path().to_path_buf());

    let mut group = c.benchmark_group("startup");
    group.bench_function("startup_context_from_cli", |b| {
        b.to_async(tokio::runtime::Runtime::new().unwrap())
            .iter(|| async {
                let ctx = StartupContext::from_cli_args(black_box(&cli))
                    .await
                    .expect("startup context initialization");
                black_box(ctx);
            });
    });
    group.finish();

    match original_home {
        Some(value) => unsafe {
            // SAFETY: see note above on benchmark-scoped environment changes.
            env::set_var("HOME", value);
        },
        None => unsafe {
            // SAFETY: see note above on benchmark-scoped environment changes.
            env::remove_var("HOME");
        },
    }

    match original_api_key {
        Some(value) => unsafe {
            // SAFETY: see note above on benchmark-scoped environment changes.
            env::set_var("GEMINI_API_KEY", value);
        },
        None => unsafe {
            // SAFETY: see note above on benchmark-scoped environment changes.
            env::remove_var("GEMINI_API_KEY");
        },
    }
}

fn benchmark_router_decisions(c: &mut Criterion) {
    let prompts = [
        "quick summary of README",
        "generate a patch fixing panic in src/main.rs",
        "research concurrency primitives across crates.io",
        "refactor module structure to separate startup logic",
        "write integration tests for router heuristics",
    ];

    let classes = [
        TaskClass::Simple,
        TaskClass::Standard,
        TaskClass::Complex,
        TaskClass::CodegenHeavy,
        TaskClass::RetrievalHeavy,
    ];

    let mut group = c.benchmark_group("router");
    group.bench_function("heuristic_classification", |b| {
        b.iter_batched(
            HeuristicSettings::default,
            |heuristics| {
                let classifier = TaskClassifier::new(&heuristics);
                for prompt in &prompts {
                    black_box(classifier.classify(black_box(prompt)));
                }
            },
            BatchSize::SmallInput,
        );
    });

    group.bench_function("model_selection", |b| {
        b.iter_batched(
            RouterConfig::default,
            |router_cfg| {
                let fallback_owned = router_cfg.models.standard.clone();
                let selector = ModelSelector::new(&router_cfg, &fallback_owned);
                for class in &classes {
                    black_box(selector.select(*class));
                }
            },
            BatchSize::SmallInput,
        );
    });
    group.finish();
}

fn benchmark_tool_execution(c: &mut Criterion) {
    let workspace = TempDir::new().expect("temp workspace");
    let file_path = workspace.path().join("example.rs");
    std::fs::write(&file_path, "fn main() {}\n").expect("seed file");
    let file_path = file_path.to_string_lossy().to_string();

    let mut registry =
        futures::executor::block_on(ToolRegistry::new(workspace.path().to_path_buf()));

    let mut group = c.benchmark_group("tool_execution");
    group.bench_function("list_files", |b| {
        b.iter(|| {
            let args = json!({"path": "."});
            let _ = futures::executor::block_on(registry.execute_tool(tools::LIST_FILES, args));
        });
    });

    group.bench_function("read_file", |b| {
        b.iter(|| {
            let args = json!({"path": &file_path});
            let _ = futures::executor::block_on(registry.execute_tool(tools::READ_FILE, args));
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    benchmark_startup_context,
    benchmark_router_decisions,
    benchmark_tool_execution
);
criterion_main!(benches);
