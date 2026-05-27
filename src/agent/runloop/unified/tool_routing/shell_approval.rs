use serde_json::Value;

use crate::agent::runloop::unified::tool_summary::{describe_tool_action, humanize_tool_name};

use super::permission_prompt::{
    extract_shell_approval_command_prefix_words, extract_shell_approval_command_words,
    extract_shell_command_text, extract_shell_permission_scope_signature,
    extract_shell_persistent_approval_prefix_rule, render_shell_approval_command_words,
    render_shell_persistent_approval_prefix_entry,
};

/// Secondary learning key for shell-command "families" (e.g. all safe
/// `find <subdir> ...` invocations share one key) so the auto-approve
/// classifier promotes equivalent-pattern calls after the user has approved a
/// few variants. Only attached for command shapes that are demonstrably safe
/// regardless of remaining flags — see [`learned_shell_pattern`].
#[derive(Debug, Clone)]
pub(super) struct LearnedPattern {
    pub key: String,
    pub label: String,
}

#[derive(Debug, Clone)]
pub(super) struct ApprovalLearningTarget {
    pub approval_key: String,
    pub display_label: String,
    pub pattern: Option<LearnedPattern>,
}

impl ApprovalLearningTarget {
    pub fn new(approval_key: String, display_label: String) -> Self {
        Self {
            approval_key,
            display_label,
            pattern: None,
        }
    }

    pub fn with_pattern(mut self, pattern: Option<LearnedPattern>) -> Self {
        self.pattern = pattern;
        self
    }

    /// Iterate over every (key, label) pair this target contributes to
    /// learning: the exact invocation first, then the optional family pattern.
    pub fn iter_keys(&self) -> impl Iterator<Item = (&str, &str)> {
        std::iter::once((self.approval_key.as_str(), self.display_label.as_str())).chain(
            self.pattern
                .iter()
                .map(|p| (p.key.as_str(), p.label.as_str())),
        )
    }
}

#[derive(Debug, Clone)]
pub(super) struct ToolDisplayLabels {
    pub prompt_label: String,
    pub learning_label: String,
}

#[derive(Debug, Clone)]
pub(super) enum PersistentApprovalTarget {
    ToolLevel,
    ExactInvocation {
        display_label: String,
    },
    PrefixRule {
        prefix_rule: Vec<String>,
        display_label: String,
    },
}

fn exact_shell_learning_target(
    tool_name: &str,
    tool_args: Option<&Value>,
    default_learning_label: &str,
) -> Option<ApprovalLearningTarget> {
    let scope_signature = extract_shell_permission_scope_signature(tool_name, tool_args)?;

    if let Some(command_words) = extract_shell_approval_command_words(tool_name, tool_args) {
        let rendered_command = render_shell_approval_command_words(&command_words);
        return Some(ApprovalLearningTarget::new(
            format!("{rendered_command}|{scope_signature}"),
            format!("command `{rendered_command}`"),
        ));
    }

    if let Some(command_text) = extract_shell_command_text(tool_name, tool_args) {
        return Some(ApprovalLearningTarget::new(
            format!("{command_text}|{scope_signature}"),
            format!("command `{command_text}`"),
        ));
    }

    let fallback_key = tool_args
        .map(Value::to_string)
        .unwrap_or_else(|| tool_name.to_string());
    Some(ApprovalLearningTarget::new(
        format!("{fallback_key}|{scope_signature}"),
        default_learning_label.to_string(),
    ))
}

