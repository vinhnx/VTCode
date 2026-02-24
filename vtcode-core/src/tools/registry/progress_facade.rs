//! Progress callback accessors for ToolRegistry.

use super::{ToolProgressCallback, ToolRegistry};

impl ToolRegistry {
    /// Replace the callback for streaming tool output and progress, returning the previous callback.
    pub fn replace_progress_callback(
        &self,
        callback: Option<ToolProgressCallback>,
    ) -> Option<ToolProgressCallback> {
        let Ok(mut slot) = self.progress_callback.write() else {
            return None;
        };
        std::mem::replace(&mut *slot, callback)
    }

    /// Set the callback for streaming tool output and progress
    pub fn set_progress_callback(&self, callback: ToolProgressCallback) {
        let _ = self.replace_progress_callback(Some(callback));
    }

    /// Clear the progress callback
    pub fn clear_progress_callback(&self) {
        let _ = self.replace_progress_callback(None);
    }

    /// Get the current progress callback if set
    pub fn progress_callback(&self) -> Option<ToolProgressCallback> {
        self.progress_callback.read().ok().and_then(|g| g.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tempfile::TempDir;

    #[tokio::test]
    async fn replace_progress_callback_restores_previous() {
        let temp_dir = TempDir::new().expect("create temp dir");
        let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

        let first_hits = Arc::new(AtomicUsize::new(0));
        let first_hits_clone = Arc::clone(&first_hits);
        registry.set_progress_callback(Arc::new(move |_, _| {
            let _ = first_hits_clone.fetch_add(1, Ordering::SeqCst);
        }));

        let second_hits = Arc::new(AtomicUsize::new(0));
        let second_hits_clone = Arc::clone(&second_hits);
        let previous = registry.replace_progress_callback(Some(Arc::new(move |_, _| {
            let _ = second_hits_clone.fetch_add(1, Ordering::SeqCst);
        })));

        if let Some(current) = registry.progress_callback() {
            current("run_pty_cmd", "chunk");
        }
        assert_eq!(second_hits.load(Ordering::SeqCst), 1);

        let _ = registry.replace_progress_callback(previous);
        if let Some(current) = registry.progress_callback() {
            current("run_pty_cmd", "chunk");
        }
        assert_eq!(first_hits.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn clear_progress_callback_removes_registered_callback() {
        let temp_dir = TempDir::new().expect("create temp dir");
        let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

        registry.set_progress_callback(Arc::new(|_, _| {}));
        assert!(registry.progress_callback().is_some());

        registry.clear_progress_callback();
        assert!(registry.progress_callback().is_none());
    }
}
