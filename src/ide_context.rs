use std::env;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use std::collections::hash_map::DefaultHasher;

const IDE_CONTEXT_ENV_VAR: &str = "VT_VSCODE_CONTEXT_FILE";

pub struct IdeContextBridge {
    path: PathBuf,
    last_digest: Option<u64>,
}

impl IdeContextBridge {
    pub fn from_env() -> Option<Self> {
        let raw = env::var(IDE_CONTEXT_ENV_VAR).ok()?;
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return None;
        }

        Some(Self {
            path: PathBuf::from(trimmed),
            last_digest: None,
        })
    }

    pub fn snapshot(&mut self) -> Result<Option<String>> {
        let content = match fs::read_to_string(&self.path) {
            Ok(value) => value,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                self.last_digest = None;
                return Ok(None);
            }
            Err(err) => {
                return Err(err).with_context(|| {
                    format!("failed to read IDE context file at {}", self.path.display())
                });
            }
        };

        let normalized = content.replace("\r\n", "\n");
        let trimmed = normalized.trim();
        if trimmed.is_empty() {
            self.last_digest = None;
            return Ok(None);
        }

        let digest = compute_digest(trimmed);
        if self.last_digest == Some(digest) {
            return Ok(None);
        }

        self.last_digest = Some(digest);
        Ok(Some(trimmed.to_string()))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

fn compute_digest(value: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}
