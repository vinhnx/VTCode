use anyhow::Result;
use vtcode_core::tools::ast_grep_binary::AST_GREP_INSTALL_COMMAND;
use vtcode_core::tools::ripgrep_binary::RIPGREP_INSTALL_COMMAND;
use vtcode_core::tools::{AstGrepStatus, RipgrepStatus};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_core::utils::dot_config::DotConfig;

const SEARCH_TOOLS_INSTALLER_FLAG: &str = "--with-search-tools";
const SEARCH_TOOLS_INSTALL_COMMAND: &str = "vtcode dependencies install search-tools";
const REASON_PREVIEW_LIMIT: usize = 120;

#[derive(Debug, Clone, PartialEq, Eq)]
enum DependencyIssue {
    Missing,
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OptionalSearchToolsNotice {
    ripgrep: Option<DependencyIssue>,
    ast_grep: Option<DependencyIssue>,
}

impl OptionalSearchToolsNotice {
    pub(crate) fn from_snapshot(
        config: &DotConfig,
        ripgrep_status: RipgrepStatus,
        ast_grep_status: AstGrepStatus,
    ) -> Option<Self> {
        let ripgrep = match ripgrep_status {
            RipgrepStatus::Available { .. } => None,
            RipgrepStatus::NotFound if config.dependency_notices.ripgrep_missing_notice_shown => {
                None
            }
            RipgrepStatus::NotFound => Some(DependencyIssue::Missing),
            RipgrepStatus::Error { reason } => Some(DependencyIssue::Error(reason)),
        };
        let ast_grep = match ast_grep_status {
            AstGrepStatus::Available { .. } => None,
            AstGrepStatus::NotFound if config.dependency_notices.ast_grep_missing_notice_shown => {
                None
            }
            AstGrepStatus::NotFound => Some(DependencyIssue::Missing),
            AstGrepStatus::Error { reason } => Some(DependencyIssue::Error(reason)),
        };

        if ripgrep.is_none() && ast_grep.is_none() {
            None
        } else {
            Some(Self { ripgrep, ast_grep })
        }
    }

    pub(crate) fn apply_to_config(&self, config: &mut DotConfig) {
        if matches!(self.ripgrep, Some(DependencyIssue::Missing)) {
            config.dependency_notices.ripgrep_missing_notice_shown = true;
        }
        if matches!(self.ast_grep, Some(DependencyIssue::Missing)) {
            config.dependency_notices.ast_grep_missing_notice_shown = true;
        }
    }

    pub(crate) fn render(&self, renderer: &mut AnsiRenderer) -> Result<()> {
        let headline = if self.ripgrep.is_some() && self.ast_grep.is_some() {
            "Search tools bundle is unavailable. VT Code will fall back where possible."
        } else {
            "Search tools bundle is partially unavailable. VT Code will fall back where possible."
        };
        renderer.line(MessageStyle::Status, headline)?;
        for line in self.lines() {
            renderer.line(MessageStyle::Info, &line)?;
        }
        renderer.line(MessageStyle::Info, "")?;
        Ok(())
    }

    fn lines(&self) -> Vec<String> {
        let mut lines = Vec::with_capacity(4);

        if let Some(issue) = &self.ripgrep {
            lines.push(match issue {
                DependencyIssue::Missing => format!(
                    "ripgrep (`rg`) missing. VT Code falls back to built-in text search -> `{RIPGREP_INSTALL_COMMAND}`"
                ),
                DependencyIssue::Error(reason) => format!(
                    "ripgrep (`rg`) failed to verify: {}. Text search may fall back -> `{RIPGREP_INSTALL_COMMAND}`",
                    compact_reason(reason)
                ),
            });
        }

        if let Some(issue) = &self.ast_grep {
            lines.push(match issue {
                DependencyIssue::Missing => format!(
                    "ast-grep missing. Structural search is unavailable -> `{AST_GREP_INSTALL_COMMAND}`"
                ),
                DependencyIssue::Error(reason) => format!(
                    "ast-grep failed to verify: {}. Structural search may be unavailable -> `{AST_GREP_INSTALL_COMMAND}`",
                    compact_reason(reason)
                ),
            });
        }

        lines.push(format!(
            "Install both with `{SEARCH_TOOLS_INSTALL_COMMAND}`."
        ));
        lines.push(format!(
            "Native installer bundle: use `{SEARCH_TOOLS_INSTALLER_FLAG}` during curl installs."
        ));

        lines
    }
}

fn compact_reason(reason: &str) -> String {
    let compact = reason.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = compact.chars();
    let preview: String = chars.by_ref().take(REASON_PREVIEW_LIMIT).collect();
    if chars.next().is_some() {
        format!("{preview}...")
    } else {
        compact
    }
}

#[cfg(test)]
mod tests {
    use super::{DependencyIssue, OptionalSearchToolsNotice, compact_reason};
    use vtcode_core::utils::dot_config::DotConfig;

    use vtcode_core::tools::{AstGrepStatus, RipgrepStatus};

    #[test]
    fn builds_notice_for_unseen_missing_dependencies() {
        let notice = OptionalSearchToolsNotice::from_snapshot(
            &DotConfig::default(),
            RipgrepStatus::NotFound,
            AstGrepStatus::NotFound,
        )
        .expect("missing dependencies should create a notice");

        assert_eq!(notice.ripgrep, Some(DependencyIssue::Missing));
        assert_eq!(notice.ast_grep, Some(DependencyIssue::Missing));
    }

    #[test]
    fn skips_dependencies_already_shown() {
        let mut config = DotConfig::default();
        config.dependency_notices.ripgrep_missing_notice_shown = true;

        let notice = OptionalSearchToolsNotice::from_snapshot(
            &config,
            RipgrepStatus::NotFound,
            AstGrepStatus::NotFound,
        )
        .expect("ast-grep notice should remain");

        assert_eq!(notice.ripgrep, None);
        assert_eq!(notice.ast_grep, Some(DependencyIssue::Missing));
    }

    #[test]
    fn applies_notice_to_config() {
        let notice = OptionalSearchToolsNotice {
            ripgrep: Some(DependencyIssue::Missing),
            ast_grep: Some(DependencyIssue::Error("broken".to_string())),
        };
        let mut config = DotConfig::default();

        notice.apply_to_config(&mut config);

        assert!(config.dependency_notices.ripgrep_missing_notice_shown);
        assert!(!config.dependency_notices.ast_grep_missing_notice_shown);
    }

    #[test]
    fn lines_include_bundle_install_guidance() {
        let notice = OptionalSearchToolsNotice {
            ripgrep: Some(DependencyIssue::Missing),
            ast_grep: None,
        };
        let lines = notice.lines();

        assert!(lines[0].contains("ripgrep"));
        assert!(lines[1].contains("search-tools"));
        assert!(lines[2].contains("--with-search-tools"));
    }

    #[test]
    fn captures_error_reasons_without_whitespace_noise() {
        let notice = OptionalSearchToolsNotice {
            ripgrep: Some(DependencyIssue::Error("bad\n  install".to_string())),
            ast_grep: None,
        };

        assert!(notice.lines()[0].contains("bad install"));
    }

    #[test]
    fn compact_reason_truncates_long_reasons() {
        let reason = "x".repeat(160);

        let compact = compact_reason(&reason);

        assert!(compact.ends_with("..."));
        assert!(compact.len() < reason.len());
    }
}
