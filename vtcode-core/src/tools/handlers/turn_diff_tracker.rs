//! Turn Diff Tracker (from Codex)
//!
//! Aggregates file diffs across multiple apply_patch tool calls within a turn.
//! This provides a unified view of all changes made during a conversation turn.
//!
//! Supports Agent Trace attribution tracking for AI-generated code.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::RwLock;

/// Attribution information for a file change (Agent Trace compatible).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct ChangeAttribution {
    /// Model ID in provider/model format (e.g., "anthropic/claude-opus-4").
    pub model_id: Option<String>,
    /// Provider name (e.g., "anthropic", "openai").
    pub provider: Option<String>,
    /// Session ID linking to the conversation.
    pub session_id: Option<String>,
    /// Turn number within the session.
    pub turn_number: Option<u32>,
    /// Contributor type: "ai", "human", "mixed", "unknown".
    pub contributor_type: String,
}

impl ChangeAttribution {
    /// Create AI attribution with model info.
    pub fn ai(model_id: impl Into<String>, provider: impl Into<String>) -> Self {
        Self {
            model_id: Some(model_id.into()),
            provider: Some(provider.into()),
            session_id: None,
            turn_number: None,
            contributor_type: "ai".to_string(),
        }
    }

    /// Create human attribution.
    pub fn human() -> Self {
        Self {
            contributor_type: "human".to_string(),
            ..Default::default()
        }
    }

    /// Create unknown attribution.
    pub fn unknown() -> Self {
        Self {
            contributor_type: "unknown".to_string(),
            ..Default::default()
        }
    }

    /// Add session context.
    pub fn with_session(mut self, session_id: impl Into<String>, turn: u32) -> Self {
        self.session_id = Some(session_id.into());
        self.turn_number = Some(turn);
        self
    }

    /// Get normalized model ID in provider/model format.
    pub fn normalized_model_id(&self) -> Option<String> {
        match (&self.model_id, &self.provider) {
            (Some(model), Some(provider)) if !model.contains('/') => {
                Some(format!("{}/{}", provider, model))
            }
            (Some(model), _) => Some(model.clone()),
            _ => None,
        }
    }
}

/// File change types (from Codex protocol)
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileChange {
    /// The type of change.
    pub kind: FileChangeKind,
    /// Attribution information (Agent Trace).
    pub attribution: Option<ChangeAttribution>,
    /// Line range affected (1-indexed, for Agent Trace).
    pub line_range: Option<(u32, u32)>,
}

/// Kind of file change.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileChangeKind {
    /// New file added
    Add { content: String },
    /// File deleted
    Delete { original_content: String },
    /// File modified
    Update {
        old_content: String,
        new_content: String,
    },
    /// File renamed
    Rename {
        new_path: PathBuf,
        old_content: Option<String>,
        new_content: Option<String>,
    },
}

impl FileChange {
    /// Create a new Add change.
    pub fn add(content: impl Into<String>) -> Self {
        Self {
            kind: FileChangeKind::Add {
                content: content.into(),
            },
            attribution: None,
            line_range: None,
        }
    }

    /// Create a new Delete change.
    pub fn delete(original_content: impl Into<String>) -> Self {
        Self {
            kind: FileChangeKind::Delete {
                original_content: original_content.into(),
            },
            attribution: None,
            line_range: None,
        }
    }

    /// Create a new Update change.
    pub fn update(old_content: impl Into<String>, new_content: impl Into<String>) -> Self {
        Self {
            kind: FileChangeKind::Update {
                old_content: old_content.into(),
                new_content: new_content.into(),
            },
            attribution: None,
            line_range: None,
        }
    }

    /// Create a new Rename change.
    pub fn rename(
        new_path: PathBuf,
        old_content: Option<String>,
        new_content: Option<String>,
    ) -> Self {
        Self {
            kind: FileChangeKind::Rename {
                new_path,
                old_content,
                new_content,
            },
            attribution: None,
            line_range: None,
        }
    }

