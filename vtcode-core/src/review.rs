use anyhow::{Result, bail};
use std::fmt::Write as _;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewTarget {
    CurrentDiff,
    LastDiff,
    Files(Vec<String>),
    Custom(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewSpec {
    pub target: ReviewTarget,
    pub style: Option<String>,
}

pub fn build_review_spec(
    last_diff: bool,
    target: Option<String>,
    files: Vec<String>,
    style: Option<String>,
) -> Result<ReviewSpec> {
    if last_diff && target.is_some() {
        bail!("--last-diff cannot be combined with --target");
    }
    if last_diff && !files.is_empty() {
        bail!("--last-diff cannot be combined with explicit files");
    }
    if target.is_some() && !files.is_empty() {
        bail!("--target cannot be combined with explicit files");
    }

    let style = style
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    let target = if last_diff {
        ReviewTarget::LastDiff
    } else if let Some(target) = target {
        let target = target.trim();
        if target.is_empty() {
            bail!("--target cannot be empty");
        }
        ReviewTarget::Custom(target.to_string())
    } else if !files.is_empty() {
        let files = files
            .into_iter()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>();
        if files.is_empty() {
            bail!("review files cannot be empty");
        }
        ReviewTarget::Files(files)
    } else {
        ReviewTarget::CurrentDiff
    };

    Ok(ReviewSpec { target, style })
}

pub fn build_review_prompt(spec: &ReviewSpec) -> String {
    let mut prompt = String::new();
    prompt.push_str("Perform a code review.\n");
    match &spec.target {
        ReviewTarget::CurrentDiff => {
            prompt.push_str("Target: review the current git diff in the workspace.\n");
        }
        ReviewTarget::LastDiff => {
            prompt.push_str("Target: review the last committed git diff.\n");
        }
        ReviewTarget::Files(files) => {
            prompt.push_str("Target: review these files:\n");
            for file in files {
                let _ = writeln!(prompt, "- {}", file);
            }
        }
        ReviewTarget::Custom(target) => {
            let _ = writeln!(prompt, "Target: review `{}`.", target);
        }
    }

    if let Some(style) = &spec.style {
        let _ = writeln!(prompt, "Style: {}.", style);
    }

    prompt.push_str(
        "\nRequirements:\n\
         - Review only. Do not modify files or run mutating commands.\n\
         - Focus on bugs, regressions, security issues, performance issues, and missing tests.\n\
         - Present findings first, ordered by severity.\n\
         - Include concrete file paths and line numbers when possible.\n\
         - If there are no findings, say that explicitly.\n",
    );

    prompt
}

#[cfg(test)]
mod tests {
    use super::{ReviewSpec, ReviewTarget, build_review_prompt, build_review_spec};

    #[test]
    fn review_spec_defaults_to_current_diff() {
        let spec = build_review_spec(false, None, Vec::new(), None).expect("spec");
        assert_eq!(
            spec,
            ReviewSpec {
                target: ReviewTarget::CurrentDiff,
                style: None,
            }
        );
    }

    #[test]
    fn review_spec_rejects_conflicting_target_selectors() {
        let err = build_review_spec(true, Some("HEAD~1..HEAD".to_string()), Vec::new(), None)
            .expect_err("conflicting selectors should fail");

        assert!(err.to_string().contains("--last-diff"));
    }

    #[test]
    fn review_prompt_includes_files_and_style() {
        let spec = build_review_spec(
            false,
            None,
            vec!["src/main.rs".to_string(), "src/lib.rs".to_string()],
            Some("security".to_string()),
        )
        .expect("spec");
        let prompt = build_review_prompt(&spec);

        assert!(prompt.contains("Target: review these files"));
        assert!(prompt.contains("- src/main.rs"));
        assert!(prompt.contains("Style: security."));
        assert!(prompt.contains("Review only."));
    }
}
