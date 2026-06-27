use crate::env_helpers::default_enabled;
use serde::Deserializer;
use serde::de::{MapAccess, Visitor};
use serde::{Deserialize, Serialize};
use std::fmt;

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PermissionDefault {
    Ask,
    Allow,
    Auto,
    Deny,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct AgentPermissionsConfig {
    /// Default permission for unmatched tool calls.
    /// Options: `"ask"`, `"allow"`, `"auto"`, `"deny"`.
    pub default: PermissionDefault,

    /// Rules that allow matching tool calls without prompting.
    ///
    /// Recommended semantic rules: `"read"`, `"write"`, `"edit"`, `"bash"`.
    /// Tool-name rules like `"read_file"` are normalized to semantic rules
    /// automatically. Supports path specifiers: `"read(/src/**/*.rs)"`.
    #[serde(default)]
    pub allow: Vec<String>,

    /// Rules that require an interactive prompt when they match.
    /// Same syntax as [`Self::allow`].
    #[serde(default)]
    pub ask: Vec<String>,

    /// Rules that auto-approve with classifier-backed review.
    /// Same syntax as [`Self::allow`].
    #[serde(default)]
    pub auto: Vec<String>,

    /// Rules that deny matching tool calls.
    /// Same syntax as [`Self::allow`].
    #[serde(default)]
    pub deny: Vec<String>,
}

impl AgentPermissionsConfig {
    #[must_use]
    pub fn new(default: PermissionDefault) -> Self {
        Self {
            default,
            allow: Vec::new(),
            ask: Vec::new(),
            auto: Vec::new(),
            deny: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Permission rule normalization
// ---------------------------------------------------------------------------

/// Check whether a rule string is already a recognized semantic rule.
///
/// Semantic rules operate on `PermissionRequestKind` rather than exact tool
/// names, which means they work correctly regardless of whether the LLM calls
/// `read_file` directly or `unified_file(action="read")`.
fn is_semantic_rule(rule: &str) -> bool {
    matches!(
        rule.to_ascii_lowercase().as_str(),
        "read" | "write" | "edit" | "bash" | "webfetch"
    )
}

/// Normalize a tool-name permission rule to its semantic category.
///
/// Raw internal tool names like `"read_file"` or `"unified_file"` are mapped to
/// the semantic rule `"read"` so they correctly match all read operations
/// regardless of which tool the LLM actually invokes.
fn normalize_tool_name_to_semantic(tool_name: &str) -> String {
    match tool_name.to_ascii_lowercase().as_str() {
        // Read operations
        "read_file" | "read" | "grep_file" | "grep" | "list_files" | "list" | "glob"
        | "listfiles" | "grepfile" => "read".to_string(),

        // Write operations
        "write_file" | "write" | "create_file" | "createfile" | "delete_file" | "deletefile"
        | "move_file" | "movefile" | "copy_file" | "copyfile" => "write".to_string(),

        // Edit operations
        "edit_file" | "edit" | "apply_patch" | "applypatch" | "search_replace"
        | "searchreplace" | "file_op" | "fileop" | "patch" => "edit".to_string(),

        // Bash operations
        "bash" | "shell" | "command" | "exec_command" | "execcommand" | "run_pty_cmd"
        | "runptycmd" | "execute_code" | "executecode" => "bash".to_string(),

        // Web fetch operations
        "webfetch" | "web_fetch" | "fetch" => "webfetch".to_string(),

        // Unified tools: pass through as-is so they compile to ExactTool rules.
        // These are multi-action dispatch tools where a single tool name maps to
        // multiple PermissionRequestKind variants (e.g., unified_file produces
        // Read, Edit, and Write depending on the action argument). Collapsing
        // them to a single semantic category would silently narrow deny/allow
        // rules to only one action type.
        "unified_file" | "unified_exec" | "unified_search" => tool_name.to_string(),

        // Pass through unknown rules as-is (will become ExactTool)
        other => other.to_string(),
    }
}

/// Parse a rule string into its tool name and optional path specifier.
///
/// For example, `"read_file(/src/**/*.rs)"` returns `Some(("read_file", "/src/**/*.rs"))`.
/// A rule without parentheses returns `None`.
fn parse_tool_specifier_parts(rule: &str) -> Option<(&str, &str)> {
    let trimmed = rule.trim();
    if let Some(open) = trimmed.find('(') {
        let tool_part = trimmed[..open].trim();
        let specifier = trimmed[open + 1..].strip_suffix(')').map(str::trim)?;
        if !tool_part.is_empty() && !specifier.is_empty() {
            return Some((tool_part, specifier));
        }
    }
    None
}

/// Normalize a permission rule string to its semantic form.
///
/// This transforms raw tool-name rules into semantic rules before compilation.
/// For example:
/// - `"read_file"` → `"read"`
/// - `"read_file(/src/**/*.rs)"` → `"read(/src/**/*.rs)"`
/// - `"mcp__server__tool"` → unchanged (MCP rules pass through)
/// - `"read"` → unchanged (already semantic)
#[must_use]
pub fn normalize_permission_rule(raw: &str) -> String {
    let trimmed = raw.trim();

    // Already a semantic rule - normalize to lowercase and pass through
    if is_semantic_rule(trimmed) {
        return trimmed.to_ascii_lowercase();
    }

    // Already an MCP rule - pass through
    if trimmed.starts_with("mcp__") {
        return trimmed.to_string();
    }

    // Has path specifier - normalize the tool name part
    if let Some((tool_part, specifier)) = parse_tool_specifier_parts(trimmed) {
        let normalized_tool = normalize_tool_name_to_semantic(tool_part);
        return format!("{}({})", normalized_tool, specifier);
    }

    // Raw tool name - normalize to semantic
    normalize_tool_name_to_semantic(trimmed)
}

/// Permission system configuration - Controls command resolution, audit logging, and caching
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PermissionsConfig {
    /// Classifier-backed auto permission review policy and environment settings.
    #[serde(
        default,
        rename = "auto",
        alias = "auto_permission",
        deserialize_with = "deserialize_auto_permission_config"
    )]
    pub auto_permission: AutoPermissionConfig,

    /// Rules that allow matching tool calls without prompting.
    #[serde(default)]
    pub allow: Vec<String>,

    /// Rules that require an interactive prompt when they match.
    #[serde(default)]
    pub ask: Vec<String>,

    /// Rules that deny matching tool calls.
    #[serde(default)]
    pub deny: Vec<String>,

    /// Enable the enhanced permission system (resolver + audit logger + cache)
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// Enable command resolution to actual paths (helps identify suspicious commands)
    #[serde(default = "default_resolve_commands")]
    pub resolve_commands: bool,

    /// Enable audit logging of all permission decisions
    #[serde(default = "default_audit_enabled")]
    pub audit_enabled: bool,

    /// Directory for audit logs (created if not exists)
    /// Defaults to ~/.vtcode/audit
    #[serde(default = "default_audit_directory")]
    pub audit_directory: String,

    /// Log allowed commands to audit trail
    #[serde(default = "default_log_allowed_commands")]
    pub log_allowed_commands: bool,

    /// Log denied commands to audit trail
    #[serde(default = "default_log_denied_commands")]
    pub log_denied_commands: bool,

    /// Log permission prompts (when user is asked for confirmation)
    #[serde(default = "default_log_permission_prompts")]
    pub log_permission_prompts: bool,

    /// Enable permission decision caching to avoid redundant evaluations
    #[serde(default = "default_cache_enabled")]
    pub cache_enabled: bool,

    /// Cache time-to-live in seconds (how long to cache decisions)
    /// Default: 300 seconds (5 minutes)
    #[serde(default = "default_cache_ttl_seconds")]
    pub cache_ttl_seconds: u64,
}

/// Classifier-backed auto permission review configuration.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AutoPermissionConfig {
    /// Optional model override for the transcript reviewer.
    #[serde(default)]
    pub model: String,

    /// Optional model override for the prompt-injection probe.
    #[serde(default)]
    pub probe_model: String,

    /// Maximum consecutive denials before auto permission review falls back.
    #[serde(default = "default_auto_permission_max_consecutive_denials")]
    pub max_consecutive_denials: u32,

    /// Maximum total denials before auto permission review falls back.
    #[serde(default = "default_auto_permission_max_total_denials")]
    pub max_total_denials: u32,

    /// Drop broad code-execution allow rules while auto permission review is active.
    #[serde(default = "default_auto_permission_drop_broad_allow_rules")]
    pub drop_broad_allow_rules: bool,

    /// Classifier block rules applied in stage 2 reasoning.
    #[serde(default = "default_auto_permission_block_rules")]
    pub block_rules: Vec<String>,

    /// Narrow allow exceptions applied after block rules.
    #[serde(default = "default_auto_permission_allow_exceptions")]
    pub allow_exceptions: Vec<String>,

    /// Trusted environment boundaries for the classifier.
    #[serde(default)]
    pub environment: AutoPermissionEnvironmentConfig,
}

fn deserialize_auto_permission_config<'de, D>(
    deserializer: D,
) -> Result<AutoPermissionConfig, D::Error>
where
    D: Deserializer<'de>,
{
    struct AutoPermissionConfigVisitor;

    impl<'de> Visitor<'de> for AutoPermissionConfigVisitor {
        type Value = AutoPermissionConfig;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("a table of auto permission settings")
        }

        fn visit_map<M>(self, map: M) -> Result<Self::Value, M::Error>
        where
            M: MapAccess<'de>,
        {
            AutoPermissionConfig::deserialize(serde::de::value::MapAccessDeserializer::new(map))
        }
    }

    deserializer.deserialize_map(AutoPermissionConfigVisitor)
}

