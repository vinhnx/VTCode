use serde::{Deserialize, Serialize};

/// A diff hunk representing a contiguous block of changes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffHunk {
    pub old_start: usize,
    pub new_start: usize,
    pub old_lines: usize,
    pub new_lines: usize,
    pub display: String,
}

impl DiffHunk {
    pub fn summary(&self) -> String {
        if self.old_lines > 0 && self.new_lines > 0 {
            format!("-{} +{}", self.old_lines, self.new_lines)
        } else if self.old_lines > 0 {
            format!("-{}", self.old_lines)
        } else if self.new_lines > 0 {
            format!("+{}", self.new_lines)
        } else {
            "No changes".to_string()
        }
    }

    pub fn old_position(&self) -> String {
        if self.old_lines == 0 {
            format!("{}", self.old_start + 1)
        } else {
            format!("{}-{}", self.old_start + 1, self.old_start + self.old_lines)
        }
    }

    pub fn new_position(&self) -> String {
        if self.new_lines == 0 {
            format!("{}", self.new_start + 1)
        } else {
            format!("{}-{}", self.new_start + 1, self.new_start + self.new_lines)
        }
    }
}

/// Trust mode for diff preview - how to handle file edit approval
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrustMode {
    Once,
    Session,
    Always,
    AutoTrust,
}

impl TrustMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Once => "Once",
            Self::Session => "Session",
            Self::Always => "Always",
            Self::AutoTrust => "AutoTrust",
        }
    }
}

/// State for diff preview modal
#[derive(Debug, Clone)]
pub struct DiffPreviewState {
    pub file_path: String,
    pub before: String,
    pub after: String,
    pub hunks: Vec<DiffHunk>,
    pub current_hunk: usize,
    pub trust_mode: TrustMode,
}

impl DiffPreviewState {
    pub fn new(file_path: String, before: String, after: String, hunks: Vec<DiffHunk>) -> Self {
        Self {
            file_path,
            before,
            after,
            hunks,
            current_hunk: 0,
            trust_mode: TrustMode::Once,
        }
    }

    pub fn current_hunk_ref(&self) -> Option<&DiffHunk> {
        self.hunks.get(self.current_hunk)
    }

    pub fn hunk_count(&self) -> usize {
        self.hunks.len()
    }
}