    /// Add attribution to the change.
    pub fn with_attribution(mut self, attribution: ChangeAttribution) -> Self {
        self.attribution = Some(attribution);
        self
    }

    /// Add line range to the change.
    pub fn with_line_range(mut self, start: u32, end: u32) -> Self {
        self.line_range = Some((start, end));
        self
    }

    /// Get the new content if any
    pub fn new_content(&self) -> Option<&str> {
        match &self.kind {
            FileChangeKind::Add { content } => Some(content),
            FileChangeKind::Update { new_content, .. } => Some(new_content),
            FileChangeKind::Rename { new_content, .. } => new_content.as_deref(),
            FileChangeKind::Delete { .. } => None,
        }
    }

    /// Get the old content if any
    pub fn old_content(&self) -> Option<&str> {
        match &self.kind {
            FileChangeKind::Delete { original_content } => Some(original_content),
            FileChangeKind::Update { old_content, .. } => Some(old_content),
            FileChangeKind::Rename { old_content, .. } => old_content.as_deref(),
            FileChangeKind::Add { .. } => None,
        }
    }

    /// Check if this is an add operation.
    pub fn is_add(&self) -> bool {
        matches!(self.kind, FileChangeKind::Add { .. })
    }

    /// Check if this is a delete operation.
    pub fn is_delete(&self) -> bool {
        matches!(self.kind, FileChangeKind::Delete { .. })
    }

    /// Check if this is an update operation.
    pub fn is_update(&self) -> bool {
        matches!(self.kind, FileChangeKind::Update { .. })
    }

    /// Check if this is a rename operation.
    pub fn is_rename(&self) -> bool {
        matches!(self.kind, FileChangeKind::Rename { .. })
    }

    /// Compute line count for the new content.
    pub fn new_line_count(&self) -> usize {
        self.new_content().map(|c| c.lines().count()).unwrap_or(0)
    }
}

/// Turn diff tracker for aggregating changes (from Codex)
#[derive(Default)]
pub struct TurnDiffTracker {
    changes: HashMap<PathBuf, FileChange>,
    pending_changes: Option<HashMap<PathBuf, FileChange>>,
    /// Current attribution context for new changes.
    current_attribution: Option<ChangeAttribution>,
}

