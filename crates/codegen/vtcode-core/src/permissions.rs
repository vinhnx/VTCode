use glob::Pattern;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use url::Url;

use crate::config::PermissionsConfig;
use crate::config::constants::tools;
use crate::tools::command_args;
use crate::tools::mcp::{MCP_QUALIFIED_TOOL_PREFIX, parse_canonical_mcp_tool_name};
use crate::tools::tool_intent;
use vtcode_config::core::permissions::{AgentPermissionsConfig, PermissionDefault, normalize_permission_rule};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionRuleDecision {
    Allow,
    Auto,
    Ask,
    Deny,
    NoMatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedPermissionDecision {
    Allow,
    Auto,
    Ask,
    Deny,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PermissionRuleMatches {
    pub deny: bool,
    pub ask: bool,
    pub auto: bool,
    pub allow: bool,
}

impl PermissionRuleMatches {
    /// Converts these matches into a single [`PermissionRuleDecision`] using deny > ask > auto > allow precedence.
    pub const fn decision(self) -> PermissionRuleDecision {
        if self.deny {
            PermissionRuleDecision::Deny
        } else if self.ask {
            PermissionRuleDecision::Ask
        } else if self.auto {
            PermissionRuleDecision::Auto
        } else if self.allow {
            PermissionRuleDecision::Allow
        } else {
            PermissionRuleDecision::NoMatch
        }
    }
}

/// Categorizes a permission request by the type of operation being performed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionRequestKind {
    /// A shell command execution request.
    Bash { command: String },
    /// A file read request targeting the given paths.
    Read { paths: Vec<PathBuf> },
    /// A file edit request targeting the given paths.
    Edit { paths: Vec<PathBuf> },
    /// A file write/create/delete request targeting the given paths.
    Write { paths: Vec<PathBuf> },
    /// A web fetch request targeting the given domains.
    WebFetch { domains: Vec<String> },
    /// An MCP tool invocation for the given server and tool.
    Mcp { server: String, tool: String },
    /// A tool call that does not fit the categories above.
    Other,
}

/// A fully described permission request carrying the tool name, operation kind,
/// and any protected-path metadata needed for prompting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionRequest {
    /// Exact (normalized) tool name that originated this request.
    pub exact_tool_name: String,
    /// Categorized operation kind for rule matching.
    pub kind: PermissionRequestKind,
    /// Whether this request mutates a file via a builtin file tool.
    pub builtin_file_mutation: bool,
    /// Paths within protected directories (`.git`, `.vtcode`, etc.) that require extra confirmation.
    pub protected_write_paths: Vec<PathBuf>,
}

impl PermissionRequest {
    /// Returns `true` if this request targets one or more protected write paths.
    pub fn requires_protected_write_prompt(&self) -> bool {
        !self.protected_write_paths.is_empty()
    }
}

/// Constructs a [`PermissionRequest`] from a normalized tool name and its arguments.
pub fn build_permission_request(
    workspace_root: &Path,
    current_dir: &Path,
    normalized_tool_name: &str,
    tool_args: Option<&Value>,
) -> PermissionRequest {
    let kind = build_request_kind(workspace_root, current_dir, normalized_tool_name, tool_args);
    let protected_write_paths = protected_write_paths(workspace_root, &kind);
    let builtin_file_mutation =
        matches!(kind, PermissionRequestKind::Edit { .. } | PermissionRequestKind::Write { .. });

    PermissionRequest {
        exact_tool_name: normalized_tool_name.to_string(),
        kind,
        builtin_file_mutation,
        protected_write_paths,
    }
}

/// Build representative permission requests for advertised tool availability.
///
/// Runtime dispatch still evaluates the concrete call arguments. Advertisement cannot know future
/// arguments, so multi-action tools include each category they can expose and callers should treat
/// any denied representative request as a reason not to grant provider-native availability.
pub fn build_advertised_permission_requests(
    workspace_root: &Path,
    current_dir: &Path,
    normalized_tool_name: &str,
) -> Vec<PermissionRequest> {
    let representative_args = advertised_permission_args(normalized_tool_name);
    if representative_args.is_empty() {
        return vec![build_permission_request(
            workspace_root,
            current_dir,
            normalized_tool_name,
            None,
        )];
    }

    representative_args
        .iter()
        .map(|args| build_permission_request(workspace_root, current_dir, normalized_tool_name, Some(args)))
        .collect()
}

