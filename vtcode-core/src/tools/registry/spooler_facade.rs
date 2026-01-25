//! Output spooler accessors for ToolRegistry.

use crate::tools::output_spooler::ToolOutputSpooler;

use super::ToolRegistry;

impl ToolRegistry {
    /// Get the output spooler for external access.
    pub fn output_spooler(&self) -> &ToolOutputSpooler {
        &self.output_spooler
    }

    /// Get the count of currently spooled files (for TUI status).
    pub async fn spooled_files_count(&self) -> usize {
        self.output_spooler.list_spooled_files().await.len()
    }
}