impl TurnDiffTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the current attribution context for subsequent changes.
    pub fn set_attribution(&mut self, attribution: ChangeAttribution) {
        self.current_attribution = Some(attribution);
    }

    /// Clear the current attribution context.
    pub fn clear_attribution(&mut self) {
        self.current_attribution = None;
    }

    /// Get the current attribution context.
    pub fn current_attribution(&self) -> Option<&ChangeAttribution> {
        self.current_attribution.as_ref()
    }

    /// Called when a patch application begins (from Codex)
    ///
    /// Stores the pending changes until the patch is confirmed
    pub fn on_patch_begin(&mut self, changes: HashMap<PathBuf, FileChange>) {
        // Apply current attribution to all changes
        let changes_with_attribution: HashMap<PathBuf, FileChange> = changes
            .into_iter()
            .map(|(path, mut change)| {
                if change.attribution.is_none() {
                    change.attribution = self.current_attribution.clone();
                }
                (path, change)
            })
            .collect();
        self.pending_changes = Some(changes_with_attribution);
    }

    /// Called when a patch application ends (from Codex)
    ///
    /// If successful, merges pending changes into the main tracker
    pub fn on_patch_end(&mut self, success: bool) {
        if success {
            if let Some(pending) = self.pending_changes.take() {
                for (path, change) in pending {
                    self.merge_change(path, change);
                }
            }
        } else {
            self.pending_changes = None;
        }
    }

    /// Merge a change into the tracker, combining with existing changes
    fn merge_change(&mut self, path: PathBuf, change: FileChange) {
        if let Some(existing) = self.changes.get(&path) {
            // Merge the changes, preserving the latest attribution
            let merged = match (&existing.kind, &change.kind) {
                // Add then Update = Add with new content
                (FileChangeKind::Add { .. }, FileChangeKind::Update { new_content, .. }) => {
                    FileChange {
                        kind: FileChangeKind::Add {
                            content: new_content.clone(),
                        },
                        attribution: change.attribution.clone().or(existing.attribution.clone()),
                        line_range: change.line_range,
                    }
                }
                // Add then Delete = No change (remove from tracker)
                (FileChangeKind::Add { .. }, FileChangeKind::Delete { .. }) => {
                    self.changes.remove(&path);
                    return;
                }
                // Update then Update = Update with combined old/new
                (
                    FileChangeKind::Update { old_content, .. },
                    FileChangeKind::Update { new_content, .. },
                ) => FileChange {
                    kind: FileChangeKind::Update {
                        old_content: old_content.clone(),
                        new_content: new_content.clone(),
                    },
                    attribution: change.attribution.clone().or(existing.attribution.clone()),
                    line_range: change.line_range,
                },
                // Update then Delete = Delete with original old content
                (FileChangeKind::Update { old_content, .. }, FileChangeKind::Delete { .. }) => {
                    FileChange {
                        kind: FileChangeKind::Delete {
                            original_content: old_content.clone(),
                        },
                        attribution: change.attribution.clone().or(existing.attribution.clone()),
                        line_range: None,
                    }
                }
                // Delete then Add = Update
                (
                    FileChangeKind::Delete { original_content },
                    FileChangeKind::Add { content },
                ) => FileChange {
                    kind: FileChangeKind::Update {
                        old_content: original_content.clone(),
                        new_content: content.clone(),
                    },
                    attribution: change.attribution.clone().or(existing.attribution.clone()),
                    line_range: change.line_range,
                },
                // Default: use the new change
                _ => change,
            };
            self.changes.insert(path, merged);
        } else {
            self.changes.insert(path, change);
        }
    }

    /// Get all tracked changes
    pub fn changes(&self) -> &HashMap<PathBuf, FileChange> {
        &self.changes
    }

    /// Get pending changes (not yet confirmed)
    pub fn pending_changes(&self) -> Option<&HashMap<PathBuf, FileChange>> {
        self.pending_changes.as_ref()
    }

    /// Check if there are any changes
    pub fn has_changes(&self) -> bool {
        !self.changes.is_empty()
    }

    /// Get unified diff for all tracked changes (from Codex)
    pub fn get_unified_diff(&self) -> String {
        let mut diff = String::new();

        for (path, change) in &self.changes {
            let path_str = path.display();
            match &change.kind {
                FileChangeKind::Add { content } => {
                    diff.push_str(&format!("--- /dev/null\n+++ b/{}\n", path_str));
                    diff.push_str(&format_addition_diff(content));
                }
                FileChangeKind::Delete { original_content } => {
                    diff.push_str(&format!("--- a/{}\n+++ /dev/null\n", path_str));
                    diff.push_str(&format_deletion_diff(original_content));
                }
                FileChangeKind::Update {
                    old_content,
                    new_content,
                } => {
                    diff.push_str(&format!("--- a/{}\n+++ b/{}\n", path_str, path_str));
                    diff.push_str(&compute_unified_diff(old_content, new_content));
                }
                FileChangeKind::Rename {
                    new_path,
                    old_content,
                    new_content,
                } => {
                    diff.push_str(&format!(
                        "--- a/{}\n+++ b/{}\n",
                        path_str,
                        new_path.display()
                    ));
                    if let (Some(old), Some(new)) = (old_content, new_content) {
                        diff.push_str(&compute_unified_diff(old, new));
                    }
                }
            }
            diff.push('\n');
        }

        diff
    }

    /// Clear all tracked changes
    pub fn clear(&mut self) {
        self.changes.clear();
        self.pending_changes = None;
    }
}

/// Shared turn diff tracker (thread-safe) (from Codex)
pub type SharedTurnDiffTracker = Arc<RwLock<TurnDiffTracker>>;

/// Create a new shared diff tracker
pub fn new_shared_tracker() -> SharedTurnDiffTracker {
    Arc::new(RwLock::new(TurnDiffTracker::new()))
}

