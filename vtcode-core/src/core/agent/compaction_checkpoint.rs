//! Compaction checkpoint: writes compaction summaries to persistent artifacts.
//!
//! Following the context engineering principle: "the summary should be written
//! into a persistent artifact, such as progress.md, so that later sessions can
//! read it." This module ensures that when compaction happens, the summary is
//! written to `memories/progress.md` and `memories/compaction_summary.md` so
//! that later sessions can orient from these artifacts.
//!
//! This bridges the gap between compaction (which compresses conversation history)
//! and the orient phase (which reads external artifacts at session start).

use std::path::Path;

use tracing::warn;

use crate::compaction::memory_envelope::SessionMemoryEnvelope;

/// Write compaction summary to persistent artifacts for later sessions.
///
/// This is called after compaction completes to ensure the summary survives
/// across sessions. It writes to:
/// - `memories/progress.md` - human-readable progress summary
/// - `memories/compaction_summary.md` - detailed compaction summary
pub fn write_compaction_checkpoint(workspace_root: &Path, envelope: &SessionMemoryEnvelope) {
    let memories_dir = workspace_root.join("memories");
    if let Err(e) = std::fs::create_dir_all(&memories_dir) {
        warn!(path = %memories_dir.display(), error = %e, "failed to create memories dir");
        return;
    }

    // Write progress.md with the compaction summary
    let progress_path = memories_dir.join("progress.md");
    let progress_content = format!(
        "# Session Progress\n\n\
         **Session:** {}\n\
         **Generated:** {}\n\n\
         ## Summary\n{}\n\n\
         ## Objective\n{}\n\n\
         ## Key Facts\n{}\n\n\
         ## Touched Files\n{}\n",
        envelope.session_id,
        envelope.generated_at,
        envelope.summary.trim(),
        envelope.objective.as_deref().unwrap_or("(not set)"),
        if envelope.grounded_facts.is_empty() {
            "(none)".to_string()
        } else {
            envelope
                .grounded_facts
                .iter()
                .map(|f| format!("- {}", f.fact))
                .collect::<Vec<_>>()
                .join("\n")
        },
        if envelope.touched_files.is_empty() {
            "(none)".to_string()
        } else {
            envelope
                .touched_files
                .iter()
                .map(|f| format!("- {}", f))
                .collect::<Vec<_>>()
                .join("\n")
        },
    );
    if let Err(e) = std::fs::write(&progress_path, progress_content) {
        warn!(path = %progress_path.display(), error = %e, "failed to write progress checkpoint");
    }

    // Write compaction_summary.md with the full summary
    let summary_path = memories_dir.join("compaction_summary.md");
    let summary_content = format!(
        "# Compaction Summary\n\n\
         **Session:** {}\n\
         **Generated:** {}\n\n\
         ## Full Summary\n{}\n\n\
         ## Constraints\n{}\n\n\
         ## Open Questions\n{}\n\n\
         ## Verification TODO\n{}\n",
        envelope.session_id,
        envelope.generated_at,
        envelope.summary.trim(),
        if envelope.constraints.is_empty() {
            "(none)".to_string()
        } else {
            envelope
                .constraints
                .iter()
                .map(|c| format!("- {}", c))
                .collect::<Vec<_>>()
                .join("\n")
        },
        if envelope.open_questions.is_empty() {
            "(none)".to_string()
        } else {
            envelope
                .open_questions
                .iter()
                .map(|q| format!("- {}", q))
                .collect::<Vec<_>>()
                .join("\n")
        },
        if envelope.verification_todo.is_empty() {
            "(none)".to_string()
        } else {
            envelope
                .verification_todo
                .iter()
                .map(|t| format!("- {}", t))
                .collect::<Vec<_>>()
                .join("\n")
        },
    );
    if let Err(e) = std::fs::write(&summary_path, summary_content) {
        warn!(path = %summary_path.display(), error = %e, "failed to write compaction summary");
    }
}
