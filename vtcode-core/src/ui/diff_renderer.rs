use crate::config::constants::diff as diff_constants;
use crate::ui::git_config::GitColorConfig;
use crate::utils::style_helpers::style_from_color_name;
use anstyle::{Reset, Style};
use anstyle_git;
use std::path::Path;

pub(crate) struct GitDiffPalette {
    pub(crate) bullet: Style,
    pub(crate) label: Style,
    pub(crate) path: Style,
    pub(crate) stat_added: Style,
    pub(crate) stat_removed: Style,
    pub(crate) line_added: Style,
    pub(crate) line_removed: Style,
    pub(crate) line_context: Style,
    pub(crate) line_header: Style,
    pub(crate) line_number: Style,
}

impl GitDiffPalette {
    fn new(use_colors: bool) -> Self {
        let git_parse = |git_color: &str| -> Style {
            if use_colors {
                anstyle_git::parse(git_color).unwrap_or(
                    // Fallback to original behavior for backwards compatibility
                    match git_color {
                        "new" => style_from_color_name("green"),
                        "old" => style_from_color_name("red"),
                        "context" => style_from_color_name("white"),
                        "meta" | "header" => style_from_color_name("cyan"),
                        _ => Style::new(),
                    },
                )
            } else {
                Style::new()
            }
        };

        Self {
            bullet: if use_colors {
                style_from_color_name("yellow")
            } else {
                Style::new()
            }, // For summary bullets
            label: if use_colors {
                style_from_color_name("white")
            } else {
                Style::new()
            }, // For summary labels
            path: if use_colors {
                style_from_color_name("white")
            } else {
                Style::new()
            }, // For file paths
            stat_added: git_parse("new"),
            stat_removed: git_parse("old"),
            line_added: git_parse("new"),
            line_removed: git_parse("old"),
            line_context: git_parse("context"),
            line_header: git_parse("meta"), // Git uses "meta" for headers
            line_number: if use_colors {
                style_from_color_name("yellow")
            } else {
                Style::new()
            }, // For line numbers
        }
    }

