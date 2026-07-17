use super::FileOpsTool;
use anyhow::Result;
use serde_json::json;
use std::path::Path;
use tracing::info;

impl FileOpsTool {
    /// Log chunking operations for debugging
    pub(super) async fn log_chunking_operation(
        &self,
        file_path: &Path,
        truncated: bool,
        total_lines: Option<usize>,
    ) -> Result<()> {
        if truncated {
            let log_entry = json!({
                "operation": "read_file_chunked",
                "file_path": file_path.to_string_lossy(),
                "truncated": true,
                "total_lines": total_lines,
                "timestamp": chrono::Utc::now().to_rfc3339()
            });

            info!(
                "File chunking operation: {}",
                serde_json::to_string(&log_entry)?
            );
        }
        Ok(())
    }
}
