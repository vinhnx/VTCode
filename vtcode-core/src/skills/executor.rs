//! Skill execution as Tool trait implementation
//!
//! Bridges Agent Skills to VT Code's tool system by implementing the Tool trait
//! for skills, enabling them to execute with full access to VT Code's permissions,
//! caching, and audit systems.
//!
//! ## LLM Sub-Calls (Phase 5)
//!
//! Skills can now execute with full LLM support via `execute_skill_with_sub_llm()`:
//! 1. Skill instructions become the system prompt
//! 2. User input is the first message
//! 3. All available tools are passed to the LLM
//! 4. Tool calls are executed and results are fed back
//! 5. Final response is returned

use crate::config::VTCodeConfig;
use crate::config::models::ModelId;
use crate::core::agent::runner::{AgentRunner, RunnerSettings};
use crate::core::agent::task::{ContextItem, Task};
use crate::core::agent::types::AgentType;
use crate::core::loop_detector::LoopDetector;
use crate::llm::provider::{FinishReason, LLMProvider, LLMRequest, Message, ToolDefinition};
use crate::sandboxing::{AdditionalPermissions, SandboxPermissions};
use crate::skills::types::{Skill, SkillNetworkPolicy};
use crate::tool_policy::ToolPolicy;
use crate::tools::ToolRegistry;
use crate::tools::registry::{ToolErrorType, ToolExecutionError};
use crate::tools::tool_intent;
use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use chrono::Utc;
use serde_json::{Map, Value};
use std::borrow::Cow;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info, warn};
use vtcode_config::auth::OpenAIChatGptAuthHandle;

type SkillToolArgTransform = dyn Fn(&str, Value) -> Value + Send + Sync;

const EMPTY_SKILL_INPUT_PROMPT: &str = "No explicit user input was provided. Follow the skill instructions using their default behavior for empty input.";
const SKILL_TOOL_FREE_SYNTHESIS_PROMPT: &str = "Do not make any more tool calls. Provide the best final answer you can using the information already gathered.";
const MAX_SKILL_LLM_ITERATIONS: usize = 10;

fn skill_tool_free_synthesis_prompt(reason: &str) -> String {
    format!("{reason}\n\n{SKILL_TOOL_FREE_SYNTHESIS_PROMPT}")
}

fn should_force_tool_free_synthesis(error: &ToolExecutionError) -> bool {
    matches!(error.error_type, ToolErrorType::ToolNotFound)
}

/// Network-capable tool names that should be filtered based on skill network policy
const NETWORK_TOOLS: &[&str] = &[
    "http",
    "fetch",
    "browser",
    "web_search",
    "read_web_page",
    "curl",
];

fn is_function_network_tool(tool: &ToolDefinition) -> bool {
    tool.function.as_ref().is_some_and(|function| {
        let name = function.name.to_ascii_lowercase();
        NETWORK_TOOLS
            .iter()
            .any(|candidate| name.contains(candidate))
    })
}

fn is_native_web_search_tool(tool: &ToolDefinition) -> bool {
    matches!(tool.tool_type.as_str(), "web_search" | "google_search")
        || tool.tool_type.starts_with("web_search_")
}

fn is_gemini_native_network_tool(tool: &ToolDefinition) -> bool {
    matches!(tool.tool_type.as_str(), "google_maps" | "url_context")
}

fn is_network_capable_tool(tool: &ToolDefinition) -> bool {
    is_native_web_search_tool(tool)
        || is_gemini_native_network_tool(tool)
        || is_function_network_tool(tool)
}

fn json_string_array(config: &Map<String, Value>, key: &str) -> Result<Option<Vec<String>>> {
    let Some(value) = config.get(key) else {
        return Ok(None);
    };
    let Value::Array(values) = value else {
        return Err(anyhow!("{key} must be an array of strings"));
    };

    values
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(ToOwned::to_owned)
                .ok_or_else(|| anyhow!("{key} must contain only strings"))
        })
        .collect::<Result<Vec<_>>>()
        .map(Some)
}

fn set_json_string_array(config: &mut Map<String, Value>, key: &str, values: Vec<String>) {
    if values.is_empty() {
        config.remove(key);
        return;
    }

    config.insert(
        key.to_string(),
        Value::Array(values.into_iter().map(Value::String).collect()),
    );
}

fn intersect_domains(existing: Option<Vec<String>>, requested: &[String]) -> Vec<String> {
    match existing {
        Some(existing) => existing
            .into_iter()
            .filter(|domain| requested.iter().any(|candidate| candidate == domain))
            .collect(),
        None => requested.to_vec(),
    }
}

fn union_domains(existing: Option<Vec<String>>, requested: &[String]) -> Vec<String> {
    let mut merged = existing.unwrap_or_default();
    for domain in requested {
        if !merged.iter().any(|candidate| candidate == domain) {
            merged.push(domain.clone());
        }
    }
    merged
}

fn apply_web_search_policy(
    skill: &Skill,
    tool: &ToolDefinition,
    policy: &SkillNetworkPolicy,
) -> Option<ToolDefinition> {
    let mut updated = tool.clone();
    let existing_config = match updated.web_search.take() {
        Some(Value::Object(config)) => config,
        Some(_) => {
            warn!(
                skill = skill.name(),
                tool_type = %tool.tool_type,
                "Dropping network tool because web search policy could not be encoded"
            );
            return None;
        }
        None => Map::new(),
    };

    let existing_allowed = match json_string_array(&existing_config, "allowed_domains") {
        Ok(value) => value,
        Err(error) => {
            warn!(
                skill = skill.name(),
                tool_type = %tool.tool_type,
                error = %error,
                "Dropping network tool because web search policy could not be encoded"
            );
            return None;
        }
    };
    let existing_blocked = match json_string_array(&existing_config, "blocked_domains") {
        Ok(value) => value,
        Err(error) => {
            warn!(
                skill = skill.name(),
                tool_type = %tool.tool_type,
                error = %error,
                "Dropping network tool because web search policy could not be encoded"
            );
            return None;
        }
    };
    let merged_allowed = if policy.allowed_domains.is_empty() {
        existing_allowed.unwrap_or_default()
    } else {
        intersect_domains(existing_allowed, &policy.allowed_domains)
    };
    let merged_blocked = if policy.denied_domains.is_empty() {
        existing_blocked.unwrap_or_default()
    } else {
        union_domains(existing_blocked, &policy.denied_domains)
    };

    if updated.is_anthropic_web_search() && !merged_allowed.is_empty() && !merged_blocked.is_empty()
    {
        warn!(
            skill = skill.name(),
            tool_type = %tool.tool_type,
            "Dropping anthropic web search tool because allowlist and denylist cannot both be enforced"
        );
        return None;
    }

    let mut config = existing_config;
    set_json_string_array(&mut config, "allowed_domains", merged_allowed);
    set_json_string_array(&mut config, "blocked_domains", merged_blocked);
    updated.web_search = Some(Value::Object(config));

    if let Err(error) = updated.validate() {
        warn!(
            skill = skill.name(),
            tool_type = %tool.tool_type,
            error = %error,
            "Dropping network tool because the enforced web search policy is invalid"
        );
        return None;
    }

    Some(updated)
}