/// Trust-boundary configuration for auto permission review.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct AutoPermissionEnvironmentConfig {
    #[serde(default)]
    pub trusted_paths: Vec<String>,

    #[serde(default)]
    pub trusted_domains: Vec<String>,

    #[serde(default)]
    pub trusted_git_hosts: Vec<String>,

    #[serde(default)]
    pub trusted_git_orgs: Vec<String>,

    #[serde(default)]
    pub trusted_services: Vec<String>,
}

impl Default for AutoPermissionConfig {
    fn default() -> Self {
        Self {
            model: String::new(),
            probe_model: String::new(),
            max_consecutive_denials: default_auto_permission_max_consecutive_denials(),
            max_total_denials: default_auto_permission_max_total_denials(),
            drop_broad_allow_rules: default_auto_permission_drop_broad_allow_rules(),
            block_rules: default_auto_permission_block_rules(),
            allow_exceptions: default_auto_permission_allow_exceptions(),
            environment: AutoPermissionEnvironmentConfig::default(),
        }
    }
}

#[inline]
const fn default_resolve_commands() -> bool {
    default_enabled()
}

#[inline]
const fn default_audit_enabled() -> bool {
    default_enabled()
}

const DEFAULT_AUDIT_DIR: &str = "~/.vtcode/audit";

