//! Progress callback accessors for ToolRegistry.

use super::{ToolProgressCallback, ToolRegistry};

impl ToolRegistry {
    /// Set the callback for streaming tool output and progress
    pub fn set_progress_callback(&self, callback: ToolProgressCallback) {
        *self.progress_callback.write().unwrap() = Some(callback);
    }

    /// Clear the progress callback
    pub fn clear_progress_callback(&self) {
        *self.progress_callback.write().unwrap() = None;
    }

    /// Get the current progress callback if set
    pub fn progress_callback(&self) -> Option<ToolProgressCallback> {
        self.progress_callback.read().unwrap().clone()
    }
}
