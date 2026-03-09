use anyhow::{Context, Result};
use vtcode_core::utils::error_log_collector::ErrorLogCollectorLayer;
use vtcode_tui::log::{is_tui_log_capture_enabled, make_tui_log_layer};

use super::debug_context::{current_debug_session_id, set_runtime_debug_log_path};
use super::debug_logs::{
    DEFAULT_MAX_DEBUG_LOG_AGE_DAYS, DEFAULT_MAX_DEBUG_LOG_SIZE_MB, prepare_debug_log_file,
};

fn maybe_tui_log_layer() -> Option<vtcode_tui::log::TuiLogLayer> {
    if is_tui_log_capture_enabled() {
        Some(make_tui_log_layer())
    } else {
        None
    }
}

pub(crate) async fn initialize_tracing() -> Result<bool> {
    use tracing_subscriber::{fmt::format::FmtSpan, prelude::*};

    if std::env::var("RUST_LOG").is_ok() {
        let env_filter = tracing_subscriber::EnvFilter::from_default_env();
        let session_id = current_debug_session_id();
        let log_file = prepare_debug_log_file(
            None,
            &session_id,
            DEFAULT_MAX_DEBUG_LOG_SIZE_MB,
            DEFAULT_MAX_DEBUG_LOG_AGE_DAYS,
        )?;
        set_runtime_debug_log_path(log_file.clone());
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
            .with(maybe_tui_log_layer())
            .with(ErrorLogCollectorLayer)
            .try_init();

        if let Err(err) = init_result {
            tracing::warn!(error = %err, "tracing already initialized; skipping env tracing setup");
        }

        return Ok(true);
    }

    Ok(false)
}

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

    let configured_dir = debug_cfg
        .debug_log_dir
        .as_ref()
        .map(|_| debug_cfg.debug_log_path());
    let session_id = current_debug_session_id();
    let log_file = prepare_debug_log_file(
        configured_dir,
        &session_id,
        debug_cfg.max_debug_log_size_mb,
        debug_cfg.max_debug_log_age_days,
    )?;
    set_runtime_debug_log_path(log_file.clone());
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
        .with(maybe_tui_log_layer())
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
