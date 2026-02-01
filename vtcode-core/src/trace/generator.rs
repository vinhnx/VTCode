//! Trace generation from file changes and session data.

use crate::tools::handlers::turn_diff_tracker::{FileChange, FileChangeKind, TurnDiffTracker};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use vtcode_exec_events::trace::{
    Contributor, RelatedResource, TraceConversation, TraceFile, TraceMetadata, TraceRange,
    TraceRecord, TraceRecordBuilder, VtCodeMetadata, compute_content_hash, normalize_model_id,
};

// Re-export TraceContext for convenience
pub use vtcode_exec_events::trace::TraceContext;

/// Generate Agent Trace records from tracked file changes.
pub struct TraceGenerator;

impl TraceGenerator {
    /// Generate a trace record from a TurnDiffTracker.
    pub fn from_diff_tracker(tracker: &TurnDiffTracker, ctx: &TraceContext) -> Option<TraceRecord> {
        if !tracker.has_changes() {
            return None;
        }

        let mut builder = TraceRecordBuilder::new();

        // Set VCS info
        if let Some(ref revision) = ctx.revision {
            builder = builder.git_revision(revision);
        }

        // Get workspace path or use current directory
        let workspace_path = ctx
            .workspace_path
            .clone()
            .unwrap_or_else(|| PathBuf::from("."));

        // Generate files with attributed ranges
        for (path, change) in tracker.changes() {
            if let Some(trace_file) =
                Self::file_change_to_trace_file(path, change, ctx, &workspace_path)
            {
                builder = builder.file(trace_file);
            }
        }

        // Add VT Code metadata
        let metadata = TraceMetadata {
            confidence: Some(1.0), // Direct attribution is high confidence
            vtcode: Some(VtCodeMetadata {
                session_id: ctx.session_id.clone(),
                turn_number: ctx.turn_number,
                workspace_path: ctx.workspace_path.as_ref().map(|p| p.display().to_string()),
                provider: Some(ctx.provider.clone()),
            }),
            ..Default::default()
        };
        builder = builder.metadata(metadata);

        let trace = builder.build();

        // Only return if there are actual attributions
        if trace.has_attributions() {
            Some(trace)
        } else {
            None
        }
    }

    /// Generate a trace record from raw file changes.
    pub fn from_changes(
        changes: &HashMap<PathBuf, FileChange>,
        ctx: &TraceContext,
    ) -> Option<TraceRecord> {
        if changes.is_empty() {
            return None;
        }

        let mut builder = TraceRecordBuilder::new();

        if let Some(ref revision) = ctx.revision {
            builder = builder.git_revision(revision);
        }

        let workspace_path = ctx
            .workspace_path
            .clone()
            .unwrap_or_else(|| PathBuf::from("."));

        for (path, change) in changes {
            if let Some(trace_file) =
                Self::file_change_to_trace_file(path, change, ctx, &workspace_path)
            {
                builder = builder.file(trace_file);
            }
        }

        let metadata = TraceMetadata {
            confidence: Some(1.0),
            vtcode: Some(VtCodeMetadata {
                session_id: ctx.session_id.clone(),
                turn_number: ctx.turn_number,
                workspace_path: ctx.workspace_path.as_ref().map(|p| p.display().to_string()),
                provider: Some(ctx.provider.clone()),
            }),
            ..Default::default()
        };
        builder = builder.metadata(metadata);

        let trace = builder.build();
        if trace.has_attributions() {
            Some(trace)
        } else {
            None
        }
    }

