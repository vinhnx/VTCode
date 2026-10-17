//! Session-local source exposure. Internal reads and proposed patches confer no authority.
//!
//! Ranges use zero-based, half-open line indices. Only retained tool-call/result
//! pairs activate read evidence; summaries and replayed sessions cannot create it.
mod exposure;
pub(crate) mod mutations;
mod shell;
pub use shell::TrustedCommandExemption;

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use parking_lot::Mutex;
use serde_json::{Value, json};
use vtcode_commons::utils::calculate_sha256;
use vtcode_config::core::agent::InspectionMode;

const MAX_FILES: usize = 128;
const MAX_CREDIT_LINES: usize = 16_384;
const MAX_SUPPORTS: usize = 128;
type Credit = Arc<BTreeSet<String>>;

#[derive(Default)]
struct FileEvidence {
    revision: String,
    line_count: usize,
    lines: HashMap<usize, Credit>,
    empty: Option<Credit>,
}

struct Support {
    signature: String,
    call_id: Option<String>,
}

#[derive(Default)]
struct Ledger {
    mode: InspectionMode,
    files: HashMap<PathBuf, FileEvidence>,
    order: VecDeque<PathBuf>,
    active: HashSet<String>,
    exemptions: HashSet<String>,
    supports: HashMap<String, Support>,
    previous_calls: HashSet<String>,
}

/// Shared by registry and direct file handlers. Never persisted or restored.
#[derive(Clone, Default)]
pub struct SourceInspection(Arc<Mutex<Ledger>>);

/// Existing tool error transport serializes this object without a new event type.
#[derive(Debug, thiserror::Error)]
#[error("source inspection required: {details}")]
pub(crate) struct InspectionRequired {
    pub(crate) details: Value,
}

fn revision(content: &str) -> String {
    calculate_sha256(content.as_bytes())
}

pub(super) fn call_key(name: &str, args: &Value) -> String {
    use crate::config::constants::tools;
    let mut name = crate::tools::names::canonical_tool_name(name);
    if name == tools::UNIFIED_FILE {
        name = match crate::tools::tool_intent::file_operation_action(args) {
            Some("read") => tools::READ_FILE,
            Some("write") => tools::WRITE_FILE,
            Some("edit") => tools::EDIT_FILE,
            Some("patch") => tools::APPLY_PATCH,
            Some("create") => tools::CREATE_FILE,
            Some("move") => tools::MOVE_FILE,
            Some("copy") => tools::COPY_FILE,
            _ => name,
        };
    }
    let mut args = if name == tools::APPLY_PATCH {
        crate::tools::apply_patch::decode_apply_patch_input(args)
            .ok()
            .flatten()
            .map(|input| json!({"input": input.text}))
            .unwrap_or_else(|| args.clone())
    } else if crate::tools::tool_intent::is_command_run_tool_call(name, args) {
        name = tools::EXEC_COMMAND;
        crate::tools::command_args::normalize_shell_args(args).unwrap_or_else(|_| args.clone())
    } else {
        args.clone()
    };
    if name == tools::CREATE_FILE {
        name = tools::WRITE_FILE;
        if let Some(object) = args.as_object_mut() {
            object.insert("mode".into(), json!("fail_if_exists"));
        }
    }
    if let Some(object) = args.as_object_mut() {
        object.remove("action");
        object.remove("max_output_tokens");
        object.remove(crate::tools::file_ops::PATCH_READ_CACHE_NONCE);
        object.remove(crate::tools::edited_file_monitor::FILE_CONFLICT_OVERRIDE_ARG);
    }
    revision(&format!("{}:{args}", crate::tools::names::canonical_tool_name(name)))
}

impl SourceInspection {
    pub fn set_mode(&self, mode: InspectionMode) {
        self.0.lock().mode = mode;
    }

    pub fn mode(&self) -> InspectionMode {
        self.0.lock().mode
    }