/// Filter available tools based on skill's network policy
///
/// - If skill has no network policy: remove network-capable tools
/// - If skill has a network policy: enforce it for native web search tools
/// - If the policy cannot be encoded safely: remove the tool
pub fn filter_tools_for_skill(skill: &Skill, tools: Vec<ToolDefinition>) -> Vec<ToolDefinition> {
    let network_policy = &skill.manifest.network_policy;

    match network_policy {
        None => tools
            .into_iter()
            .filter(|t| {
                let is_network = is_network_capable_tool(t);
                if is_network {
                    debug!(
                        tool = t.function_name(),
                        "Filtered network tool for skill '{}' (no network policy)",
                        skill.name()
                    );
                }
                !is_network
            })
            .collect(),
        Some(policy) => tools
            .into_iter()
            .filter_map(|tool| {
                if !is_network_capable_tool(&tool) {
                    return Some(tool);
                }

                if is_native_web_search_tool(&tool) {
                    return apply_web_search_policy(skill, &tool, policy);
                }

                if is_gemini_native_network_tool(&tool) {
                    info!(
                        skill = skill.name(),
                        tool = tool.function_name(),
                        "Dropping Gemini native network tool because skill domain policy cannot be enforced safely"
                    );
                    return None;
                }

                info!(
                    skill = skill.name(),
                    tool = tool.function_name(),
                    "Dropping network tool because skill policy cannot be enforced for function-style tools"
                );
                None
            })
            .collect(),
    }
}

fn skill_additional_permissions(skill: &Skill) -> Option<AdditionalPermissions> {
    let file_system = skill.manifest.permissions.as_ref()?.file_system.as_ref()?;
    let fs_read = resolve_skill_permission_paths(skill.path.as_path(), &file_system.read);
    let fs_write = resolve_skill_permission_paths(skill.path.as_path(), &file_system.write);
    let permissions = AdditionalPermissions { fs_read, fs_write };
    (!permissions.is_empty()).then_some(permissions)
}

fn resolve_skill_permission_paths(skill_root: &Path, paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut resolved = Vec::with_capacity(paths.len());
    let mut seen = BTreeSet::new();

    for path in paths {
        if path.as_os_str().is_empty() {
            continue;
        }

        let absolute = if path.is_absolute() {
            path.clone()
        } else {
            skill_root.join(path)
        };
        let normalized = crate::utils::path::normalize_path(&absolute);
        if seen.insert(normalized.clone()) {
            resolved.push(normalized);
        }
    }

    resolved
}

fn merge_permission_paths(existing: &[PathBuf], extra: &[PathBuf]) -> Vec<PathBuf> {
    let mut merged = Vec::with_capacity(existing.len() + extra.len());
    let mut seen = BTreeSet::new();

    for path in existing.iter().chain(extra.iter()) {
        if seen.insert(path.clone()) {
            merged.push(path.clone());
        }
    }

    merged
}

fn merge_additional_permissions(
    existing: &AdditionalPermissions,
    extra: &AdditionalPermissions,
) -> AdditionalPermissions {
    AdditionalPermissions {
        fs_read: merge_permission_paths(&existing.fs_read, &extra.fs_read),
        fs_write: merge_permission_paths(&existing.fs_write, &extra.fs_write),
    }
}

fn merge_skill_command_permissions(skill: &Skill, tool_name: &str, tool_args: Value) -> Value {
    if !tool_intent::is_command_run_tool_call(tool_name, &tool_args) {
        return tool_args;
    }

    let Some(skill_permissions) = skill_additional_permissions(skill) else {
        return tool_args;
    };

    let mut args = match tool_args {
        Value::Object(args) => args,
        other => return other,
    };

    let sandbox_permissions = match args.get("sandbox_permissions") {
        Some(value) => match serde_json::from_value::<SandboxPermissions>(value.clone()) {
            Ok(value) => value,
            Err(_) => return Value::Object(args),
        },
        None => SandboxPermissions::UseDefault,
    };

    if matches!(
        sandbox_permissions,
        SandboxPermissions::RequireEscalated | SandboxPermissions::BypassSandbox
    ) {
        return Value::Object(args);
    }

    let existing_permissions = match args.get("additional_permissions") {
        Some(value) => match serde_json::from_value::<AdditionalPermissions>(value.clone()) {
            Ok(value) => value,
            Err(_) => return Value::Object(args),
        },
        None => AdditionalPermissions::default(),
    };

    let merged_permissions =
        merge_additional_permissions(&existing_permissions, &skill_permissions);
    args.insert(
        "sandbox_permissions".to_string(),
        serde_json::to_value(SandboxPermissions::WithAdditionalPermissions)
            .expect("sandbox permissions should serialize"),
    );
    args.insert(
        "additional_permissions".to_string(),
        serde_json::to_value(&merged_permissions).expect("additional permissions should serialize"),
    );
    debug!(
        "Applied skill-scoped sandbox permissions for '{}' to tool '{}'",
        skill.name(),
        tool_name
    );

    Value::Object(args)
}

#[derive(Debug, Clone)]
pub struct ForkSkillRuntimeConfig {
    pub workspace: PathBuf,
    pub model: String,
    pub api_key: String,
    pub openai_chatgpt_auth: Option<OpenAIChatGptAuthHandle>,
    pub vt_cfg: Option<VTCodeConfig>,
}

#[async_trait]
pub trait ForkSkillExecutor: Send + Sync {
    async fn execute(&self, skill: &Skill, user_input: Value) -> Result<Value>;
}

#[derive(Clone)]
pub struct ChildAgentSkillExecutor {
    tool_registry: Arc<ToolRegistry>,
    runtime: ForkSkillRuntimeConfig,
}

impl ChildAgentSkillExecutor {
    pub fn new(tool_registry: Arc<ToolRegistry>, runtime: ForkSkillRuntimeConfig) -> Self {
        Self {
            tool_registry,
            runtime,
        }
    }

    async fn build_runner(&self, skill: &Skill, session_id: String) -> Result<AgentRunner> {
        let model = self
            .runtime
            .model
            .parse::<ModelId>()
            .with_context(|| format!("invalid model for forked skill '{}'", skill.name()))?;

        let mut runner = if let Some(vt_cfg) = self.runtime.vt_cfg.clone() {
            AgentRunner::new_with_thread_bootstrap_and_config_with_openai_auth(
                fork_agent_type(skill),
                model,
                self.runtime.api_key.clone(),
                self.runtime.workspace.clone(),
                session_id,
                RunnerSettings {
                    reasoning_effort: None,
                    verbosity: None,
                },
                None,
                crate::core::threads::ThreadBootstrap::new(None),
                vt_cfg,
                self.runtime.openai_chatgpt_auth.clone(),
            )
            .await?
        } else {
            AgentRunner::new_with_thread_bootstrap_and_openai_auth(
                fork_agent_type(skill),
                model,
                self.runtime.api_key.clone(),
                self.runtime.workspace.clone(),
                session_id,
                RunnerSettings {
                    reasoning_effort: None,
                    verbosity: None,
                },
                None,
                crate::core::threads::ThreadBootstrap::new(None),
                self.runtime.openai_chatgpt_auth.clone(),
            )
            .await?
        };
        runner.set_quiet(true);
        Ok(runner)
    }
}

