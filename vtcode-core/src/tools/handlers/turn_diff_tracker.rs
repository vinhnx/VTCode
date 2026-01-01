//! Turn Diff Tracker (from Codex)
//!
//! Aggregates file diffs across multiple apply_patch tool calls within a turn.
//! This provides a unified view of all changes made during a conversation turn.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::RwLock;

/// File change types (from Codex protocol)
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileChange {
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
    /// Get the new content if any
    pub fn new_content(&self) -> Option<&str> {
        match self {
            FileChange::Add { content } => Some(content),
            FileChange::Update { new_content, .. } => Some(new_content),
            FileChange::Rename { new_content, .. } => new_content.as_deref(),
            FileChange::Delete { .. } => None,
        }
    }

    /// Get the old content if any
    pub fn old_content(&self) -> Option<&str> {
        match self {
            FileChange::Delete { original_content } => Some(original_content),
            FileChange::Update { old_content, .. } => Some(old_content),
            FileChange::Rename { old_content, .. } => old_content.as_deref(),
            FileChange::Add { .. } => None,
        }
    }
}

/// Turn diff tracker for aggregating changes (from Codex)
#[derive(Default)]
pub struct TurnDiffTracker {
    changes: HashMap<PathBuf, FileChange>,
    pending_changes: Option<HashMap<PathBuf, FileChange>>,
}

impl TurnDiffTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Called when a patch application begins (from Codex)
    ///
    /// Stores the pending changes until the patch is confirmed
    pub fn on_patch_begin(&mut self, changes: HashMap<PathBuf, FileChange>) {
        self.pending_changes = Some(changes);
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
            // Merge the changes
            let merged = match (existing, &change) {
                // Add then Update = Add with new content
                (FileChange::Add { .. }, FileChange::Update { new_content, .. }) => {
                    FileChange::Add {
                        content: new_content.clone(),
                    }
                }
                // Add then Delete = No change (remove from tracker)
                (FileChange::Add { .. }, FileChange::Delete { .. }) => {
                    self.changes.remove(&path);
                    return;
                }
                // Update then Update = Update with combined old/new
                (
                    FileChange::Update { old_content, .. },
                    FileChange::Update { new_content, .. },
                ) => FileChange::Update {
                    old_content: old_content.clone(),
                    new_content: new_content.clone(),
                },
                // Update then Delete = Delete with original old content
                (FileChange::Update { old_content, .. }, FileChange::Delete { .. }) => {
                    FileChange::Delete {
                        original_content: old_content.clone(),
                    }
                }
                // Delete then Add = Update
                (FileChange::Delete { original_content }, FileChange::Add { content }) => {
                    FileChange::Update {
                        old_content: original_content.clone(),
                        new_content: content.clone(),
                    }
                }
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
            match change {
                FileChange::Add { content } => {
                    diff.push_str(&format!("--- /dev/null\n+++ b/{}\n", path_str));
                    diff.push_str(&format_addition_diff(content));
                }
                FileChange::Delete { original_content } => {
                    diff.push_str(&format!("--- a/{}\n+++ /dev/null\n", path_str));
                    diff.push_str(&format_deletion_diff(original_content));
                }
                FileChange::Update {
                    old_content,
                    new_content,
                } => {
                    diff.push_str(&format!("--- a/{}\n+++ b/{}\n", path_str, path_str));
                    diff.push_str(&compute_unified_diff(old_content, new_content));
                }
                FileChange::Rename {
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
        changes.insert(
            PathBuf::from("test.txt"),
            FileChange::Add {
                content: "hello".to_string(),
            },
        );

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
        changes.insert(
            PathBuf::from("test.txt"),
            FileChange::Add {
                content: "hello".to_string(),
            },
        );

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
        changes1.insert(
            PathBuf::from("test.txt"),
            FileChange::Add {
                content: "hello".to_string(),
            },
        );
        tracker.on_patch_begin(changes1);
        tracker.on_patch_end(true);

        // Second patch: Update file
        let mut changes2 = HashMap::new();
        changes2.insert(
            PathBuf::from("test.txt"),
            FileChange::Update {
                old_content: "hello".to_string(),
                new_content: "world".to_string(),
            },
        );
        tracker.on_patch_begin(changes2);
        tracker.on_patch_end(true);

        // Result should be Add with new content
        let change = tracker.changes().get(&PathBuf::from("test.txt")).unwrap();
        match change {
            FileChange::Add { content } => assert_eq!(content, "world"),
            _ => panic!("Expected Add"),
        }
    }

    #[test]
    fn test_merge_add_then_delete() {
        let mut tracker = TurnDiffTracker::new();

        // First patch: Add file
        let mut changes1 = HashMap::new();
        changes1.insert(
            PathBuf::from("test.txt"),
            FileChange::Add {
                content: "hello".to_string(),
            },
        );
        tracker.on_patch_begin(changes1);
        tracker.on_patch_end(true);

        // Second patch: Delete file
        let mut changes2 = HashMap::new();
        changes2.insert(
            PathBuf::from("test.txt"),
            FileChange::Delete {
                original_content: "hello".to_string(),
            },
        );
        tracker.on_patch_begin(changes2);
        tracker.on_patch_end(true);

        // Result: no change (add then delete cancels out)
        assert!(!tracker.has_changes());
    }

    #[test]
    fn test_get_unified_diff() {
        let mut tracker = TurnDiffTracker::new();

        let mut changes = HashMap::new();
        changes.insert(
            PathBuf::from("new.txt"),
            FileChange::Add {
                content: "line1\nline2".to_string(),
            },
        );
        tracker.on_patch_begin(changes);
        tracker.on_patch_end(true);

        let diff = tracker.get_unified_diff();
        assert!(diff.contains("--- /dev/null"));
        assert!(diff.contains("+++ b/new.txt"));
        assert!(diff.contains("+line1"));
        assert!(diff.contains("+line2"));
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
        let add = FileChange::Add {
            content: "hello".to_string(),
        };
        assert_eq!(add.new_content(), Some("hello"));
        assert_eq!(add.old_content(), None);

        let delete = FileChange::Delete {
            original_content: "goodbye".to_string(),
        };
        assert_eq!(delete.new_content(), None);
        assert_eq!(delete.old_content(), Some("goodbye"));

        let update = FileChange::Update {
            old_content: "old".to_string(),
            new_content: "new".to_string(),
        };
        assert_eq!(update.new_content(), Some("new"));
        assert_eq!(update.old_content(), Some("old"));
    }

    #[tokio::test]
    async fn test_shared_tracker() {
        let tracker = new_shared_tracker();

        {
            let mut t = tracker.write().await;
            let mut changes = HashMap::new();
            changes.insert(
                PathBuf::from("test.txt"),
                FileChange::Add {
                    content: "hello".to_string(),
                },
            );
            t.on_patch_begin(changes);
            t.on_patch_end(true);
        }

        {
            let t = tracker.read().await;
            assert!(t.has_changes());
        }
    }
}