/// Format an addition as unified diff lines
fn format_addition_diff(content: &str) -> String {
    let mut result = String::new();
    let lines: Vec<&str> = content.lines().collect();
    let line_count = lines.len();

    result.push_str(&format!("@@ -0,0 +1,{} @@\n", line_count));
    for line in lines {
        result.push_str(&format!("+{}\n", line));
    }
    result
}

/// Format a deletion as unified diff lines
fn format_deletion_diff(content: &str) -> String {
    let mut result = String::new();
    let lines: Vec<&str> = content.lines().collect();
    let line_count = lines.len();

    result.push_str(&format!("@@ -1,{} +0,0 @@\n", line_count));
    for line in lines {
        result.push_str(&format!("-{}\n", line));
    }
    result
}

/// Compute unified diff between old and new content
fn compute_unified_diff(old: &str, new: &str) -> String {
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();

    // Simple line-by-line diff (production would use proper diff algorithm)
    let mut result = String::new();
    let max_len = old_lines.len().max(new_lines.len());

    result.push_str(&format!(
        "@@ -1,{} +1,{} @@\n",
        old_lines.len(),
        new_lines.len()
    ));

    for i in 0..max_len {
        let old_line = old_lines.get(i);
        let new_line = new_lines.get(i);

        match (old_line, new_line) {
            (Some(o), Some(n)) if o == n => {
                result.push_str(&format!(" {}\n", o));
            }
            (Some(o), Some(n)) => {
                result.push_str(&format!("-{}\n", o));
                result.push_str(&format!("+{}\n", n));
            }
            (Some(o), None) => {
                result.push_str(&format!("-{}\n", o));
            }
            (None, Some(n)) => {
                result.push_str(&format!("+{}\n", n));
            }
            (None, None) => {}
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_on_patch_begin_and_end_success() {
        let mut tracker = TurnDiffTracker::new();

        let mut changes = HashMap::new();
        changes.insert(PathBuf::from("test.txt"), FileChange::add("hello"));

        tracker.on_patch_begin(changes);
        assert!(tracker.pending_changes().is_some());
        assert!(!tracker.has_changes());

        tracker.on_patch_end(true);
        assert!(tracker.pending_changes().is_none());
        assert!(tracker.has_changes());
    }

    #[test]
    fn test_on_patch_end_failure() {
        let mut tracker = TurnDiffTracker::new();

        let mut changes = HashMap::new();
        changes.insert(PathBuf::from("test.txt"), FileChange::add("hello"));

        tracker.on_patch_begin(changes);
        tracker.on_patch_end(false);

        assert!(tracker.pending_changes().is_none());
        assert!(!tracker.has_changes());
    }

    #[test]
    fn test_merge_add_then_update() {
        let mut tracker = TurnDiffTracker::new();

        // First patch: Add file
        let mut changes1 = HashMap::new();
        changes1.insert(PathBuf::from("test.txt"), FileChange::add("hello"));
        tracker.on_patch_begin(changes1);
        tracker.on_patch_end(true);

        // Second patch: Update file
        let mut changes2 = HashMap::new();
        changes2.insert(
            PathBuf::from("test.txt"),
            FileChange::update("hello", "world"),
        );
        tracker.on_patch_begin(changes2);
        tracker.on_patch_end(true);

        // Result should be Add with new content
        let change = tracker.changes().get(&PathBuf::from("test.txt")).unwrap();
        assert!(change.is_add());
        assert_eq!(change.new_content(), Some("world"));
    }

    #[test]
    fn test_merge_add_then_delete() {
        let mut tracker = TurnDiffTracker::new();

        // First patch: Add file
        let mut changes1 = HashMap::new();
        changes1.insert(PathBuf::from("test.txt"), FileChange::add("hello"));
        tracker.on_patch_begin(changes1);
        tracker.on_patch_end(true);

        // Second patch: Delete file
        let mut changes2 = HashMap::new();
        changes2.insert(PathBuf::from("test.txt"), FileChange::delete("hello"));
        tracker.on_patch_begin(changes2);
        tracker.on_patch_end(true);

        // Result: no change (add then delete cancels out)
        assert!(!tracker.has_changes());
    }

    #[test]
    fn test_get_unified_diff() {
        let mut tracker = TurnDiffTracker::new();

        let mut changes = HashMap::new();
        changes.insert(PathBuf::from("new.txt"), FileChange::add("line1\nline2"));
        tracker.on_patch_begin(changes);
        tracker.on_patch_end(true);

        let diff = tracker.get_unified_diff();
        assert!(diff.contains("--- /dev/null"));
        assert!(diff.contains("+++ b/new.txt"));
        assert!(diff.contains("+line1"));
        assert!(diff.contains("+line2"));
    }

    #[test]
    fn test_attribution_propagation() {
        let mut tracker = TurnDiffTracker::new();
        tracker.set_attribution(ChangeAttribution::ai("claude-opus-4", "anthropic"));

        let mut changes = HashMap::new();
        changes.insert(PathBuf::from("test.txt"), FileChange::add("hello"));
        tracker.on_patch_begin(changes);
        tracker.on_patch_end(true);

        let change = tracker.changes().get(&PathBuf::from("test.txt")).unwrap();
        assert!(change.attribution.is_some());
        let attr = change.attribution.as_ref().unwrap();
        assert_eq!(attr.contributor_type, "ai");
        assert_eq!(attr.model_id, Some("claude-opus-4".to_string()));
    }

    #[test]
    fn test_format_addition_diff() {
        let diff = format_addition_diff("line1\nline2");
        assert!(diff.contains("@@ -0,0 +1,2 @@"));
        assert!(diff.contains("+line1"));
        assert!(diff.contains("+line2"));
    }

    #[test]
    fn test_format_deletion_diff() {
        let diff = format_deletion_diff("line1\nline2");
        assert!(diff.contains("@@ -1,2 +0,0 @@"));
        assert!(diff.contains("-line1"));
        assert!(diff.contains("-line2"));
    }

    #[test]
    fn test_compute_unified_diff() {
        let old = "line1\nline2\nline3";
        let new = "line1\nmodified\nline3";
        let diff = compute_unified_diff(old, new);

        assert!(diff.contains(" line1"));
        assert!(diff.contains("-line2"));
        assert!(diff.contains("+modified"));
        assert!(diff.contains(" line3"));
    }

    #[test]
    fn test_file_change_accessors() {
        let add = FileChange::add("hello");
        assert_eq!(add.new_content(), Some("hello"));
        assert_eq!(add.old_content(), None);
        assert!(add.is_add());

        let delete = FileChange::delete("goodbye");
        assert_eq!(delete.new_content(), None);
        assert_eq!(delete.old_content(), Some("goodbye"));
        assert!(delete.is_delete());

        let update = FileChange::update("old", "new");
        assert_eq!(update.new_content(), Some("new"));
        assert_eq!(update.old_content(), Some("old"));
        assert!(update.is_update());
    }

    #[test]
    fn test_normalized_model_id() {
        let attr = ChangeAttribution::ai("claude-opus-4", "anthropic");
        assert_eq!(
            attr.normalized_model_id(),
            Some("anthropic/claude-opus-4".to_string())
        );

        let attr2 = ChangeAttribution::ai("anthropic/claude-opus-4", "anthropic");
        assert_eq!(
            attr2.normalized_model_id(),
            Some("anthropic/claude-opus-4".to_string())
        );
    }

    #[tokio::test]
    async fn test_shared_tracker() {
        let tracker = new_shared_tracker();

        {
            let mut t = tracker.write().await;
            let mut changes = HashMap::new();
            changes.insert(PathBuf::from("test.txt"), FileChange::add("hello"));
            t.on_patch_begin(changes);
            t.on_patch_end(true);
        }

        {
            let t = tracker.read().await;
            assert!(t.has_changes());
        }
    }
}
