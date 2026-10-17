//! One bounded fresh read shared by typed edit/patch mismatches and strict inspection.
use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};

use serde_json::Value;

use super::{ToolExecutionError, ToolRegistry};
use crate::config::constants::tools;

#[derive(Default)]
pub(super) struct PatchRecoveryReads {
    canonical_paths_by_alias: HashMap<PathBuf, PathBuf>,
    consumed_paths: HashSet<PathBuf>,
}

impl ToolRegistry {
    /// Start a user/task turn without changing permissions or loop history.
    pub fn begin_patch_recovery_turn(&self) {
        *self.patch_recovery_reads.lock() = PatchRecoveryReads::default();
    }

    fn patch_recovery_key(&self, path: impl AsRef<Path>) -> Option<PathBuf> {
        let path = self.workspace_root().join(path);
        let mut normalized = PathBuf::new();
        for component in path.components() {
            match component {
                Component::CurDir => {}
                Component::ParentDir => {
                    normalized.pop();
                }
                component => normalized.push(component.as_os_str()),
            }
        }
        normalized.starts_with(self.workspace_root()).then_some(normalized)
    }

    pub(super) async fn grant_patch_recovery_read(&self, error: &ToolExecutionError) {
        let Some(path) = error.patch_context_mismatch_path().or_else(|| {
            error
                .inspection_failure
                .as_ref()
                .and_then(|failure| failure.get("path"))
                .and_then(Value::as_str)
        }) else {
            return;
        };
        // Repeat containment validation before creating an allowance. Invalid
        // paths and symlink escapes never acquire a recovery read.
        let Ok(canonical_path) = self.file_ops_tool().normalize_user_path(path).await else {
            return;
        };
        if let Some(key) = self.patch_recovery_key(path) {
            let mut reads = self.patch_recovery_reads.lock();
            if !reads.consumed_paths.contains(&canonical_path) {
                reads.canonical_paths_by_alias.insert(key, canonical_path.clone());
                reads.canonical_paths_by_alias.insert(canonical_path.clone(), canonical_path);
            }
        }
    }

    fn patch_recovery_read_key(&self, tool_name: &str, args: &Value) -> Option<PathBuf> {
        if crate::tools::tool_intent::is_command_run_tool_call(tool_name, args) {
            let parts = crate::tools::command_args::command_words(args).ok()??;
            let [command, option, script, path] = parts.as_slice() else {
                return None;
            };
            if command != "sed" || option != "-n" {
                return None;
            }
            let range = script.strip_suffix('p')?;
            let (start, end) = range.split_once(',').unwrap_or((range, range));
            let start = start.parse::<u64>().ok()?;
            let end = end.parse::<u64>().ok()?;
            if start == 0 || end < start || end.saturating_sub(start) >= 200 {
                return None;
            }
            let directory = self
                .pty_manager()
                .working_dir_candidate(crate::tools::command_args::working_dir_text(args))
                .ok()?;
            return self.patch_recovery_key(directory.join(path));
        }
        if tool_name != tools::READ_FILE
            && !(tool_name == tools::UNIFIED_FILE
                && crate::tools::tool_intent::file_operation_action(args) == Some("read"))
        {
            return None;
        }
        let path = crate::tools::file_ops::bounded_line_read_path(args, 200)?;
        self.patch_recovery_key(path)
    }

    /// Check admission without consuming the read before other guards run.
    pub fn has_patch_recovery_read(&self, tool_name: &str, args: &Value) -> bool {
        self.pending_patch_recovery_read_path(tool_name, args).is_some()
    }

    /// Canonical identity used to reserve a capped read before batch execution.
    pub fn pending_patch_recovery_read_path(&self, tool_name: &str, args: &Value) -> Option<PathBuf> {
        let key = self.patch_recovery_read_key(tool_name, args)?;
        let reads = self.patch_recovery_reads.lock();
        reads
            .canonical_paths_by_alias
            .get(&key)
            .filter(|path| !reads.consumed_paths.contains(*path))
            .cloned()
    }

