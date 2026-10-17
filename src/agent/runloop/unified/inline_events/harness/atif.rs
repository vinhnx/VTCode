use anyhow::{Context, Result};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

use tokio::task::spawn_blocking;
use vtcode_core::exec::events::atif::{AtifAgent, AtifTrajectoryBuilder};

/// Minimal envelope for the existing-trajectory probe. Decoding only `steps`
/// as skipped values avoids materializing the full trajectory (`Value` IR)
/// when all we need is emptiness.
#[derive(Deserialize)]
struct TrajectoryStepsProbe {
    #[serde(default)]
    steps: Vec<serde::de::IgnoredAny>,
}

/// Returns true when the existing derived file already holds a non-empty
/// trajectory. Best-effort read-only probe: any I/O or parse failure means
/// "no usable trajectory", so the fresh export proceeds normally.
fn existing_trajectory_has_steps(path: &Path) -> bool {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(_) => return false,
    };
    serde_json::from_slice::<TrajectoryStepsProbe>(&bytes)
        .ok()
        .is_some_and(|probe| !probe.steps.is_empty())
}

/// Optional ATIF trajectory exporter.
pub(crate) struct AtifExporter {
    builder: AtifTrajectoryBuilder,
    output_path: PathBuf,
}

impl AtifExporter {
    pub(crate) fn new(model: &str, output_path: PathBuf) -> Self {
        let agent = AtifAgent::vtcode().with_model(model);
        Self {
            builder: AtifTrajectoryBuilder::new(agent),
            output_path,
        }
    }

    pub(crate) fn process_event(&mut self, event: &vtcode_core::exec::events::ThreadEvent) {
        self.builder.process_event(event);
    }

    /// Finalize, serialize, and write off the async executor.
    ///
    /// Never regress a fuller trajectory with an emptier one: resumed or
    /// short-lived probes share the session derived path but start with a
    /// fresh in-memory builder. Overwriting would destroy the good export
    /// (observed: 1220-event session left with `steps: []`), breaking
    /// eval/replay parity with the canonical `events.jsonl`.
    pub(crate) async fn finish(self) -> Result<(u64, u64, u64)> {
        let Self { builder, output_path } = self;
        let (json, metrics, new_steps) = spawn_blocking(move || {
            let new_steps = builder.step_count() as u64;
            let trajectory = builder.finish(None);
            let metrics = trajectory
                .final_metrics
                .as_ref()
                .map(|final_metrics| {
                    (
                        final_metrics.total_prompt_tokens.unwrap_or(0),
                        final_metrics.total_completion_tokens.unwrap_or(0),
                        final_metrics.total_cached_tokens.unwrap_or(0),
                    )
                })
                .unwrap_or((0, 0, 0));
            let json = serde_json::to_vec_pretty(&trajectory)?;
            Ok::<_, serde_json::Error>((json, metrics, new_steps))
        })
        .await
        .context("ATIF serialization task failed")??;

        if new_steps == 0 && existing_trajectory_has_steps(&output_path) {
            tracing::warn!(
                target: "vtcode.harness",
                phase = "atif_finish",
                path = %output_path.display(),
                "skipping empty ATIF overwrite to preserve existing trajectory"
            );
            return Ok(metrics);
        }

        let output_path_for_write = output_path.clone();
        spawn_blocking(move || {
            if let Some(parent) = output_path_for_write.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(output_path_for_write, json)
        })
        .await
        .context("ATIF write task failed")??;
        Ok(metrics)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failed_turn(message: &str) -> vtcode_core::exec::events::ThreadEvent {
        vtcode_core::exec::events::ThreadEvent::TurnFailed(vtcode_core::exec::events::TurnFailedEvent {
            completed_at: None,
            message: message.to_string(),
            usage: None,
        })
    }

    #[tokio::test]
    async fn empty_export_preserves_existing_trajectory() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let path = dir.path().join("atif-trajectory.json");
        fs::write(
            &path,
            serde_json::json!({
                "schema_version": "ATIF-v1.4",
                "session_id": "sess",
                "agent": {"name": "vtcode", "version": "test"},
                "steps": [{"step_id": 1, "timestamp": "t", "source": "agent", "message": "prior work"}],
                "final_metrics": {"total_prompt_tokens": 10, "total_completion_tokens": 1, "total_steps": 1}
            })
            .to_string(),
        )
        .expect("seed existing trajectory");

        let exporter = AtifExporter::new("test-model", path.clone());
        let _ = exporter.finish().await.expect("finish");

        let preserved: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).expect("read preserved")).expect("parse preserved");
        assert_eq!(preserved["steps"].as_array().map(Vec::len), Some(1));
        assert_eq!(preserved["steps"][0]["message"], serde_json::json!("prior work"));
    }

    #[tokio::test]
    async fn non_empty_export_overwrites() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let path = dir.path().join("atif-trajectory.json");
        fs::write(
            &path,
            serde_json::json!({
                "schema_version": "ATIF-v1.4",
                "session_id": "sess",
                "agent": {"name": "vtcode", "version": "test"},
                "steps": [],
                "final_metrics": {"total_prompt_tokens": 0, "total_completion_tokens": 0, "total_steps": 0}
            })
            .to_string(),
        )
        .expect("seed empty trajectory");

        let mut exporter = AtifExporter::new("test-model", path.clone());
        exporter.process_event(&failed_turn("turn blocked"));
        let _ = exporter.finish().await.expect("finish");

        let written: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).expect("read written")).expect("parse written");
        assert_eq!(written["steps"].as_array().map(Vec::len), Some(1));
    }

    #[test]
    fn existing_trajectory_probe_only_cares_about_non_empty_steps() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let cases = [
            ("corrupt bytes are unusable", "{not json", false),
            ("missing steps are empty", r#"{"schema_version":"ATIF-v1.4"}"#, false),
            ("null steps are empty", r#"{"steps":null}"#, false),
            ("wrong-typed steps are empty", r#"{"steps":"done"}"#, false),
            ("empty steps are empty", r#"{"steps":[]}"#, false),
            ("non-empty steps are kept", r#"{"schema_version":"ATIF-v1.4","steps":[{"step_id":1}]}"#, true),
        ];
        for (name, content, expected) in cases {
            let path = dir.path().join(format!("probe-{expected}-{name}.json"));
            fs::write(&path, content).expect("seed probe file");
            assert_eq!(existing_trajectory_has_steps(&path), expected, "{name}");
        }
    }
}
