use anyhow::{Context, Result, bail};
use console::style;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use tokio::fs::{self, OpenOptions};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tokio::sync::Mutex;
use vtcode_core::config::TBenchConfig;
use vtcode_core::config::constants::benchmarks::env;
use vtcode_core::config::loader::VTCodeConfig;

const TBENCH_GUIDE_PATH: &str = "docs/guides/tbench-terminal-benchmark.md";

pub async fn handle_benchmark_command(config: &VTCodeConfig, workspace: &Path) -> Result<()> {
    println!(
        "{}",
        style("Terminal Benchmark (TBench) integration")
            .blue()
            .bold()
    );

    let tbench_cfg = &config.benchmark.tbench;
    if !tbench_cfg.enabled {
        println!(
            "{}",
            style("TBench integration is disabled in vtcode.toml ([benchmark.tbench]).").yellow()
        );
        println!("{} {}", style("See guide:").cyan(), TBENCH_GUIDE_PATH);
        return Ok(());
    }

    let command = tbench_cfg.resolved_command().with_context(|| {
        format!(
            "Unable to determine benchmark CLI command. Set `command` or define the `{}` \
             environment variable in [benchmark.tbench].",
            tbench_cfg.command_env
        )
    })?;

    let working_dir = resolve_path(workspace, tbench_cfg.working_directory.as_ref());
    if fs::metadata(&working_dir).await.is_err() {
        bail!(
            "Benchmark working directory '{}' does not exist",
            working_dir.display()
        );
    }

    let (resolved_config, resolved_results, log_writer, log_path) =
        prepare_runtime_paths(workspace, tbench_cfg).await?;

    let mut command_builder = Command::new(&command);
    command_builder
        .args(&tbench_cfg.args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .current_dir(&working_dir);

    if tbench_cfg.attach_workspace_env {
        command_builder.env(env::WORKSPACE_DIR, workspace);
    }

    if let Some(config_path) = &resolved_config {
        command_builder.env(env::TBENCH_CONFIG, config_path);
    }

    if let Some(results_path) = &resolved_results {
        command_builder.env(env::TBENCH_OUTPUT_DIR, results_path);
    }

    if let Some(log_path) = &log_path {
        command_builder.env(env::TBENCH_RUN_LOG, log_path);
    }

    for (key, value) in &tbench_cfg.env {
        command_builder.env(key, value);
    }

    for key in tbench_cfg.resolved_passthrough_env() {
        if let Ok(value) = std::env::var(&key) {
            command_builder.env(&key, value);
        }
    }

    println!(
        "{} {}",
        style("Command:").cyan(),
        format_command(&command, &tbench_cfg.args)
    );
    if let Some(config_path) = &resolved_config {
        println!("{} {}", style("Scenario:").cyan(), config_path.display());
    }
    if let Some(results_path) = &resolved_results {
        println!(
            "{} {}",
            style("Results dir:").cyan(),
            results_path.display()
        );
    }
    if let Some(log_path) = &log_path {
        println!("{} {}", style("Log file:").cyan(), log_path.display());
    }

    let mut child = command_builder
        .spawn()
        .with_context(|| format!("Failed to launch benchmark command '{}'.", command))?;

    let stdout_handle = if let Some(stdout) = child.stdout.take() {
        let log = log_writer.clone();
        Some(tokio::spawn(async move {
            forward_stream(stdout, StreamKind::Stdout, log).await
        }))
    } else {
        None
    };

    let stderr_handle = if let Some(stderr) = child.stderr.take() {
        let log = log_writer.clone();
        Some(tokio::spawn(async move {
            forward_stream(stderr, StreamKind::Stderr, log).await
        }))
    } else {
        None
    };

    if let Some(handle) = stdout_handle {
        handle
            .await
            .context("Failed to process benchmark stdout stream")??;
    }

    if let Some(handle) = stderr_handle {
        handle
            .await
            .context("Failed to process benchmark stderr stream")??;
    }

    let status = child
        .wait()
        .await
        .with_context(|| format!("Failed while awaiting benchmark command '{}'.", command))?;

    if status.success() {
        println!("{}", style("Benchmark run completed successfully.").green());
        Ok(())
    } else if let Some(code) = status.code() {
        bail!("Benchmark process exited with status code {}.", code);
    } else {
        bail!("Benchmark process terminated by signal.");
    }
}

fn resolve_path(base: &Path, candidate: Option<&PathBuf>) -> PathBuf {
    match candidate {
        Some(path) if path.is_absolute() => path.clone(),
        Some(path) => base.join(path),
        None => base.to_path_buf(),
    }
}

fn resolve_optional_file(base: &Path, candidate: &Path) -> PathBuf {
    if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        base.join(candidate)
    }
}

async fn prepare_runtime_paths(
    workspace: &Path,
    cfg: &TBenchConfig,
) -> Result<(
    Option<PathBuf>,
    Option<PathBuf>,
    Option<Arc<Mutex<tokio::fs::File>>>,
    Option<PathBuf>,
)> {
    let resolved_config = if let Some(config_path) = &cfg.config_path {
        let resolved = resolve_optional_file(workspace, config_path);
        fs::metadata(&resolved)
            .await
            .with_context(|| format!("TBench scenario file '{}' not found", resolved.display()))?;
        Some(resolved)
    } else {
        None
    };

    let resolved_results = if let Some(results_dir) = &cfg.results_dir {
        let resolved = resolve_optional_file(workspace, results_dir);
        fs::create_dir_all(&resolved).await.with_context(|| {
            format!(
                "Failed to create results directory '{}'",
                resolved.display()
            )
        })?;
        Some(resolved)
    } else {
        None
    };

    let (log_writer, log_path) = if let Some(run_log) = &cfg.run_log {
        let resolved = resolve_optional_file(workspace, run_log);
        if let Some(parent) = resolved.parent() {
            fs::create_dir_all(parent).await.with_context(|| {
                format!("Failed to create log directory '{}'", parent.display())
            })?;
        }
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&resolved)
            .await
            .with_context(|| format!("Failed to create log file '{}'", resolved.display()))?;
        (Some(Arc::new(Mutex::new(file))), Some(resolved))
    } else {
        (None, None)
    };

    Ok((resolved_config, resolved_results, log_writer, log_path))
}

async fn forward_stream<R>(
    mut reader: R,
    kind: StreamKind,
    log: Option<Arc<Mutex<tokio::fs::File>>>,
) -> Result<()>
where
    R: AsyncRead + Unpin,
{
    let mut buffer = [0_u8; 4096];

    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            break;
        }

        match kind {
            StreamKind::Stdout => {
                let mut writer = tokio::io::stdout();
                writer.write_all(&buffer[..read]).await?;
                writer.flush().await?;
            }
            StreamKind::Stderr => {
                let mut writer = tokio::io::stderr();
                writer.write_all(&buffer[..read]).await?;
                writer.flush().await?;
            }
        }

        if let Some(file) = &log {
            let mut guard = file.lock().await;
            guard.write_all(kind.log_prefix()).await?;
            guard.write_all(&buffer[..read]).await?;
            guard.flush().await?;
        }
    }

    Ok(())
}

enum StreamKind {
    Stdout,
    Stderr,
}

impl StreamKind {
    fn log_prefix(&self) -> &'static [u8] {
        match self {
            Self::Stdout => b"[stdout] ",
            Self::Stderr => b"[stderr] ",
        }
    }
}

fn format_command(command: &str, args: &[String]) -> String {
    if args.is_empty() {
        command.to_string()
    } else {
        format!("{} {}", command, args.join(" "))
    }
}
