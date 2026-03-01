use anyhow::{Context, Result};
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use vtcode_core::cli::args::AgentClientProtocolTarget;
use vtcode_core::cli::args::Cli;
use vtcode_core::utils::dot_config::DotManager;
use vtcode_core::utils::error_log_collector::ErrorLogCollectorLayer;
use vtcode_core::utils::session_archive::SESSION_DIR_ENV;
use vtcode_core::utils::tty::TtyExt;
use vtcode_tui::log::make_tui_log_layer;

const DEBUG_LOG_FILE_NAME: &str = "vtcode-debug.log";
const DEBUG_LOG_ROTATED_PREFIX: &str = "vtcode-debug-";
const DEFAULT_MAX_DEBUG_LOG_SIZE_MB: u64 = 50;
const DEFAULT_MAX_DEBUG_LOG_AGE_DAYS: u32 = 7;
const DEBUG_BYTES_PER_MB: u64 = 1024 * 1024;
const SECONDS_PER_DAY: u64 = 24 * 60 * 60;

fn default_debug_log_dir() -> PathBuf {
    if let Some(custom) = std::env::var_os(SESSION_DIR_ENV) {
        return PathBuf::from(custom);
    }
    if let Ok(manager) = DotManager::new() {
        return manager.sessions_dir();
    }
    PathBuf::from(".vtcode/sessions")
}

fn is_debug_log_file(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
        return false;
    };
    name == DEBUG_LOG_FILE_NAME
        || (name.starts_with(DEBUG_LOG_ROTATED_PREFIX) && name.ends_with(".log"))
}

fn prune_expired_debug_logs(log_dir: &Path, max_age_days: u32) -> Result<()> {
    let cutoff = if max_age_days == 0 {
        SystemTime::now()
    } else {
        SystemTime::now()
            .checked_sub(Duration::from_secs(
                u64::from(max_age_days).saturating_mul(SECONDS_PER_DAY),
            ))
            .unwrap_or(UNIX_EPOCH)
    };

    for entry in fs::read_dir(log_dir)
        .with_context(|| format!("Failed to read debug log directory {}", log_dir.display()))?
    {
        let entry = match entry {
            Ok(value) => value,
            Err(err) => {
                eprintln!(
                    "warning: failed to read a debug log entry in {}: {}",
                    log_dir.display(),
                    err
                );
                continue;
            }
        };
        let path = entry.path();
        if !is_debug_log_file(&path) {
            continue;
        }
        let metadata = match entry.metadata() {
            Ok(value) => value,
            Err(err) => {
                eprintln!(
                    "warning: failed to read debug log metadata {}: {}",
                    path.display(),
                    err
                );
                continue;
            }
        };
        if !metadata.is_file() {
            continue;
        }
        if metadata.modified().unwrap_or(UNIX_EPOCH) <= cutoff
            && let Err(err) = fs::remove_file(&path)
        {
            eprintln!(
                "warning: failed to remove expired debug log {}: {}",
                path.display(),
                err
            );
        }
    }

    Ok(())
}

fn rotate_debug_log_if_needed(log_file: &Path, max_size_mb: u64) -> Result<()> {
    if max_size_mb == 0 {
        return Ok(());
    }

    let max_bytes = max_size_mb.saturating_mul(DEBUG_BYTES_PER_MB);
    let metadata = match fs::metadata(log_file) {
        Ok(value) => value,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => {
            return Err(err)
                .with_context(|| format!("Failed to inspect debug log {}", log_file.display()));
        }
    };

    if metadata.len() < max_bytes {
        return Ok(());
    }

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let rotated_name = format!(
        "{}{}-{}.log",
        DEBUG_LOG_ROTATED_PREFIX,
        timestamp,
        std::process::id()
    );
    let rotated_path = log_file
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(rotated_name);

    fs::rename(log_file, &rotated_path).with_context(|| {
        format!(
            "Failed to rotate debug log {} -> {}",
            log_file.display(),
            rotated_path.display()
        )
    })?;
    Ok(())
}

fn prepare_debug_log_file(
    configured_dir: Option<PathBuf>,
    max_size_mb: u64,
    max_age_days: u32,
) -> Result<PathBuf> {
    let log_dir = configured_dir.unwrap_or_else(default_debug_log_dir);
    fs::create_dir_all(&log_dir)
        .with_context(|| format!("Failed to create debug log directory {}", log_dir.display()))?;
    prune_expired_debug_logs(&log_dir, max_age_days)?;
    let log_file = log_dir.join(DEBUG_LOG_FILE_NAME);
    rotate_debug_log_if_needed(&log_file, max_size_mb)?;
    Ok(log_file)
}

/// Detect available IDE for automatic connection when --ide flag is used.
pub(crate) fn detect_available_ide() -> Result<Option<AgentClientProtocolTarget>> {
    use std::env;

    let mut available_ides = Vec::new();

    // Check for Zed (currently the only supported IDE)
    // Zed sets VIMRUNTIME or ZED_CLI when running with ACP
    if env::var("ZED_CLI").is_ok() || env::var("VIMRUNTIME").is_ok() {
        available_ides.push(AgentClientProtocolTarget::Zed);
    }

    // In the future, we could check for other IDEs here:
    // - VS Code: Check for VSCODE_IPC_HOOK_CLI
    // - Others: Add detection logic as needed

    match available_ides.len() {
        0 => Ok(None),
        1 => Ok(Some(available_ides[0])),
        _ => {
            // Multiple IDEs detected, be explicit and don't auto-connect
            Ok(None)
        }
    }
}

