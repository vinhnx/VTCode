use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;
use vtcode_core::config::constants::model_helpers;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::models::{ModelId, Provider};
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::git_info::{get_git_remote_urls, get_git_repo_root, get_head_commit_hash};
use vtcode_core::llm::factory::{
    ProviderConfig, create_provider_with_config, infer_provider_from_model,
};
use vtcode_core::llm::provider as uni;
use vtcode_core::tools::ToolRegistry;
use vtcode_tui::{InlineHeaderHighlight, InlineHeaderStatusBadge, InlineHeaderStatusTone};

use crate::agent::runloop::unified::state::SessionStats;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PromptSuggestion {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) prompt: String,
    pub(crate) subtitle: Option<String>,
    pub(crate) badge: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BackgroundJobSummary {
    pub(crate) id: String,
    pub(crate) command: String,
    pub(crate) working_dir: Option<String>,
    pub(crate) status: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PrReviewStatus {
    NotApplicable,
    GhMissing,
    AuthRequired,
    NotOnPr,
    Ready { url: Option<String> },
    ReviewedCurrent { url: Option<String> },
    ReviewOutdated { url: Option<String> },
    ChangesRequested { url: Option<String> },
    NoWriteAccess { url: Option<String> },
    Error(String),
}

impl PrReviewStatus {
    pub(crate) fn to_badge(&self) -> Option<InlineHeaderStatusBadge> {
        let (text, tone) = match self {
            Self::NotApplicable | Self::NotOnPr => return None,
            Self::GhMissing => (
                "PR: install gh".to_string(),
                InlineHeaderStatusTone::Warning,
            ),
            Self::AuthRequired => ("PR: gh auth".to_string(), InlineHeaderStatusTone::Warning),
            Self::Ready { .. } => ("PR: ready".to_string(), InlineHeaderStatusTone::Ready),
            Self::ReviewedCurrent { .. } => {
                ("PR: reviewed".to_string(), InlineHeaderStatusTone::Ready)
            }
            Self::ReviewOutdated { .. } => {
                ("PR: outdated".to_string(), InlineHeaderStatusTone::Warning)
            }
            Self::ChangesRequested { .. } => {
                ("PR: changes".to_string(), InlineHeaderStatusTone::Warning)
            }
            Self::NoWriteAccess { .. } => {
                ("PR: read-only".to_string(), InlineHeaderStatusTone::Warning)
            }
            Self::Error(_) => ("PR: error".to_string(), InlineHeaderStatusTone::Error),
        };

        Some(InlineHeaderStatusBadge { text, tone })
    }

    pub(crate) fn to_highlight(&self) -> Option<InlineHeaderHighlight> {
        let lines = match self {
            Self::GhMissing => {
                vec!["Install GitHub CLI (`gh`) to show PR review status.".to_string()]
            }
            Self::AuthRequired => {
                vec!["Run `gh auth login` to enable PR review status in the header.".to_string()]
            }
            Self::ReviewOutdated { url } => vec![match url {
                Some(url) => format!(
                    "Your previous review is behind the current PR head. Review the latest commit: {url}"
                ),
                None => "Your previous review is behind the current PR head.".to_string(),
            }],
            Self::ChangesRequested { url } => vec![match url {
                Some(url) => format!(
                    "This PR currently has requested changes. Re-check the discussion: {url}"
                ),
                None => "This PR currently has requested changes.".to_string(),
            }],
            Self::NoWriteAccess { url } => vec![match url {
                Some(url) => format!(
                    "You do not appear to have write access for this PR branch. Open in GitHub: {url}"
                ),
                None => "You do not appear to have write access for this PR branch.".to_string(),
            }],
            Self::Error(message) => vec![format!("PR status refresh failed: {message}")],
            _ => return None,
        };

        Some(InlineHeaderHighlight {
            title: "PR Review".to_string(),
            lines,
        })
    }
}

#[derive(Clone, Debug)]
struct CachedPrStatus {
    status: PrReviewStatus,
    stored_at: Instant,
}

static PR_STATUS_CACHE: LazyLock<Mutex<HashMap<String, CachedPrStatus>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static PROMPT_SUGGESTION_CACHE: LazyLock<Mutex<HashMap<String, Vec<PromptSuggestion>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
const PR_STATUS_CACHE_TTL: Duration = Duration::from_secs(30);
const PROMPT_SUGGESTION_CACHE_LIMIT: usize = 64;
const DEFAULT_PROMPT_SUGGESTION_TEMPERATURE: f32 = 0.4;

#[derive(Clone, Debug, PartialEq)]
struct PromptSuggestionRoute {
    provider_name: String,
    model: String,
    temperature: f32,
}

impl PromptSuggestionRoute {
    fn cache_key(&self) -> String {
        format!(
            "{}:{}:{:.2}",
            self.provider_name, self.model, self.temperature
        )
    }
}

pub(crate) async fn generate_prompt_suggestions(
    provider: &dyn uni::LLMProvider,
    config: &CoreAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
    workspace: &Path,
    history: &[uni::Message],
    session_stats: &SessionStats,
    tool_registry: &ToolRegistry,
) -> Vec<PromptSuggestion> {
    let route = resolve_prompt_suggestion_route(config, vt_cfg);
    let cache_key =
        prompt_suggestion_cache_key(&route, workspace, history, session_stats, tool_registry);
    if let Some(cached) = PROMPT_SUGGESTION_CACHE
        .lock()
        .ok()
        .and_then(|cache| cache.get(&cache_key).cloned())
    {
        return cached;
    }

    let fallback =
        deterministic_prompt_suggestions(workspace, history, session_stats, tool_registry);
    let llm_generated = llm_prompt_suggestions(provider, config, vt_cfg, &route, history).await;
    let resolved = if llm_generated.is_empty() {
        fallback
    } else {
        llm_generated
    };

    if let Ok(mut cache) = PROMPT_SUGGESTION_CACHE.lock() {
        if cache.len() >= PROMPT_SUGGESTION_CACHE_LIMIT {
            cache.clear();
        }
        cache.insert(cache_key, resolved.clone());
    }

    resolved
}

fn deterministic_prompt_suggestions(
    workspace: &Path,
    history: &[uni::Message],
    session_stats: &SessionStats,
    tool_registry: &ToolRegistry,
) -> Vec<PromptSuggestion> {
    let mut suggestions = Vec::new();

    if session_stats.is_plan_mode() {
        suggestions.push(PromptSuggestion {
            id: "plan-refine".to_string(),
            title: "Refine the current plan".to_string(),
            prompt: "Refine the current plan, close any remaining open decisions, and keep it implementation-ready.".to_string(),
            subtitle: Some("Useful while Plan Mode is active.".to_string()),
            badge: Some("Plan".to_string()),
        });
    }

    if session_stats.task_panel_visible {
        suggestions.push(PromptSuggestion {
            id: "task-next".to_string(),
            title: "Advance the current tasks".to_string(),
            prompt: "Review the current task checklist, identify the top pending item, and continue with the smallest concrete next step.".to_string(),
            subtitle: Some("Uses the dedicated TODO/task panel state.".to_string()),
            badge: Some("Tasks".to_string()),
        });
    }

    if tool_registry.active_pty_sessions() > 0 {
        suggestions.push(PromptSuggestion {
            id: "jobs-check".to_string(),
            title: "Check running jobs".to_string(),
            prompt: "Inspect the active jobs, summarize which one matters most, and tell me the next action.".to_string(),
            subtitle: Some("Derived from active PTY sessions.".to_string()),
            badge: Some("Jobs".to_string()),
        });
    }

    if let Some(last_error) = history.iter().rev().find_map(last_error_like_message) {
        suggestions.push(PromptSuggestion {
            id: "last-error".to_string(),
            title: "Investigate the latest failure".to_string(),
            prompt: format!(
                "Investigate the latest failure and propose the smallest next fix. Context: {}",
                truncate_for_prompt(&last_error, 180)
            ),
            subtitle: Some("Based on the most recent error-like output.".to_string()),
            badge: Some("Debug".to_string()),
        });
    }

    let touched = session_stats.recent_touched_files();
    if !touched.is_empty() {
        suggestions.push(PromptSuggestion {
            id: "review-touched".to_string(),
            title: "Continue from recent files".to_string(),
            prompt: format!(
                "Review the recent changes in {} and continue with the next concrete step.",
                touched.join(", ")
            ),
            subtitle: Some("Uses the most recently touched files.".to_string()),
            badge: Some("Files".to_string()),
        });
    }

    if let Ok(Some(summary)) = crate::agent::runloop::git::git_status_summary(workspace) {
        let dirty_label = if summary.dirty { "dirty" } else { "clean" };
        suggestions.push(PromptSuggestion {
            id: "git-state".to_string(),
            title: "Review git state".to_string(),
            prompt: format!(
                "Review the current git state on branch `{}` ({}), highlight the most important change, and suggest the next action.",
                summary.branch, dirty_label
            ),
            subtitle: Some("Derived from the current git branch and dirty state.".to_string()),
            badge: Some("Git".to_string()),
        });
    }

    suggestions.push(PromptSuggestion {
        id: "review-diff".to_string(),
        title: "Review the current diff".to_string(),
        prompt:
            "Review the current diff, call out the highest-risk issue, and suggest the next change."
                .to_string(),
        subtitle: Some("General follow-up for active coding sessions.".to_string()),
        badge: Some("Review".to_string()),
    });

    dedupe_prompt_suggestions(suggestions)
}

async fn llm_prompt_suggestions(
    provider: &dyn uni::LLMProvider,
    config: &CoreAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
    route: &PromptSuggestionRoute,
    history: &[uni::Message],
) -> Vec<PromptSuggestion> {
    let context = recent_history_summary(history);
    if context.trim().is_empty() {
        return Vec::new();
    }

    let routed = llm_prompt_suggestions_with_route(route, config, vt_cfg, history).await;
    if !routed.is_empty() {
        return routed;
    }

    llm_prompt_suggestions_from_provider(
        provider,
        &config.model,
        DEFAULT_PROMPT_SUGGESTION_TEMPERATURE,
        history,
    )
    .await
}

async fn llm_prompt_suggestions_with_route(
    route: &PromptSuggestionRoute,
    config: &CoreAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
    history: &[uni::Message],
) -> Vec<PromptSuggestion> {
    if route.model == config.model {
        return Vec::new();
    }

    let Ok(provider) = create_provider_with_config(
        &route.provider_name,
        ProviderConfig {
            api_key: Some(config.api_key.clone()),
            openai_chatgpt_auth: config.openai_chatgpt_auth.clone(),
            base_url: None,
            model: Some(route.model.clone()),
            prompt_cache: Some(config.prompt_cache.clone()),
            timeouts: None,
            openai: vt_cfg.map(|cfg| cfg.provider.openai.clone()),
            anthropic: vt_cfg.map(|cfg| cfg.provider.anthropic.clone()),
            model_behavior: vt_cfg.map(|cfg| cfg.model.clone()),
        },
    ) else {
        return Vec::new();
    };

    llm_prompt_suggestions_from_provider(&*provider, &route.model, route.temperature, history).await
}

async fn llm_prompt_suggestions_from_provider(
    provider: &dyn uni::LLMProvider,
    model: &str,
    temperature: f32,
    history: &[uni::Message],
) -> Vec<PromptSuggestion> {
    let context = recent_history_summary(history);
    if context.trim().is_empty() {
        return Vec::new();
    }

    let request = uni::LLMRequest {
        messages: vec![uni::Message::user(format!(
            "Generate 3 short follow-up prompts for this VT Code session. Return one prompt per line.\n\nRecent session context:\n{}",
            context
        ))],
        system_prompt: Some(std::sync::Arc::new(
            "You write concise follow-up prompts for a coding assistant UI. Return plain text only, one prompt per line, no bullets or numbering.".to_string(),
        )),
        model: model.to_string(),
        max_tokens: Some(180),
        temperature: Some(temperature),
        tool_choice: Some(uni::ToolChoice::None),
        ..Default::default()
    };

    let Ok(response) = provider.generate(request).await else {
        return Vec::new();
    };
    let Some(content) = response.content else {
        return Vec::new();
    };

    let suggestions = content
        .lines()
        .map(|line| {
            line.trim()
                .trim_start_matches('-')
                .trim_start_matches('•')
                .trim()
                .to_string()
        })
        .filter(|line| !line.is_empty())
        .take(3)
        .enumerate()
        .map(|(index, prompt)| PromptSuggestion {
            id: format!("llm-{index}"),
            title: truncate_for_prompt(&prompt, 56),
            prompt,
            subtitle: Some("Suggested from recent session context.".to_string()),
            badge: Some("Suggested".to_string()),
        })
        .collect::<Vec<_>>();

    dedupe_prompt_suggestions(suggestions)
}

pub(crate) fn collect_background_jobs(tool_registry: &ToolRegistry) -> Vec<BackgroundJobSummary> {
    let mut jobs = tool_registry
        .pty_manager()
        .list_sessions()
        .into_iter()
        .map(|session| {
            let status = match tool_registry
                .pty_manager()
                .is_session_completed(&session.id)
            {
                Ok(Some(0)) => "done".to_string(),
                Ok(Some(code)) => format!("exit {code}"),
                Ok(None) => "running".to_string(),
                Err(_) => "unknown".to_string(),
            };
            BackgroundJobSummary {
                id: session.id,
                command: session.command,
                working_dir: session.working_dir,
                status,
            }
        })
        .collect::<Vec<_>>();

    jobs.sort_by(|left, right| left.id.cmp(&right.id));
    jobs
}

pub(crate) fn detect_pr_review_status(workspace: &Path) -> PrReviewStatus {
    let remotes = match get_git_remote_urls(workspace) {
        Ok(remotes) => remotes,
        Err(_) => return PrReviewStatus::NotApplicable,
    };
    if remotes.is_empty()
        || !remotes
            .values()
            .any(|remote| remote.contains("github.com") || remote.contains("git@github.com"))
    {
        return PrReviewStatus::NotApplicable;
    }

    let head = match get_head_commit_hash(workspace) {
        Ok(Some(head)) => head,
        Ok(None) => return PrReviewStatus::NotApplicable,
        Err(_) => return PrReviewStatus::NotApplicable,
    };
    let branch = match crate::agent::runloop::git::git_status_summary(workspace) {
        Ok(Some(summary)) => summary.branch,
        Ok(None) => "unknown".to_string(),
        Err(_) => return PrReviewStatus::NotApplicable,
    };
    let repo_root = match get_git_repo_root(workspace) {
        Ok(Some(root)) => root,
        Ok(None) => workspace.display().to_string(),
        Err(_) => workspace.display().to_string(),
    };
    let cache_key = format!("{repo_root}:{branch}:{head}");
    if let Some(cached) = PR_STATUS_CACHE.lock().ok().and_then(|cache| {
        cache
            .get(&cache_key)
            .filter(|cached| cached.stored_at.elapsed() < PR_STATUS_CACHE_TTL)
            .cloned()
    }) {
        return cached.status;
    }

    let status = detect_pr_review_status_uncached(workspace, &head);
    if let Ok(mut cache) = PR_STATUS_CACHE.lock() {
        cache.insert(
            cache_key,
            CachedPrStatus {
                status: status.clone(),
                stored_at: Instant::now(),
            },
        );
    }
    status
}

fn detect_pr_review_status_uncached(workspace: &Path, local_head: &str) -> PrReviewStatus {
    if !command_succeeds("gh", &["--version"], workspace) {
        return PrReviewStatus::GhMissing;
    }
    if !command_succeeds(
        "gh",
        &["auth", "status", "--hostname", "github.com"],
        workspace,
    ) {
        return PrReviewStatus::AuthRequired;
    }

    let viewer = run_command("gh", &["api", "user", "--jq", ".login"], workspace)
        .ok()
        .map(|output| output.trim().to_string())
        .filter(|value| !value.is_empty());

    let pr_json = run_command(
        "gh",
        &[
            "pr",
            "view",
            "--json",
            "reviewDecision,headRefOid,latestReviews,maintainerCanModify,url",
        ],
        workspace,
    )
    .or_else(|_| {
        run_command(
            "gh",
            &["pr", "view", "--json", "reviewDecision,headRefOid,url"],
            workspace,
        )
    });

    let pr_json = match pr_json {
        Ok(json) => json,
        Err(err) => {
            let lower = err.to_lowercase();
            if lower.contains("no pull requests found") || lower.contains("not found") {
                return PrReviewStatus::NotOnPr;
            }
            return PrReviewStatus::Error(err);
        }
    };

    let Ok(payload) = serde_json::from_str::<Value>(&pr_json) else {
        return PrReviewStatus::Error("Invalid JSON from `gh pr view`".to_string());
    };
    let url = payload
        .get("url")
        .and_then(Value::as_str)
        .map(str::to_string);
    let head_ref_oid = payload
        .get("headRefOid")
        .and_then(Value::as_str)
        .map(normalize_sha);
    let review_decision = payload
        .get("reviewDecision")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let maintainer_can_modify = payload
        .get("maintainerCanModify")
        .and_then(Value::as_bool)
        .unwrap_or(true);

    if !maintainer_can_modify {
        return PrReviewStatus::NoWriteAccess { url };
    }

    if review_decision == "CHANGES_REQUESTED" {
        return PrReviewStatus::ChangesRequested { url };
    }

    let reviewed_by_viewer = payload
        .get("latestReviews")
        .and_then(Value::as_array)
        .map(|reviews| {
            reviews.iter().any(|review| {
                let author_login = review
                    .get("author")
                    .and_then(|author| author.get("login"))
                    .and_then(Value::as_str);
                match (&viewer, author_login) {
                    (Some(viewer), Some(author_login)) => viewer == author_login,
                    _ => false,
                }
            })
        })
        .unwrap_or(false);

    if reviewed_by_viewer {
        if head_ref_oid.as_deref() == Some(normalize_sha(local_head).as_str()) {
            return PrReviewStatus::ReviewedCurrent { url };
        }
        return PrReviewStatus::ReviewOutdated { url };
    }

    PrReviewStatus::Ready { url }
}

fn last_error_like_message(message: &uni::Message) -> Option<String> {
    let text = message.content.as_text();
    let lower = text.to_lowercase();
    ["error", "failed", "denied", "panic", "timeout"]
        .iter()
        .any(|needle| lower.contains(needle))
        .then(|| text.to_string())
}

fn prompt_suggestion_cache_key(
    route: &PromptSuggestionRoute,
    workspace: &Path,
    history: &[uni::Message],
    session_stats: &SessionStats,
    tool_registry: &ToolRegistry,
) -> String {
    let recent_history = history
        .iter()
        .rev()
        .take(4)
        .map(|message| truncate_for_prompt(message.content.as_text().trim(), 120))
        .collect::<Vec<_>>()
        .join("|");
    format!(
        "{}:{}:{}:{}:{}:{}:{}:{}",
        route.cache_key(),
        workspace.display(),
        history.len(),
        session_stats.is_plan_mode(),
        session_stats.task_panel_visible,
        tool_registry.active_pty_sessions(),
        git_status_fragment(workspace),
        recent_history
    )
}

fn resolve_prompt_suggestion_route(
    config: &CoreAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
) -> PromptSuggestionRoute {
    let provider_name = prompt_suggestion_provider_name(config);
    let fallback = PromptSuggestionRoute {
        provider_name: provider_name.clone(),
        model: config.model.clone(),
        temperature: DEFAULT_PROMPT_SUGGESTION_TEMPERATURE,
    };

    let Some(vt_cfg) = vt_cfg else {
        return fallback;
    };
    if !vt_cfg.agent.small_model.enabled {
        return fallback;
    }

    let model = if vt_cfg.agent.small_model.model.trim().is_empty() {
        auto_small_model(&provider_name, &config.model)
    } else {
        vt_cfg.agent.small_model.model.clone()
    };

    PromptSuggestionRoute {
        provider_name,
        model,
        temperature: vt_cfg.agent.small_model.temperature,
    }
}

fn prompt_suggestion_provider_name(config: &CoreAgentConfig) -> String {
    if !config.provider.trim().is_empty() {
        return config.provider.to_lowercase();
    }

    infer_provider_from_model(&config.model)
        .map(|provider| provider.to_string().to_lowercase())
        .unwrap_or_else(|| "gemini".to_string())
}

fn auto_small_model(provider_name: &str, active_model: &str) -> String {
    if let Ok(model_id) = active_model.parse::<ModelId>()
        && model_id.is_efficient_variant()
    {
        return model_id.as_str().to_string();
    }

    let provider = infer_provider_from_model(active_model).unwrap_or(match provider_name {
        "openai" => Provider::OpenAI,
        "anthropic" => Provider::Anthropic,
        "deepseek" => Provider::DeepSeek,
        "gemini" | "google" => Provider::Gemini,
        _ => Provider::Gemini,
    });

    match provider {
        Provider::OpenAI => ModelId::GPT5Mini.as_str().to_string(),
        Provider::Anthropic => ModelId::ClaudeHaiku45.as_str().to_string(),
        Provider::DeepSeek => ModelId::DeepSeekChat.as_str().to_string(),
        Provider::Gemini => ModelId::Gemini3FlashPreview.as_str().to_string(),
        _ => model_helpers::default_for(provider_name)
            .unwrap_or(active_model)
            .to_string(),
    }
}

fn recent_history_summary(history: &[uni::Message]) -> String {
    history
        .iter()
        .rev()
        .filter_map(|message| {
            let text = message.content.as_text();
            let trimmed = text.trim();
            (!trimmed.is_empty()).then(|| truncate_for_prompt(trimmed, 240))
        })
        .take(4)
        .collect::<Vec<_>>()
        .join("\n")
}

fn dedupe_prompt_suggestions(suggestions: Vec<PromptSuggestion>) -> Vec<PromptSuggestion> {
    let mut seen = HashMap::new();
    let mut ordered = Vec::new();
    for suggestion in suggestions {
        let key = suggestion.prompt.to_lowercase();
        if seen.contains_key(&key) {
            continue;
        }
        seen.insert(key, ());
        ordered.push(suggestion);
        if ordered.len() == 4 {
            break;
        }
    }
    ordered
}

fn truncate_for_prompt(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut truncated = text
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    truncated.push('…');
    truncated
}

fn normalize_sha(sha: &str) -> String {
    sha.trim().to_ascii_lowercase()
}

fn git_status_fragment(workspace: &Path) -> String {
    crate::agent::runloop::git::git_status_summary(workspace)
        .ok()
        .flatten()
        .map(|summary| format!("{}:{}", summary.branch, summary.dirty))
        .unwrap_or_else(|| "no-git".to_string())
}

fn command_succeeds(program: &str, args: &[&str], workspace: &Path) -> bool {
    Command::new(program)
        .args(args)
        .current_dir(workspace)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn run_command(program: &str, args: &[&str], workspace: &Path) -> Result<String, String> {
    let output = Command::new(program)
        .args(args)
        .current_dir(workspace)
        .output()
        .map_err(|err| err.to_string())?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use vtcode_core::config::PromptCachingConfig;
    use vtcode_core::config::loader::VTCodeConfig;
    use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
    use vtcode_core::config::types::ModelSelectionSource;

    fn prompt_config(provider: &str, model: &str) -> CoreAgentConfig {
        CoreAgentConfig {
            model: model.to_string(),
            api_key: "test-key".to_string(),
            provider: provider.to_string(),
            api_key_env: "TEST_API_KEY".to_string(),
            workspace: PathBuf::from("."),
            verbose: false,
            quiet: false,
            theme: "default".to_string(),
            reasoning_effort: Default::default(),
            ui_surface: Default::default(),
            prompt_cache: PromptCachingConfig::default(),
            model_source: ModelSelectionSource::WorkspaceConfig,
            custom_api_keys: Default::default(),
            checkpointing_enabled: false,
            checkpointing_storage_dir: None,
            checkpointing_max_snapshots: 0,
            checkpointing_max_age_days: None,
            max_conversation_turns: 0,
            model_behavior: None,
            openai_chatgpt_auth: None,
        }
    }

    #[test]
    fn detect_pr_status_is_not_applicable_outside_git_repo() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        assert_eq!(
            detect_pr_review_status(tempdir.path()),
            PrReviewStatus::NotApplicable
        );
    }

    #[test]
    fn pr_review_status_badges_match_expected_tones() {
        let reviewed = PrReviewStatus::ReviewedCurrent { url: None }
            .to_badge()
            .expect("reviewed badge");
        assert_eq!(reviewed.text, "PR: reviewed");
        assert_eq!(reviewed.tone, InlineHeaderStatusTone::Ready);

        let auth = PrReviewStatus::AuthRequired.to_badge().expect("auth badge");
        assert_eq!(auth.text, "PR: gh auth");
        assert_eq!(auth.tone, InlineHeaderStatusTone::Warning);
    }

    #[test]
    fn prompt_suggestion_route_prefers_configured_small_model() {
        let config = prompt_config("openai", "gpt-5.4");

        let mut vt_cfg = VTCodeConfig::default();
        vt_cfg.agent.small_model.enabled = true;
        vt_cfg.agent.small_model.model = "gpt-5-mini".to_string();
        vt_cfg.agent.small_model.temperature = 0.2;

        let route = resolve_prompt_suggestion_route(&config, Some(&vt_cfg));
        assert_eq!(route.provider_name, "openai");
        assert_eq!(route.model, "gpt-5-mini");
        assert_eq!(route.temperature, 0.2);
    }

    #[test]
    fn prompt_suggestion_route_auto_selects_efficient_variant() {
        let config = prompt_config("anthropic", "claude-sonnet-4.6");

        let mut vt_cfg = VTCodeConfig::default();
        vt_cfg.agent.small_model.enabled = true;

        let route = resolve_prompt_suggestion_route(&config, Some(&vt_cfg));
        assert_eq!(route.provider_name, "anthropic");
        assert_eq!(route.model, ModelId::ClaudeHaiku45.as_str());
    }
}
