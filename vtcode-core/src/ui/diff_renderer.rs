use crate::config::constants::diff as diff_constants;
use crate::ui::git_config::GitColorConfig;
use crate::utils::style_helpers::style_from_color_name;
use anstyle::{Reset, Style};
use std::fmt::Write as _;
use std::path::Path;
use vtcode_commons::styling::DiffColorPalette;

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

fn strip_diff_background(style: Style) -> Style {
    style.bg_color(None)
}

impl GitDiffPalette {
    fn new(use_colors: bool) -> Self {
        // Use consolidated DiffColorPalette with standard ANSI colors (no bold)
        let palette = DiffColorPalette::default();
        let added = palette.added_style();
        let removed = palette.removed_style();
        let header = palette.header_style();

        Self {
            bullet: if use_colors {
                style_from_color_name("cyan")
            } else {
                Style::new()
            }, // For summary bullets
            label: Style::new(), // For summary labels
            path: Style::new(),  // For file paths
            stat_added: strip_diff_background(added),
            stat_removed: strip_diff_background(removed),
            line_added: strip_diff_background(added),
            line_removed: strip_diff_background(removed),
            line_context: if use_colors {
                Style::new().dimmed()
            } else {
                Style::new()
            },
            line_header: strip_diff_background(header),
            line_number: if use_colors {
                style_from_color_name("cyan")
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

        // Use consolidated DiffColorPalette - ignore git config theme for consistency
        let palette = DiffColorPalette::default();
        let added = palette.added_style();
        let removed = palette.removed_style();
        let header = palette.header_style();

        Self {
            bullet: style_from_color_name("cyan"),
            label: Style::new(),
            path: Style::new(),
            stat_added: strip_diff_background(added),
            stat_removed: strip_diff_background(removed),
            line_added: strip_diff_background(added),
            line_removed: strip_diff_background(removed),
            line_context: strip_diff_background(config.diff_context),
            line_header: strip_diff_background(header),
            line_number: style_from_color_name("cyan"),
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

/// Cached diff entry to avoid recomputation
#[derive(Debug)]
struct DiffCacheEntry {
    diff: FileDiff,
}

/// Result of suppression check with optional cached diffs
pub struct SuppressionResult {
    pub check: DiffSuppressionCheck,
    /// Cached diffs if not suppressed (avoids recomputation)
    cached_diffs: Option<Vec<DiffCacheEntry>>,
}

impl SuppressionResult {
    fn suppressed(check: DiffSuppressionCheck) -> Self {
        Self {
            check,
            cached_diffs: None,
        }
    }

    fn not_suppressed(check: DiffSuppressionCheck, diffs: Vec<DiffCacheEntry>) -> Self {
        Self {
            check,
            cached_diffs: Some(diffs),
        }
    }
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
    #[allow(dead_code)]
    pub(crate) palette: GitDiffPalette,
    // Pre-rendered ANSI codes for performance (cached)
    cached_styles: CachedStyles,
}

/// Pre-rendered ANSI escape codes to avoid repeated calls to style.render()
struct CachedStyles {
    bullet: String,
    label: String,
    path: String,
    stat_added: String,
    stat_removed: String,
    line_added: String,
    line_removed: String,
    line_context: String,
    line_header: String,
    line_number: String,
    reset: String,
}

impl CachedStyles {
    fn new(palette: &GitDiffPalette, use_colors: bool) -> Self {
        if !use_colors {
            return Self {
                bullet: String::new(),
                label: String::new(),
                path: String::new(),
                stat_added: String::new(),
                stat_removed: String::new(),
                line_added: String::new(),
                line_removed: String::new(),
                line_context: String::new(),
                line_header: String::new(),
                line_number: String::new(),
                reset: String::new(),
            };
        }

        let reset = format!("{}", Reset.render());
        Self {
            bullet: format!("{}", palette.bullet.render()),
            label: format!("{}", palette.label.render()),
            path: format!("{}", palette.path.render()),
            stat_added: format!("{}", palette.stat_added.render()),
            stat_removed: format!("{}", palette.stat_removed.render()),
            line_added: format!("{}", palette.line_added.render()),
            line_removed: format!("{}", palette.line_removed.render()),
            line_context: format!("{}", palette.line_context.render()),
            line_header: format!("{}", palette.line_header.render()),
            line_number: format!("{}", palette.line_number.render()),
            reset,
        }
    }
}

impl DiffRenderer {
    pub fn new(show_line_numbers: bool, context_lines: usize, use_colors: bool) -> Self {
        let palette = GitDiffPalette::new(use_colors);
        let cached_styles = CachedStyles::new(&palette, use_colors);
        Self {
            show_line_numbers,
            context_lines,
            use_colors,
            palette,
            cached_styles,
        }
    }

    /// Create renderer with colors from Git config
    pub fn with_git_config(
        show_line_numbers: bool,
        context_lines: usize,
        use_colors: bool,
        config: &GitColorConfig,
    ) -> Self {
        let palette = GitDiffPalette::from_git_config(config, use_colors);
        let cached_styles = CachedStyles::new(&palette, use_colors);
        Self {
            show_line_numbers,
            context_lines,
            use_colors,
            palette,
            cached_styles,
        }
    }

    pub fn render_diff(&self, diff: &FileDiff) -> String {
        // Pre-allocate buffer: header (~100 chars) + lines (~80 chars each)
        let estimated_size = 100 + diff.lines.len() * 80;
        let mut output = String::with_capacity(estimated_size);

        // File header with edit indicator
        output.push_str("─ ");
        output.push_str(&self.render_summary(diff));
        output.push('\n');

        for line in &diff.lines {
            self.render_line_into(&mut output, line);
            output.push('\n');
        }

        output
    }

    fn render_summary(&self, diff: &FileDiff) -> String {
        if !self.use_colors {
            return format!(
                "▸ Edit {} (+{} -{})",
                diff.file_path, diff.stats.additions, diff.stats.deletions
            );
        }

        // Pre-allocate: bullet(4) + label(4) + path + stats + resets + separators
        let estimated_size = 50 + diff.file_path.len();
        let mut output = String::with_capacity(estimated_size);

        // Bullet: "▸" (3 bytes UTF-8)
        output.push_str(&self.cached_styles.bullet);
        output.push('▸');
        output.push_str(&self.cached_styles.reset);
        output.push(' ');

        // Label: "Edit"
        output.push_str(&self.cached_styles.label);
        output.push_str("Edit");
        output.push_str(&self.cached_styles.reset);
        output.push(' ');

        // Path
        output.push_str(&self.cached_styles.path);
        output.push_str(&diff.file_path);
        output.push_str(&self.cached_styles.reset);
        output.push(' ');

        // Opening paren for stats
        output.push('(');

        // Additions
        output.push_str(&self.cached_styles.stat_added);
        output.push('+');
        use std::fmt::Write as FmtWrite;
        let _ = write!(output, "{}", diff.stats.additions);
        output.push_str(&self.cached_styles.reset);
        output.push(' ');

        // Deletions
        output.push_str(&self.cached_styles.stat_removed);
        output.push('-');
        let _ = write!(output, "{}", diff.stats.deletions);
        output.push_str(&self.cached_styles.reset);
        output.push(')');

        output
    }

    /// Render line directly into buffer to avoid allocation
    fn render_line_into(&self, output: &mut String, line: &DiffLine) {
        let (style_code, prefix, line_number) = match line.line_type {
            DiffLineType::Added => (&self.cached_styles.line_added, "+", line.line_number_new),
            DiffLineType::Removed => (&self.cached_styles.line_removed, "-", line.line_number_old),
            DiffLineType::Context => (
                &self.cached_styles.line_context,
                " ",
                line.line_number_new.or(line.line_number_old),
            ),
            DiffLineType::Header => (&self.cached_styles.line_header, "", None),
        };

        if self.show_line_numbers {
            if let Some(n) = line_number {
                output.push_str(&self.cached_styles.line_number);
                // Format line number right-aligned in 4 chars without heap allocation
                if n < 10 {
                    output.push_str("   ");
                } else if n < 100 {
                    output.push_str("  ");
                } else if n < 1000 {
                    output.push(' ');
                }
                use std::fmt::Write as FmtWrite;
                let _ = write!(output, "{}", n);
                output.push_str(&self.cached_styles.reset);
            } else {
                output.push_str("    ");
            }
            output.push(' ');
        }

        match line.line_type {
            DiffLineType::Header => {
                output.push_str(style_code);
                output.push_str(&line.content);
                output.push_str(&self.cached_styles.reset);
            }
            _ => {
                output.push_str(style_code);
                output.push_str(prefix);
                if !line.content.is_empty() {
                    output.push(' ');
                    output.push_str(&line.content);
                }
                output.push_str(&self.cached_styles.reset);
            }
        }
    }

    #[allow(dead_code)]
    pub(crate) fn paint(&self, style: &Style, text: &str) -> String {
        if self.use_colors {
            // CRITICAL: Apply style and reset without including newlines in the styled block
            // This ensures Reset appears before any line terminators, preventing color bleed
            format!("{}{}{}", style.render(), text, Reset.render())
        } else {
            text.to_owned()
        }
    }

    pub fn generate_diff(&self, old_content: &str, new_content: &str, file_path: &str) -> FileDiff {
        let bundle = crate::utils::diff::compute_diff_with_theme(
            old_content,
            new_content,
            crate::utils::diff::DiffOptions {
                context_lines: self.context_lines,
                old_label: None,
                new_label: None,
                missing_newline_hint: false,
            },
        );

        let mut lines = Vec::new();
        let mut additions = 0;
        let mut deletions = 0;

        for hunk in &bundle.hunks {
            lines.push(DiffLine {
                line_type: DiffLineType::Header,
                content: format!("@@ -{} +{} @@", hunk.old_start, hunk.new_start),
                line_number_old: None,
                line_number_new: None,
            });
            for line in &hunk.lines {
                let line_type = match line.kind {
                    crate::utils::diff::DiffLineKind::Addition => {
                        additions += 1;
                        DiffLineType::Added
                    }
                    crate::utils::diff::DiffLineKind::Deletion => {
                        deletions += 1;
                        DiffLineType::Removed
                    }
                    crate::utils::diff::DiffLineKind::Context => DiffLineType::Context,
                };

                lines.push(DiffLine {
                    line_type,
                    content: line.text.trim_end_matches('\n').to_string(),
                    line_number_old: line.old_line,
                    line_number_new: line.new_line,
                });
            }
        }

        let changes = additions + deletions;

        FileDiff {
            file_path: file_path.to_owned(),
            lines,
            stats: DiffStats {
                additions,
                deletions,
                changes,
            },
        }
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
        // Check suppression and get cached diffs if not suppressed
        let result = self.check_suppression_with_cache(&changes);

        if result.check.should_suppress {
            return self.render_suppressed_summary(&result.check);
        }

        // Pre-allocate output buffer with estimated size
        let estimated_size = changes.len() * 512; // Rough estimate per file
        let mut output = String::with_capacity(estimated_size);

        let _ = write!(
            output,
            "\nMultiple File Changes ({} files)\n",
            changes.len()
        );
        output.push_str(&"═".repeat(60));
        output.push_str("\n\n");

        // Use cached diffs to avoid recomputation
        if let Some(cached_diffs) = result.cached_diffs {
            for entry in cached_diffs {
                output.push_str(&self.diff_renderer.render_diff(&entry.diff));
            }
        }

        output
    }

    /// Check if diffs should be suppressed based on size/count thresholds
    /// Returns cached diffs if not suppressed to avoid recomputation
    fn check_suppression_with_cache(
        &self,
        changes: &[(String, String, String)],
    ) -> SuppressionResult {
        let file_count = changes.len();

        // Early termination: check file count first (cheapest check)
        if file_count > diff_constants::MAX_INLINE_DIFF_FILES {
            // Still need to compute stats for summary, but can use lightweight estimation
            let (file_stats, total_additions, total_deletions) =
                self.estimate_stats_lightweight(changes);
            return SuppressionResult::suppressed(DiffSuppressionCheck::suppressed(
                format!(
                    "Too many files changed ({} files, max {})",
                    file_count,
                    diff_constants::MAX_INLINE_DIFF_FILES
                ),
                file_count,
                0, // Lines not computed for performance
                total_additions,
                total_deletions,
                file_stats,
            ));
        }

        let mut total_lines = 0usize;
        let mut total_additions = 0usize;
        let mut total_deletions = 0usize;
        let mut file_stats = Vec::with_capacity(file_count);
        let mut cached_diffs = Vec::with_capacity(file_count);
        let mut suppression_reason: Option<String> = None;

        for (file_path, old_content, new_content) in changes {
            let diff = self
                .diff_renderer
                .generate_diff(old_content, new_content, file_path);

            total_lines += diff.lines.len();
            total_additions += diff.stats.additions;
            total_deletions += diff.stats.deletions;

            file_stats.push(FileChangeStats {
                path: file_path.clone(),
                additions: diff.stats.additions,
                deletions: diff.stats.deletions,
            });

            // Check thresholds with early termination
            if suppression_reason.is_none() {
                if diff.stats.changes > diff_constants::MAX_SINGLE_FILE_CHANGES {
                    suppression_reason = Some(format!(
                        "Single file exceeds change limit (max {} changes per file)",
                        diff_constants::MAX_SINGLE_FILE_CHANGES
                    ));
                } else if total_lines > diff_constants::MAX_TOTAL_DIFF_LINES {
                    suppression_reason = Some(format!(
                        "Too many diff lines ({} lines, max {})",
                        total_lines,
                        diff_constants::MAX_TOTAL_DIFF_LINES
                    ));
                }
            }

            // Cache diff for potential reuse
            cached_diffs.push(DiffCacheEntry { diff });
        }

        if let Some(reason) = suppression_reason {
            SuppressionResult::suppressed(DiffSuppressionCheck::suppressed(
                reason,
                file_count,
                total_lines,
                total_additions,
                total_deletions,
                file_stats,
            ))
        } else {
            SuppressionResult::not_suppressed(
                DiffSuppressionCheck::no_suppression(
                    file_count,
                    total_lines,
                    total_additions,
                    total_deletions,
                    file_stats,
                ),
                cached_diffs,
            )
        }
    }

    /// Lightweight stats estimation without full diff generation
    fn estimate_stats_lightweight(
        &self,
        changes: &[(String, String, String)],
    ) -> (Vec<FileChangeStats>, usize, usize) {
        let mut file_stats = Vec::with_capacity(changes.len());
        let mut total_additions = 0usize;
        let mut total_deletions = 0usize;

        for (file_path, old_content, new_content) in changes {
            // Estimate changes by line count difference (much faster than full diff)
            let old_lines = old_content.lines().count();
            let new_lines = new_content.lines().count();
            let (additions, deletions) = if new_lines >= old_lines {
                (new_lines - old_lines, 0)
            } else {
                (0, old_lines - new_lines)
            };

            total_additions += additions;
            total_deletions += deletions;

            file_stats.push(FileChangeStats {
                path: file_path.clone(),
                additions,
                deletions,
            });
        }

        (file_stats, total_additions, total_deletions)
    }

    /// Public API for checking suppression (without cache access)
    pub fn check_suppression(&self, changes: &[(String, String, String)]) -> DiffSuppressionCheck {
        self.check_suppression_with_cache(changes).check
    }

    /// Render a summary when diffs are suppressed
    pub fn render_suppressed_summary(&self, check: &DiffSuppressionCheck) -> String {
        // Pre-estimate size: header + summary + file list with colors
        let estimated_size = 256 + (check.file_stats.len() * 100);
        let mut output = String::with_capacity(estimated_size);

        // Header with warning indicator
        output.push_str("\n[WARN] ");
        output.push_str(diff_constants::SUPPRESSION_MESSAGE);
        output.push_str("\n\n");

        // Overall summary with colored stats
        output.push_str("Summary: ");
        use std::fmt::Write as FmtWrite;
        let _ = write!(output, "{} file(s) changed", check.file_count);

        if check.total_additions > 0 || check.total_deletions > 0 && self.diff_renderer.use_colors {
            output.push_str(" (");

            // Additions stat
            if check.total_additions > 0 {
                output.push_str(&self.diff_renderer.cached_styles.stat_added);
                output.push('+');
                let _ = write!(output, "{}", check.total_additions);
                output.push_str(&self.diff_renderer.cached_styles.reset);
            }

            if check.total_additions > 0 && check.total_deletions > 0 {
                output.push(' ');
            }

            // Deletions stat
            if check.total_deletions > 0 {
                output.push_str(&self.diff_renderer.cached_styles.stat_removed);
                output.push('-');
                let _ = write!(output, "{}", check.total_deletions);
                output.push_str(&self.diff_renderer.cached_styles.reset);
            }

            output.push(')');
        }
        output.push('\n');

        // List changed files with individual stats
        if !check.file_stats.is_empty() {
            output.push_str("\nChanged files:\n");
            let max_files_to_show = diff_constants::MAX_FILES_IN_SUMMARY;
            for (i, stat) in check.file_stats.iter().enumerate() {
                if i >= max_files_to_show {
                    let remaining = check.file_stats.len() - max_files_to_show;
                    let _ = writeln!(output, "  ... and {} more file(s)", remaining);
                    break;
                }

                output.push_str("  • ");
                output.push_str(&stat.path);
                output.push_str(" (");

                if self.diff_renderer.use_colors {
                    // Additions
                    if stat.additions > 0 {
                        output.push_str(&self.diff_renderer.cached_styles.stat_added);
                        output.push('+');
                        let _ = write!(output, "{}", stat.additions);
                        output.push_str(&self.diff_renderer.cached_styles.reset);
                    }

                    if stat.additions > 0 && stat.deletions > 0 {
                        output.push(' ');
                    }

                    // Deletions
                    if stat.deletions > 0 {
                        output.push_str(&self.diff_renderer.cached_styles.stat_removed);
                        output.push('-');
                        let _ = write!(output, "{}", stat.deletions);
                        output.push_str(&self.diff_renderer.cached_styles.reset);
                    }
                } else {
                    output.push('+');
                    let _ = write!(output, "{}", stat.additions);
                    output.push(' ');
                    output.push('-');
                    let _ = write!(output, "{}", stat.deletions);
                }

                output.push_str(")\n");
            }
        }

        // Show reason in dimmed text
        if let Some(reason) = &check.reason {
            let _ = writeln!(output, "\nReason: {}", reason);
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
        let _ = writeln!(summary, "└─ {} file(s) affected", files_affected);

        if success {
            summary.push_str("   Operation completed successfully\n");
        } else {
            summary.push_str("   Operation completed with errors\n");
        }

        summary
    }
}

pub fn generate_unified_diff(old_content: &str, new_content: &str, filename: &str) -> String {
    let old_label = format!("a/{}", filename);
    let new_label = format!("b/{}", filename);
    let options = crate::utils::diff::DiffOptions {
        context_lines: 3,
        old_label: Some(&old_label),
        new_label: Some(&new_label),
        missing_newline_hint: false,
    };
    crate::utils::diff::format_unified_diff(old_content, new_content, options)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_suppression_check_no_suppression() {
        let renderer = DiffChatRenderer::new(true, 3, false);
        let changes = vec![
            ("file1.rs".to_owned(), "old1".to_owned(), "new1".to_owned()),
            ("file2.rs".to_owned(), "old2".to_owned(), "new2".to_owned()),
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

        let changes = vec![("large_file.rs".to_owned(), old_content, new_content)];

        let check = renderer.check_suppression(&changes);
        assert!(check.should_suppress);
        assert!(check.reason.is_some());
    }

    #[test]
    fn test_render_suppressed_summary() {
        let renderer = DiffChatRenderer::new(true, 3, false);
        let file_stats = vec![
            FileChangeStats {
                path: "file1.rs".to_owned(),
                additions: 20,
                deletions: 10,
            },
            FileChangeStats {
                path: "file2.rs".to_owned(),
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

    #[test]
    fn test_render_file_change_includes_summary_and_hunk_header() {
        let renderer = DiffChatRenderer::new(true, 3, false);
        let output = renderer.render_file_change(Path::new("file.rs"), "a\nb\n", "a\nc\n");

        assert!(output.contains("▸ Edit file.rs (+1 -1)"));
        assert!(output.contains("@@ -1 +1 @@"));
    }
}