    pub(crate) fn require(&self, path: &Path, content: &str, ranges: &[Range<usize>]) -> Result<()> {
        if self.mode() == InspectionMode::Off {
            return Ok(());
        }
        let digest = revision(content);
        let mut ledger = self.0.lock();
        let stale = ledger.files.get(path).is_some_and(|file| file.revision != digest);
        if stale {
            ledger.files.remove(path);
            ledger.order.retain(|entry| entry != path);
        }
        let file = ledger.files.get(path);
        let valid = |credit: &Credit| credit.iter().all(|support| ledger.active.contains(support));
        let missing = ranges
            .iter()
            .find_map(|range| (range.clone()).find(|line| !file.and_then(|f| f.lines.get(line)).is_some_and(valid)));
        let empty_missing = content.is_empty() && !file.and_then(|f| f.empty.as_ref()).is_some_and(valid);
        if missing.is_none() && !empty_missing {
            return Ok(());
        }
        let start = missing.unwrap_or(0) + 1;
        let reason = if stale {
            "stale_revision"
        } else {
            "source_not_retained_or_not_exposed"
        };
        let details = json!({
            "failure_kind": "source_inspection_required", "path": path,
            "reason": reason, "missing_ranges": [{"start_line": start, "end_line": start}],
            "next_read_args": {"path": path, "offset": start, "limit": 200, "max_tokens": 6000, "condense": false},
            "message": "Read the missing source, then retry a localized patch. Verification does not replace inspection."
        });
        if ledger.mode == InspectionMode::Warn {
            tracing::warn!(%details, "Source inspection warning");
            return Ok(());
        }
        Err(InspectionRequired { details }.into())
    }

    pub(crate) fn require_full(&self, path: &Path, content: &str) -> Result<()> {
        self.require(path, content, std::slice::from_ref(&(0..content.lines().count())))
    }

    /// Inspect existing text for destructive operations. Missing files contain no
    /// source to destroy; advisory inspection cannot impose text-only admission.
    pub(crate) async fn require_existing_file(&self, path: &Path) -> Result<()> {
        if self.mode() == InspectionMode::Off {
            return Ok(());
        }
        match tokio::fs::read_to_string(path).await {
            Ok(content) => self.require_full(path, &content),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) if self.mode() == InspectionMode::Warn => {
                tracing::warn!(path = %path.display(), %error, "Source inspection unavailable");
                Ok(())
            }
            Err(error) => Err(error).with_context(|| format!("Failed to inspect existing source {}", path.display())),
        }
    }

    fn register_support(&self, signature: String) -> String {
        let mut ledger = self.0.lock();
        if ledger.supports.len() >= MAX_FILES * MAX_SUPPORTS {
            ledger.supports.clear();
            ledger.files.clear();
            ledger.order.clear();
            ledger.active.clear();
        }
        let key = uuid::Uuid::new_v4().to_string();
        ledger.supports.insert(key.clone(), Support { signature, call_id: None });
        key
    }

    fn insert_lines(&self, path: PathBuf, content: &str, lines: Vec<usize>, support: String, empty: bool) {
        let mut ledger = self.0.lock();
        if ledger.files.len() >= MAX_FILES
            && !ledger.files.contains_key(&path)
            && let Some(oldest) = ledger.order.pop_front()
        {
            ledger.files.remove(&oldest);
        }
        if !ledger.files.contains_key(&path) {
            ledger.order.retain(|entry| entry != &path);
            ledger.order.push_back(path.clone());
        }
        let digest = revision(content);
        let file = ledger.files.entry(path).or_default();
        if file.revision != digest {
            *file = FileEvidence {
                revision: digest,
                line_count: content.lines().count(),
                ..FileEvidence::default()
            };
        }
        let credit = Arc::new(BTreeSet::from([support]));
        if empty {
            file.empty = Some(Arc::clone(&credit));
        }
        for line in lines {
            if file.lines.len() >= MAX_CREDIT_LINES {
                break;
            }
            file.lines.insert(line, Arc::clone(&credit));
        }
    }
}

#[cfg(test)]
mod tests;