#[inline]
fn default_audit_directory() -> String {
    DEFAULT_AUDIT_DIR.into()
}

#[inline]
const fn default_log_allowed_commands() -> bool {
    true
}

#[inline]
const fn default_log_denied_commands() -> bool {
    true
}

#[inline]
const fn default_log_permission_prompts() -> bool {
    true
}

#[inline]
const fn default_cache_enabled() -> bool {
    true
}

#[inline]
const fn default_cache_ttl_seconds() -> u64 {
    300 // 5 minutes
}

#[inline]
const fn default_auto_permission_max_consecutive_denials() -> u32 {
    3
}

#[inline]
const fn default_auto_permission_max_total_denials() -> u32 {
    20
}

#[inline]
const fn default_auto_permission_drop_broad_allow_rules() -> bool {
    true
}

fn default_auto_permission_block_rules() -> Vec<String> {
    vec![
        "Block destructive source-control actions such as force-pushes, direct pushes to protected branches, or remote branch deletion unless the user explicitly authorized that exact blast radius.".to_string(),
        "Block remote code download-and-execute flows, including curl-or-wget pipes into interpreters or shells, and running code from freshly cloned external repositories.".to_string(),
        "Block sending workspace contents, credentials, environment variables, or other sensitive data to external endpoints unless the user explicitly named both the destination and the data.".to_string(),
        "Block credential hunting, secret discovery, or scanning config and environment stores for alternative tokens unless the user explicitly asked for that credential workflow.".to_string(),
        "Block commands that bypass safety checks, disable verification, install persistence, or weaken the host security posture.".to_string(),
        "Block destructive or production/shared-infrastructure actions unless the user explicitly authorized the target and the action.".to_string(),
        "Block destructive actions against inferred, fuzzy-matched, or agent-selected targets when the user did not name the exact target.".to_string(),
    ]
}