    /// Convert a FileChange to a TraceFile.
    fn file_change_to_trace_file(
        path: &Path,
        change: &FileChange,
        ctx: &TraceContext,
        workspace_path: &Path,
    ) -> Option<TraceFile> {
        // Get relative path from workspace
        let relative_path = path
            .strip_prefix(workspace_path)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();

        // Determine line range and content for hash
        let (line_range, content_for_hash) = match &change.kind {
            FileChangeKind::Add { content } => {
                let line_count = content.lines().count() as u32;
                (Some((1, line_count.max(1))), Some(content.as_str()))
            }
            FileChangeKind::Update { new_content, .. } => {
                // For updates, use the change's line_range if available
                if let Some((start, end)) = change.line_range {
                    (Some((start, end)), Some(new_content.as_str()))
                } else {
                    let line_count = new_content.lines().count() as u32;
                    (Some((1, line_count.max(1))), Some(new_content.as_str()))
                }
            }
            FileChangeKind::Delete { .. } => {
                // Deletions don't have attributed ranges in the new content
                return None;
            }
            FileChangeKind::Rename { new_content, .. } => {
                if let Some(content) = new_content {
                    let line_count = content.lines().count() as u32;
                    (Some((1, line_count.max(1))), Some(content.as_str()))
                } else {
                    return None;
                }
            }
        };

        let (start_line, end_line) = line_range?;

        // Build the range
        let mut range = TraceRange::new(start_line, end_line);
        if let Some(content) = content_for_hash {
            range = range.with_hash(compute_content_hash(content));
        }

        // Determine contributor
        let contributor = if let Some(ref attr) = change.attribution {
            if let Some(model_id) = attr.normalized_model_id() {
                Contributor::ai(model_id)
            } else {
                match attr.contributor_type.as_str() {
                    "human" => Contributor::human(),
                    "mixed" => Contributor::mixed(),
                    _ => Contributor::ai(normalize_model_id(&ctx.model_id, &ctx.provider)),
                }
            }
        } else {
            // Default to AI with context model
            Contributor::ai(normalize_model_id(&ctx.model_id, &ctx.provider))
        };

        // Build conversation
        let mut conversation = TraceConversation {
            url: None,
            contributor: Some(contributor),
            ranges: vec![range],
            related: None,
        };

        // Add session URL if available
        if let Some(ref session_id) = ctx.session_id {
            // Use file:// URL for local session files
            let session_url = format!(
                "file://{}/sessions/{}.json",
                workspace_path.join(".vtcode").display(),
                session_id
            );
            conversation.url = Some(session_url.clone());
            conversation.related = Some(vec![RelatedResource::session(session_url)]);
        }

        let mut trace_file = TraceFile::new(relative_path);
        trace_file.add_conversation(conversation);

        Some(trace_file)
    }
}

/// Get the current git HEAD revision.
pub fn get_git_head_revision(workspace_path: &Path) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(workspace_path)
        .output()
        .ok()?;

    if output.status.success() {
        let revision = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if revision.len() >= 40 {
            Some(revision)
        } else {
            None
        }
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::handlers::turn_diff_tracker::ChangeAttribution;

    #[test]
    fn test_trace_from_diff_tracker() {
        let mut tracker = TurnDiffTracker::new();
        tracker.set_attribution(ChangeAttribution::ai("claude-opus-4", "anthropic"));

        let mut changes = HashMap::new();
        changes.insert(
            PathBuf::from("/workspace/src/main.rs"),
            FileChange::add("fn main() {\n    println!(\"Hello\");\n}"),
        );
        tracker.on_patch_begin(changes);
        tracker.on_patch_end(true);

        let ctx = TraceContext::new("claude-opus-4", "anthropic")
            .with_workspace_path("/workspace")
            .with_session_id("session-123")
            .with_turn_number(1);

        let trace = TraceGenerator::from_diff_tracker(&tracker, &ctx);
        assert!(trace.is_some());

        let trace = trace.unwrap();
        assert_eq!(trace.files.len(), 1);
        assert_eq!(trace.files[0].path, "src/main.rs");
        assert_eq!(trace.files[0].conversations.len(), 1);
        assert_eq!(trace.files[0].conversations[0].ranges.len(), 1);
    }

    #[test]
    fn test_trace_with_git_revision() {
        let ctx = TraceContext::new("gpt-4", "openai")
            .with_workspace_path("/workspace")
            .with_revision("abc123def456789012345678901234567890abcd");

        let mut changes = HashMap::new();
        changes.insert(PathBuf::from("/workspace/test.rs"), FileChange::add("test"));

        let trace = TraceGenerator::from_changes(&changes, &ctx);
        assert!(trace.is_some());

        let trace = trace.unwrap();
        assert!(trace.vcs.is_some());
        assert_eq!(
            trace.vcs.unwrap().revision,
            "abc123def456789012345678901234567890abcd"
        );
    }

    #[test]
    fn test_trace_empty_changes() {
        let ctx = TraceContext::new("model", "provider").with_workspace_path("/workspace");
        let changes = HashMap::new();

        let trace = TraceGenerator::from_changes(&changes, &ctx);
        assert!(trace.is_none());
    }

    #[test]
    fn test_trace_delete_not_included() {
        let ctx = TraceContext::new("model", "provider").with_workspace_path("/workspace");

        let mut changes = HashMap::new();
        changes.insert(
            PathBuf::from("/workspace/deleted.rs"),
            FileChange::delete("old content"),
        );

        let trace = TraceGenerator::from_changes(&changes, &ctx);
        // Deletions don't create attributions
        assert!(trace.is_none());
    }
}
