//! Exec response metadata: spool references and monitor digests.
//!
//! Extracted from `mod.rs` so response shaping stays under the file-length
//! ceiling; every run/wait/poll/write response funnels through
//! [`ToolRegistry::attach_pipe_output_metadata`].

use super::*;

impl ToolRegistry {
    pub(super) async fn attach_pipe_output_metadata(
        &self,
        response: &mut Value,
        session_id: &str,
        session_exited: bool,
    ) -> Result<bool> {
        let stats = match self.exec_sessions.output_stats(session_id).await {
            Ok(stats) => stats,
            // A session closed while its output was still being captured
            // keeps returning its partial capture; there is no spool left to
            // describe, and pruning stays gated on the observed exit code.
            Err(err) if err.downcast_ref::<crate::tools::exec_session::ExecSessionNotFound>().is_some() => {
                return Ok(true);
            }
            Err(err) => return Err(err),
        };
        self.attach_monitor_digest(response, session_id, true).await?;
        if let Some(stats) = stats {
            let safe_to_prune = !stats.spool_available || (stats.spool_complete && stats.spool_integrity.is_some());
            let spooler_config = self.output_spooler.config();
            attach_spool_metadata(
                response,
                &stats,
                session_exited,
                spooler_config.enabled,
                spooler_config.threshold_bytes,
            );
            return Ok(safe_to_prune);
        }
        Ok(true)
    }

    /// Attach the bounded `monitor_digest` window; draining for output reads,
    /// peeking for inspect. A torn-down session has no window left to report.
    pub(super) async fn attach_monitor_digest(
        &self,
        response: &mut Value,
        session_id: &str,
        drain: bool,
    ) -> Result<()> {
        match self.exec_sessions.read_monitor_digest(session_id, drain).await {
            Ok(Some(digest)) => {
                response["monitor_digest"] = monitor_digest_json(&digest);
                Ok(())
            }
            Ok(None) => Ok(()),
            Err(err) if err.downcast_ref::<crate::tools::exec_session::ExecSessionNotFound>().is_some() => Ok(()),
            Err(err) => Err(err),
        }
    }
}

fn monitor_digest_json(digest: &crate::tools::exec_session::MonitorDigest) -> Value {
    json!({
        "text": digest.render(),
        "dropped_lines": digest.dropped_lines,
        "dropped_bytes": digest.dropped_bytes,
        "total_seen": digest.total_seen,
    })
}

pub(super) fn attach_spool_metadata(
    response: &mut Value,
    stats: &crate::tools::exec_session::PipeOutputStats,
    session_exited: bool,
    spooling_enabled: bool,
    spool_threshold_bytes: usize,
) {
    response["total_output_bytes"] = json!(stats.total_bytes);
    response["output_truncated"] = json!(stats.truncated);

    let output_exceeds_threshold = stats.total_bytes > spool_threshold_bytes as u64;
    let response_capped = ["truncated", "query_truncated"]
        .into_iter()
        .any(|field| response.get(field).and_then(Value::as_bool).unwrap_or(false));
    if !spooling_enabled || !(output_exceeds_threshold || stats.truncated || response_capped) {
        return;
    }

    if stats.spool_available && stats.spool_complete {
        let Some(integrity) = stats.spool_integrity.as_ref() else {
            response["spool_complete"] = json!(false);
            response["spool_pending"] = json!(true);
            response["spool_state"] = json!("pending");
            return;
        };
        response["spool_path"] = json!(stats.spool_path);
        response["output_spooled"] = json!(true);
        response["spool_complete"] = json!(true);
        response["spool_state"] = json!("completed");
        response["spooled_bytes"] = json!(integrity.byte_count);
        response["spool_line_count"] = json!(integrity.line_count);
        response["spool_sha256"] = json!(integrity.sha256);
    } else if stats.spool_available && !session_exited {
        response["spool_path"] = json!(stats.spool_path);
        response["output_spooled"] = json!(true);
        response["spool_complete"] = json!(false);
        response["spool_pending"] = json!(true);
        response["spool_state"] = json!("pending");
    } else if stats.spool_available {
        response["spool_complete"] = json!(false);
        response["spool_pending"] = json!(true);
        response["spool_state"] = json!("pending");
    }
}