    pub(super) fn consume_patch_recovery_read(&self, tool_name: &str, args: &Value) -> bool {
        let Some(key) = self.patch_recovery_read_key(tool_name, args) else {
            return false;
        };
        let mut reads = self.patch_recovery_reads.lock();
        let Some(path) = reads.canonical_paths_by_alias.get(&key).cloned() else {
            return false;
        };
        reads.consumed_paths.insert(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::editing::PatchError;
    use serde_json::json;

    #[tokio::test]
    async fn patch_recovery_requires_typed_context_and_cannot_replenish_within_turn() {
        let temp = tempfile::tempdir().unwrap();
        let registry = ToolRegistry::new(temp.path().to_path_buf()).await;
        tokio::fs::write(temp.path().join("sample.txt"), "content\n").await.unwrap();
        let read = json!({"path":"sample.txt", "limit":10});
        for source in [
            anyhow::anyhow!("permission denied"),
            anyhow::Error::new(PatchError::InvalidFormat("bad".into())),
            anyhow::Error::new(PatchError::InvalidPath {
                operation: "update",
                path: "../outside".into(),
                reason: "outside workspace".into(),
            }),
        ] {
            let error = ToolExecutionError::from_anyhow(tools::APPLY_PATCH, &source, 0, false, false, None);
            registry.grant_patch_recovery_read(&error).await;
            assert!(!registry.has_patch_recovery_read(tools::READ_FILE, &read));
        }
        let source = anyhow::Error::new(PatchError::ContextNotFound {
            path: "sample.txt".into(),
            context: "missing".into(),
        })
        .context("contextual wrapping");
        let error = ToolExecutionError::from_anyhow(tools::APPLY_PATCH, &source, 0, false, false, None);
        registry.grant_patch_recovery_read(&error).await;
        assert!(!registry.has_patch_recovery_read(tools::READ_FILE, &json!({"path":"sample.txt"})));
        assert!(!registry.has_patch_recovery_read(tools::READ_FILE, &json!({"path":"sample.txt", "limit":201})));
        assert!(!registry.has_patch_recovery_read(tools::READ_FILE, &json!({"path":"other.txt", "limit":10})));
        assert!(registry.has_patch_recovery_read(tools::EXEC_COMMAND, &json!({"cmd":"sed -n '2,4p' sample.txt"})));
        for command in [
            "sed -n '0,2p' sample.txt",
            "sed -n '1,201p' sample.txt",
            "sed -n '4,2p' sample.txt",
            "sed -n '1,2p' sample.txt && touch ignored",
        ] {
            assert!(!registry.has_patch_recovery_read(tools::EXEC_COMMAND, &json!({"cmd":command})));
        }
        // The modern handler honors limit and ignores legacy max_lines.
        assert!(
            registry
                .has_patch_recovery_read(tools::READ_FILE, &json!({"path":"sample.txt", "limit":10, "max_lines":1000}))
        );
        assert!(registry.has_patch_recovery_read(tools::READ_FILE, &read));
        assert!(registry.consume_patch_recovery_read(tools::READ_FILE, &read));
        registry.grant_patch_recovery_read(&error).await;
        assert!(!registry.consume_patch_recovery_read(tools::READ_FILE, &read));
        registry.begin_patch_recovery_turn();
        assert!(!registry.has_patch_recovery_read(tools::READ_FILE, &read));
        registry.grant_patch_recovery_read(&error).await;
        assert!(registry.consume_patch_recovery_read(tools::READ_FILE, &read));
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn patch_recovery_deduplicates_symlink_aliases_and_denies_escapes() {
        let temp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        tokio::fs::write(temp.path().join("sample.txt"), "content\n").await.unwrap();
        tokio::fs::write(outside.path().join("outside.txt"), "outside\n").await.unwrap();
        std::os::unix::fs::symlink(temp.path().join("sample.txt"), temp.path().join("alias.txt")).unwrap();
        std::os::unix::fs::symlink(outside.path().join("outside.txt"), temp.path().join("escape.txt")).unwrap();
        let registry = ToolRegistry::new(temp.path().to_path_buf()).await;
        for path in ["sample.txt", "alias.txt", "escape.txt"] {
            let source =
                anyhow::Error::new(PatchError::ContextNotFound { path: path.into(), context: "missing".into() });
            let error = ToolExecutionError::from_anyhow(tools::APPLY_PATCH, &source, 0, false, false, None);
            registry.grant_patch_recovery_read(&error).await;
            let read = json!({"path":path, "limit":10});
            assert_eq!(registry.consume_patch_recovery_read(tools::READ_FILE, &read), path == "sample.txt");
        }
    }
    #[tokio::test]
    async fn patch_recovery_checks_the_effective_reader_range() {
        let temp = tempfile::tempdir().unwrap();
        let registry = ToolRegistry::new(temp.path().to_path_buf()).await;
        let content = (1..=400).map(|line| format!("fixture-{line}\n")).collect::<String>();
        tokio::fs::write(temp.path().join("sample.txt"), content).await.unwrap();
        let source = anyhow::Error::new(PatchError::ContextNotFound {
            path: "sample.txt".into(),
            context: "missing".into(),
        });
        let error = ToolExecutionError::from_anyhow(tools::APPLY_PATCH, &source, 0, false, false, None);
        registry.grant_patch_recovery_read(&error).await;
        for args in [
            json!({"path":"sample.txt", "limit_lines":1, "condense":false}),
            json!({"path":"sample.txt", "max_lines":1}),
            json!({"path":"sample.txt", "chunk_lines":1}),
            json!({"path":"sample.txt", "limit":0}),
            json!({"path":"sample.txt", "limit":201, "l":1}),
            json!({"path":"sample.txt", "limit":1, "offset_bytes":0, "page_size_bytes":8192}),
        ] {
            assert!(!registry.has_patch_recovery_read(tools::READ_FILE, &args), "{args}");
            assert!(!registry.consume_patch_recovery_read(tools::READ_FILE, &args), "{args}");
        }
        let narrow = json!({"path":"sample.txt", "limit":1, "condense":false, "max_tokens":4096});
        assert!(registry.has_patch_recovery_read(tools::READ_FILE, &narrow));
        let output = registry.file_ops_tool().read_file(narrow).await.unwrap();
        let text = output["content"].as_str().unwrap();
        assert!(text.contains("fixture-1"));
        assert!(!text.contains("fixture-2"));
        assert!(
            crate::tools::file_ops::bounded_line_read_path(&json!({"path":"sample.txt", "limit":1}), 100).is_none()
        );
        for args in [
            json!({"path":"sample.txt", "limit":200, "condense":false}),
            json!({"path":"sample.txt", "limit":"200", "condense":false}),
            json!({"path":"sample.txt", "l":200, "condense":false}),
            json!({"path":"sample.txt", "page_size_lines":200, "condense":false}),
            json!({"path":"sample.txt", "start_line":1, "end_line":200, "condense":false}),
        ] {
            assert!(registry.has_patch_recovery_read(tools::READ_FILE, &args), "{args}");
            let output = registry.file_ops_tool().read_file(args.clone()).await.unwrap();
            let text = output["content"].as_str().unwrap();
            assert!(text.contains("fixture-200"), "{args}: {output}");
            assert!(!text.contains("fixture-201"), "{args}: {output}");
            assert!(!text.contains("fixture-400"), "{args}: {output}");
        }
    }

    #[tokio::test]
    async fn patch_recovery_resolves_shell_targets_in_the_execution_directory() {
        let temp = tempfile::tempdir().unwrap();
        tokio::fs::create_dir(temp.path().join("sub")).await.unwrap();
        tokio::fs::write(temp.path().join("sample.txt"), "root-only\n").await.unwrap();
        tokio::fs::write(temp.path().join("sub/sample.txt"), "nested-only\n")
            .await
            .unwrap();
        let registry = ToolRegistry::new(temp.path().to_path_buf()).await;
        registry.allow_all_tools().await.unwrap();
        for path in ["sample.txt", "sub/sample.txt"] {
            registry.begin_patch_recovery_turn();
            let source =
                anyhow::Error::new(PatchError::ContextNotFound { path: path.into(), context: "missing".into() });
            let error = ToolExecutionError::from_anyhow(tools::APPLY_PATCH, &source, 0, false, false, None);
            registry.grant_patch_recovery_read(&error).await;
            for field in ["workdir", "cwd", "working_dir"] {
                let args = json!({"cmd":"sed -n '1,2p' sample.txt", field:"sub"});
                assert_eq!(
                    registry.has_patch_recovery_read(tools::EXEC_COMMAND, &args),
                    path == "sub/sample.txt",
                    "{path}: {args}"
                );
            }
            let args = json!({"cmd":"sed -n '1,2p' sample.txt", "workdir":"sub"});
            if path == "sample.txt" {
                assert!(!registry.consume_patch_recovery_read(tools::EXEC_COMMAND, &args));
                assert!(
                    registry.has_patch_recovery_read(tools::EXEC_COMMAND, &json!({"cmd":"sed -n '1,2p' sample.txt"}))
                );
            } else {
                let output = registry.execute_tool(tools::EXEC_COMMAND, args.clone()).await.unwrap();
                let text = output.to_string();
                assert!(text.contains("nested-only"), "{output}");
                assert!(!text.contains("root-only"), "{output}");
                assert!(!registry.has_patch_recovery_read(tools::EXEC_COMMAND, &args));
            }
        }
    }
}