fn skill_runs_in_fork(skill: &Skill) -> bool {
    skill.manifest.context.as_deref() == Some("fork")
}

fn skill_tool_arg_transform(skill: Skill) -> Arc<SkillToolArgTransform> {
    Arc::new(move |tool_name, tool_args| {
        merge_skill_command_permissions(&skill, tool_name, tool_args)
    })
}

fn fork_agent_type(skill: &Skill) -> AgentType {
    match skill.manifest.agent.as_deref() {
        Some("explore") => AgentType::Explore,
        Some("plan") => AgentType::Plan,
        Some("general") => AgentType::General,
        _ => AgentType::General,
    }
}

fn format_skill_user_input(user_input: &Value) -> String {
    match user_input {
        Value::String(text) => normalized_skill_user_input(text),
        other => other.to_string(),
    }
}

fn normalized_skill_user_input(user_input: &str) -> String {
    if user_input.trim().is_empty() {
        EMPTY_SKILL_INPUT_PROMPT.to_string()
    } else {
        user_input.to_string()
    }
}

fn child_session_id(parent_session_id: &str, skill_name: &str) -> String {
    format!(
        "{}-skill-{}-{}",
        crate::utils::session_debug::sanitize_debug_component(parent_session_id, "session"),
        crate::utils::session_debug::sanitize_debug_component(skill_name, "skill"),
        Utc::now().format("%Y%m%dT%H%M%SZ")
    )
}

fn blocked_handoff_paths(events: &[crate::exec::events::ThreadEvent]) -> Vec<String> {
    let mut paths = Vec::new();
    for event in events {
        let crate::exec::events::ThreadEvent::ItemCompleted(completed) = event else {
            continue;
        };
        let crate::exec::events::ThreadItemDetails::Harness(harness) = &completed.item.details
        else {
            continue;
        };
        if harness.event == crate::exec::events::HarnessEventKind::BlockedHandoffWritten
            && let Some(path) = harness.path.as_ref()
            && !paths.iter().any(|existing| existing == path)
        {
            paths.push(path.clone());
        }
    }
    paths
}

#[async_trait]
impl ForkSkillExecutor for ChildAgentSkillExecutor {
    async fn execute(&self, skill: &Skill, user_input: Value) -> Result<Value> {
        let parent_session_id = self.tool_registry.harness_context_snapshot().session_id;
        let session_id = child_session_id(&parent_session_id, skill.name());
        let mut runner = self.build_runner(skill, session_id.clone()).await?;

        let restricted_tools = filter_tools_for_skill(skill, runner.build_universal_tools().await?);
        let allowed_tools = restricted_tools
            .iter()
            .map(|tool| tool.function_name().to_string())
            .collect::<Vec<_>>();
        runner.set_tool_definitions_override(restricted_tools);
        runner.set_tool_arg_transform(skill_tool_arg_transform(skill.clone()));
        runner.enable_full_auto(&allowed_tools).await;

        let mut task = Task::new(
            format!("fork-skill-{}", skill.name()),
            format!("Skill {}", skill.name()),
            format_skill_user_input(&user_input),
        );
        task.instructions = Some(skill.instructions.clone());

        let results = runner
            .execute_task(&task, &Vec::<ContextItem>::new())
            .await?;
        let mut artifact_paths = results.modified_files.clone();
        let handoff_paths = blocked_handoff_paths(&results.thread_events);
        for path in handoff_paths {
            if !artifact_paths.iter().any(|existing| existing == &path) {
                artifact_paths.push(path);
            }
        }

        Ok(serde_json::json!({
            "execution_context": "fork",
            "status": results.outcome.code(),
            "summary": if results.summary.trim().is_empty() {
                results.outcome.description()
            } else {
                results.summary
            },
            "artifact_paths": artifact_paths,
            "delegate_session_id": session_id,
        }))
    }
}

