mod report;
mod run;
mod spec;

use std::path::PathBuf;

pub use run::handle_benchmark_command;

/// Options passed from the CLI layer for running the benchmark command.
#[derive(Debug, Clone)]
pub struct BenchmarkCommandOptions {
    pub task_file: Option<PathBuf>,
    pub inline_task: Option<String>,
    pub output: Option<PathBuf>,
    pub max_tasks: Option<usize>,
}