/// Evaluates a permission request against the global permission configuration and
/// returns which rule tiers matched.
pub fn evaluate_permissions(
    config: &PermissionsConfig,
    workspace_root: &Path,
    current_dir: &Path,
    request: &PermissionRequest,
) -> PermissionRuleMatches {
    let evaluator = PermissionRuleSet::from_global_config(config, workspace_root, current_dir);
    evaluator.evaluate_matches(request)
}

/// Evaluates a permission request against an agent's permission configuration,
/// returning the resolved decision using the agent's default when no rule matches.
pub fn evaluate_agent_permissions(
    agent_permissions: &AgentPermissionsConfig,
    workspace_root: &Path,
    current_dir: &Path,
    request: &PermissionRequest,
) -> ResolvedPermissionDecision {
    let evaluator = PermissionRuleSet::from_agent_config(agent_permissions, workspace_root, current_dir);
    evaluator.resolve(request, agent_permissions.default)
}

/// Evaluates a permission request by combining global and agent-level rules.
/// Global deny is a hard ceiling; global ask forces a prompt unless the agent denies.
pub fn evaluate_effective_permissions(
    global_config: &PermissionsConfig,
    agent_permissions: &AgentPermissionsConfig,
    workspace_root: &Path,
    current_dir: &Path,
    request: &PermissionRequest,
) -> ResolvedPermissionDecision {
    let global = PermissionRuleSet::from_global_config(global_config, workspace_root, current_dir);
    let global_matches = global.evaluate_matches(request);
    if global_matches.deny {
        return ResolvedPermissionDecision::Deny;
    }

    let agent_decision = evaluate_agent_permissions(agent_permissions, workspace_root, current_dir, request);
    if global_matches.ask && agent_decision != ResolvedPermissionDecision::Deny {
        return ResolvedPermissionDecision::Ask;
    }

    agent_decision
}

struct PermissionRuleSet {
    deny: Vec<CompiledPermissionRule>,
    ask: Vec<CompiledPermissionRule>,
    auto: Vec<CompiledPermissionRule>,
    allow: Vec<CompiledPermissionRule>,
}

impl PermissionRuleSet {
    fn from_global_config(config: &PermissionsConfig, workspace_root: &Path, current_dir: &Path) -> Self {
        Self {
            deny: compile_rules(&config.deny, workspace_root, current_dir),
            ask: compile_rules(&config.ask, workspace_root, current_dir),
            auto: Vec::new(),
            allow: compile_rules(&config.allow, workspace_root, current_dir),
        }
    }

    fn from_agent_config(config: &AgentPermissionsConfig, workspace_root: &Path, current_dir: &Path) -> Self {
        Self {
            deny: compile_rules(&config.deny, workspace_root, current_dir),
            ask: compile_rules(&config.ask, workspace_root, current_dir),
            auto: compile_rules(&config.auto, workspace_root, current_dir),
            allow: compile_rules(&config.allow, workspace_root, current_dir),
        }
    }

    fn evaluate_matches(&self, request: &PermissionRequest) -> PermissionRuleMatches {
        PermissionRuleMatches {
            deny: self.deny.iter().any(|rule| rule.matches(request)),
            ask: self.ask.iter().any(|rule| rule.matches(request)),
            auto: self.auto.iter().any(|rule| rule.matches(request)),
            allow: self.allow.iter().any(|rule| rule.matches(request)),
        }
    }

    fn resolve(&self, request: &PermissionRequest, default: PermissionDefault) -> ResolvedPermissionDecision {
        let matches = self.evaluate_matches(request);
        if matches.deny {
            ResolvedPermissionDecision::Deny
        } else if matches.ask {
            ResolvedPermissionDecision::Ask
        } else if matches.auto {
            ResolvedPermissionDecision::Auto
        } else if matches.allow {
            ResolvedPermissionDecision::Allow
        } else {
            default.into()
        }
    }
}