pub(crate) fn build_print_prompt(print_value: String) -> Result<String> {
    let piped_input = collect_piped_stdin()?;
    let inline_prompt = if print_value.trim().is_empty() {
        None
    } else {
        Some(print_value)
    };

    match (piped_input, inline_prompt) {
        (Some(piped), Some(prompt)) => {
            let mut combined = piped;
            if !combined.ends_with("\n\n") {
                if combined.ends_with('\n') {
                    combined.push('\n');
                } else {
                    combined.push_str("\n\n");
                }
            }
            combined.push_str(&prompt);
            Ok(combined)
        }
        (Some(piped), None) => Ok(piped),
        (None, Some(prompt)) => Ok(prompt),
        (None, None) => Err(anyhow::anyhow!(
            "No prompt provided. Pass text to -p/--print or pipe input via stdin."
        )),
    }
}

fn collect_piped_stdin() -> Result<Option<String>> {
    let mut stdin = io::stdin();
    // Use crossterm's IsTty trait for consistent TTY detection
    if stdin.is_tty_ext() {
        return Ok(None);
    }

    let mut buffer = String::new();
    stdin
        .read_to_string(&mut buffer)
        .context("Failed to read prompt from stdin")?;

    if buffer.trim().is_empty() {
        Ok(None)
    } else {
        Ok(Some(buffer))
    }
}

pub(crate) async fn initialize_tracing(args: &Cli) -> Result<bool> {
    use tracing_subscriber::{fmt::format::FmtSpan, prelude::*};

    // Check if RUST_LOG env var is set (takes precedence)
    if std::env::var("RUST_LOG").is_ok() {
        let env_filter = tracing_subscriber::EnvFilter::from_default_env();

        // When running in interactive TUI mode, redirect logs to a file to avoid corrupting the display
        // Use crossterm's IsTty trait for consistent TTY detection
        let is_interactive_tui = args.command.is_none() && io::stdin().is_tty_ext();

        if is_interactive_tui {
            // Redirect logs to a file instead of stderr to avoid TUI corruption
            let log_file = prepare_debug_log_file(
                None,
                DEFAULT_MAX_DEBUG_LOG_SIZE_MB,
                DEFAULT_MAX_DEBUG_LOG_AGE_DAYS,
            )?;
            let file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&log_file)
                .context("Failed to open debug log file")?;

            let fmt_layer = tracing_subscriber::fmt::layer()
                .with_writer(std::sync::Arc::new(file))
                .with_span_events(FmtSpan::FULL)
                .with_ansi(false); // No ANSI codes in file

            let init_result = tracing_subscriber::registry()
                .with(env_filter)
                .with(fmt_layer)
                .with(make_tui_log_layer())
                .with(ErrorLogCollectorLayer)
                .try_init();

            if let Err(err) = init_result {
                tracing::warn!(error = %err, "tracing already initialized; skipping env tracing setup");
            }
        } else {
            // Non-interactive mode: write to stderr as normal
            let fmt_layer = tracing_subscriber::fmt::layer().with_span_events(FmtSpan::FULL);
            let init_result = tracing_subscriber::registry()
                .with(env_filter)
                .with(fmt_layer)
                .with(make_tui_log_layer())
                .with(ErrorLogCollectorLayer)
                .try_init();

            if let Err(err) = init_result {
                tracing::warn!(error = %err, "tracing already initialized; skipping env tracing setup");
            }
        }

        return Ok(true);
    }
    // Note: Config-based tracing initialization is handled in initialize_tracing_from_config()
    // when DebugConfig is loaded. This function just ensures RUST_LOG is respected.

    Ok(false)
}

/// Initialize a minimal tracing subscriber that only collects ERROR-level logs
/// into the session archive. Used when neither `RUST_LOG` nor config-based
/// tracing is enabled.
pub(crate) fn initialize_default_error_tracing() -> Result<()> {
    use tracing_subscriber::prelude::*;

    let env_filter = tracing_subscriber::EnvFilter::new("error");

    let init_result = tracing_subscriber::registry()
        .with(env_filter)
        .with(ErrorLogCollectorLayer)
        .try_init();

    if let Err(err) = init_result {
        tracing::warn!(error = %err, "tracing already initialized; skipping default error tracing setup");
    }

    Ok(())
}

pub(crate) fn initialize_tracing_from_config(
    config: &vtcode_core::config::loader::VTCodeConfig,
) -> Result<()> {
    use tracing_subscriber::{fmt::format::FmtSpan, prelude::*};

    let debug_cfg = &config.debug;
    let targets = if debug_cfg.trace_targets.is_empty() {
        "vtcode_core,vtcode".to_string()
    } else {
        debug_cfg.trace_targets.join(",")
    };

    let filter_str = format!("{}={}", targets, debug_cfg.trace_level.as_str());

    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(&filter_str));

    // Always redirect config-based tracing to a file to avoid TUI corruption
    let configured_dir = debug_cfg
        .debug_log_dir
        .as_ref()
        .map(|_| debug_cfg.debug_log_path());
    let log_file = prepare_debug_log_file(
        configured_dir,
        debug_cfg.max_debug_log_size_mb,
        debug_cfg.max_debug_log_age_days,
    )?;
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_file)
        .context("Failed to open debug log file")?;

    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::sync::Arc::new(file))
        .with_span_events(FmtSpan::FULL)
        .with_ansi(false);

    let init_result = tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt_layer)
        .with(make_tui_log_layer())
        .with(ErrorLogCollectorLayer)
        .try_init();

    match init_result {
        Ok(()) => {
            tracing::info!(
                "Debug tracing enabled: targets={}, level={}, log_file={}",
                targets,
                debug_cfg.trace_level,
                log_file.display()
            );
        }
        Err(err) => {
            tracing::warn!(
                error = %err,
                "tracing already initialized; skipping config tracing setup"
            );
        }
    }

    Ok(())
}