/// Execute a skill with LLM sub-call support (Phase 5)
///
/// Creates a sub-conversation where:
/// 1. Skill instructions become the system prompt
/// 2. User input becomes the first user message
/// 3. All available tools are passed to the LLM
/// 4. Tool calls are executed via the tool registry
/// 5. Tool results are fed back to continue the conversation
/// 6. Final response is returned
///
/// # Arguments
/// * `skill` - The skill to execute
/// * `user_input` - The user's input/request for the skill
/// * `provider` - The LLM provider for sub-calls
/// * `tool_registry` - The tool registry for executing nested tools
/// * `available_tools` - Tools available to the skill
/// * `model` - The model to use for skill execution
pub async fn execute_skill_with_sub_llm(
    skill: &Skill,
    user_input: String,
    provider: &dyn LLMProvider,
    tool_registry: &mut ToolRegistry,
    available_tools: Vec<ToolDefinition>,
    model: String,
) -> Result<String> {
    debug!("Executing skill '{}' with LLM sub-call", skill.name());

    // Apply network policy filtering
    let available_tools = filter_tools_for_skill(skill, available_tools);
    let tool_definitions = if available_tools.is_empty() {
        None
    } else {
        Some(Arc::new(available_tools.clone()))
    };
    let normalized_user_input = normalized_skill_user_input(&user_input);

    // Build conversation starting with user input
    let mut messages = vec![Message::user(normalized_user_input)];

    // Create LLM request with skill instructions as system prompt
    let mut request = LLMRequest {
        messages: messages.clone(),
        system_prompt: Some(Arc::new(skill.instructions.clone())),
        tools: tool_definitions.clone(),
        model: model.clone(),
        max_tokens: Some(4096),
        ..Default::default()
    };

    // Loop: Make LLM request and handle tool calls
    const BACKOFF_BASE_MS: u64 = 50; // initial back‑off delay
    const MAX_RATE_LIMIT_WAIT_CYCLES: usize = 20;
    const SKILL_RATE_LIMIT_KEY: &str = "skill_sub_llm";
    let mut iterations = 0;
    let mut backoff = BACKOFF_BASE_MS;
    let mut wait_cycles = 0usize;
    let mut loop_detector = LoopDetector::new();
    let mut force_tool_free_synthesis = None;

    loop {
        let tool_free_synthesis_reason = force_tool_free_synthesis.take();
        let is_tool_free_synthesis = tool_free_synthesis_reason.is_some();

        if let Some(reason) = tool_free_synthesis_reason {
            messages.push(Message::user(reason));
            request.messages = messages.clone();
            request.tools = None;
        } else {
            request.tools = tool_definitions.clone();
        }

        // Rate-limit tool-bearing iterations, but let the final no-tools recovery
        // pass complete immediately so a stalled skill can still synthesize a result.
        if !is_tool_free_synthesis {
            if let Err(wait_hint) =
                crate::tools::adaptive_rate_limiter::try_acquire_global(SKILL_RATE_LIMIT_KEY)
            {
                wait_cycles += 1;
                if wait_cycles > MAX_RATE_LIMIT_WAIT_CYCLES {
                    return Err(anyhow!(
                        "Skill execution stayed rate-limited for too long ({} cycles)",
                        MAX_RATE_LIMIT_WAIT_CYCLES
                    ));
                }

                let delay = wait_hint
                    .max(Duration::from_millis(backoff))
                    .min(Duration::from_secs(2));
                // If rate limited, wait a bit and retry without counting as an iteration
                warn!(
                    "Rate limit hit for skill execution – backing off {}ms",
                    delay.as_millis()
                );
                tokio::time::sleep(delay).await;
                backoff = (backoff * 2).min(2000); // cap back‑off at 2 s
                continue;
            }
            wait_cycles = 0;
            backoff = BACKOFF_BASE_MS;
        }

        if is_tool_free_synthesis {
            info!(
                "Skill '{}' entering tool-free final synthesis",
                skill.name()
            );
        } else {
            iterations += 1;
            if iterations > MAX_SKILL_LLM_ITERATIONS {
                let reason = skill_tool_free_synthesis_prompt(&format!(
                    "Skill execution reached the maximum tool-call iterations ({}).",
                    MAX_SKILL_LLM_ITERATIONS
                ));
                warn!(
                    skill = skill.name(),
                    iterations = iterations - 1,
                    max_iterations = MAX_SKILL_LLM_ITERATIONS,
                    "Skill hit max iterations; forcing tool-free final synthesis"
                );
                force_tool_free_synthesis = Some(reason);
                continue;
            }

            info!("Skill LLM iteration {} for '{}'", iterations, skill.name());
        }

        // Make LLM request
        let response = provider.generate(request.clone()).await?;

        // Extract content - handle Option
        let content = response.content.unwrap_or_default();

        // Add assistant response to conversation
        if let Some(tool_calls) = &response.tool_calls {
            messages.push(Message::assistant_with_tools(
                content.clone(),
                tool_calls.clone(),
            ));
        } else {
            messages.push(Message::assistant(content.clone()));
        }

        // Check if there are tool calls to handle
        if let Some(tool_calls) = response.tool_calls {
            if !tool_calls.is_empty() {
                info!(
                    "Skill '{}' made {} tool calls",
                    skill.name(),
                    tool_calls.len()
                );
                let mut force_tool_free_synthesis_reason = None;

                // Execute each tool call
                for tool_call in tool_calls {
                    // Extract function name and arguments
                    if let Some(tool_name) = tool_call.tool_name() {
                        let tool_name = tool_name.to_string();

                        debug!(
                            "Executing tool '{}' for skill '{}'",
                            tool_name,
                            skill.name()
                        );

                        let tool_args = tool_call
                            .execution_arguments()
                            .unwrap_or_else(|_| serde_json::json!({}));
                        let tool_args =
                            merge_skill_command_permissions(skill, &tool_name, tool_args);

                        if let Some(loop_warning) =
                            loop_detector.record_call(&tool_name, &tool_args)
                            && loop_detector.is_hard_limit_exceeded(&tool_name)
                        {
                            messages.push(Message::tool_response(
                                tool_call.id.clone(),
                                format!(
                                    "{}\n\nTool execution was skipped to prevent a loop.",
                                    loop_warning
                                ),
                            ));
                            force_tool_free_synthesis_reason =
                                Some(skill_tool_free_synthesis_prompt(&loop_warning));
                            break;
                        }

                        // Execute tool via registry
                        let tool_output = match tool_registry
                            .execute_public_tool_ref(&tool_name, &tool_args)
                            .await
                        {
                            Ok(result) => result,
                            Err(e) => {
                                warn!("Tool '{}' failed: {}", tool_name, e);
                                ToolExecutionError::from_anyhow(
                                    tool_name.to_string(),
                                    &e,
                                    0,
                                    false,
                                    false,
                                    Some("skill_sub_llm"),
                                )
                                .to_json_value()
                            }
                        };
                        let tool_error = ToolExecutionError::from_tool_output(&tool_output);
                        let tool_result = tool_output.to_string();

                        // Add tool result to conversation
                        messages.push(Message::tool_response(tool_call.id.clone(), tool_result));
                        if let Some(tool_error) = tool_error
                            && should_force_tool_free_synthesis(&tool_error)
                        {
                            force_tool_free_synthesis_reason =
                                Some(skill_tool_free_synthesis_prompt(&format!(
                                    "The tool '{}' is not available for this skill. {}",
                                    tool_name,
                                    tool_error.user_message()
                                )));
                            break;
                        }
                    } else {
                        warn!("Tool call has no function: {:?}", tool_call.call_type);
                    }
                }

                // Update request for next iteration
                request.messages = messages.clone();
                if let Some(reason) = force_tool_free_synthesis_reason {
                    force_tool_free_synthesis = Some(reason);
                    continue;
                }

                // Continue loop to process tool results
            } else {
                // No tool calls, return the text response
                return Ok(content);
            }
        } else {
            // No tool calls, return the final response
            return Ok(content);
        }

        // Check finish reason
        match response.finish_reason {
            FinishReason::Stop => {
                // Normal termination
                return Ok(content);
            }
            FinishReason::ToolCalls => {
                // Continue to handle tool calls (already handled above)
            }
            FinishReason::Length => {
                warn!("Skill '{}' hit token limit", skill.name());
                return Ok(content);
            }
            FinishReason::ContentFilter => {
                warn!(
                    "Skill '{}' response filtered by content policy",
                    skill.name()
                );
                return Ok(content);
            }
            FinishReason::Error(ref msg) => {
                return Err(anyhow!("LLM error during skill execution: {}", msg));
            }
            FinishReason::Pause => {
                // For skill execution, treatment is similar to ToolCalls: we continue the loop
                // to process whatever triggered the pause (usually server-side tool use).
            }
            FinishReason::Refusal => {
                return Err(anyhow!(
                    "LLM refused to continue generating response due to policy violations"
                ));
            }
        }
    }
}

/// Adapter implementing Tool trait for a Skill
#[derive(Clone)]
pub struct SkillToolAdapter {
    skill: Skill,
    fork_executor: Option<Arc<dyn ForkSkillExecutor>>,
}

impl SkillToolAdapter {
    /// Create a new skill tool adapter
    pub fn new(skill: Skill) -> Self {
        SkillToolAdapter {
            skill,
            fork_executor: None,
        }
    }

    pub fn with_fork_executor(skill: Skill, fork_executor: Arc<dyn ForkSkillExecutor>) -> Self {
        SkillToolAdapter {
            skill,
            fork_executor: Some(fork_executor),
        }
    }

    /// Get reference to underlying skill
    pub fn skill(&self) -> &Skill {
        &self.skill
    }

    /// Get mutable reference to underlying skill
    pub fn skill_mut(&mut self) -> &mut Skill {
        &mut self.skill
    }

