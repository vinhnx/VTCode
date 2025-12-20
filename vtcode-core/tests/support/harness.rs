use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use tempfile::TempDir;
use vtcode_core::ctx_err;
use vtcode_core::tools::ToolRegistry;
use vtcode_core::utils::error_messages::{ERR_CREATE_DIR, ERR_WRITE_FILE};

/// Lightweight harness helper for tests needing a workspace and tool registry.
pub struct TestHarness {
    temp_dir: TempDir,
    #[allow(dead_code)]
    session_id: String,
}

impl TestHarness {
    pub fn new() -> Result<Self> {
        let temp_dir = TempDir::new().context(ERR_CREATE_DIR)?;
        Ok(Self {
            temp_dir,
            session_id: next_session_id(),
        })
    }

    pub fn workspace(&self) -> &Path {
        self.temp_dir.path()
    }

    #[allow(dead_code)]
    pub fn workspace_path(&self) -> PathBuf {
        self.temp_dir.path().to_path_buf()
    }

    #[allow(dead_code)]
    pub fn write_file(
        &self,
        relative: impl AsRef<Path>,
        contents: impl AsRef<[u8]>,
    ) -> Result<PathBuf> {
        let path = self.workspace().join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| ctx_err!(ERR_CREATE_DIR, parent.display()))?;
        }

        fs::write(&path, contents).with_context(|| ctx_err!(ERR_WRITE_FILE, path.display()))?;
        Ok(path)
    }

    #[allow(dead_code)]
    pub async fn registry(&self) -> ToolRegistry {
        let mut registry = ToolRegistry::new(self.workspace_path()).await;
        registry.set_harness_session(self.session_id.clone());
        registry
    }

    #[allow(dead_code)]
    pub fn session_id(&self) -> &str {
        &self.session_id
    }
}

fn next_session_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("harness_session_{}", id)
}