fn default_auto_permission_allow_exceptions() -> Vec<String> {
    vec![
        "Allow read-only tools and read-only browsing/search actions.".to_string(),
        "Allow file edits and writes inside the current workspace when the path is not protected.".to_string(),
        "Allow pushes only to the current session branch or configured git remotes inside the trusted environment.".to_string(),
    ]
}

impl Default for PermissionsConfig {
    fn default() -> Self {
        Self {
            auto_permission: AutoPermissionConfig::default(),
            allow: Vec::new(),
            ask: Vec::new(),
            deny: Vec::new(),
            enabled: default_enabled(),
            resolve_commands: default_resolve_commands(),
            audit_enabled: default_audit_enabled(),
            audit_directory: default_audit_directory(),
            log_allowed_commands: default_log_allowed_commands(),
            log_denied_commands: default_log_denied_commands(),
            log_permission_prompts: default_log_permission_prompts(),
            cache_enabled: default_cache_enabled(),
            cache_ttl_seconds: default_cache_ttl_seconds(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{AgentPermissionsConfig, PermissionDefault, PermissionsConfig};

    #[test]
    fn parses_agent_permission_defaults_and_empty_buckets() {
        for (value, expected) in [
            ("ask", PermissionDefault::Ask),
            ("allow", PermissionDefault::Allow),
            ("auto", PermissionDefault::Auto),
            ("deny", PermissionDefault::Deny),
        ] {
            let config: AgentPermissionsConfig =
                toml::from_str(&format!(r#"default = "{value}""#)).expect("agent permissions");
            assert_eq!(config.default, expected);
            assert!(config.allow.is_empty());
            assert!(config.ask.is_empty());
            assert!(config.auto.is_empty());
            assert!(config.deny.is_empty());
        }

        let err = toml::from_str::<AgentPermissionsConfig>(r#"default = "plan""#).unwrap_err();
        assert!(err.to_string().contains("unknown variant"));
    }

    #[test]
    fn parses_exact_tool_rules() {
        let config: PermissionsConfig = toml::from_str(
            r#"
            allow = ["read_file", "unified_search"]
            deny = ["unified_exec"]
            "#,
        )
        .expect("permissions config");

        assert_eq!(
            config.allow,
            vec!["read_file".to_string(), "unified_search".to_string()]
        );
        assert_eq!(config.deny, vec!["unified_exec".to_string()]);
    }

    #[test]
    fn ignores_unknown_fields_for_forward_compatibility() {
        // Unknown fields are silently ignored so that a config written by a newer
        // vtcode version does not break older binaries.
        let removed_field = format!("default_{}", "mode");
        let input = format!(
            r#"
            {removed_field} = "ask"
            "#,
        );
        let config: PermissionsConfig = toml::from_str(&input).unwrap();
        // The unknown field is ignored; defaults are used.
        assert!(config.allow.is_empty());

        // "auto" as a string value for the top-level field is still rejected
        // because the struct field expects AutoPermissionConfig, not a list.
        let err = toml::from_str::<PermissionsConfig>(
            r#"
            auto = ["unified_exec"]
            "#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("invalid type"));
    }

    #[test]
    fn parses_auto_permission_settings_from_canonical_auto_table() {
        let config: PermissionsConfig = toml::from_str(
            r#"
            [auto]
            model = "gpt-5-mini"
            max_consecutive_denials = 2
            drop_broad_allow_rules = false

            [auto.environment]
            trusted_paths = ["/work/project"]
            trusted_domains = ["example.com"]
            "#,
        )
        .expect("permissions config");

        assert_eq!(config.auto_permission.model, "gpt-5-mini");
        assert_eq!(config.auto_permission.max_consecutive_denials, 2);
        assert!(!config.auto_permission.drop_broad_allow_rules);
        assert_eq!(
            config.auto_permission.environment.trusted_paths,
            vec!["/work/project".to_string()]
        );
        assert_eq!(
            config.auto_permission.environment.trusted_domains,
            vec!["example.com".to_string()]
        );
    }

    #[test]
    fn auto_permission_defaults_are_conservative() {
        let config = PermissionsConfig::default();

        assert_eq!(config.auto_permission.max_consecutive_denials, 3);
        assert_eq!(config.auto_permission.max_total_denials, 20);
        assert!(config.auto_permission.drop_broad_allow_rules);
        assert!(!config.auto_permission.block_rules.is_empty());
        assert!(!config.auto_permission.allow_exceptions.is_empty());
        assert!(config.auto_permission.environment.trusted_paths.is_empty());
    }

    #[test]
    fn normalizes_read_tool_names_to_semantic_rule() {
        for input in ["read_file", "Read_File", "READ_FILE", "read", "Read"] {
            assert_eq!(
                super::normalize_permission_rule(input),
                "read",
                "input: {input}"
            );
        }
    }

    #[test]
    fn normalizes_write_tool_names_to_semantic_rule() {
        for input in ["write_file", "Write_File", "create_file", "delete_file"] {
            assert_eq!(
                super::normalize_permission_rule(input),
                "write",
                "input: {input}"
            );
        }
    }

    #[test]
    fn normalizes_edit_tool_names_to_semantic_rule() {
        for input in ["edit_file", "Edit_File", "apply_patch", "file_op"] {
            assert_eq!(
                super::normalize_permission_rule(input),
                "edit",
                "input: {input}"
            );
        }
    }

    #[test]
    fn normalizes_bash_tool_names_to_semantic_rule() {
        for input in ["bash", "Bash", "exec_command", "run_pty_cmd"] {
            assert_eq!(
                super::normalize_permission_rule(input),
                "bash",
                "input: {input}"
            );
        }
    }

    #[test]
    fn unified_tools_pass_through_as_exact_tool_rules() {
        // Unified tools are multi-action dispatch tools that should NOT be
        // collapsed to semantic rules. They pass through as-is so they compile
        // to ExactTool rules matching on exact_tool_name.
        assert_eq!(
            super::normalize_permission_rule("unified_file"),
            "unified_file"
        );
        assert_eq!(
            super::normalize_permission_rule("unified_exec"),
            "unified_exec"
        );
        assert_eq!(
            super::normalize_permission_rule("unified_search"),
            "unified_search"
        );
    }

    #[test]
    fn normalizes_tool_name_with_path_specifier() {
        assert_eq!(
            super::normalize_permission_rule("read_file(/src/**/*.rs)"),
            "read(/src/**/*.rs)"
        );
        assert_eq!(
            super::normalize_permission_rule("write_file(/docs/**)"),
            "write(/docs/**)"
        );
    }

    #[test]
    fn mcp_rules_pass_through_unchanged() {
        assert_eq!(
            super::normalize_permission_rule("mcp__server__tool"),
            "mcp__server__tool"
        );
        assert_eq!(
            super::normalize_permission_rule("mcp__context7__*"),
            "mcp__context7__*"
        );
    }

    #[test]
    fn semantic_rules_pass_through_unchanged() {
        for input in ["read", "write", "edit", "bash", "webfetch"] {
            assert_eq!(
                super::normalize_permission_rule(input),
                input,
                "input: {input}"
            );
        }
    }

    #[test]
    fn unknown_rules_pass_through_unchanged() {
        assert_eq!(
            super::normalize_permission_rule("some_custom_tool"),
            "some_custom_tool"
        );
    }
}