pub(super) fn approval_learning_target(
    tool_name: &str,
    tool_args: Option<&Value>,
    default_learning_label: &str,
) -> ApprovalLearningTarget {
    let pattern = learned_shell_pattern(tool_name, tool_args);

    if let Some(scope_signature) = extract_shell_permission_scope_signature(tool_name, tool_args) {
        if let Some(prefix_rule) =
            extract_shell_persistent_approval_prefix_rule(tool_name, tool_args)
            && let Some(rendered_rule) =
                render_shell_persistent_approval_prefix_entry(tool_name, tool_args, &prefix_rule)
        {
            let rendered_prefix = render_shell_approval_command_words(&prefix_rule);
            return ApprovalLearningTarget::new(
                rendered_rule,
                format!("commands starting with `{rendered_prefix}`"),
            )
            .with_pattern(pattern);
        }

        return exact_shell_learning_target(tool_name, tool_args, default_learning_label)
            .unwrap_or_else(|| {
                ApprovalLearningTarget::new(
                    format!("{tool_name}|{scope_signature}"),
                    default_learning_label.to_string(),
                )
            })
            .with_pattern(pattern);
    }

    ApprovalLearningTarget::new(
        vtcode_core::tools::names::canonical_tool_name(tool_name).to_owned(),
        default_learning_label.to_string(),
    )
}

pub(super) fn exact_shell_approval_target(
    tool_name: &str,
    tool_args: Option<&Value>,
    default_learning_label: &str,
) -> Option<ApprovalLearningTarget> {
    // Exact persistent cache entries intentionally omit any broader pattern:
    // "always approve this exact invocation" must not silently widen its scope.
    exact_shell_learning_target(tool_name, tool_args, default_learning_label)
}

pub(super) fn persistent_approval_target(
    tool_name: &str,
    tool_args: Option<&Value>,
    default_learning_label: &str,
) -> PersistentApprovalTarget {
    if let Some(prefix_rule) = extract_shell_persistent_approval_prefix_rule(tool_name, tool_args) {
        let rendered_prefix = render_shell_approval_command_words(&prefix_rule);
        return PersistentApprovalTarget::PrefixRule {
            prefix_rule,
            display_label: format!("commands starting with `{rendered_prefix}`"),
        };
    }

    if extract_shell_permission_scope_signature(tool_name, tool_args).is_some() {
        let learning = approval_learning_target(tool_name, tool_args, default_learning_label);
        return PersistentApprovalTarget::ExactInvocation {
            display_label: learning.display_label,
        };
    }

    PersistentApprovalTarget::ToolLevel
}

pub(super) fn tool_display_labels(tool_name: &str, tool_args: Option<&Value>) -> ToolDisplayLabels {
    let learning_label = humanize_tool_name(tool_name);
    let prompt_label = tool_args
        .map(|args| describe_tool_action(tool_name, args).0)
        .filter(|headline| !headline.is_empty())
        .unwrap_or_else(|| learning_label.clone());

    ToolDisplayLabels {
        prompt_label,
        learning_label,
    }
}

/// Build a conservative family/pattern learning key for safe shell commands.
///
/// Currently only matches `find <subdir> ...` invocations that:
/// - contain no destructive options (`-delete`, `-exec`, `-execdir`, `-ok`,
///   `-okdir`, `-fls`, `-fprint*`),
/// - are a single simple command (no `&&`, `||`, `;`, `|`, nested shells, etc.
///   — gated via [`extract_shell_approval_command_prefix_words`]),
/// - target a non-absolute, non-traversal, workspace-relative subdirectory.
///
/// Scope (sandbox + additional permissions) is baked into the key so a
/// pattern approved under default permissions does not promote escalated runs.
fn learned_shell_pattern(tool_name: &str, tool_args: Option<&Value>) -> Option<LearnedPattern> {
    let scope_signature = extract_shell_permission_scope_signature(tool_name, tool_args)?;
    // Use the *prefix* extractor which already rejects compound commands and
    // nested shell invocations — a broader pattern key must never be trained
    // by commands like `find src && rm -rf target` or `bash -c '...'`.
    let command_words = extract_shell_approval_command_prefix_words(tool_name, tool_args)?;

    learned_find_pattern(&command_words, &scope_signature)
}

fn learned_find_pattern(command_words: &[String], scope_signature: &str) -> Option<LearnedPattern> {
    if command_words.first().map(String::as_str) != Some("find") {
        return None;
    }

    if command_words
        .iter()
        .any(|word| is_destructive_find_option(word))
    {
        return None;
    }

    let root = command_words.get(1)?;
    if root.starts_with('-') {
        return None;
    }
    let normalized_root = normalize_find_root(root)?;

    Some(LearnedPattern {
        key: format!("shell-pattern:find {normalized_root}|{scope_signature}"),
        label: format!("safe `find {normalized_root}` commands"),
    })
}

