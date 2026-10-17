mod file_listing;
mod harness;
mod indexing;
mod memory;
mod protocols;
mod retention;
mod runner;
mod streaming;

pub(super) fn benchmarks(c: &mut criterion::Criterion) {
    if let Some(scope) = std::env::var_os("VTCODE_BENCH_SCOPE") {
        match scope.to_str().expect("VTCODE_BENCH_SCOPE must be UTF-8") {
            "harness" => harness::benchmarks(c),
            "indexing" => indexing::benchmarks(c),
            "file_listing" => file_listing::benchmarks(c),
            "streaming" => streaming::benchmarks(c),
            "memory" => memory_benchmarks(c),
            "runner" => runner::benchmarks(c),
            "protocols" => protocols::benchmarks(c),
            _ => {
                eprintln!("Unknown VTCODE_BENCH_SCOPE: {scope:?}");
                std::process::exit(2);
            }
        }
        return;
    }
    harness::benchmarks(c);
    streaming::benchmarks(c);
    indexing::benchmarks(c);
    file_listing::benchmarks(c);
    memory_benchmarks(c);
    runner::benchmarks(c);
    protocols::benchmarks(c);
}

fn memory_benchmarks(c: &mut criterion::Criterion) {
    memory::benchmarks(c);
    retention::benchmarks(c);
}

use tokio::{net::TcpListener, runtime::Runtime, task::JoinHandle};

/// Own the loopback server so failed fixtures cannot leave a peer running.
struct LoopbackPeer {
    url: String,
    task: JoinHandle<()>,
}

impl LoopbackPeer {
    fn start(runtime: &Runtime, router: axum::Router) -> Self {
        runtime.block_on(async {
            let listener = TcpListener::bind("127.0.0.1:0").await.expect("loopback listener");
            let address = listener.local_addr().expect("loopback address");
            let task = tokio::spawn(async move {
                axum::serve(listener, router).await.expect("loopback server");
            });
            Self { url: format!("http://{address}"), task }
        })
    }
}

impl Drop for LoopbackPeer {
    fn drop(&mut self) {
        self.task.abort();
    }
}