    /// Execute skill by invoking LLM with skill instructions as system prompt
    async fn execute_skill_with_lm(&self, user_input: Value) -> Result<Value> {
        debug!("Executing skill: {}", self.skill.name());

        // Return structured result with skill instructions and context
        // The agent harness will use this to invoke an LLM sub-call with:
        // 1. Skill instructions as system prompt
        // 2. User input in the message
        // 3. Available tools for the skill to use
        Ok(serde_json::json!({
            "skill_name": self.skill.name(),
            "status": "executing",
            "description": self.skill.description(),
            "instructions": self.skill.instructions,
            "resources_available": self.skill.list_resources(),
            "user_input": user_input,
        }))
    }

    async fn execute_forked_skill(&self, user_input: Value) -> Result<Value> {
        let executor = self
            .fork_executor
            .as_ref()
            .ok_or_else(|| anyhow!("forked skill execution is not configured for this session"))?;
        executor.execute(&self.skill, user_input).await
    }
}

#[async_trait]
impl crate::tools::traits::Tool for SkillToolAdapter {
    async fn execute(&self, args: Value) -> Result<Value> {
        info!("Skill tool executing: {}", self.skill.name());

        let result = if skill_runs_in_fork(&self.skill) {
            self.execute_forked_skill(args).await?
        } else {
            self.execute_skill_with_lm(args).await?
        };

        Ok(result)
    }

    fn name(&self) -> &'static str {
        "traditional_skill_tool"
    }

    fn description(&self) -> &'static str {
        "Traditional VT Code skill adapter"
    }

    fn validate_args(&self, args: &Value) -> Result<()> {
        // Skills are flexible; accept any args
        // The skill instructions will guide the LLM on what to do with them
        if args.is_null() {
            return Ok(());
        }
        Ok(())
    }

    fn parameter_schema(&self) -> Option<Value> {
        // Skills are flexible, accept any input
        Some(serde_json::json!({
            "type": "object",
            "description": "Flexible input for skill execution",
            "additionalProperties": true,
        }))
    }

    fn default_permission(&self) -> ToolPolicy {
        // Skills require explicit permission due to potential resource usage
        ToolPolicy::Prompt
    }

    fn allow_patterns(&self) -> Option<&'static [&'static str]> {
        // Skills can define their own patterns, but by default none
        None
    }

    fn deny_patterns(&self) -> Option<&'static [&'static str]> {
        None
    }

    fn prompt_path(&self) -> Option<Cow<'static, str>> {
        // Skills can bundle companion prompts
        Some(Cow::Borrowed("skills/skill_instructions.md"))
    }
}

/// Skill execution context passed to sub-LLM calls
pub struct SkillExecutionContext {
    pub skill_name: String,
    pub instructions: String,
    pub available_tools: Vec<String>,
    pub user_input: Value,
}

impl SkillExecutionContext {
    pub fn new(skill: &Skill, user_input: Value, available_tools: Vec<String>) -> Self {
        SkillExecutionContext {
            skill_name: skill.name().to_string(),
            instructions: skill.instructions.clone(),
            available_tools,
            user_input,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::types::CapabilityLevel;
    use crate::llm::provider::{
        FinishReason, LLMError, LLMProvider, LLMRequest, LLMResponse, ToolCall,
    };
    use crate::skills::types::{SkillFileSystemPermissions, SkillManifest, SkillPermissionProfile};
    use crate::tools::registry::ToolRegistration;
    use crate::tools::traits::Tool;
    use serde_json::json;
    use std::path::PathBuf;
    use std::sync::Mutex;
    use tempfile::tempdir;

    struct FakeForkExecutor;

    struct EchoFirstUserProvider;
    struct UnknownToolThenFinalizeProvider {
        calls: Mutex<usize>,
    }
    struct RepeatToolThenFinalizeProvider {
        tool_name: &'static str,
        calls: Mutex<usize>,
    }
    struct MaxIterationsThenFinalizeProvider {
        tool_names: Vec<String>,
        calls: Mutex<usize>,
    }
    struct CountingSkillTool {
        calls: Arc<Mutex<usize>>,
    }

    #[async_trait]
    impl LLMProvider for EchoFirstUserProvider {
        fn name(&self) -> &str {
            "echo-first-user"
        }

        fn supported_models(&self) -> Vec<String> {
            vec!["gpt-5.1-codex".to_string()]
        }

        fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
            Ok(())
        }

        async fn generate(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
            let first_message = request
                .messages
                .first()
                .map(|message| message.content.as_text().to_string())
                .unwrap_or_default();

            Ok(LLMResponse {
                content: Some(first_message),
                model: request.model,
                finish_reason: FinishReason::Stop,
                ..Default::default()
            })
        }
    }

    #[async_trait]
    impl LLMProvider for UnknownToolThenFinalizeProvider {
        fn name(&self) -> &str {
            "unknown-tool-then-finalize"
        }

        fn supported_models(&self) -> Vec<String> {
            vec!["gpt-5.1-codex".to_string()]
        }

        fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
            Ok(())
        }

        async fn generate(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
            let mut calls = self.calls.lock().expect("provider calls mutex");
            *calls += 1;

            match *calls {
                1 => Ok(LLMResponse {
                    content: Some(String::new()),
                    model: request.model,
                    tool_calls: Some(vec![ToolCall::function(
                        "call_unknown_tool".to_string(),
                        "unified_diff".to_string(),
                        "{}".to_string(),
                    )]),
                    finish_reason: FinishReason::ToolCalls,
                    ..Default::default()
                }),
                2 => {
                    assert!(request.tools.is_none());
                    let prompt = request
                        .messages
                        .last()
                        .map(|message| message.content.as_text().to_string())
                        .unwrap_or_default();
                    assert!(prompt.contains("unified_diff"));
                    assert!(prompt.contains(SKILL_TOOL_FREE_SYNTHESIS_PROMPT));

                    Ok(LLMResponse {
                        content: Some("finalized after unknown tool".to_string()),
                        model: request.model,
                        finish_reason: FinishReason::Stop,
                        ..Default::default()
                    })
                }
                _ => panic!("unexpected provider call count: {}", *calls),
            }
        }
    }

    #[async_trait]
    impl LLMProvider for RepeatToolThenFinalizeProvider {
        fn name(&self) -> &str {
            "repeat-tool-then-finalize"
        }

        fn supported_models(&self) -> Vec<String> {
            vec!["gpt-5.1-codex".to_string()]
        }

        fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
            Ok(())
        }