fn is_destructive_find_option(word: &str) -> bool {
    matches!(
        word,
        "-delete"
            | "-exec"
            | "-execdir"
            | "-ok"
            | "-okdir"
            | "-fls"
            | "-fprint"
            | "-fprint0"
            | "-fprintf"
    )
}

/// Reduce a `find <root>` argument to a stable, safe, workspace-relative
/// top-level segment. Rejects anything that would escape the workspace
/// (absolute paths, `..` traversal, `~` home expansion, empty segments) so the
/// resulting pattern key can never accidentally span filesystems or escalate.
fn normalize_find_root(root: &str) -> Option<String> {
    let trimmed = root.trim();
    if trimmed.is_empty() {
        return None;
    }
    let stripped = trimmed
        .strip_prefix("./")
        .unwrap_or(trimmed)
        .trim_end_matches('/');

    if stripped.is_empty()
        || stripped == "."
        || stripped == "/"
        || stripped.starts_with('/')
        || stripped.starts_with('~')
        || stripped
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return None;
    }

    // Collapse `src/foo/bar` to `src` so all safe finds under the same
    // top-level workspace subdirectory share a single family key.
    stripped.split('/').next().map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn pattern_for(command: &str) -> Option<LearnedPattern> {
        let args = json!({ "action": "run", "command": command });
        learned_shell_pattern("unified_exec", Some(&args))
    }

    #[test]
    fn find_under_subdir_yields_pattern_key() {
        let pattern = pattern_for("find src -type f -name '*.rs'").expect("pattern");
        assert!(
            pattern
                .key
                .starts_with("shell-pattern:find src|sandbox_permissions=")
        );
        assert_eq!(pattern.label, "safe `find src` commands");
    }

    #[test]
    fn find_root_directory_does_not_get_pattern() {
        assert!(pattern_for("find . -type f").is_none());
        assert!(pattern_for("find / -type f").is_none());
        assert!(pattern_for("find ./ -type f").is_none());
    }

    #[test]
    fn find_with_destructive_flags_does_not_get_pattern() {
        assert!(pattern_for("find src -delete").is_none());
        assert!(pattern_for("find src -exec rm {} +").is_none());
        assert!(pattern_for("find src -name foo -ok rm {} \\;").is_none());
    }

    #[test]
    fn compound_shell_commands_do_not_get_pattern() {
        assert!(pattern_for("find src -type f ; rm -rf target").is_none());
        assert!(pattern_for("find src -type f && rm -rf target").is_none());
        assert!(pattern_for("find src -type f || true").is_none());
        assert!(pattern_for("find src -type f | xargs rm").is_none());
        assert!(pattern_for("bash -c 'find src -type f'").is_none());
        assert!(pattern_for("sh -lc \"find src -type f\"").is_none());
    }

    #[test]
    fn absolute_and_traversal_roots_do_not_get_pattern() {
        assert!(pattern_for("find /tmp -type f").is_none());
        assert!(pattern_for("find /Users/me/project -type f").is_none());
        assert!(pattern_for("find ../other -type f").is_none());
        assert!(pattern_for("find src/../other -type f").is_none());
        assert!(pattern_for("find ~/src -type f").is_none());
        assert!(pattern_for("find ~ -type f").is_none());
        assert!(pattern_for("find / -type f").is_none());
    }

    #[test]
    fn non_find_command_has_no_pattern() {
        assert!(pattern_for("grep -r foo src").is_none());
        assert!(pattern_for("ls src").is_none());
    }

    #[test]
    fn find_subdir_path_collapses_to_first_segment() {
        let pattern = pattern_for("find src/agent/runloop -type f").expect("pattern");
        assert!(
            pattern
                .key
                .starts_with("shell-pattern:find src|sandbox_permissions=")
        );
    }

    #[test]
    fn iter_keys_yields_only_exact_when_no_pattern() {
        let target = ApprovalLearningTarget::new("key".into(), "label".into());
        let keys: Vec<_> = target.iter_keys().collect();
        assert_eq!(keys, vec![("key", "label")]);
    }

    #[test]
    fn iter_keys_yields_pattern_after_exact_when_present() {
        let target = ApprovalLearningTarget::new("exact".into(), "exact-label".into())
            .with_pattern(Some(LearnedPattern {
                key: "pattern".into(),
                label: "pattern-label".into(),
            }));
        let keys: Vec<_> = target.iter_keys().collect();
        assert_eq!(
            keys,
            vec![("exact", "exact-label"), ("pattern", "pattern-label")]
        );
    }

    #[tokio::test]
    async fn record_blocking_records_both_exact_and_pattern_keys() {
        use vtcode_core::tools::ApprovalRecorder;

        let temp_dir = std::env::temp_dir().join(format!(
            "vtcode_record_blocking_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        ));
        let _ = std::fs::remove_dir_all(&temp_dir);

        let recorder = ApprovalRecorder::new(temp_dir.clone());
        let target = super::approval_learning_target(
            "unified_exec",
            Some(&json!({"action":"run","command":"find src -type f"})),
            "default",
        );
        let pattern = target.pattern.as_ref().expect("pattern attached");

        super::super::approval_cache::record_approval_blocking(&recorder, &target, true).await;

        assert_eq!(recorder.get_approval_count(&target.approval_key).await, 1);
        assert_eq!(recorder.get_approval_count(&pattern.key).await, 1);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn denial_propagates_to_pattern_key() {
        use vtcode_core::tools::ApprovalRecorder;

        let temp_dir = std::env::temp_dir().join(format!(
            "vtcode_pattern_denial_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        ));
        let _ = std::fs::remove_dir_all(&temp_dir);

        let recorder = ApprovalRecorder::new(temp_dir.clone());
        let target = super::approval_learning_target(
            "unified_exec",
            Some(&json!({"action":"run","command":"find src -type f"})),
            "default",
        );
        let pattern = target.pattern.as_ref().expect("pattern attached");

        super::super::approval_cache::record_approval_blocking(&recorder, &target, false).await;

        assert_eq!(recorder.get_approval_count(&target.approval_key).await, 0);
        assert_eq!(recorder.get_approval_count(&pattern.key).await, 0);
        // ...but the pattern key's deny_count is bumped, so a future burst of
        // approvals is tempered when computing approval rate.
        let stored = recorder.get_pattern(&pattern.key).await.expect("stored");
        assert_eq!(stored.deny_count, 1);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn three_safe_find_invocations_promote_pattern_to_auto_approve() {
        use vtcode_core::tools::ApprovalRecorder;

        let temp_dir = std::env::temp_dir().join(format!(
            "vtcode_pattern_promote_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        ));
        let _ = std::fs::remove_dir_all(&temp_dir);

        let recorder = ApprovalRecorder::new(temp_dir.clone());

        // Three different (but equally safe) `find src ...` approvals,
        // simulating the user manually approving each variant.
        for command in [
            "find src -type f -name '*.rs'",
            "find src -type d",
            "find src -name foo",
        ] {
            let target = super::approval_learning_target(
                "unified_exec",
                Some(&json!({"action":"run","command":command})),
                "default",
            );
            super::super::approval_cache::record_approval_blocking(&recorder, &target, true).await;
        }

        // A *new* safe `find src ...` invocation should auto-approve via the
        // pattern key even though its exact form has never been seen before.
        let new_target = super::approval_learning_target(
            "unified_exec",
            Some(&json!({"action":"run","command":"find src -path '*runloop*'"})),
            "default",
        );
        let pattern = new_target.pattern.as_ref().expect("pattern attached");
        assert!(recorder.should_auto_approve(&pattern.key).await);
        assert_eq!(
            recorder.get_approval_count(&new_target.approval_key).await,
            0
        );

        // Destructive `find src -delete` MUST NOT inherit the pattern.
        let destructive = super::approval_learning_target(
            "unified_exec",
            Some(&json!({"action":"run","command":"find src -delete"})),
            "default",
        );
        assert!(
            destructive.pattern.is_none(),
            "destructive find must not carry pattern"
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
