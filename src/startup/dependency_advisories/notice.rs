use anyhow::Result;
use vtcode_core::tools::ast_grep_binary::AST_GREP_INSTALL_COMMAND;
use vtcode_core::tools::ripgrep_binary::RIPGREP_INSTALL_COMMAND;
use vtcode_core::tools::{AstGrepStatus, RipgrepStatus};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_core::utils::dot_config::DotConfig;
use vtcode_tui::InlineHeaderHighlight;

const SEARCH_TOOLS_INSTALLER_FLAG: &str = "--with-search-tools";
const SEARCH_TOOLS_INSTALL_COMMAND: &str = "vtcode dependencies install search-tools";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OptionalDependency {
    Ripgrep,
    AstGrep,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct OptionalSearchToolsNotice {
    missing: Vec<OptionalDependency>,
}

impl OptionalSearchToolsNotice {
    pub(super) fn from_snapshot(
        config: &DotConfig,
        ripgrep_status: RipgrepStatus,
        ast_grep_status: AstGrepStatus,
    ) -> Option<Self> {
        let mut missing = Vec::with_capacity(2);

        if matches!(ripgrep_status, RipgrepStatus::NotFound)
            && !config.dependency_notices.ripgrep_missing_notice_shown
        {
            missing.push(OptionalDependency::Ripgrep);
        }

        if matches!(ast_grep_status, AstGrepStatus::NotFound)
            && !config.dependency_notices.ast_grep_missing_notice_shown
        {
            missing.push(OptionalDependency::AstGrep);
        }

        if missing.is_empty() {
            None
        } else {
            Some(Self { missing })
        }
    }

    pub(super) fn apply_to_config(&self, config: &mut DotConfig) {
        for dependency in &self.missing {
            match dependency {
                OptionalDependency::Ripgrep => {
                    config.dependency_notices.ripgrep_missing_notice_shown = true;
                }
                OptionalDependency::AstGrep => {
                    config.dependency_notices.ast_grep_missing_notice_shown = true;
                }
            }
        }
    }

    pub(super) fn to_highlight(&self) -> InlineHeaderHighlight {
        InlineHeaderHighlight {
            title: "Optional Search Tools".to_string(),
            lines: self.lines(),
        }
    }

    pub(super) fn render(&self, renderer: &mut AnsiRenderer) -> Result<()> {
        renderer.line(
            MessageStyle::Status,
            "Optional search tools are available. VT Code still works without them.",
        )?;
        for line in self.lines() {
            renderer.line(MessageStyle::Info, &line)?;
        }
        renderer.line(MessageStyle::Info, "")?;
        Ok(())
    }

    fn lines(&self) -> Vec<String> {
        let mut lines = Vec::with_capacity(self.missing.len() + 2);

        if self.missing.contains(&OptionalDependency::Ripgrep) {
            lines.push(format!(
                "ripgrep (`rg`): faster text search and file discovery -> `{RIPGREP_INSTALL_COMMAND}`"
            ));
        }

        if self.missing.contains(&OptionalDependency::AstGrep) {
            lines.push(format!(
                "ast-grep: syntax-aware structural search -> `{AST_GREP_INSTALL_COMMAND}`"
            ));
        }

        lines.push(format!(
            "One-step bundle: run `{SEARCH_TOOLS_INSTALL_COMMAND}` after install."
        ));
        lines.push(format!(
            "Native installer bundle: use `{SEARCH_TOOLS_INSTALLER_FLAG}` during curl installs."
        ));

        lines
    }
}

#[cfg(test)]
mod tests {
    use super::{OptionalDependency, OptionalSearchToolsNotice};
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

        assert_eq!(
            notice.missing,
            vec![OptionalDependency::Ripgrep, OptionalDependency::AstGrep]
        );
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

        assert_eq!(notice.missing, vec![OptionalDependency::AstGrep]);
    }

    #[test]
    fn applies_notice_to_config() {
        let notice = OptionalSearchToolsNotice {
            missing: vec![OptionalDependency::Ripgrep, OptionalDependency::AstGrep],
        };
        let mut config = DotConfig::default();

        notice.apply_to_config(&mut config);

        assert!(config.dependency_notices.ripgrep_missing_notice_shown);
        assert!(config.dependency_notices.ast_grep_missing_notice_shown);
    }

    #[test]
    fn renders_highlight_lines() {
        let notice = OptionalSearchToolsNotice {
            missing: vec![OptionalDependency::Ripgrep],
        };
        let highlight = notice.to_highlight();

        assert_eq!(highlight.title, "Optional Search Tools");
        assert!(highlight.lines[0].contains("ripgrep"));
        assert!(highlight.lines[1].contains("search-tools"));
        assert!(highlight.lines[2].contains("--with-search-tools"));
    }
}