        async fn generate(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
            let mut calls = self.calls.lock().expect("provider calls mutex");
            *calls += 1;

            match *calls {
                1 | 2 => Ok(LLMResponse {
                    content: Some(String::new()),
                    model: request.model,
                    tool_calls: Some(vec![ToolCall::function(
                        format!("repeat_tool_call_{}", *calls),
                        self.tool_name.to_string(),
                        "{\"input\":\"same\"}".to_string(),
                    )]),
                    finish_reason: FinishReason::ToolCalls,
                    ..Default::default()
                }),
                3 => {
                    assert!(request.tools.is_none());
                    let prompt = request
                        .messages
                        .last()
                        .map(|message| message.content.as_text().to_string())
                        .unwrap_or_default();
                    assert!(prompt.contains("HARD STOP"));
                    assert!(prompt.contains(SKILL_TOOL_FREE_SYNTHESIS_PROMPT));

                    Ok(LLMResponse {
                        content: Some("finalized after loop detection".to_string()),
                        model: request.model,
                        finish_reason: FinishReason::Stop,
                        ..Default::default()
                    })
                }
                _ => panic!("unexpected provider call count: {}", *calls),
            }
        }
    }

    #[async_trait]
    impl LLMProvider for MaxIterationsThenFinalizeProvider {
        fn name(&self) -> &str {
            "max-iterations-then-finalize"
        }

        fn supported_models(&self) -> Vec<String> {
            vec!["gpt-5.1-codex".to_string()]
        }

        fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
            Ok(())
        }

        async fn generate(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
            let mut calls = self.calls.lock().expect("provider calls mutex");
            *calls += 1;

            if *calls <= MAX_SKILL_LLM_ITERATIONS {
                let tool_name = self.tool_names[*calls - 1].clone();
                return Ok(LLMResponse {
                    content: Some(String::new()),
                    model: request.model,
                    tool_calls: Some(vec![ToolCall::function(
                        format!("max_iterations_tool_call_{}", *calls),
                        tool_name,
                        format!("{{\"step\":{}}}", *calls),
                    )]),
                    finish_reason: FinishReason::ToolCalls,
                    ..Default::default()
                });
            }

            assert_eq!(*calls, MAX_SKILL_LLM_ITERATIONS + 1);
            assert!(request.tools.is_none());
            let prompt = request
                .messages
                .last()
                .map(|message| message.content.as_text().to_string())
                .unwrap_or_default();
            assert!(prompt.contains("maximum tool-call iterations"));
            assert!(prompt.contains(&MAX_SKILL_LLM_ITERATIONS.to_string()));
            assert!(prompt.contains(SKILL_TOOL_FREE_SYNTHESIS_PROMPT));

            Ok(LLMResponse {
                content: Some("finalized after max iterations".to_string()),
                model: request.model,
                finish_reason: FinishReason::Stop,
                ..Default::default()
            })
        }
    }

    #[async_trait]
    impl ForkSkillExecutor for FakeForkExecutor {
        async fn execute(&self, skill: &Skill, user_input: Value) -> Result<Value> {
            Ok(serde_json::json!({
                "execution_context": "fork",
                "status": "success",
                "summary": format!("forked {}", skill.name()),
                "artifact_paths": [],
                "delegate_session_id": "child-session",
                "echo": user_input,
            }))
        }
    }

    #[async_trait]
    impl Tool for CountingSkillTool {
        async fn execute(&self, args: Value) -> Result<Value> {
            let mut calls = self.calls.lock().expect("tool calls mutex");
            *calls += 1;
            Ok(json!({
                "success": true,
                "echo": args,
            }))
        }

        fn name(&self) -> &'static str {
            "counting_skill_tool"
        }

        fn description(&self) -> &'static str {
            "Counts skill tool invocations"
        }
    }

    #[tokio::test]
    async fn test_skill_tool_adapter_exposes_underlying_skill_name() {
        let manifest = SkillManifest {
            name: "test-skill".to_string(),
            description: "Test skill".to_string(),
            vtcode_native: Some(true),
            ..Default::default()
        };

        let skill = Skill::new(
            manifest,
            PathBuf::from("/tmp"),
            "# Instructions".to_string(),
        )
        .expect("failed to create skill");

        let adapter = SkillToolAdapter::new(skill);
        assert_eq!(adapter.skill().name(), "test-skill");
    }

    #[tokio::test]
    async fn test_skill_tool_adapter_execute() {
        let manifest = SkillManifest {
            name: "test-skill".to_string(),
            description: "Test skill".to_string(),
            vtcode_native: Some(true),
            ..Default::default()
        };

        let skill = Skill::new(
            manifest,
            PathBuf::from("/tmp"),
            "# Test Instructions".to_string(),
        )
        .expect("failed to create skill");

        let adapter = SkillToolAdapter::new(skill);
        let args = serde_json::json!({"test": "value"});
        let result = adapter.execute(args).await;

        assert!(result.is_ok());
        let res = result.unwrap();
        assert_eq!(res["skill_name"], "test-skill");
        assert_eq!(res["status"], "executing");
    }

    #[tokio::test]
    async fn test_fork_skill_adapter_uses_fork_executor() {
        let manifest = SkillManifest {
            name: "fork-skill".to_string(),
            description: "Forked skill".to_string(),
            context: Some("fork".to_string()),
            vtcode_native: Some(true),
            ..Default::default()
        };

        let skill = Skill::new(
            manifest,
            PathBuf::from("/tmp"),
            "# Test Instructions".to_string(),
        )
        .expect("failed to create skill");

        let adapter = SkillToolAdapter::with_fork_executor(skill, Arc::new(FakeForkExecutor));
        let args = serde_json::json!({"task": "value"});
        let result = adapter.execute(args.clone()).await.expect("fork execution");

        assert_eq!(result["execution_context"], "fork");
        assert_eq!(result["delegate_session_id"], "child-session");
        assert_eq!(result["echo"], args);
    }

    #[tokio::test]
    async fn blank_skill_input_uses_default_prompt_for_sub_llm() {
        let manifest = SkillManifest {
            name: "test-skill".to_string(),
            description: "Test skill".to_string(),
            vtcode_native: Some(true),
            ..Default::default()
        };
        let skill = Skill::new(
            manifest,
            PathBuf::from("/tmp"),
            "# Test Instructions".to_string(),
        )
        .expect("failed to create skill");
        let workspace = tempdir().expect("temp workspace");
        let mut registry = ToolRegistry::new(workspace.path().to_path_buf()).await;

        let result = execute_skill_with_sub_llm(
            &skill,
            String::new(),
            &EchoFirstUserProvider,
            &mut registry,
            Vec::new(),
            "gpt-5.1-codex".to_string(),
        )
        .await
        .expect("blank input should be normalized");

        assert_eq!(result, EMPTY_SKILL_INPUT_PROMPT);
    }

    #[tokio::test]
    async fn non_empty_skill_input_is_preserved_for_sub_llm() {
        let manifest = SkillManifest {
            name: "test-skill".to_string(),
            description: "Test skill".to_string(),
            vtcode_native: Some(true),
            ..Default::default()
        };
        let skill = Skill::new(
            manifest,
            PathBuf::from("/tmp"),
            "# Test Instructions".to_string(),
        )
        .expect("failed to create skill");
        let workspace = tempdir().expect("temp workspace");
        let mut registry = ToolRegistry::new(workspace.path().to_path_buf()).await;

        let result = execute_skill_with_sub_llm(
            &skill,
            "security".to_string(),
            &EchoFirstUserProvider,
            &mut registry,
            Vec::new(),
            "gpt-5.1-codex".to_string(),
        )
        .await
        .expect("non-empty input should be preserved");

        assert_eq!(result, "security");
    }

    #[tokio::test]
    async fn skill_executor_forces_final_synthesis_after_unknown_tool() {
        let manifest = SkillManifest {
            name: "test-skill".to_string(),
            description: "Test skill".to_string(),
            vtcode_native: Some(true),
            ..Default::default()
        };
        let skill = Skill::new(
            manifest,
            PathBuf::from("/tmp"),
            "# Test Instructions".to_string(),
        )
        .expect("failed to create skill");
        let workspace = tempdir().expect("temp workspace");
        let mut registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
        registry.allow_all_tools().await.expect("allow tools");
        let provider = UnknownToolThenFinalizeProvider {
            calls: Mutex::new(0),
        };

        let result = execute_skill_with_sub_llm(
            &skill,
            "review".to_string(),
            &provider,
            &mut registry,
            vec![ToolDefinition::function(
                "read_file".to_string(),
                "Read".to_string(),
                json!({"type": "object"}),
            )],
            "gpt-5.1-codex".to_string(),
        )
        .await
        .expect("unknown tool should trigger final synthesis");

        assert_eq!(result, "finalized after unknown tool");
    }

    #[tokio::test]
    async fn skill_executor_skips_repeated_tool_call_and_finalizes() {
        let manifest = SkillManifest {
            name: "test-skill".to_string(),
            description: "Test skill".to_string(),
            vtcode_native: Some(true),
            ..Default::default()
        };
        let skill = Skill::new(
            manifest,
            PathBuf::from("/tmp"),
            "# Test Instructions".to_string(),
        )
        .expect("failed to create skill");
        let workspace = tempdir().expect("temp workspace");
        let mut registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
        let tool_name = "skill_loop_test_tool";
        let tool_calls = Arc::new(Mutex::new(0usize));
        registry
            .register_tool(ToolRegistration::from_tool_instance(
                tool_name,
                CapabilityLevel::CodeSearch,
                CountingSkillTool {
                    calls: Arc::clone(&tool_calls),
                },
            ))
            .await
            .expect("register tool");
        registry.allow_all_tools().await.expect("allow tools");
        let provider = RepeatToolThenFinalizeProvider {
            tool_name,
            calls: Mutex::new(0),
        };

        let result = execute_skill_with_sub_llm(
            &skill,
            "review".to_string(),
            &provider,
            &mut registry,
            vec![ToolDefinition::function(
                tool_name.to_string(),
                "Loop test tool".to_string(),
                json!({"type": "object"}),
            )],
            "gpt-5.1-codex".to_string(),
        )
        .await
        .expect("looping tool calls should force a final synthesis");

        assert_eq!(result, "finalized after loop detection");
        assert_eq!(*tool_calls.lock().expect("tool calls mutex"), 1);
    }

    #[tokio::test]
    async fn skill_executor_forces_final_synthesis_after_max_iterations() {
        let manifest = SkillManifest {
            name: "test-skill".to_string(),
            description: "Test skill".to_string(),
            vtcode_native: Some(true),
            ..Default::default()
        };
        let skill = Skill::new(
            manifest,
            PathBuf::from("/tmp"),
            "# Test Instructions".to_string(),
        )
        .expect("failed to create skill");
        let workspace = tempdir().expect("temp workspace");
        let mut registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
        let tool_calls = Arc::new(Mutex::new(0usize));
        let mut available_tools = Vec::with_capacity(MAX_SKILL_LLM_ITERATIONS);
        let mut tool_names = Vec::with_capacity(MAX_SKILL_LLM_ITERATIONS);

        for index in 0..MAX_SKILL_LLM_ITERATIONS {
            let tool_name = format!("skill_iteration_test_tool_{index}");
            registry
                .register_tool(ToolRegistration::from_tool_instance(
                    tool_name.as_str(),
                    CapabilityLevel::CodeSearch,
                    CountingSkillTool {
                        calls: Arc::clone(&tool_calls),
                    },
                ))
                .await
                .unwrap_or_else(|error| panic!("register tool {tool_name}: {error}"));
            available_tools.push(ToolDefinition::function(
                tool_name.clone(),
                format!("Iteration tool {index}"),
                json!({"type": "object"}),
            ));
            tool_names.push(tool_name);
        }

        registry.allow_all_tools().await.expect("allow tools");
        let provider = MaxIterationsThenFinalizeProvider {
            tool_names,
            calls: Mutex::new(0),
        };

        let result = execute_skill_with_sub_llm(
            &skill,
            "analyze".to_string(),
            &provider,
            &mut registry,
            available_tools,
            "gpt-5.1-codex".to_string(),
        )
        .await
        .expect("max-iteration recovery should force a final synthesis");

        assert_eq!(result, "finalized after max iterations");
        assert_eq!(
            *tool_calls.lock().expect("tool calls mutex"),
            MAX_SKILL_LLM_ITERATIONS
        );
    }

    #[test]
    fn test_filter_tools_no_network_policy() {
        let manifest = SkillManifest {
            name: "test-skill".to_string(),
            description: "Test".to_string(),
            network_policy: None,
            vtcode_native: Some(true),
            ..Default::default()
        };
        let skill = Skill::new(manifest, PathBuf::from("/tmp"), "instructions".to_string())
            .expect("failed to create skill");

        let tools = vec![
            ToolDefinition::function(
                "read_file".to_string(),
                "Read".to_string(),
                serde_json::json!({}),
            ),
            ToolDefinition::web_search(serde_json::json!({})),
            ToolDefinition::function(
                "web_search".to_string(),
                "Search".to_string(),
                serde_json::json!({}),
            ),
        ];
        let filtered = filter_tools_for_skill(&skill, tools);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].function.as_ref().unwrap().name, "read_file");
    }

    #[test]
    fn test_filter_tools_with_network_policy_updates_native_web_search() {
        let manifest = SkillManifest {
            name: "test-skill".to_string(),
            description: "Test".to_string(),
            network_policy: Some(SkillNetworkPolicy {
                allowed_domains: vec!["api.example.com".to_string()],
                denied_domains: vec!["blocked.example.com".to_string()],
            }),
            vtcode_native: Some(true),
            ..Default::default()
        };
        let skill = Skill::new(manifest, PathBuf::from("/tmp"), "instructions".to_string())
            .expect("failed to create skill");

        let tools = vec![ToolDefinition::web_search(serde_json::json!({
            "user_location": "US"
        }))];
        let filtered = filter_tools_for_skill(&skill, tools);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].tool_type, "web_search");
        assert_eq!(
            filtered[0].web_search.as_ref(),
            Some(&serde_json::json!({
                "user_location": "US",
                "allowed_domains": ["api.example.com"],
                "blocked_domains": ["blocked.example.com"]
            }))
        );
    }

    #[test]
    fn test_filter_tools_no_network_policy_removes_gemini_native_network_tools() {
        let manifest = SkillManifest {
            name: "test-skill".to_string(),
            description: "Test".to_string(),
            network_policy: None,
            vtcode_native: Some(true),
            ..Default::default()
        };
        let skill = Skill::new(manifest, PathBuf::from("/tmp"), "instructions".to_string())
            .expect("failed to create skill");

        let tools = vec![
            ToolDefinition::google_maps(serde_json::json!({})),
            ToolDefinition::url_context(serde_json::json!({})),
            ToolDefinition::function(
                "read_file".to_string(),
                "Read".to_string(),
                serde_json::json!({}),
            ),
        ];

        let filtered = filter_tools_for_skill(&skill, tools);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].function_name(), "read_file");
    }

    #[test]
    fn test_filter_tools_with_network_policy_drops_gemini_native_network_tools() {
        let manifest = SkillManifest {
            name: "test-skill".to_string(),
            description: "Test".to_string(),
            network_policy: Some(SkillNetworkPolicy {
                allowed_domains: vec!["example.com".to_string()],
                denied_domains: vec![],
            }),
            vtcode_native: Some(true),
            ..Default::default()
        };
        let skill = Skill::new(manifest, PathBuf::from("/tmp"), "instructions".to_string())
            .expect("failed to create skill");

        let filtered = filter_tools_for_skill(
            &skill,
            vec![
                ToolDefinition::google_maps(serde_json::json!({})),
                ToolDefinition::url_context(serde_json::json!({})),
            ],
        );

        assert!(filtered.is_empty());
    }

    #[test]
    fn test_filter_tools_drops_function_style_network_tools_when_policy_is_present() {
        let manifest = SkillManifest {
            name: "test-skill".to_string(),
            description: "Test".to_string(),
            network_policy: Some(SkillNetworkPolicy {
                allowed_domains: vec!["api.example.com".to_string()],
                denied_domains: vec![],
            }),
            vtcode_native: Some(true),
            ..Default::default()
        };
        let skill = Skill::new(manifest, PathBuf::from("/tmp"), "instructions".to_string())
            .expect("failed to create skill");

        let tools = vec![
            ToolDefinition::function(
                "read_web_page".to_string(),
                "Read web page".to_string(),
                serde_json::json!({}),
            ),
            ToolDefinition::function(
                "read_file".to_string(),
                "Read".to_string(),
                serde_json::json!({}),
            ),
        ];
        let filtered = filter_tools_for_skill(&skill, tools);

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].function_name(), "read_file");
    }

    #[test]
    fn test_filter_tools_fails_closed_for_unrepresentable_web_search_policy() {
        let manifest = SkillManifest {
            name: "test-skill".to_string(),
            description: "Test".to_string(),
            network_policy: Some(SkillNetworkPolicy {
                allowed_domains: vec!["docs.rs".to_string()],
                denied_domains: vec!["example.com".to_string()],
            }),
            vtcode_native: Some(true),
            ..Default::default()
        };
        let skill = Skill::new(manifest, PathBuf::from("/tmp"), "instructions".to_string())
            .expect("failed to create skill");

        let mut anthropic_web_search = ToolDefinition::web_search(serde_json::json!({}));
        anthropic_web_search.tool_type = "web_search_20250305".to_string();

        let filtered = filter_tools_for_skill(&skill, vec![anthropic_web_search]);

        assert!(filtered.is_empty());
    }

    #[test]
    fn test_skill_execution_context() {
        let manifest = SkillManifest {
            name: "test-skill".to_string(),
            description: "Test skill".to_string(),
            vtcode_native: Some(true),
            ..Default::default()
        };

        let skill = Skill::new(manifest, PathBuf::from("/tmp"), "Instructions".to_string())
            .expect("failed to create skill");

        let tools = vec!["file_ops".to_string(), "shell".to_string()];
        let input = serde_json::json!({"test": "input"});

        let ctx = SkillExecutionContext::new(&skill, input, tools);
        assert_eq!(ctx.skill_name, "test-skill");
        assert_eq!(ctx.available_tools.len(), 2);
    }

    fn test_skill_with_permissions(permission_profile: Option<SkillPermissionProfile>) -> Skill {
        let manifest = SkillManifest {
            name: "test-skill".to_string(),
            description: "Test skill".to_string(),
            permissions: permission_profile,
            vtcode_native: Some(true),
            ..Default::default()
        };

        Skill::new(
            manifest,
            PathBuf::from("/tmp/test-skill"),
            "Instructions".to_string(),
        )
        .expect("failed to create skill")
    }

    #[test]
    fn skill_command_permissions_inject_additional_permissions() {
        let skill = test_skill_with_permissions(Some(SkillPermissionProfile {
            file_system: Some(SkillFileSystemPermissions {
                read: vec![PathBuf::from("references")],
                write: vec![PathBuf::from("outputs")],
            }),
        }));

        let merged =
            merge_skill_command_permissions(&skill, "shell", serde_json::json!({"command": "pwd"}));

        assert_eq!(
            merged["sandbox_permissions"],
            serde_json::json!("with_additional_permissions")
        );
        assert_eq!(
            merged["additional_permissions"]["fs_read"],
            serde_json::json!(["/tmp/test-skill/references"])
        );
        assert_eq!(
            merged["additional_permissions"]["fs_write"],
            serde_json::json!(["/tmp/test-skill/outputs"])
        );
    }

    #[test]
    fn skill_command_permissions_merge_existing_permissions() {
        let skill = test_skill_with_permissions(Some(SkillPermissionProfile {
            file_system: Some(SkillFileSystemPermissions {
                read: vec![PathBuf::from("references")],
                write: vec![PathBuf::from("outputs")],
            }),
        }));

        let merged = merge_skill_command_permissions(
            &skill,
            "shell",
            serde_json::json!({
                "command": "pwd",
                "sandbox_permissions": "with_additional_permissions",
                "additional_permissions": {
                    "fs_read": ["/tmp/existing-read"],
                    "fs_write": ["/tmp/existing-write"]
                }
            }),
        );

        assert_eq!(
            merged["additional_permissions"]["fs_read"],
            serde_json::json!(["/tmp/existing-read", "/tmp/test-skill/references"])
        );
        assert_eq!(
            merged["additional_permissions"]["fs_write"],
            serde_json::json!(["/tmp/existing-write", "/tmp/test-skill/outputs"])
        );
    }

    #[test]
    fn skill_command_permissions_ignore_require_escalated() {
        let skill = test_skill_with_permissions(Some(SkillPermissionProfile {
            file_system: Some(SkillFileSystemPermissions {
                read: Vec::new(),
                write: vec![PathBuf::from("outputs")],
            }),
        }));
        let original = serde_json::json!({
            "command": "pwd",
            "sandbox_permissions": "require_escalated",
            "justification": "Do you want to run this command without sandbox restrictions?"
        });

        let merged = merge_skill_command_permissions(&skill, "shell", original.clone());

        assert_eq!(merged, original);
    }

    #[test]
    fn skill_command_permissions_ignore_empty_skill_permissions() {
        let skill = test_skill_with_permissions(None);
        let original = serde_json::json!({"command": "pwd"});

        let merged = merge_skill_command_permissions(&skill, "shell", original.clone());

        assert_eq!(merged, original);
    }
}