impl From<PermissionDefault> for ResolvedPermissionDecision {
    fn from(default: PermissionDefault) -> Self {
        match default {
            PermissionDefault::Ask => Self::Ask,
            PermissionDefault::Allow => Self::Allow,
            PermissionDefault::Auto => Self::Auto,
            PermissionDefault::Deny => Self::Deny,
        }
    }
}

fn compile_rules(rules: &[String], workspace_root: &Path, current_dir: &Path) -> Vec<CompiledPermissionRule> {
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
        let rule = normalize_permission_rule(raw);
        let rule = rule.trim();
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
            let domain = specifier.strip_prefix("domain:")?.trim().to_ascii_lowercase();
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
                    return Some(Self::McpTool { server: server.to_string(), tool: tool.to_string() });
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
                PermissionRequestKind::Bash { command } => {
                    pattern.as_ref().is_none_or(|pattern| pattern.matches(command))
                }
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
                PermissionRequestKind::WebFetch { domains } => {
                    domains.iter().any(|candidate| domain_matches_allowed(candidate, domain))
                }
                _ => false,
            },
            Self::McpServer(server) | Self::McpWildcard(server) => match &request.kind {
                PermissionRequestKind::Mcp { server: candidate, .. } => candidate == server,
                _ => false,
            },
            Self::McpTool { server, tool } => match &request.kind {
                PermissionRequestKind::Mcp { server: candidate_server, tool: candidate_tool } => {
                    candidate_server == server && candidate_tool == tool
                }
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
    prefix.eq_ignore_ascii_case(tool_name).then_some(rule[open + 1..close].trim())
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
            (PathBuf::from("/"), format!("/{path}"))
        } else if let Some(path) = raw.strip_prefix("~/") {
            (home_dir?, format!("/{path}"))
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
        self.matcher.matched_path_or_any_parents(candidate, false).is_ignore()
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

    if normalized_tool_name == tools::CODE_SEARCH {
        let paths = tool_args.map_or_else(Vec::new, |args| {
            extract_candidate_paths(workspace_root, current_dir, normalized_tool_name, args)
        });
        return PermissionRequestKind::Read { paths };
    }

    let Some(args) = tool_args else {
        return PermissionRequestKind::Other;
    };

    if normalized_tool_name == tools::EXEC_COMMAND {
        let command = command_args::command_text(args).ok().flatten().unwrap_or_default();
        return PermissionRequestKind::Bash { command };
    }

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

fn advertised_permission_args(normalized_tool_name: &str) -> Vec<Value> {
    match normalized_tool_name {
        tools::UNIFIED_EXEC | tools::EXEC_PTY_CMD | "exec" => {
            vec![json!({ "action": "run", "command": "true" })]
        }
        tools::EXEC_COMMAND => vec![json!({ "cmd": "rg --files" })],
        tools::RUN_PTY_CMD | tools::CREATE_PTY_SESSION | tools::SHELL | "bash" => {
            vec![json!({ "command": "true" })]
        }
        tools::READ_FILE | tools::GREP_FILE | tools::LIST_FILES => {
            vec![json!({ "path": "." })]
        }
        tools::CODE_SEARCH => vec![json!({ "query": "probe", "path": "." })],
        tools::WRITE_FILE | tools::CREATE_FILE | tools::DELETE_FILE => {
            vec![json!({ "path": "advertised-permission-probe.txt" })]
        }
        tools::MOVE_FILE | tools::COPY_FILE => vec![json!({
            "path": "advertised-permission-probe.txt",
            "destination": "advertised-permission-probe-copy.txt"
        })],
        tools::EDIT_FILE | tools::SEARCH_REPLACE => {
            vec![json!({ "path": "advertised-permission-probe.txt" })]
        }
        tools::APPLY_PATCH => vec![json!({
            "patch": "*** Begin Patch\n*** Update File: advertised-permission-probe.txt\n@@\n-old\n+new\n*** End Patch\n"
        })],
        tools::FILE_OP => vec![json!({ "path": "advertised-permission-probe.txt" })],
        tools::UNIFIED_FILE => vec![
            json!({ "action": "read", "path": "." }),
            json!({ "action": "edit", "path": "advertised-permission-probe.txt" }),
            json!({ "action": "write", "path": "advertised-permission-probe.txt" }),
        ],
        tools::WEB_FETCH | tools::FETCH_URL => vec![json!({ "url": "https://example.com/" })],
        _ => Vec::new(),
    }
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

fn is_web_fetch_request(normalized_tool_name: &str, _args: &Value) -> bool {
    normalized_tool_name == tools::WEB_FETCH || normalized_tool_name == tools::FETCH_URL
}

fn file_request_kind(
    workspace_root: &Path,
    current_dir: &Path,
    normalized_tool_name: &str,
    args: &Value,
) -> Option<PermissionRequestKind> {
    let paths = extract_candidate_paths(workspace_root, current_dir, normalized_tool_name, args);

    match normalized_tool_name {
        tools::READ_FILE | tools::GREP_FILE | tools::LIST_FILES | tools::CODE_SEARCH => {
            Some(PermissionRequestKind::Read { paths })
        }
        tools::WRITE_FILE | tools::CREATE_FILE | tools::DELETE_FILE | tools::MOVE_FILE | tools::COPY_FILE => {
            Some(PermissionRequestKind::Write { paths })
        }
        tools::EDIT_FILE | tools::APPLY_PATCH | tools::SEARCH_REPLACE | tools::FILE_OP => {
            Some(PermissionRequestKind::Edit { paths })
        }
        tools::UNIFIED_FILE => match tool_intent::file_operation_action(args) {
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
        for key in ["path", "file_path", "filepath", "target_path", "destination"] {
            if let Some(path) = obj.get(key).and_then(Value::as_str) {
                push_resolved_path(&mut paths, workspace_root, current_dir, path);
            }
        }
    }

    if normalized_tool_name == tools::APPLY_PATCH {
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

fn push_resolved_path(paths: &mut Vec<PathBuf>, workspace_root: &Path, current_dir: &Path, raw: &str) {
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
    parsed.host_str().map(|host| host.trim_end_matches('.').to_ascii_lowercase())
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
    if matches!(as_string.as_str(), ".vtcode/commands" | ".vtcode/agents" | ".vtcode/skills")
        || as_string.starts_with(".vtcode/commands/")
        || as_string.starts_with(".vtcode/agents/")
        || as_string.starts_with(".vtcode/skills/")
    {
        return false;
    }

    matches!(as_string.split('/').next(), Some(".git" | ".vtcode" | ".vscode" | ".idea"))
}

fn domain_matches_allowed(domain: &str, allowed: &str) -> bool {
    let normalized_domain = domain.trim_end_matches('.').to_ascii_lowercase();
    let normalized_allowed = allowed.trim_start_matches('.').trim_end_matches('.').to_ascii_lowercase();

    normalized_domain == normalized_allowed || normalized_domain.ends_with(&format!(".{normalized_allowed}"))
}

#[cfg(test)]
mod tests {
    use super::{
        PermissionRequest, PermissionRequestKind, PermissionRuleDecision, ResolvedPermissionDecision,
        build_advertised_permission_requests, build_permission_request, evaluate_agent_permissions,
        evaluate_effective_permissions, evaluate_permissions,
    };
    use crate::config::{PermissionsConfig, constants::tools};
    use serde_json::json;
    use tempfile::TempDir;
    use vtcode_config::core::permissions::{AgentPermissionsConfig, PermissionDefault};

    fn workspace_roots() -> (TempDir, std::path::PathBuf, std::path::PathBuf) {
        let temp = TempDir::new().expect("temp dir");
        let workspace = temp.path().join("workspace");
        let cwd = workspace.join("nested");
        std::fs::create_dir_all(&cwd).expect("create dirs");
        (temp, workspace, cwd)
    }

    fn agent_permissions(default: PermissionDefault) -> AgentPermissionsConfig {
        AgentPermissionsConfig::new(default)
    }

    fn exact_tool_request(tool_name: &str) -> PermissionRequest {
        PermissionRequest {
            exact_tool_name: tool_name.to_string(),
            kind: PermissionRequestKind::Other,
            builtin_file_mutation: false,
            protected_write_paths: Vec::new(),
        }
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
            kind: PermissionRequestKind::Read { paths: vec![workspace.join("docs/secret.txt")] },
            builtin_file_mutation: false,
            protected_write_paths: Vec::new(),
        };

        assert_eq!(evaluate_permissions(&config, &workspace, &cwd, &request).decision(), PermissionRuleDecision::Deny);
    }

    #[test]
    fn bash_glob_matches_command_text() {
        let (_temp, workspace, cwd) = workspace_roots();
        let config = PermissionsConfig {
            allow: vec!["Bash(cargo test *)".to_string()],
            ..PermissionsConfig::default()
        };
        let request = PermissionRequest {
            exact_tool_name: "command_session".to_string(),
            kind: PermissionRequestKind::Bash { command: "cargo test -p vtcode".to_string() },
            builtin_file_mutation: false,
            protected_write_paths: Vec::new(),
        };

        assert_eq!(evaluate_permissions(&config, &workspace, &cwd, &request).decision(), PermissionRuleDecision::Allow);
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
            kind: PermissionRequestKind::Read { paths: vec![workspace.join("src/lib.rs")] },
            builtin_file_mutation: false,
            protected_write_paths: Vec::new(),
        };

        assert_eq!(evaluate_permissions(&config, &workspace, &cwd, &request).decision(), PermissionRuleDecision::Ask);
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

        assert_eq!(evaluate_permissions(&config, &workspace, &cwd, &request).decision(), PermissionRuleDecision::Allow);
    }

    #[test]
    fn protected_directory_exceptions_are_not_flagged() {
        let (_temp, workspace, cwd) = workspace_roots();
        let request = build_permission_request(
            &workspace,
            &cwd,
            tools::UNIFIED_FILE,
            Some(&json!({
                "action": "write",
                "path": "../.vtcode/skills/example.md"
            })),
        );
        assert!(!request.requires_protected_write_prompt());

        let request = build_permission_request(
            &workspace,
            &cwd,
            tools::UNIFIED_FILE,
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

        assert_eq!(evaluate_permissions(&config, &workspace, &cwd, &request).decision(), PermissionRuleDecision::Ask);
    }

    #[test]
    fn relative_paths_resolve_from_current_directory() {
        let (_temp, workspace, cwd) = workspace_roots();
        let config = PermissionsConfig {
            ask: vec!["Read(./nested-file.rs)".to_string()],
            ..PermissionsConfig::default()
        };
        let request = build_permission_request(&workspace, &cwd, "read_file", Some(&json!({"path": "nested-file.rs"})));

        assert_eq!(evaluate_permissions(&config, &workspace, &cwd, &request).decision(), PermissionRuleDecision::Ask);
    }

    #[test]
    fn exact_tool_rules_feed_rule_tiers() {
        let (_temp, workspace, cwd) = workspace_roots();
        // Use semantic rules which are the recommended approach
        let config = PermissionsConfig {
            allow: vec!["read".to_string()],
            deny: vec!["bash".to_string()],
            ..PermissionsConfig::default()
        };

        let read_request = PermissionRequest {
            exact_tool_name: "read_file".to_string(),
            kind: PermissionRequestKind::Read { paths: vec![] },
            builtin_file_mutation: false,
            protected_write_paths: Vec::new(),
        };
        let exec_request = PermissionRequest {
            exact_tool_name: "command_session".to_string(),
            kind: PermissionRequestKind::Bash { command: "test".to_string() },
            builtin_file_mutation: false,
            protected_write_paths: Vec::new(),
        };

        assert!(evaluate_permissions(&config, &workspace, &cwd, &read_request).allow);
        assert!(evaluate_permissions(&config, &workspace, &cwd, &exec_request).deny);
    }

    #[test]
    fn agent_deny_wins_over_ask_auto_allow_and_default() {
        let (_temp, workspace, cwd) = workspace_roots();
        // Use a custom tool name that won't be normalized to a semantic rule
        let request = exact_tool_request("custom_tool");
        let mut permissions = agent_permissions(PermissionDefault::Allow);
        permissions.allow = vec!["custom_tool".to_string()];
        permissions.auto = vec!["custom_tool".to_string()];
        permissions.ask = vec!["custom_tool".to_string()];
        permissions.deny = vec!["custom_tool".to_string()];

        assert_eq!(
            evaluate_agent_permissions(&permissions, &workspace, &cwd, &request),
            ResolvedPermissionDecision::Deny
        );
    }

    #[test]
    fn agent_ask_wins_over_auto_allow_and_default() {
        let (_temp, workspace, cwd) = workspace_roots();
        // Use a custom tool name that won't be normalized to a semantic rule
        let request = exact_tool_request("custom_tool");
        let mut permissions = agent_permissions(PermissionDefault::Deny);
        permissions.allow = vec!["custom_tool".to_string()];
        permissions.auto = vec!["custom_tool".to_string()];
        permissions.ask = vec!["custom_tool".to_string()];

        assert_eq!(
            evaluate_agent_permissions(&permissions, &workspace, &cwd, &request),
            ResolvedPermissionDecision::Ask
        );
    }

    #[test]
    fn agent_auto_wins_over_allow_and_default() {
        let (_temp, workspace, cwd) = workspace_roots();
        // Use a custom tool name that won't be normalized to a semantic rule
        let request = exact_tool_request("custom_tool");
        let mut permissions = agent_permissions(PermissionDefault::Deny);
        permissions.allow = vec!["custom_tool".to_string()];
        permissions.auto = vec!["custom_tool".to_string()];

        assert_eq!(
            evaluate_agent_permissions(&permissions, &workspace, &cwd, &request),
            ResolvedPermissionDecision::Auto
        );
    }

    #[test]
    fn agent_allow_wins_over_default() {
        let (_temp, workspace, cwd) = workspace_roots();
        // Use a custom tool name that won't be normalized to a semantic rule
        let request = exact_tool_request("custom_tool");
        let mut permissions = agent_permissions(PermissionDefault::Deny);
        permissions.allow = vec!["custom_tool".to_string()];

        assert_eq!(
            evaluate_agent_permissions(&permissions, &workspace, &cwd, &request),
            ResolvedPermissionDecision::Allow
        );
    }

    #[test]
    fn agent_read_permission_allows_file_operation_read_only() {
        let (_temp, workspace, cwd) = workspace_roots();
        let mut permissions = agent_permissions(PermissionDefault::Deny);
        permissions.allow = vec!["read".to_string()];

        let read_request = build_permission_request(
            &workspace,
            &cwd,
            tools::UNIFIED_FILE,
            Some(&json!({"action": "read", "path": "README.md"})),
        );
        let write_request = build_permission_request(
            &workspace,
            &cwd,
            tools::UNIFIED_FILE,
            Some(&json!({"action": "write", "path": "README.md", "content": "x"})),
        );

        assert_eq!(
            evaluate_agent_permissions(&permissions, &workspace, &cwd, &read_request),
            ResolvedPermissionDecision::Allow
        );
        assert_eq!(
            evaluate_agent_permissions(&permissions, &workspace, &cwd, &write_request),
            ResolvedPermissionDecision::Deny
        );
    }

    #[test]
    fn agent_read_permission_allows_code_search_but_not_exec_command() {
        let (_temp, workspace, cwd) = workspace_roots();
        let mut permissions = agent_permissions(PermissionDefault::Deny);
        permissions.allow = vec!["Read".to_string()];

        let code_search = build_permission_request(
            &workspace,
            &cwd,
            tools::CODE_SEARCH,
            Some(&json!({"query": "PermissionRequest"})),
        );
        let exec_command =
            build_permission_request(&workspace, &cwd, tools::EXEC_COMMAND, Some(&json!({"cmd": "rg --files"})));

        assert!(matches!(code_search.kind, PermissionRequestKind::Read { .. }));
        assert_eq!(exec_command.kind, PermissionRequestKind::Bash { command: "rg --files".to_string() });
        assert_eq!(
            evaluate_agent_permissions(&permissions, &workspace, &cwd, &code_search),
            ResolvedPermissionDecision::Allow
        );
        assert_eq!(
            evaluate_agent_permissions(&permissions, &workspace, &cwd, &exec_command),
            ResolvedPermissionDecision::Deny
        );
    }

    #[test]
    fn dry_run_exec_command_remains_bash_under_read_only_permissions() {
        let (_temp, workspace, cwd) = workspace_roots();
        let mut permissions = agent_permissions(PermissionDefault::Deny);
        permissions.allow = vec!["Read".to_string()];
        let request = build_permission_request(
            &workspace,
            &cwd,
            tools::EXEC_COMMAND,
            Some(&json!({"cmd": "python mutate.py --dry-run"})),
        );

        assert_eq!(request.kind, PermissionRequestKind::Bash { command: "python mutate.py --dry-run".to_string() });
        assert_eq!(
            evaluate_agent_permissions(&permissions, &workspace, &cwd, &request),
            ResolvedPermissionDecision::Deny
        );
    }

    #[test]
    fn exec_command_advertisement_uses_bash_permission() {
        let (_temp, workspace, cwd) = workspace_roots();
        let requests = build_advertised_permission_requests(&workspace, &cwd, tools::EXEC_COMMAND);

        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].kind, PermissionRequestKind::Bash { command: "rg --files".to_string() });
    }

    #[test]
    fn public_read_requests_preserve_global_deny_and_ask_precedence() {
        let (_temp, workspace, cwd) = workspace_roots();
        let request = build_permission_request(
            &workspace,
            &cwd,
            tools::CODE_SEARCH,
            Some(&json!({"query": "PermissionRequest"})),
        );
        let mut permissions = agent_permissions(PermissionDefault::Deny);
        permissions.allow = vec!["Read".to_string()];

        let deny = PermissionsConfig {
            deny: vec!["Read".to_string()],
            ..PermissionsConfig::default()
        };
        assert_eq!(
            evaluate_effective_permissions(&deny, &permissions, &workspace, &cwd, &request),
            ResolvedPermissionDecision::Deny
        );

        let ask = PermissionsConfig {
            ask: vec!["Read".to_string()],
            ..PermissionsConfig::default()
        };
        assert_eq!(
            evaluate_effective_permissions(&ask, &permissions, &workspace, &cwd, &request),
            ResolvedPermissionDecision::Ask
        );
    }

    #[test]
    fn explicit_bash_rules_continue_to_govern_mutating_exec_command() {
        let (_temp, workspace, cwd) = workspace_roots();
        let request = build_permission_request(
            &workspace,
            &cwd,
            tools::EXEC_COMMAND,
            Some(&json!({"cmd": "printf changed > file.txt"})),
        );
        let mut permissions = agent_permissions(PermissionDefault::Deny);
        permissions.allow = vec!["Bash(printf changed*)".to_string()];

        assert_eq!(
            evaluate_agent_permissions(&permissions, &workspace, &cwd, &request),
            ResolvedPermissionDecision::Allow
        );

        let global = PermissionsConfig {
            deny: vec!["Bash(printf changed*)".to_string()],
            ..PermissionsConfig::default()
        };
        assert_eq!(
            evaluate_effective_permissions(&global, &permissions, &workspace, &cwd, &request),
            ResolvedPermissionDecision::Deny
        );
    }

    #[test]
    fn unmatched_agent_calls_use_permissions_default() {
        let (_temp, workspace, cwd) = workspace_roots();
        let request = exact_tool_request("read_file");
        let permissions = agent_permissions(PermissionDefault::Auto);

        assert_eq!(
            evaluate_agent_permissions(&permissions, &workspace, &cwd, &request),
            ResolvedPermissionDecision::Auto
        );
    }

    #[test]
    fn missing_permissions_default_is_invalid_before_evaluation() {
        let err = toml::from_str::<AgentPermissionsConfig>(r#"allow = ["read_file"]"#).unwrap_err();

        assert!(err.to_string().contains("missing field `default`"));
    }

    #[test]
    fn global_deny_is_hard_ceiling() {
        let (_temp, workspace, cwd) = workspace_roots();
        // Use a custom tool name that won't be normalized to a semantic rule
        let request = exact_tool_request("custom_tool");
        let global = PermissionsConfig {
            deny: vec!["custom_tool".to_string()],
            ..PermissionsConfig::default()
        };
        let permissions = agent_permissions(PermissionDefault::Allow);

        assert_eq!(
            evaluate_effective_permissions(&global, &permissions, &workspace, &cwd, &request),
            ResolvedPermissionDecision::Deny
        );
    }

    #[test]
    fn global_ask_forces_prompt_over_agent_allow_or_auto() {
        let (_temp, workspace, cwd) = workspace_roots();
        // Use a custom tool name that won't be normalized to a semantic rule
        let request = exact_tool_request("custom_tool");
        let global = PermissionsConfig {
            ask: vec!["custom_tool".to_string()],
            ..PermissionsConfig::default()
        };

        assert_eq!(
            evaluate_effective_permissions(
                &global,
                &agent_permissions(PermissionDefault::Allow),
                &workspace,
                &cwd,
                &request,
            ),
            ResolvedPermissionDecision::Ask
        );
        assert_eq!(
            evaluate_effective_permissions(
                &global,
                &agent_permissions(PermissionDefault::Auto),
                &workspace,
                &cwd,
                &request,
            ),
            ResolvedPermissionDecision::Ask
        );
    }

    #[test]
    fn global_allow_cannot_override_agent_deny_or_auto() {
        let (_temp, workspace, cwd) = workspace_roots();
        let request = exact_tool_request("command_session");
        let global = PermissionsConfig {
            allow: vec!["command_session".to_string()],
            ..PermissionsConfig::default()
        };

        assert_eq!(
            evaluate_effective_permissions(
                &global,
                &agent_permissions(PermissionDefault::Deny),
                &workspace,
                &cwd,
                &request,
            ),
            ResolvedPermissionDecision::Deny
        );
        assert_eq!(
            evaluate_effective_permissions(
                &global,
                &agent_permissions(PermissionDefault::Auto),
                &workspace,
                &cwd,
                &request,
            ),
            ResolvedPermissionDecision::Auto
        );
    }

    #[test]
    fn agent_specific_deny_wins_within_agent_scope() {
        let (_temp, workspace, cwd) = workspace_roots();
        // Use a custom tool name that won't be normalized to a semantic rule
        let request = exact_tool_request("custom_tool");
        let global = PermissionsConfig {
            allow: vec!["custom_tool".to_string()],
            ..PermissionsConfig::default()
        };
        let mut permissions = agent_permissions(PermissionDefault::Allow);
        permissions.deny = vec!["custom_tool".to_string()];

        assert_eq!(
            evaluate_effective_permissions(&global, &permissions, &workspace, &cwd, &request),
            ResolvedPermissionDecision::Deny
        );
    }

    #[test]
    fn auto_bucket_resolves_to_classifier_backed_decision() {
        let (_temp, workspace, cwd) = workspace_roots();
        // Use a custom tool name that won't be normalized to a semantic rule
        let request = exact_tool_request("custom_tool");
        let mut permissions = agent_permissions(PermissionDefault::Ask);
        permissions.auto = vec!["custom_tool".to_string()];

        assert_eq!(
            evaluate_agent_permissions(&permissions, &workspace, &cwd, &request),
            ResolvedPermissionDecision::Auto
        );
    }
}
