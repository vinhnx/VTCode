use clap::Parser;
use std::num::NonZero;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use tokio::signal;

#[derive(Parser)]
#[command(name = "vtcode-file-search")]
#[command(about = "Fast fuzzy file search for VT Code")]
#[command(version)]
struct Cli {
    /// Search pattern (fuzzy match)
    pattern: Option<String>,

    /// Search directory
    #[arg(short, long, default_value = ".")]
    cwd: PathBuf,

    /// Maximum number of results
    #[arg(short, long, default_value = "100")]
    limit: NonZero<usize>,

    /// Exclude patterns (glob-style, can be repeated)
    #[arg(short, long)]
    exclude: Vec<String>,

    /// Number of worker threads
    #[arg(short, long)]
    threads: Option<NonZero<usize>>,

    /// Output results as JSON
    #[arg(long)]
    json: bool,

    /// Compute character indices for highlighting
    #[arg(long)]
    compute_indices: bool,

    /// Respect .gitignore files
    #[arg(long, default_value = "true")]
    respect_gitignore: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // If no pattern provided, show available options
    let pattern = match cli.pattern {
        Some(pattern) => pattern,
        None => {
            eprintln!("Missing required search pattern");
            std::process::exit(1);
        }
    };
    let threads = match cli.threads {
        Some(threads) => threads,
        None => NonZero::new(num_cpus::get()).ok_or_else(|| {
            anyhow::anyhow!("num_cpus::get() returned 0 while resolving worker thread count")
        })?,
    };

    // Set up cancellation flag
    let cancel_flag = Arc::new(AtomicBool::new(false));
    let cancel_flag_clone = cancel_flag.clone();

    // Handle Ctrl+C gracefully
    tokio::spawn(async move {
        let _ = signal::ctrl_c().await;
        cancel_flag_clone.store(true, std::sync::atomic::Ordering::Relaxed);
    });

    let results = vtcode_file_search::run(vtcode_file_search::FileSearchConfig {
        pattern_text: pattern,
        limit: cli.limit,
        search_directory: cli.cwd,
        exclude: cli.exclude,
        threads,
        cancel_flag,
        compute_indices: cli.compute_indices,
        respect_gitignore: cli.respect_gitignore,
    })?;

    if cli.json {
        let json = serde_json::to_string_pretty(&results.matches)?;
        println!("{}", json);
    } else {
        for m in &results.matches {
            println!("{} (score: {})", m.path, m.score);
        }

        if results.total_match_count > cli.limit.get() {}
    }

    Ok(())
}