    /// Create palette from Git config colors
    fn from_git_config(config: &GitColorConfig, use_colors: bool) -> Self {
        if !use_colors {
            return Self::new(false);
        }

        Self {
            bullet: style_from_color_name("yellow"),
            label: style_from_color_name("white"),
            path: style_from_color_name("white"),
            stat_added: config.diff_new,
            stat_removed: config.diff_old,
            line_added: config.diff_new,
            line_removed: config.diff_old,
            line_context: config.diff_context,
            line_header: config.diff_header, // Use the configuration from Git
            line_number: style_from_color_name("yellow"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct DiffLine {
    pub line_type: DiffLineType,
    pub content: String,
    pub line_number_old: Option<usize>,
    pub line_number_new: Option<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DiffLineType {
    Added,
    Removed,
    Context,
    Header,
}

#[derive(Debug)]
pub struct FileDiff {
    pub file_path: String,
    pub old_content: String,
    pub new_content: String,
    pub lines: Vec<DiffLine>,
    pub stats: DiffStats,
}

#[derive(Debug, Clone)]
pub struct DiffStats {
    pub additions: usize,
    pub deletions: usize,
    pub changes: usize,
}

/// Result of checking whether diffs should be suppressed
#[derive(Debug, Clone)]
pub struct DiffSuppressionCheck {
    /// Whether diffs should be suppressed
    pub should_suppress: bool,
    /// Reason for suppression (if applicable)
    pub reason: Option<String>,
    /// Total number of files with changes
    pub file_count: usize,
    /// Total number of diff lines across all files
    pub total_lines: usize,
    /// Total additions across all files
    pub total_additions: usize,
    /// Total deletions across all files
    pub total_deletions: usize,
    /// List of changed files with their individual stats (path, additions, deletions)
    pub file_stats: Vec<FileChangeStats>,
}

/// Statistics for a single changed file
#[derive(Debug, Clone)]
pub struct FileChangeStats {
    pub path: String,
    pub additions: usize,
    pub deletions: usize,
}

impl DiffSuppressionCheck {
    /// Create a new check indicating no suppression needed
    pub fn no_suppression(
        file_count: usize,
        total_lines: usize,
        additions: usize,
        deletions: usize,
        file_stats: Vec<FileChangeStats>,
    ) -> Self {
        Self {
            should_suppress: false,
            reason: None,
            file_count,
            total_lines,
            total_additions: additions,
            total_deletions: deletions,
            file_stats,
        }
    }

    /// Create a new check indicating suppression is needed
    pub fn suppressed(
        reason: String,
        file_count: usize,
        total_lines: usize,
        additions: usize,
        deletions: usize,
        file_stats: Vec<FileChangeStats>,
    ) -> Self {
        Self {
            should_suppress: true,
            reason: Some(reason),
            file_count,
            total_lines,
            total_additions: additions,
            total_deletions: deletions,
            file_stats,
        }
    }
}

pub struct DiffRenderer {
    show_line_numbers: bool,
    context_lines: usize,
    use_colors: bool,
    pub(crate) palette: GitDiffPalette,
}

impl DiffRenderer {
    pub fn new(show_line_numbers: bool, context_lines: usize, use_colors: bool) -> Self {
        Self {
            show_line_numbers,
            context_lines,
            use_colors,
            palette: GitDiffPalette::new(use_colors),
        }
    }

    /// Create renderer with colors from Git config
    pub fn with_git_config(
        show_line_numbers: bool,
        context_lines: usize,
        use_colors: bool,
        config: &GitColorConfig,
    ) -> Self {
        Self {
            show_line_numbers,
            context_lines,
            use_colors,
            palette: GitDiffPalette::from_git_config(config, use_colors),
        }
    }

    pub fn render_diff(&self, diff: &FileDiff) -> String {
        let mut output = String::new();

        // File header with edit indicator
        output.push_str("─ ");
        output.push_str(&self.render_summary(diff));
        output.push('\n');

        for line in &diff.lines {
            output.push_str(&self.render_line(line));
            output.push('\n');
        }

        output
    }

    fn render_summary(&self, diff: &FileDiff) -> String {
        let bullet = self.paint(&self.palette.bullet, "▸");
        let label = self.paint(&self.palette.label, "Edit");
        let path = self.paint(&self.palette.path, &diff.file_path);
        let additions = format!("+{}", diff.stats.additions);
        let deletions = format!("-{}", diff.stats.deletions);
        let added_span = self.paint(&self.palette.stat_added, &additions);
        let removed_span = self.paint(&self.palette.stat_removed, &deletions);
        format!(
            "{} {} {} {} {}",
            bullet, label, path, added_span, removed_span
        )
    }

    fn render_line(&self, line: &DiffLine) -> String {
        let (style, prefix, line_number) = match line.line_type {
            DiffLineType::Added => (&self.palette.line_added, "+", line.line_number_new),
            DiffLineType::Removed => (&self.palette.line_removed, "-", line.line_number_old),
            DiffLineType::Context => (
                &self.palette.line_context,
                " ",
                line.line_number_new.or(line.line_number_old),
            ),
            DiffLineType::Header => (&self.palette.line_header, "", None),
        };

        let mut rendered = String::new();

        if self.show_line_numbers {
            let number_text = line_number
                .map(|n| format!("{:>4}", n))
                .unwrap_or_else(|| "    ".to_string());
            rendered.push_str(&self.paint(&self.palette.line_number, &number_text));
            rendered.push(' ');
        }

        let content = match line.line_type {
            DiffLineType::Header => line.content.clone(),
            _ => {
                if line.content.is_empty() {
                    prefix.to_string()
                } else {
                    format!("{} {}", prefix, line.content)
                }
            }
        };

        rendered.push_str(&self.paint(style, &content));
        rendered
    }

    pub(crate) fn paint(&self, style: &Style, text: &str) -> String {
        if self.use_colors {
            // CRITICAL: Apply style and reset without including newlines in the styled block
            // This ensures Reset appears before any line terminators, preventing color bleed
            format!("{}{}{}", style.render(), text, Reset.render())
        } else {
            text.to_string()
        }
    }

    pub fn generate_diff(&self, old_content: &str, new_content: &str, file_path: &str) -> FileDiff {
        let old_lines: Vec<&str> = old_content.lines().collect();
        let new_lines: Vec<&str> = new_content.lines().collect();

        let mut lines = Vec::new();
        let mut additions = 0;
        let mut deletions = 0;
        let _changes = 0;

        // Simple diff algorithm - can be enhanced with more sophisticated diffing
        let mut old_idx = 0;
        let mut new_idx = 0;

        while old_idx < old_lines.len() || new_idx < new_lines.len() {
            if old_idx < old_lines.len() && new_idx < new_lines.len() {
                if old_lines[old_idx] == new_lines[new_idx] {
                    // Same line - context
                    lines.push(DiffLine {
                        line_type: DiffLineType::Context,
                        content: old_lines[old_idx].to_string(),
                        line_number_old: Some(old_idx + 1),
                        line_number_new: Some(new_idx + 1),
                    });
                    old_idx += 1;
                    new_idx += 1;
                } else {
                    // Lines differ - find the difference
                    let (old_end, new_end) =
                        self.find_difference(&old_lines, &new_lines, old_idx, new_idx);

                    // Add removed lines
                    for i in old_idx..old_end {
                        lines.push(DiffLine {
                            line_type: DiffLineType::Removed,
                            content: old_lines[i].to_string(),
                            line_number_old: Some(i + 1),
                            line_number_new: None,
                        });
                        deletions += 1;
                    }

                    // Add added lines
                    for i in new_idx..new_end {
                        lines.push(DiffLine {
                            line_type: DiffLineType::Added,
                            content: new_lines[i].to_string(),
                            line_number_old: None,
                            line_number_new: Some(i + 1),
                        });
                        additions += 1;
                    }

                    old_idx = old_end;
                    new_idx = new_end;
                }
            } else if old_idx < old_lines.len() {
                // Remaining old lines are deletions
                lines.push(DiffLine {
                    line_type: DiffLineType::Removed,
                    content: old_lines[old_idx].to_string(),
                    line_number_old: Some(old_idx + 1),
                    line_number_new: None,
                });
                deletions += 1;
                old_idx += 1;
            } else if new_idx < new_lines.len() {
                // Remaining new lines are additions
                lines.push(DiffLine {
                    line_type: DiffLineType::Added,
                    content: new_lines[new_idx].to_string(),
                    line_number_old: None,
                    line_number_new: Some(new_idx + 1),
                });
                additions += 1;
                new_idx += 1;
            }
        }

        let changes = additions + deletions;

        FileDiff {
            file_path: file_path.to_string(),
            old_content: old_content.to_string(),
            new_content: new_content.to_string(),
            lines,
            stats: DiffStats {
                additions,
                deletions,
                changes,
            },
        }
    }

    fn find_difference(
        &self,
        old_lines: &[&str],
        new_lines: &[&str],
        start_old: usize,
        start_new: usize,
    ) -> (usize, usize) {
        let mut old_end = start_old;
        let mut new_end = start_new;

        // Look for the next matching line
        while old_end < old_lines.len() && new_end < new_lines.len() {
            if old_lines[old_end] == new_lines[new_end] {
                return (old_end, new_end);
            }

            // Check if we can find a match within context window
            let mut found = false;
            for i in 1..=self.context_lines {
                if old_end + i < old_lines.len() && new_end + i < new_lines.len() {
                    if old_lines[old_end + i] == new_lines[new_end + i] {
                        old_end += i;
                        new_end += i;
                        found = true;
                        break;
                    }
                }
            }

            if !found {
                old_end += 1;
                new_end += 1;
            }
        }

        (old_end, new_end)
    }
}

pub struct DiffChatRenderer {
    diff_renderer: DiffRenderer,
}

impl DiffChatRenderer {
    pub fn new(show_line_numbers: bool, context_lines: usize, use_colors: bool) -> Self {
        Self {
            diff_renderer: DiffRenderer::new(show_line_numbers, context_lines, use_colors),
        }
    }

    /// Create renderer with colors from Git config
    pub fn with_git_config(
        show_line_numbers: bool,
        context_lines: usize,
        use_colors: bool,
        config: &GitColorConfig,
    ) -> Self {
        Self {
            diff_renderer: DiffRenderer::with_git_config(
                show_line_numbers,
                context_lines,
                use_colors,
                config,
            ),
        }
    }

    pub fn render_file_change(
        &self,
        file_path: &Path,
        old_content: &str,
        new_content: &str,
    ) -> String {
        let diff = self.diff_renderer.generate_diff(
            old_content,
            new_content,
            &file_path.to_string_lossy(),
        );
        self.diff_renderer.render_diff(&diff)
    }

    pub fn render_multiple_changes(&self, changes: Vec<(String, String, String)>) -> String {
        // Check if we should suppress diffs
        let suppression_check = self.check_suppression(&changes);

        if suppression_check.should_suppress {
            return self.render_suppressed_summary(&suppression_check);
        }

        let mut output = format!("\nMultiple File Changes ({} files)\n", changes.len());
        output.push_str("═".repeat(60).as_str());
        output.push_str("\n\n");

        for (file_path, old_content, new_content) in changes {
            let diff = self
                .diff_renderer
                .generate_diff(&old_content, &new_content, &file_path);
            output.push_str(&self.diff_renderer.render_diff(&diff));
        }

        output
    }

    /// Check if diffs should be suppressed based on size/count thresholds
    pub fn check_suppression(&self, changes: &[(String, String, String)]) -> DiffSuppressionCheck {
        let file_count = changes.len();
        let mut total_lines = 0usize;
        let mut total_additions = 0usize;
        let mut total_deletions = 0usize;
        let mut single_file_exceeds = false;
        let mut file_stats = Vec::with_capacity(file_count);

        for (file_path, old_content, new_content) in changes {
            let diff = self
                .diff_renderer
                .generate_diff(old_content, new_content, file_path);
            total_lines += diff.lines.len();
            total_additions += diff.stats.additions;
            total_deletions += diff.stats.deletions;

            // Collect per-file stats for the summary
            file_stats.push(FileChangeStats {
                path: file_path.clone(),
                additions: diff.stats.additions,
                deletions: diff.stats.deletions,
            });

            // Check if a single file exceeds the threshold
            if diff.stats.changes > diff_constants::MAX_SINGLE_FILE_CHANGES {
                single_file_exceeds = true;
            }
        }

        // Check suppression conditions
        if file_count > diff_constants::MAX_INLINE_DIFF_FILES {
            return DiffSuppressionCheck::suppressed(
                format!(
                    "Too many files changed ({} files, max {})",
                    file_count,
                    diff_constants::MAX_INLINE_DIFF_FILES
                ),
                file_count,
                total_lines,
                total_additions,
                total_deletions,
                file_stats,
            );
        }

        if total_lines > diff_constants::MAX_TOTAL_DIFF_LINES {
            return DiffSuppressionCheck::suppressed(
                format!(
                    "Too many diff lines ({} lines, max {})",
                    total_lines,
                    diff_constants::MAX_TOTAL_DIFF_LINES
                ),
                file_count,
                total_lines,
                total_additions,
                total_deletions,
                file_stats,
            );
        }

        if single_file_exceeds {
            return DiffSuppressionCheck::suppressed(
                format!(
                    "Single file exceeds change limit (max {} changes per file)",
                    diff_constants::MAX_SINGLE_FILE_CHANGES
                ),
                file_count,
                total_lines,
                total_additions,
                total_deletions,
                file_stats,
            );
        }

        DiffSuppressionCheck::no_suppression(
            file_count,
            total_lines,
            total_additions,
            total_deletions,
            file_stats,
        )
    }

    /// Render a summary when diffs are suppressed
    pub fn render_suppressed_summary(&self, check: &DiffSuppressionCheck) -> String {
        let mut output = String::new();

        // Header with warning indicator
        output.push_str("\n⚠ ");
        output.push_str(diff_constants::SUPPRESSION_MESSAGE);
        output.push_str("\n\n");

        // Overall summary with colored stats
        output.push_str(&format!("Summary: {} file(s) changed", check.file_count));
        if check.total_additions > 0 || check.total_deletions > 0 {
            let additions = self.diff_renderer.paint(
                &self.diff_renderer.palette.stat_added,
                &format!("+{}", check.total_additions),
            );
            let deletions = self.diff_renderer.paint(
                &self.diff_renderer.palette.stat_removed,
                &format!("-{}", check.total_deletions),
            );
            output.push_str(&format!(" ({} {})", additions, deletions));
        }
        output.push_str("\n");

        // List changed files with individual stats
        if !check.file_stats.is_empty() {
            output.push_str("\nChanged files:\n");
            let max_files_to_show = diff_constants::MAX_FILES_IN_SUMMARY;
            for (i, stat) in check.file_stats.iter().enumerate() {
                if i >= max_files_to_show {
                    let remaining = check.file_stats.len() - max_files_to_show;
                    output.push_str(&format!("  ... and {} more file(s)\n", remaining));
                    break;
                }
                let additions = self.diff_renderer.paint(
                    &self.diff_renderer.palette.stat_added,
                    &format!("+{}", stat.additions),
                );
                let deletions = self.diff_renderer.paint(
                    &self.diff_renderer.palette.stat_removed,
                    &format!("-{}", stat.deletions),
                );
                output.push_str(&format!(
                    "  • {} ({} {})\n",
                    stat.path, additions, deletions
                ));
            }
        }

        // Show reason in dimmed text
        if let Some(reason) = &check.reason {
            output.push_str(&format!("\nReason: {}\n", reason));
        }

        // Tip for viewing full diff
        output.push('\n');
        output.push_str(diff_constants::SUPPRESSION_HINT);
        output.push('\n');

        output
    }

    pub fn render_operation_summary(
        &self,
        operation: &str,
        files_affected: usize,
        success: bool,
    ) -> String {
        let status_indicator = if success { "✓" } else { "✗" };
        let status_label = if success { "Success" } else { "Failure" };
        let mut summary = format!("\n{} [{}] {}\n", status_indicator, status_label, operation);
        summary.push_str(&format!("└─ {} file(s) affected\n", files_affected));

        if success {
            summary.push_str("   Operation completed successfully\n");
        } else {
            summary.push_str("   Operation completed with errors\n");
        }

        summary
    }
}

pub fn generate_unified_diff(old_content: &str, new_content: &str, filename: &str) -> String {
    let mut diff = format!("--- a/{}\n+++ b/{}\n", filename, filename);

    let old_lines: Vec<&str> = old_content.lines().collect();
    let new_lines: Vec<&str> = new_content.lines().collect();

    let mut old_idx = 0;
    let mut new_idx = 0;

    while old_idx < old_lines.len() || new_idx < new_lines.len() {
        // Find the next difference
        let start_old = old_idx;
        let start_new = new_idx;

        // Skip matching lines
        while old_idx < old_lines.len()
            && new_idx < new_lines.len()
            && old_lines[old_idx] == new_lines[new_idx]
        {
            old_idx += 1;
            new_idx += 1;
        }

        if old_idx == old_lines.len() && new_idx == new_lines.len() {
            break; // No more differences
        }

        // Find the end of the difference
        let mut end_old = old_idx;
        let mut end_new = new_idx;

        // Look for next matching context
        let mut context_found = false;
        for i in 0..3 {
            // Look ahead 3 lines for context
            if end_old + i < old_lines.len() && end_new + i < new_lines.len() {
                if old_lines[end_old + i] == new_lines[end_new + i] {
                    end_old += i;
                    end_new += i;
                    context_found = true;
                    break;
                }
            }
        }

        if !context_found {
            end_old = old_lines.len();
            end_new = new_lines.len();
        }

        // Generate hunk
        let old_count = end_old - start_old;
        let new_count = end_new - start_new;

        diff.push_str(&format!(
            "@@ -{},{} +{},{} @@\n",
            start_old + 1,
            old_count,
            start_new + 1,
            new_count
        ));

        // Add context before
        for i in (start_old.saturating_sub(3))..start_old {
            if i < old_lines.len() {
                diff.push_str(&format!(" {}\n", old_lines[i]));
            }
        }

        // Add removed lines
        for i in start_old..end_old {
            if i < old_lines.len() {
                diff.push_str(&format!("-{}\n", old_lines[i]));
            }
        }

        // Add added lines
        for i in start_new..end_new {
            if i < new_lines.len() {
                diff.push_str(&format!("+{}\n", new_lines[i]));
            }
        }

        // Add context after
        for i in end_old..(end_old + 3) {
            if i < old_lines.len() {
                diff.push_str(&format!(" {}\n", old_lines[i]));
            }
        }

        old_idx = end_old;
        new_idx = end_new;
    }

    diff
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_suppression_check_no_suppression() {
        let renderer = DiffChatRenderer::new(true, 3, false);
        let changes = vec![
            (
                "file1.rs".to_string(),
                "old1".to_string(),
                "new1".to_string(),
            ),
            (
                "file2.rs".to_string(),
                "old2".to_string(),
                "new2".to_string(),
            ),
        ];

        let check = renderer.check_suppression(&changes);
        assert!(!check.should_suppress);
        assert!(check.reason.is_none());
        assert_eq!(check.file_count, 2);
    }

    #[test]
    fn test_suppression_check_too_many_files() {
        let renderer = DiffChatRenderer::new(true, 3, false);
        // Create more files than MAX_INLINE_DIFF_FILES
        let mut changes = Vec::new();
        for i in 0..(diff_constants::MAX_INLINE_DIFF_FILES + 5) {
            changes.push((
                format!("file{}.rs", i),
                format!("old{}", i),
                format!("new{}", i),
            ));
        }

        let check = renderer.check_suppression(&changes);
        assert!(check.should_suppress);
        assert!(check.reason.is_some());
        assert!(check.reason.as_ref().unwrap().contains("Too many files"));
    }

    #[test]
    fn test_suppression_check_large_single_file() {
        let renderer = DiffChatRenderer::new(true, 3, false);

        // Create a file with many changes
        let old_content: String = (0..300).map(|i| format!("old line {}\n", i)).collect();
        let new_content: String = (0..300).map(|i| format!("new line {}\n", i)).collect();

        let changes = vec![("large_file.rs".to_string(), old_content, new_content)];

        let check = renderer.check_suppression(&changes);
        assert!(check.should_suppress);
        assert!(check.reason.is_some());
    }

    #[test]
    fn test_render_suppressed_summary() {
        let renderer = DiffChatRenderer::new(true, 3, false);
        let file_stats = vec![
            FileChangeStats {
                path: "file1.rs".to_string(),
                additions: 20,
                deletions: 10,
            },
            FileChangeStats {
                path: "file2.rs".to_string(),
                additions: 30,
                deletions: 20,
            },
        ];
        let check =
            DiffSuppressionCheck::suppressed("Test reason".to_string(), 5, 100, 50, 30, file_stats);

        let output = renderer.render_suppressed_summary(&check);
        assert!(output.contains(diff_constants::SUPPRESSION_MESSAGE));
        assert!(output.contains("5 file(s) changed"));
        assert!(output.contains("50")); // additions
        assert!(output.contains("30")); // deletions
        assert!(output.contains("Test reason"));
        assert!(output.contains("file1.rs"));
        assert!(output.contains("file2.rs"));
    }

    #[test]
    fn test_render_multiple_changes_with_suppression() {
        let renderer = DiffChatRenderer::new(true, 3, false);

        // Create enough changes to trigger suppression
        let mut changes = Vec::new();
        for i in 0..(diff_constants::MAX_INLINE_DIFF_FILES + 2) {
            changes.push((
                format!("file{}.rs", i),
                "old".to_string(),
                "new".to_string(),
            ));
        }

        let output = renderer.render_multiple_changes(changes);
        assert!(output.contains(diff_constants::SUPPRESSION_MESSAGE));
    }

    #[test]
    fn test_render_multiple_changes_without_suppression() {
        let renderer = DiffChatRenderer::new(true, 3, false);
        let changes = vec![
            ("file1.rs".to_string(), "a".to_string(), "b".to_string()),
            ("file2.rs".to_string(), "c".to_string(), "d".to_string()),
        ];

        let output = renderer.render_multiple_changes(changes);
        assert!(output.contains("Multiple File Changes"));
        assert!(!output.contains(diff_constants::SUPPRESSION_MESSAGE));
    }
}
