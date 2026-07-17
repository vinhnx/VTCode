use super::FileOpsTool;
use crate::config::constants::tools;
use anyhow::Result;
use serde_json::json;
use std::path::Path;
use tracing::info;

impl FileOpsTool {
    /// Log write operations for debugging
    pub(super) async fn log_write_operation(
        &self,
        file_path: &Path,
        bytes_written: usize,
        chunked: bool,
    ) -> Result<()> {
        let log_entry = json!({
            "operation": if chunked { "write_file_chunked" } else { tools::WRITE_FILE },
            "file_path": file_path.to_string_lossy(),
            "bytes_written": bytes_written,
            "chunked": chunked,
            "chunk_size": chunked.then_some(crate::config::constants::chunking::WRITE_CHUNK_SIZE),
            "timestamp": chrono::Utc::now().to_rfc3339()
        });

        info!("File write operation: {}", serde_json::to_string(&log_entry)?);
        Ok(())
    }
}
