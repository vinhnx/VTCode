use glob::Pattern;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use serde_json::Value;
use std::path::{Path, PathBuf};
use url::Url;

use crate::config::PermissionsConfig;
use crate::config::constants::tools;
use crate::tools::command_args;
use crate::tools::mcp::{MCP_QUALIFIED_TOOL_PREFIX, parse_canonical_mcp_tool_name};
use crate::tools::tool_intent;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionRuleDecision {
    Allow,
    Ask,
    Deny,
    NoMatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PermissionRuleMatches {
    pub deny: bool,
    pub ask: bool,
    pub allow: bool,
}

impl PermissionRuleMatches {
    pub const fn decision(self) -> PermissionRuleDecision {
        if self.deny {
            PermissionRuleDecision::Deny
        } else if self.ask {
            PermissionRuleDecision::Ask
        } else if self.allow {
            PermissionRuleDecision::Allow
        } else {
            PermissionRuleDecision::NoMatch
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionRequestKind {
    Bash { command: String },
    Read { paths: Vec<PathBuf> },
    Edit { paths: Vec<PathBuf> },
    Write { paths: Vec<PathBuf> },
    WebFetch { domains: Vec<String> },
    Mcp { server: String, tool: String },
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionRequest {
    pub exact_tool_name: String,
    pub kind: PermissionRequestKind,
    pub builtin_file_mutation: bool,
    pub protected_write_paths: Vec<PathBuf>,
}

impl PermissionRequest {
    pub fn requires_protected_write_prompt(&self) -> bool {
        !self.protected_write_paths.is_empty()
    }
}

pub fn build_permission_request(
    workspace_root: &Path,
    current_dir: &Path,
    normalized_tool_name: &str,
    tool_args: Option<&Value>,
) -> PermissionRequest {
    let kind = build_request_kind(workspace_root, current_dir, normalized_tool_name, tool_args);
    let protected_write_paths = protected_write_paths(workspace_root, &kind);
    let builtin_file_mutation = matches!(
        kind,
        PermissionRequestKind::Edit { .. } | PermissionRequestKind::Write { .. }
    );

    PermissionRequest {
        exact_tool_name: normalized_tool_name.to_string(),
        kind,
        builtin_file_mutation,
        protected_write_paths,
    }
}

pub fn evaluate_permissions(
    config: &PermissionsConfig,
    workspace_root: &Path,
    current_dir: &Path,
    request: &PermissionRequest,
) -> PermissionRuleMatches {
    let evaluator = PermissionEvaluator::new(config, workspace_root, current_dir);
    evaluator.evaluate(request)
}

struct PermissionEvaluator {
    deny: Vec<CompiledPermissionRule>,
    ask: Vec<CompiledPermissionRule>,
    allow: Vec<CompiledPermissionRule>,
}

impl PermissionEvaluator {
    fn new(config: &PermissionsConfig, workspace_root: &Path, current_dir: &Path) -> Self {
        Self {
            deny: compile_rules(&config.deny, workspace_root, current_dir),
            ask: compile_rules(&config.ask, workspace_root, current_dir),
            allow: compile_rules(&config.allow, workspace_root, current_dir),
        }
    }

    fn evaluate(&self, request: &PermissionRequest) -> PermissionRuleMatches {
        PermissionRuleMatches {
            deny: self.deny.iter().any(|rule| rule.matches(request)),
            ask: self.ask.iter().any(|rule| rule.matches(request)),
            allow: self.allow.iter().any(|rule| rule.matches(request)),
        }
    }
}

fn compile_rules(
    rules: &[String],
    workspace_root: &Path,
    current_dir: &Path,
) -> Vec<CompiledPermissionRule> {
    rules
        .iter()
        .filter_map(|rule| CompiledPermissionRule::compile(rule, workspace_root, current_dir))
        .collect()
}

#[derive(Debug)]
enum CompiledPermissionRule {
    Bash(Option<Pattern>),
    Read(Option<PathRuleMatcher>),
    Edit(Option<PathRuleMatcher>),
    Write(Option<PathRuleMatcher>),
    WebFetchAll,
    WebFetchDomain(String),
    McpServer(String),
    McpWildcard(String),
    McpTool { server: String, tool: String },
    ExactTool(String),
}

impl CompiledPermissionRule {
    fn compile(raw: &str, workspace_root: &Path, current_dir: &Path) -> Option<Self> {
        let rule = raw.trim();
        if rule.is_empty() {
            return None;
        }

        if rule.eq_ignore_ascii_case("bash") || rule.eq_ignore_ascii_case("bash(*)") {
            return Some(Self::Bash(None));
        }
        if let Some(specifier) = parse_tool_specifier(rule, "bash") {
            return compile_bash_rule(specifier).map(Self::Bash);
        }

        if rule.eq_ignore_ascii_case("read") || rule.eq_ignore_ascii_case("read(*)") {
            return Some(Self::Read(None));
        }
        if let Some(specifier) = parse_tool_specifier(rule, "read") {
            return PathRuleMatcher::compile(specifier, workspace_root, current_dir)
                .map(Some)
                .map(Self::Read);
        }

        if rule.eq_ignore_ascii_case("edit") || rule.eq_ignore_ascii_case("edit(*)") {
            return Some(Self::Edit(None));
        }
        if let Some(specifier) = parse_tool_specifier(rule, "edit") {
            return PathRuleMatcher::compile(specifier, workspace_root, current_dir)
                .map(Some)
                .map(Self::Edit);
        }

        if rule.eq_ignore_ascii_case("write") || rule.eq_ignore_ascii_case("write(*)") {
            return Some(Self::Write(None));
        }
        if let Some(specifier) = parse_tool_specifier(rule, "write") {
            return PathRuleMatcher::compile(specifier, workspace_root, current_dir)
                .map(Some)
                .map(Self::Write);
        }

        if rule.eq_ignore_ascii_case("webfetch") || rule.eq_ignore_ascii_case("webfetch(*)") {
            return Some(Self::WebFetchAll);
        }
        if let Some(specifier) = parse_tool_specifier(rule, "webfetch") {
            let domain = specifier
                .strip_prefix("domain:")?
                .trim()
                .to_ascii_lowercase();
            if domain.is_empty() {
                return None;
            }
            return Some(Self::WebFetchDomain(domain));
        }

        if let Some(server) = rule.strip_prefix(MCP_QUALIFIED_TOOL_PREFIX) {
            if let Some((server, tool)) = server.split_once("__") {
                if tool == "*" {
                    return Some(Self::McpWildcard(server.to_string()));
                }
                if !server.is_empty() && !tool.is_empty() {
                    return Some(Self::McpTool {
                        server: server.to_string(),
                        tool: tool.to_string(),
                    });
                }
                return None;
            }
            if !server.is_empty() {
                return Some(Self::McpServer(server.to_string()));
            }
            return None;
        }

        if rule.contains('(') || rule.contains(')') {
            return None;
        }

        Some(Self::ExactTool(rule.to_string()))
    }

    fn matches(&self, request: &PermissionRequest) -> bool {
        match self {
            Self::Bash(pattern) => match &request.kind {
                PermissionRequestKind::Bash { command } => pattern
                    .as_ref()
                    .is_none_or(|pattern| pattern.matches(command)),
                _ => false,
            },
            Self::Read(matcher) => match &request.kind {
                PermissionRequestKind::Read { paths } => matcher
                    .as_ref()
                    .is_none_or(|matcher| paths.iter().any(|path| matcher.matches(path))),
                _ => false,
            },
            Self::Edit(matcher) => match &request.kind {
                PermissionRequestKind::Edit { paths } => matcher
                    .as_ref()
                    .is_none_or(|matcher| paths.iter().any(|path| matcher.matches(path))),
                _ => false,
            },
            Self::Write(matcher) => match &request.kind {
                PermissionRequestKind::Write { paths } => matcher
                    .as_ref()
                    .is_none_or(|matcher| paths.iter().any(|path| matcher.matches(path))),
                _ => false,
            },
            Self::WebFetchAll => matches!(request.kind, PermissionRequestKind::WebFetch { .. }),
            Self::WebFetchDomain(domain) => match &request.kind {
                PermissionRequestKind::WebFetch { domains } => domains
                    .iter()
                    .any(|candidate| domain_matches_allowed(candidate, domain)),
                _ => false,
            },
            Self::McpServer(server) | Self::McpWildcard(server) => match &request.kind {
                PermissionRequestKind::Mcp {
                    server: candidate, ..
                } => candidate == server,
                _ => false,
            },
            Self::McpTool { server, tool } => match &request.kind {
                PermissionRequestKind::Mcp {
                    server: candidate_server,
                    tool: candidate_tool,
                } => candidate_server == server && candidate_tool == tool,
                _ => false,
            },
            Self::ExactTool(tool_name) => request.exact_tool_name == *tool_name,
        }
    }
}

fn parse_tool_specifier<'a>(rule: &'a str, tool_name: &str) -> Option<&'a str> {
    let open = rule.find('(')?;
    let close = rule.rfind(')')?;
    if close <= open || close + 1 != rule.len() {
        return None;
    }
    let prefix = &rule[..open];
    prefix
        .eq_ignore_ascii_case(tool_name)
        .then_some(rule[open + 1..close].trim())
}

fn compile_bash_rule(specifier: &str) -> Option<Option<Pattern>> {
    if specifier.is_empty() || specifier == "*" {
        return Some(None);
    }
    Pattern::new(specifier).ok().map(Some)
}

#[derive(Debug)]
struct PathRuleMatcher {
    matcher: Gitignore,
}

impl PathRuleMatcher {
    fn compile(raw: &str, workspace_root: &Path, current_dir: &Path) -> Option<Self> {
        let home_dir = dirs::home_dir();
        let (root, pattern) = if let Some(path) = raw.strip_prefix("//") {
            (PathBuf::from("/"), format!("/{}", path))
        } else if let Some(path) = raw.strip_prefix("~/") {
            (home_dir?, format!("/{}", path))
        } else if raw.starts_with('/') {
            (workspace_root.to_path_buf(), raw.to_string())
        } else if let Some(path) = raw.strip_prefix("./") {
            (current_dir.to_path_buf(), path.to_string())
        } else {
            (current_dir.to_path_buf(), raw.to_string())
        };

        let mut builder = GitignoreBuilder::new(root);
        builder.add_line(None, &pattern).ok()?;
        let matcher = builder.build().ok()?;
        Some(Self { matcher })
    }

    fn matches(&self, candidate: &Path) -> bool {
        self.matcher
            .matched_path_or_any_parents(candidate, false)
            .is_ignore()
    }
}

fn build_request_kind(
    workspace_root: &Path,
    current_dir: &Path,
    normalized_tool_name: &str,
    tool_args: Option<&Value>,
) -> PermissionRequestKind {
    if let Some((server, tool)) = parse_mcp_request(normalized_tool_name) {
        return PermissionRequestKind::Mcp { server, tool };
    }

    let Some(args) = tool_args else {
        return PermissionRequestKind::Other;
    };

    if tool_intent::is_command_run_tool_call(normalized_tool_name, args)
        && let Ok(Some(command)) = command_args::command_text(args)
    {
        return PermissionRequestKind::Bash { command };
    }

    if is_web_fetch_request(normalized_tool_name, args) {
        let domains = extract_web_domains(args);
        return PermissionRequestKind::WebFetch { domains };
    }

    if let Some(kind) = file_request_kind(workspace_root, current_dir, normalized_tool_name, args) {
        return kind;
    }

    PermissionRequestKind::Other
}

fn parse_mcp_request(normalized_tool_name: &str) -> Option<(String, String)> {
    if let Some((server, tool)) = parse_canonical_mcp_tool_name(normalized_tool_name) {
        return Some((server.to_string(), tool.to_string()));
    }

    let stripped = normalized_tool_name.strip_prefix(MCP_QUALIFIED_TOOL_PREFIX)?;
    let (server, tool) = stripped.split_once("__")?;
    if server.is_empty() || tool.is_empty() || tool == "*" {
        return None;
    }
    Some((server.to_string(), tool.to_string()))
}

fn is_web_fetch_request(normalized_tool_name: &str, args: &Value) -> bool {
    normalized_tool_name == "web_fetch"
        || normalized_tool_name == tools::FETCH_URL
        || (normalized_tool_name == tools::UNIFIED_SEARCH
            && tool_intent::unified_search_action(args).is_some_and(|action| action == "web"))
}

fn file_request_kind(
    workspace_root: &Path,
    current_dir: &Path,
    normalized_tool_name: &str,
    args: &Value,
) -> Option<PermissionRequestKind> {
    let paths = extract_candidate_paths(workspace_root, current_dir, normalized_tool_name, args);

    match normalized_tool_name {
        tools::READ_FILE | tools::GREP_FILE | tools::LIST_FILES => {
            Some(PermissionRequestKind::Read { paths })
        }
        tools::WRITE_FILE
        | tools::CREATE_FILE
        | tools::DELETE_FILE
        | tools::MOVE_FILE
        | tools::COPY_FILE => Some(PermissionRequestKind::Write { paths }),
        tools::EDIT_FILE | tools::APPLY_PATCH | tools::SEARCH_REPLACE | tools::FILE_OP => {
            Some(PermissionRequestKind::Edit { paths })
        }
        tools::UNIFIED_SEARCH => {
            if tool_intent::unified_search_action(args).is_some_and(|action| action == "web") {
                None
            } else {
                Some(PermissionRequestKind::Read { paths })
            }
        }
        tools::UNIFIED_FILE => match tool_intent::unified_file_action(args) {
            Some("read") => Some(PermissionRequestKind::Read { paths }),
            Some("edit") | Some("patch") => Some(PermissionRequestKind::Edit { paths }),
            Some(_) => Some(PermissionRequestKind::Write { paths }),
            None => None,
        },
        _ => None,
    }
}

fn extract_candidate_paths(
    workspace_root: &Path,
    current_dir: &Path,
    normalized_tool_name: &str,
    args: &Value,
) -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Some(obj) = args.as_object() {
        for key in [
            "path",
            "file_path",
            "filepath",
            "target_path",
            "destination",
        ] {
            if let Some(path) = obj.get(key).and_then(Value::as_str) {
                push_resolved_path(&mut paths, workspace_root, current_dir, path);
            }
        }
    }

    if normalized_tool_name == tools::APPLY_PATCH
        || tool_intent::unified_file_action(args) == Some("patch")
    {
        for patch_path in extract_patch_paths(args) {
            push_resolved_path(&mut paths, workspace_root, current_dir, &patch_path);
        }
    }

    paths.sort();
    paths.dedup();
    paths
}

fn extract_patch_paths(args: &Value) -> Vec<String> {
    let patch = args
        .get("patch")
        .and_then(Value::as_str)
        .or_else(|| args.get("input").and_then(Value::as_str))
        .or_else(|| args.as_str());
    let Some(patch) = patch else {
        return Vec::new();
    };

    patch
        .lines()
        .filter_map(|line| {
            for prefix in [
                "*** Update File: ",
                "*** Add File: ",
                "*** Delete File: ",
                "*** Move to: ",
            ] {
                if let Some(path) = line.strip_prefix(prefix) {
                    let trimmed = path.trim();
                    if !trimmed.is_empty() {
                        return Some(trimmed.to_string());
                    }
                }
            }
            None
        })
        .collect()
}

fn push_resolved_path(
    paths: &mut Vec<PathBuf>,
    workspace_root: &Path,
    current_dir: &Path,
    raw: &str,
) {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return;
    }

    let resolved = if Path::new(trimmed).is_absolute() {
        PathBuf::from(trimmed)
    } else {
        current_dir
            .strip_prefix(workspace_root)
            .ok()
            .filter(|relative| !relative.as_os_str().is_empty())
            .map(|relative| workspace_root.join(relative).join(trimmed))
            .unwrap_or_else(|| workspace_root.join(trimmed))
    };
    paths.push(crate::utils::path::normalize_path(&resolved));
}

fn extract_web_domains(args: &Value) -> Vec<String> {
    args.get("url")
        .and_then(Value::as_str)
        .and_then(extract_url_domain)
        .into_iter()
        .collect::<Vec<_>>()
}

fn extract_url_domain(url: &str) -> Option<String> {
    let parsed = Url::parse(url).ok()?;
    parsed
        .host_str()
        .map(|host| host.trim_end_matches('.').to_ascii_lowercase())
}

fn protected_write_paths(workspace_root: &Path, kind: &PermissionRequestKind) -> Vec<PathBuf> {
    let paths = match kind {
        PermissionRequestKind::Edit { paths } | PermissionRequestKind::Write { paths } => paths,
        _ => return Vec::new(),
    };

    paths
        .iter()
        .filter(|path| is_protected_write_path(workspace_root, path))
        .cloned()
        .collect()
}

fn is_protected_write_path(workspace_root: &Path, path: &Path) -> bool {
    let relative = path.strip_prefix(workspace_root).ok();
    let Some(relative) = relative else {
        return false;
    };

    let as_string = relative.to_string_lossy().replace('\\', "/");
    if matches!(
        as_string.as_str(),
        ".vtcode/commands" | ".vtcode/agents" | ".vtcode/skills"
    ) || as_string.starts_with(".vtcode/commands/")
        || as_string.starts_with(".vtcode/agents/")
        || as_string.starts_with(".vtcode/skills/")
    {
        return false;
    }

    matches!(
        as_string.split('/').next(),
        Some(".git" | ".vtcode" | ".vscode" | ".idea")
    )
}

fn domain_matches_allowed(domain: &str, allowed: &str) -> bool {
    let normalized_domain = domain.trim_end_matches('.').to_ascii_lowercase();
    let normalized_allowed = allowed
        .trim_start_matches('.')
        .trim_end_matches('.')
        .to_ascii_lowercase();

    normalized_domain == normalized_allowed
        || normalized_domain.ends_with(&format!(".{normalized_allowed}"))
}

#[cfg(test)]
mod tests {
    use super::{
        PermissionRequest, PermissionRequestKind, PermissionRuleDecision, build_permission_request,
        evaluate_permissions,
    };
    use crate::config::{PermissionMode, PermissionsConfig};
    use serde_json::json;
    use tempfile::TempDir;

    fn workspace_roots() -> (TempDir, std::path::PathBuf, std::path::PathBuf) {
        let temp = TempDir::new().expect("temp dir");
        let workspace = temp.path().join("workspace");
        let cwd = workspace.join("nested");
        std::fs::create_dir_all(&cwd).expect("create dirs");
        (temp, workspace, cwd)
    }

    #[test]
    fn deny_precedes_ask_and_allow() {
        let (_temp, workspace, cwd) = workspace_roots();
        let config = PermissionsConfig {
            allow: vec!["Read".to_string()],
            ask: vec!["Read(/docs/**)".to_string()],
            deny: vec!["Read(/docs/secret.txt)".to_string()],
            ..PermissionsConfig::default()
        };
        let request = PermissionRequest {
            exact_tool_name: "read_file".to_string(),
            kind: PermissionRequestKind::Read {
                paths: vec![workspace.join("docs/secret.txt")],
            },
            builtin_file_mutation: false,
            protected_write_paths: Vec::new(),
        };

        assert_eq!(
            evaluate_permissions(&config, &workspace, &cwd, &request).decision(),
            PermissionRuleDecision::Deny
        );
    }

    #[test]
    fn bash_glob_matches_command_text() {
        let (_temp, workspace, cwd) = workspace_roots();
        let config = PermissionsConfig {
            allow: vec!["Bash(cargo test *)".to_string()],
            ..PermissionsConfig::default()
        };
        let request = PermissionRequest {
            exact_tool_name: "unified_exec".to_string(),
            kind: PermissionRequestKind::Bash {
                command: "cargo test -p vtcode".to_string(),
            },
            builtin_file_mutation: false,
            protected_write_paths: Vec::new(),
        };

        assert_eq!(
            evaluate_permissions(&config, &workspace, &cwd, &request).decision(),
            PermissionRuleDecision::Allow
        );
    }

    #[test]
    fn read_path_rules_use_workspace_relative_matching() {
        let (_temp, workspace, cwd) = workspace_roots();
        let config = PermissionsConfig {
            ask: vec!["Read(/src/**/*.rs)".to_string()],
            ..PermissionsConfig::default()
        };
        let request = PermissionRequest {
            exact_tool_name: "read_file".to_string(),
            kind: PermissionRequestKind::Read {
                paths: vec![workspace.join("src/lib.rs")],
            },
            builtin_file_mutation: false,
            protected_write_paths: Vec::new(),
        };

        assert_eq!(
            evaluate_permissions(&config, &workspace, &cwd, &request).decision(),
            PermissionRuleDecision::Ask
        );
    }

    #[test]
    fn mcp_rules_match_canonical_requests() {
        let (_temp, workspace, cwd) = workspace_roots();
        let config = PermissionsConfig {
            allow: vec!["mcp__context7__*".to_string()],
            ..PermissionsConfig::default()
        };
        let request = PermissionRequest {
            exact_tool_name: "mcp::context7::search-docs".to_string(),
            kind: PermissionRequestKind::Mcp {
                server: "context7".to_string(),
                tool: "search-docs".to_string(),
            },
            builtin_file_mutation: false,
            protected_write_paths: Vec::new(),
        };

        assert_eq!(
            evaluate_permissions(&config, &workspace, &cwd, &request).decision(),
            PermissionRuleDecision::Allow
        );
    }

    #[test]
    fn protected_directory_exceptions_are_not_flagged() {
        let (_temp, workspace, cwd) = workspace_roots();
        let request = build_permission_request(
            &workspace,
            &cwd,
            "unified_file",
            Some(&json!({
                "action": "write",
                "path": "../.vtcode/skills/example.md"
            })),
        );
        assert!(!request.requires_protected_write_prompt());

        let request = build_permission_request(
            &workspace,
            &cwd,
            "unified_file",
            Some(&json!({
                "action": "write",
                "path": "../.vtcode/settings.toml"
            })),
        );
        assert!(request.requires_protected_write_prompt());
    }

    #[test]
    fn apply_patch_paths_are_extracted_for_edit_rules() {
        let (_temp, workspace, cwd) = workspace_roots();
        let config = PermissionsConfig {
            ask: vec!["Edit(/src/**)".to_string()],
            ..PermissionsConfig::default()
        };
        let request = build_permission_request(
            &workspace,
            &cwd,
            "apply_patch",
            Some(&json!({
                "patch": "*** Begin Patch\n*** Update File: ../src/main.rs\n@@\n-test\n+test\n*** End Patch\n"
            })),
        );

        assert_eq!(
            evaluate_permissions(&config, &workspace, &cwd, &request).decision(),
            PermissionRuleDecision::Ask
        );
    }

    #[test]
    fn mode_defaults_to_standard_behavior() {
        assert_eq!(PermissionMode::default(), PermissionMode::Default);
    }

    #[test]
    fn relative_paths_resolve_from_current_directory() {
        let (_temp, workspace, cwd) = workspace_roots();
        let config = PermissionsConfig {
            ask: vec!["Read(./nested-file.rs)".to_string()],
            ..PermissionsConfig::default()
        };
        let request = build_permission_request(
            &workspace,
            &cwd,
            "read_file",
            Some(&json!({"path": "nested-file.rs"})),
        );

        assert_eq!(
            evaluate_permissions(&config, &workspace, &cwd, &request).decision(),
            PermissionRuleDecision::Ask
        );
    }

    #[test]
    fn exact_tool_rules_feed_rule_tiers() {
        let (_temp, workspace, cwd) = workspace_roots();
        let config = PermissionsConfig {
            allow: vec!["read_file".to_string()],
            deny: vec!["unified_exec".to_string()],
            ..PermissionsConfig::default()
        };

        let read_request = PermissionRequest {
            exact_tool_name: "read_file".to_string(),
            kind: PermissionRequestKind::Read { paths: vec![] },
            builtin_file_mutation: false,
            protected_write_paths: Vec::new(),
        };
        let exec_request = PermissionRequest {
            exact_tool_name: "unified_exec".to_string(),
            kind: PermissionRequestKind::Other,
            builtin_file_mutation: false,
            protected_write_paths: Vec::new(),
        };

        assert!(evaluate_permissions(&config, &workspace, &cwd, &read_request).allow);
        assert!(evaluate_permissions(&config, &workspace, &cwd, &exec_request).deny);
    }
}
