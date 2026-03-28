use std::path::Path;

use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::core::trajectory::{TrajectoryLogger, TrajectoryRetention};

const BYTES_PER_MB: u64 = 1024 * 1024;

pub(crate) fn build_trajectory_logger(
    workspace: &Path,
    vt_cfg: Option<&VTCodeConfig>,
) -> TrajectoryLogger {
    match vt_cfg {
        Some(cfg) if !cfg.telemetry.trajectory_enabled => TrajectoryLogger::disabled(),
        Some(cfg) => {
            let retention = TrajectoryRetention {
                max_files: cfg.telemetry.trajectory_max_files,
                max_age_days: cfg.telemetry.trajectory_max_age_days,
                max_total_size_bytes: cfg
                    .telemetry
                    .trajectory_max_size_mb
                    .saturating_mul(BYTES_PER_MB),
            };
            TrajectoryLogger::with_retention(workspace, retention)
        }
        None => TrajectoryLogger::new(workspace),
    }
}
