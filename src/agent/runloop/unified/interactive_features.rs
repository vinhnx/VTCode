use std::collections::HashMap;
use std::path::Path;
use std::sync::{LazyLock, Mutex};

use vtcode_core::config::constants::model_helpers;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::models::{ModelId, Provider};
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::llm::factory::{
    ProviderConfig, create_provider_with_config, infer_provider_from_model,
};
use vtcode_core::llm::provider as uni;
use vtcode_core::tools::ToolRegistry;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PromptSuggestionSource {
    Llm,
    Local,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct InlinePromptSuggestion {
    pub(crate) prompt: String,
    pub(crate) source: PromptSuggestionSource,
}

static PROMPT_SUGGESTION_CACHE: LazyLock<Mutex<HashMap<String, Vec<PromptSuggestion>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
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

pub(crate) async fn generate_inline_prompt_suggestion(
    provider: &dyn uni::LLMProvider,
    config: &CoreAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
    workspace: &Path,
    history: &[uni::Message],
    session_stats: &SessionStats,
    tool_registry: &ToolRegistry,
    draft: &str,
) -> Option<InlinePromptSuggestion> {
    if vt_cfg
        .map(|cfg| cfg.agent.prompt_suggestions.enabled)
        .unwrap_or(true)
        == false
    {
        return None;
    }

    let route = resolve_prompt_suggestion_route(config, vt_cfg);
    if let Some(prompt) =
        llm_inline_prompt_suggestion(provider, config, vt_cfg, &route, history, draft).await
    {
        return Some(InlinePromptSuggestion {
            prompt,
            source: PromptSuggestionSource::Llm,
        });
    }

    deterministic_inline_prompt_suggestion(workspace, history, session_stats, tool_registry, draft)
        .map(|prompt| InlinePromptSuggestion {
            prompt,
            source: PromptSuggestionSource::Local,
        })
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

fn deterministic_inline_prompt_suggestion(
    workspace: &Path,
    history: &[uni::Message],
    session_stats: &SessionStats,
    tool_registry: &ToolRegistry,
    draft: &str,
) -> Option<String> {
    let suggestions =
        deterministic_prompt_suggestions(workspace, history, session_stats, tool_registry);
    if suggestions.is_empty() {
        return None;
    }

    if draft.trim().is_empty() {
        return suggestions
            .first()
            .map(|suggestion| suggestion.prompt.clone());
    }

    let normalized = draft.to_lowercase();
    suggestions
        .into_iter()
        .map(|suggestion| suggestion.prompt)
        .find(|prompt| prompt.to_lowercase().starts_with(&normalized))
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

    if route.model == config.model {
        return llm_prompt_suggestions_from_provider(
            provider,
            &route.model,
            route.temperature,
            history,
        )
        .await;
    }

    let Some(provider) = create_prompt_suggestion_provider(route, config, vt_cfg) else {
        return Vec::new();
    };

    llm_prompt_suggestions_from_provider(&*provider, &route.model, route.temperature, history).await
}

async fn llm_inline_prompt_suggestion(
    provider: &dyn uni::LLMProvider,
    config: &CoreAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
    route: &PromptSuggestionRoute,
    history: &[uni::Message],
    draft: &str,
) -> Option<String> {
    let context = recent_history_summary(history);
    if context.trim().is_empty() && draft.trim().is_empty() {
        return None;
    }

    if route.model == config.model {
        return llm_inline_prompt_suggestion_from_provider(
            provider,
            &route.model,
            route.temperature,
            history,
            draft,
        )
        .await;
    }

    let Some(provider) = create_prompt_suggestion_provider(route, config, vt_cfg) else {
        return None;
    };

    llm_inline_prompt_suggestion_from_provider(
        &*provider,
        &route.model,
        route.temperature,
        history,
        draft,
    )
    .await
}

fn create_prompt_suggestion_provider(
    route: &PromptSuggestionRoute,
    config: &CoreAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
) -> Option<Box<dyn uni::LLMProvider>> {
    create_provider_with_config(
        &route.provider_name,
        ProviderConfig {
            api_key: Some(config.api_key.clone()),
            openai_chatgpt_auth: config.openai_chatgpt_auth.clone(),
            copilot_auth: vt_cfg.map(|cfg| cfg.auth.copilot.clone()),
            base_url: None,
            model: Some(route.model.clone()),
            prompt_cache: Some(config.prompt_cache.clone()),
            timeouts: None,
            openai: vt_cfg.map(|cfg| cfg.provider.openai.clone()),
            anthropic: vt_cfg.map(|cfg| cfg.provider.anthropic.clone()),
            model_behavior: vt_cfg.map(|cfg| cfg.model.clone()),
            workspace_root: Some(config.workspace.clone()),
        },
    )
    .ok()
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

async fn llm_inline_prompt_suggestion_from_provider(
    provider: &dyn uni::LLMProvider,
    model: &str,
    temperature: f32,
    history: &[uni::Message],
    draft: &str,
) -> Option<String> {
    let request = build_inline_prompt_suggestion_request(model, temperature, history, draft);
    if !provider.supports_non_streaming(&request.model)
        || provider.validate_request(&request).is_err()
    {
        return None;
    }

    let Ok(response) = provider.generate(request).await else {
        return None;
    };
    let content = response.content?;
    normalize_inline_prompt_suggestion(&content, draft)
}

fn build_inline_prompt_suggestion_request(
    model: &str,
    temperature: f32,
    history: &[uni::Message],
    draft: &str,
) -> uni::LLMRequest {
    let context = recent_history_summary(history);
    let user_prompt = if draft.trim().is_empty() {
        format!(
            "Recent session context:\n{context}\n\nDraft: <empty>\nReturn exactly one short continuation the user would type next."
        )
    } else {
        format!(
            "Recent session context:\n{context}\n\nCurrent draft:\n{draft}\n\nReturn exactly one continuation that starts with the exact draft text and extends it."
        )
    };

    uni::LLMRequest {
        messages: vec![uni::Message::user(user_prompt)],
        system_prompt: Some(std::sync::Arc::new(
            "Predict the user's next chat prompt for VT Code. Match the user's phrasing, keep it concise, and return plain text only. Do not add bullets, numbering, quotes, explanations, or assistant voice. If a draft is provided, the response must begin with the exact draft text."
                .to_string(),
        )),
        model: model.to_string(),
        max_tokens: Some(48),
        temperature: Some(temperature),
        tool_choice: Some(uni::ToolChoice::None),
        ..Default::default()
    }
}

fn normalize_inline_prompt_suggestion(content: &str, draft: &str) -> Option<String> {
    let trimmed = content.lines().find_map(|line| {
        let candidate = line
            .trim()
            .trim_start_matches('-')
            .trim_start_matches('•')
            .trim();
        (!candidate.is_empty()).then(|| candidate.to_string())
    })?;

    if draft.trim().is_empty() {
        return Some(trimmed);
    }

    trimmed
        .to_lowercase()
        .starts_with(&draft.to_lowercase())
        .then_some(trimmed)
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
    let prompt_suggestions_cfg = &vt_cfg.agent.prompt_suggestions;
    let model = if prompt_suggestions_cfg.model.trim().is_empty() {
        auto_small_model(&provider_name, &config.model)
    } else {
        prompt_suggestions_cfg.model.clone()
    };

    PromptSuggestionRoute {
        provider_name,
        model,
        temperature: prompt_suggestions_cfg.temperature,
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

fn git_status_fragment(workspace: &Path) -> String {
    crate::agent::runloop::git::git_status_summary(workspace)
        .ok()
        .flatten()
        .map(|summary| format!("{}:{}", summary.branch, summary.dirty))
        .unwrap_or_else(|| "no-git".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use vtcode_core::config::PromptCachingConfig;
    use vtcode_core::config::loader::VTCodeConfig;
    use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
    use vtcode_core::config::types::ModelSelectionSource;
    use vtcode_core::llm::provider::{FinishReason, LLMError, LLMRequest, LLMResponse};

    #[derive(Clone)]
    struct RecordingProvider {
        requests: Arc<Mutex<Vec<LLMRequest>>>,
        response: Option<String>,
    }

    impl RecordingProvider {
        fn with_response(response: &str) -> Self {
            Self {
                requests: Arc::new(Mutex::new(Vec::new())),
                response: Some(response.to_string()),
            }
        }

        fn recorded_requests(&self) -> Vec<LLMRequest> {
            self.requests.lock().expect("requests lock").clone()
        }
    }

    #[async_trait]
    impl uni::LLMProvider for RecordingProvider {
        fn name(&self) -> &str {
            "openai"
        }

        async fn generate(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
            self.requests
                .lock()
                .expect("requests lock")
                .push(request.clone());
            Ok(LLMResponse {
                content: self.response.clone(),
                model: request.model,
                finish_reason: FinishReason::Stop,
                ..Default::default()
            })
        }

        fn supported_models(&self) -> Vec<String> {
            vec!["gpt-5-mini".to_string()]
        }

        fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
            Ok(())
        }
    }

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
    fn prompt_suggestion_route_prefers_configured_small_model() {
        let config = prompt_config("openai", "gpt-5.4");

        let mut vt_cfg = VTCodeConfig::default();
        vt_cfg.agent.prompt_suggestions.model = "gpt-5-mini".to_string();
        vt_cfg.agent.prompt_suggestions.temperature = 0.2;

        let route = resolve_prompt_suggestion_route(&config, Some(&vt_cfg));
        assert_eq!(route.provider_name, "openai");
        assert_eq!(route.model, "gpt-5-mini");
        assert_eq!(route.temperature, 0.2);
    }

    #[test]
    fn prompt_suggestion_route_auto_selects_lightweight_sibling() {
        let config = prompt_config("openai", "gpt-5.4");

        let vt_cfg = VTCodeConfig::default();
        let route = resolve_prompt_suggestion_route(&config, Some(&vt_cfg));
        assert_eq!(route.provider_name, "openai");
        assert_eq!(route.model, ModelId::GPT5Mini.as_str());
    }

    #[test]
    fn deterministic_inline_prompt_suggestion_uses_first_suggestion_for_empty_draft() {
        let session_stats = SessionStats::default();
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let tool_registry = runtime.block_on(ToolRegistry::new(PathBuf::from(".")));

        let suggestion = deterministic_inline_prompt_suggestion(
            Path::new("."),
            &[],
            &session_stats,
            &tool_registry,
            "",
        )
        .expect("suggestion");

        assert!(!suggestion.trim().is_empty());
    }

    #[test]
    fn deterministic_inline_prompt_suggestion_matches_draft_prefix() {
        let session_stats = SessionStats::default();
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let tool_registry = runtime.block_on(ToolRegistry::new(PathBuf::from(".")));

        let suggestion = deterministic_inline_prompt_suggestion(
            Path::new("."),
            &[],
            &session_stats,
            &tool_registry,
            "Review the current diff, call",
        )
        .expect("suggestion");

        assert_eq!(
            suggestion,
            "Review the current diff, call out the highest-risk issue, and suggest the next change."
        );
    }

    #[test]
    fn deterministic_inline_prompt_suggestion_preserves_trailing_space_prefix() {
        let session_stats = SessionStats::default();
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let tool_registry = runtime.block_on(ToolRegistry::new(PathBuf::from(".")));

        let suggestion = deterministic_inline_prompt_suggestion(
            Path::new("."),
            &[],
            &session_stats,
            &tool_registry,
            "Review the current diff ",
        );

        assert!(suggestion.is_none());
    }

    #[test]
    fn normalize_inline_prompt_suggestion_requires_matching_prefix_for_partial_draft() {
        assert_eq!(
            normalize_inline_prompt_suggestion("Review the current diff", "Review the current"),
            Some("Review the current diff".to_string())
        );
        assert_eq!(
            normalize_inline_prompt_suggestion("Start a new plan", "Review the current"),
            None
        );
    }

    #[test]
    fn normalize_inline_prompt_suggestion_preserves_trailing_space_prefix() {
        assert_eq!(
            normalize_inline_prompt_suggestion("Review the currentdiff", "Review the current "),
            None
        );
        assert_eq!(
            normalize_inline_prompt_suggestion(
                "Review the current diff and summarize it",
                "Review the current diff "
            ),
            Some("Review the current diff and summarize it".to_string())
        );
    }

    #[tokio::test]
    async fn inline_prompt_suggestion_uses_route_temperature_on_active_provider() {
        let provider = RecordingProvider::with_response("Review the current diff in detail");
        let config = prompt_config("openai", "gpt-5-mini");
        let mut vt_cfg = VTCodeConfig::default();
        vt_cfg.agent.prompt_suggestions.temperature = 0.15;

        let suggestion = generate_inline_prompt_suggestion(
            &provider,
            &config,
            Some(&vt_cfg),
            Path::new("."),
            &[uni::Message::user(
                "Please keep reviewing the diff".to_string(),
            )],
            &SessionStats::default(),
            &ToolRegistry::new(PathBuf::from(".")).await,
            "Review the current diff ",
        )
        .await
        .expect("inline suggestion");

        assert_eq!(suggestion.source, PromptSuggestionSource::Llm);
        assert_eq!(
            suggestion.prompt,
            "Review the current diff in detail".to_string()
        );

        let requests = provider.recorded_requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].model, "gpt-5-mini");
        assert_eq!(requests[0].temperature, Some(0.15));
    }
}
