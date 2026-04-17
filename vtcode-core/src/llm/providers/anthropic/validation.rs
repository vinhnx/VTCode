//! Request validation for Anthropic Claude API
//!
//! Validates:
//! - Message requirements
//! - Structured output schema compliance
//! - Extended thinking parameter constraints

use crate::config::core::AnthropicConfig;
use crate::config::types::ReasoningEffortLevel;
use crate::llm::error_display;
use crate::llm::provider::{
    AnthropicOptionalU32Override, AnthropicThinkingModeOverride, LLMError, LLMRequest, MessageRole,
    ToolChoice,
};

use super::capabilities::{
    adaptive_thinking_is_default, allowed_efforts_for_model, claude_thinking_profile,
    effort_allowed_for_model, resolve_model_name, supports_effort,
    supports_manual_interleaved_beta, supports_manual_thinking_budget, supports_structured_output,
    supports_task_budget,
};

pub fn validate_request(
    request: &LLMRequest,
    default_model: &str,
    anthropic_config: &AnthropicConfig,
) -> Result<(), LLMError> {
    if request.messages.is_empty() {
        let formatted_error =
            error_display::format_llm_error("Anthropic", "Messages cannot be empty");
        return Err(LLMError::InvalidRequest {
            message: formatted_error,
            metadata: None,
        });
    }

    // Note: Model name validation removed. The Anthropic API will validate model names
    // and return appropriate errors. This avoids maintenance burden of keeping hardcoded
    // model lists in sync and allows flexibility for proxies/aggregators.

    if request.output_format.is_some() && !supports_structured_output(&request.model, default_model)
    {
        let formatted_error = error_display::format_llm_error(
            "Anthropic",
            &format!(
                "Structured output is not supported for model '{}'. Structured outputs are only available for Claude Sonnet 4.5/4.6, Claude Opus 4.5/4.7, and Claude Haiku 4.5 models.",
                request.model
            ),
        );
        return Err(LLMError::InvalidRequest {
            message: formatted_error,
            metadata: None,
        });
    }

    if let Some(ref schema) = request.output_format
        && supports_structured_output(&request.model, default_model)
    {
        validate_anthropic_schema(schema)?;
    }

    if let Some(ref effort) = request.effort {
        validate_effort_setting(effort, &request.model, default_model)?;
    }

    let resolved_model = resolve_model_name(&request.model, default_model);
    let effective_thinking_mode =
        resolve_effective_thinking_mode(request, default_model, anthropic_config);

    if adaptive_thinking_is_default(resolved_model, default_model)
        && matches!(effective_thinking_mode, EffectiveThinkingMode::Disabled)
    {
        let formatted_error = error_display::format_llm_error(
            "Anthropic",
            &format!(
                "{} does not support disabled thinking on the Anthropic provider. Leave provider.anthropic.extended_thinking_enabled=true or choose another model.",
                resolved_model
            ),
        );
        return Err(LLMError::InvalidRequest {
            message: formatted_error,
            metadata: None,
        });
    }

    if resolved_model == crate::config::constants::models::anthropic::CLAUDE_OPUS_4_7
        && (request.temperature.is_some() || request.top_p.is_some() || request.top_k.is_some())
    {
        let formatted_error = error_display::format_llm_error(
            "Anthropic",
            "Claude Opus 4.7 rejects explicit temperature, top_p, and top_k values; omit sampling parameters entirely.",
        );
        return Err(LLMError::InvalidRequest {
            message: formatted_error,
            metadata: None,
        });
    }

    if matches!(
        effective_thinking_mode,
        EffectiveThinkingMode::ManualBudget(_)
    ) && !supports_manual_thinking_budget(resolved_model, default_model)
    {
        let formatted_error = error_display::format_llm_error(
            "Anthropic",
            &format!(
                "{} does not support thinking_budget/budget_tokens. Use adaptive thinking plus effort instead.",
                resolved_model
            ),
        );
        return Err(LLMError::InvalidRequest {
            message: formatted_error,
            metadata: None,
        });
    }

    if request.thinking_budget.is_some()
        && claude_thinking_profile(resolved_model, default_model).is_some_and(|profile| {
            matches!(
                profile.mode,
                super::capabilities::ClaudeThinkingMode::Adaptive
            ) && !profile.supports_manual_budget
        })
    {
        let formatted_error = error_display::format_llm_error(
            "Anthropic",
            &format!(
                "{} does not support thinking_budget/budget_tokens. Use adaptive thinking plus effort instead.",
                resolved_model
            ),
        );
        return Err(LLMError::InvalidRequest {
            message: formatted_error,
            metadata: None,
        });
    }

    if let Some(budget) = effective_manual_thinking_budget_override(request)
        && budget < 1024
    {
        let formatted_error = error_display::format_llm_error(
            "Anthropic",
            &format!("thinking_budget ({}) must be at least 1024 tokens.", budget),
        );
        return Err(LLMError::InvalidRequest {
            message: formatted_error,
            metadata: None,
        });
    }

    let thinking_active = !matches!(effective_thinking_mode, EffectiveThinkingMode::Disabled);
    if thinking_active {
        validate_reasoning_constraints(request, default_model, anthropic_config)?;
    }

    if request.prefill.is_some()
        && resolved_model == crate::config::constants::models::anthropic::CLAUDE_OPUS_4_7
    {
        let formatted_error = error_display::format_llm_error(
            "Anthropic",
            "Pre-filling assistant responses is not supported by Claude Opus 4.7.",
        );
        return Err(LLMError::InvalidRequest {
            message: formatted_error,
            metadata: None,
        });
    }

    if let Some(task_budget) = effective_task_budget_tokens(request, anthropic_config)
        && supports_task_budget(&request.model, default_model)
        && task_budget < 20_000
    {
        let formatted_error = error_display::format_llm_error(
            "Anthropic",
            &format!(
                "task_budget_tokens ({}) must be at least 20000 for Claude Opus 4.7.",
                task_budget
            ),
        );
        return Err(LLMError::InvalidRequest {
            message: formatted_error,
            metadata: None,
        });
    }

    if request.prefill.is_some() && thinking_active {
        let formatted_error = error_display::format_llm_error(
            "Anthropic",
            "Pre-filling assistant responses is not supported when extended thinking is enabled. Use 'prefill' only for non-reasoning requests.",
        );
        return Err(LLMError::InvalidRequest {
            message: formatted_error,
            metadata: None,
        });
    }

    if request.prefill.is_some() && request.output_format.is_some() {
        let formatted_error = error_display::format_llm_error(
            "Anthropic",
            "Pre-filling assistant responses is not supported when structured outputs are enabled.",
        );
        return Err(LLMError::InvalidRequest {
            message: formatted_error,
            metadata: None,
        });
    }

    if request.anthropic_request_overrides.is_some()
        && request.effort.is_some()
        && matches!(
            effective_thinking_mode,
            EffectiveThinkingMode::Disabled | EffectiveThinkingMode::ManualBudget(_)
        )
    {
        let formatted_error = error_display::format_llm_error(
            "Anthropic",
            "output_config.effort is only valid for adaptive-thinking Anthropic requests.",
        );
        return Err(LLMError::InvalidRequest {
            message: formatted_error,
            metadata: None,
        });
    }

    validate_tool_definitions(request)?;

    for message in &request.messages {
        if let Err(err) = message.validate_for_provider("anthropic") {
            let formatted = error_display::format_llm_error("Anthropic", &err);
            return Err(LLMError::InvalidRequest {
                message: formatted,
                metadata: None,
            });
        }
    }

    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EffectiveThinkingMode {
    Disabled,
    Adaptive,
    ManualBudget(u32),
}

fn resolve_effective_thinking_mode(
    request: &LLMRequest,
    default_model: &str,
    anthropic_config: &AnthropicConfig,
) -> EffectiveThinkingMode {
    let resolved_model = resolve_model_name(&request.model, default_model);
    if let Some(overrides) = request.anthropic_request_overrides.as_ref() {
        match overrides.thinking_mode {
            AnthropicThinkingModeOverride::Disabled => return EffectiveThinkingMode::Disabled,
            AnthropicThinkingModeOverride::Adaptive => return EffectiveThinkingMode::Adaptive,
            AnthropicThinkingModeOverride::ManualBudget(budget) => {
                return EffectiveThinkingMode::ManualBudget(budget);
            }
            AnthropicThinkingModeOverride::Inherit => {}
        }
    }

    let Some(profile) = claude_thinking_profile(resolved_model, default_model) else {
        if let Some(budget) = request.thinking_budget {
            return EffectiveThinkingMode::ManualBudget(budget);
        }
        if request
            .reasoning_effort
            .is_some_and(|effort| effort != ReasoningEffortLevel::None)
        {
            return EffectiveThinkingMode::Adaptive;
        }
        return EffectiveThinkingMode::Disabled;
    };

    if !anthropic_config.extended_thinking_enabled {
        return EffectiveThinkingMode::Disabled;
    }

    match profile.mode {
        super::capabilities::ClaudeThinkingMode::Adaptive => {
            if profile.supports_manual_budget
                && let Some(budget) = request.thinking_budget
            {
                EffectiveThinkingMode::ManualBudget(budget)
            } else {
                EffectiveThinkingMode::Adaptive
            }
        }
        super::capabilities::ClaudeThinkingMode::ManualBudget => {
            let budget = effective_manual_thinking_budget(request, anthropic_config);
            if budget >= 1024 {
                EffectiveThinkingMode::ManualBudget(budget)
            } else {
                EffectiveThinkingMode::Disabled
            }
        }
    }
}

fn effective_manual_thinking_budget_override(request: &LLMRequest) -> Option<u32> {
    if let Some(overrides) = request.anthropic_request_overrides.as_ref()
        && let AnthropicThinkingModeOverride::ManualBudget(budget) = overrides.thinking_mode
    {
        return Some(budget);
    }

    request.thinking_budget
}

fn effective_manual_thinking_budget(
    request: &LLMRequest,
    anthropic_config: &AnthropicConfig,
) -> u32 {
    if let Some(budget) = request.thinking_budget {
        return budget;
    }

    if let Some(effort) = request.reasoning_effort {
        return match effort {
            ReasoningEffortLevel::None => 0,
            ReasoningEffortLevel::Minimal => 1024,
            ReasoningEffortLevel::Low => 4096,
            ReasoningEffortLevel::Medium => 8192,
            ReasoningEffortLevel::High => 16384,
            ReasoningEffortLevel::XHigh => 32768,
            ReasoningEffortLevel::Max => 32768,
        };
    }

    anthropic_config.interleaved_thinking_budget_tokens
}

fn effective_task_budget_tokens(
    request: &LLMRequest,
    anthropic_config: &AnthropicConfig,
) -> Option<u32> {
    if let Some(overrides) = request.anthropic_request_overrides.as_ref() {
        return match overrides.task_budget_tokens {
            AnthropicOptionalU32Override::Explicit(total) => Some(total),
            AnthropicOptionalU32Override::Omit => None,
            AnthropicOptionalU32Override::Inherit => anthropic_config.task_budget_tokens,
        };
    }

    anthropic_config.task_budget_tokens
}

fn validate_tool_definitions(request: &LLMRequest) -> Result<(), LLMError> {
    let Some(tools) = request.tools.as_ref() else {
        return Ok(());
    };

    let mut has_programmatic_tool_calling = false;

    for tool in tools.iter() {
        let has_allowed_callers = tool
            .allowed_callers
            .as_ref()
            .is_some_and(|callers| !callers.is_empty());
        let has_input_examples = tool
            .input_examples
            .as_ref()
            .is_some_and(|examples| !examples.is_empty());

        if let Some(function) = tool.function.as_ref() {
            validate_anthropic_tool_name(&function.name)?;

            if has_allowed_callers && tool.strict == Some(true) {
                let formatted_error = error_display::format_llm_error(
                    "Anthropic",
                    &format!(
                        "tool '{}' cannot combine strict=true with allowed_callers; strict tool use is incompatible with programmatic tool calling",
                        function.name
                    ),
                );
                return Err(LLMError::InvalidRequest {
                    message: formatted_error,
                    metadata: None,
                });
            }
        } else if has_allowed_callers || has_input_examples {
            let formatted_error = error_display::format_llm_error(
                "Anthropic",
                &format!(
                    "tool type '{}' cannot use allowed_callers or input_examples without a function definition",
                    tool.tool_type
                ),
            );
            return Err(LLMError::InvalidRequest {
                message: formatted_error,
                metadata: None,
            });
        }

        has_programmatic_tool_calling |= has_allowed_callers;
    }

    if has_programmatic_tool_calling
        && request
            .parallel_tool_config
            .as_ref()
            .is_some_and(|config| config.disable_parallel_tool_use)
    {
        let formatted_error = error_display::format_llm_error(
            "Anthropic",
            "programmatic tool calling is incompatible with disable_parallel_tool_use=true",
        );
        return Err(LLMError::InvalidRequest {
            message: formatted_error,
            metadata: None,
        });
    }

    if has_programmatic_tool_calling
        && matches!(
            request.tool_choice,
            Some(ToolChoice::Any | ToolChoice::Specific(_))
        )
    {
        let formatted_error = error_display::format_llm_error(
            "Anthropic",
            "programmatic tool calling is incompatible with forced tool_choice values; use 'auto' or omit tool_choice",
        );
        return Err(LLMError::InvalidRequest {
            message: formatted_error,
            metadata: None,
        });
    }

    Ok(())
}

fn validate_anthropic_tool_name(name: &str) -> Result<(), LLMError> {
    let is_valid = !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'));

    if is_valid {
        return Ok(());
    }

    let formatted_error = error_display::format_llm_error(
        "Anthropic",
        &format!(
            "tool name '{}' must match ^[a-zA-Z0-9_-]{{1,64}}$ for Anthropic tool use",
            name
        ),
    );
    Err(LLMError::InvalidRequest {
        message: formatted_error,
        metadata: None,
    })
}

fn validate_effort_setting(effort: &str, model: &str, default_model: &str) -> Result<(), LLMError> {
    let normalized = effort.trim().to_ascii_lowercase();
    let is_supported = supports_effort(model, default_model);

    if !is_supported {
        let formatted_error = error_display::format_llm_error(
            "Anthropic",
            &format!(
                "effort is not supported for model '{}'. VT Code currently enables effort on Claude Opus 4.7, Claude Opus 4.6, Claude Sonnet 4.6, and Claude Mythos Preview on the Anthropic provider.",
                if model.trim().is_empty() {
                    default_model
                } else {
                    model
                }
            ),
        );
        return Err(LLMError::InvalidRequest {
            message: formatted_error,
            metadata: None,
        });
    }

    let allowed = allowed_efforts_for_model(model, default_model).unwrap_or(&[]);
    if !effort_allowed_for_model(model, default_model, &normalized) {
        let formatted_error = error_display::format_llm_error(
            "Anthropic",
            &format!(
                "effort must be one of {} (got '{}').",
                allowed.join(", "),
                effort,
            ),
        );
        return Err(LLMError::InvalidRequest {
            message: formatted_error,
            metadata: None,
        });
    }

    Ok(())
}

fn validate_reasoning_constraints(
    request: &LLMRequest,
    default_model: &str,
    anthropic_config: &AnthropicConfig,
) -> Result<(), LLMError> {
    if let Some(ToolChoice::Any | ToolChoice::Specific(_)) = request.tool_choice {
        let formatted_error = error_display::format_llm_error(
            "Anthropic",
            "Forced tool use (any/specific) is incompatible with extended thinking. Use 'auto' or 'none'.",
        );
        return Err(LLMError::InvalidRequest {
            message: formatted_error,
            metadata: None,
        });
    }

    if let EffectiveThinkingMode::ManualBudget(budget) =
        resolve_effective_thinking_mode(request, default_model, anthropic_config)
    {
        let max_tokens = request.max_tokens.unwrap_or(4096);
        if supports_manual_thinking_budget(&request.model, default_model)
            && budget >= max_tokens
            && !supports_manual_interleaved_beta(&request.model, default_model)
        {
            let formatted_error = error_display::format_llm_error(
                "Anthropic",
                &format!(
                    "The value of max_tokens ({}) must be strictly greater than budget_tokens ({}) when extended thinking is enabled without interleaved-thinking support.",
                    max_tokens, budget
                ),
            );
            return Err(LLMError::InvalidRequest {
                message: formatted_error,
                metadata: None,
            });
        }

        if request.temperature.is_some() || request.top_k.is_some() {
            let formatted_error = error_display::format_llm_error(
                "Anthropic",
                "temperature and top_k parameters must not be set when extended thinking is enabled.",
            );
            return Err(LLMError::InvalidRequest {
                message: formatted_error,
                metadata: None,
            });
        }
    }

    if let Some(top_p) = request.top_p
        && !(0.95..=1.0).contains(&top_p)
    {
        let formatted_error = error_display::format_llm_error(
            "Anthropic",
            &format!(
                "top_p must be between 0.95 and 1.0 (got {}) when extended thinking is enabled.",
                top_p
            ),
        );
        return Err(LLMError::InvalidRequest {
            message: formatted_error,
            metadata: None,
        });
    }

    if let Some(last_msg) = request.messages.last()
        && last_msg.role == MessageRole::Assistant
    {
        let formatted_error = error_display::format_llm_error(
            "Anthropic",
            "Pre-filling assistant responses is not supported when extended thinking is enabled.",
        );
        return Err(LLMError::InvalidRequest {
            message: formatted_error,
            metadata: None,
        });
    }

    Ok(())
}

pub fn validate_anthropic_schema(schema: &serde_json::Value) -> Result<(), LLMError> {
    use serde_json::Value;

    match schema {
        Value::Object(obj) => {
            validate_schema_object(obj, "root")?;
        }
        Value::String(_) | Value::Number(_) | Value::Bool(_) | Value::Array(_) | Value::Null => {
            let formatted_error = error_display::format_llm_error(
                "Anthropic",
                "Structured output schema must be a JSON object",
            );
            return Err(LLMError::InvalidRequest {
                message: formatted_error,
                metadata: None,
            });
        }
    }
    Ok(())
}

fn validate_schema_object(
    obj: &serde_json::Map<String, serde_json::Value>,
    path: &str,
) -> Result<(), LLMError> {
    use serde_json::Value;

    for (key, value) in obj {
        match key.as_str() {
            "type" => {
                if let Some(type_str) = value.as_str() {
                    match type_str {
                        "object" | "array" | "string" | "number" | "integer" | "boolean"
                        | "null" => {}
                        _ => {
                            let formatted_error = error_display::format_llm_error(
                                "Anthropic",
                                &format!("Unsupported schema type '{}', path: {}", type_str, path),
                            );
                            return Err(LLMError::InvalidRequest {
                                message: formatted_error,
                                metadata: None,
                            });
                        }
                    }
                }
            }
            "minimum" | "maximum" | "multipleOf" => {
                let formatted_error = error_display::format_llm_error(
                    "Anthropic",
                    &format!(
                        "Numeric constraints like '{}' are not supported by Anthropic structured output. Path: {}",
                        key, path
                    ),
                );
                return Err(LLMError::InvalidRequest {
                    message: formatted_error,
                    metadata: None,
                });
            }
            "minLength" | "maxLength" => {
                let formatted_error = error_display::format_llm_error(
                    "Anthropic",
                    &format!(
                        "String constraints like '{}' are not supported by Anthropic structured output. Path: {}",
                        key, path
                    ),
                );
                return Err(LLMError::InvalidRequest {
                    message: formatted_error,
                    metadata: None,
                });
            }
            "minItems" | "maxItems" | "uniqueItems" => {
                if key == "minItems" {
                    if let Some(min_items) = value.as_u64()
                        && min_items > 1
                    {
                        let formatted_error = error_display::format_llm_error(
                            "Anthropic",
                            &format!(
                                "Array minItems only supports values 0 or 1, got {}, path: {}",
                                min_items, path
                            ),
                        );
                        return Err(LLMError::InvalidRequest {
                            message: formatted_error,
                            metadata: None,
                        });
                    }
                } else {
                    let formatted_error = error_display::format_llm_error(
                        "Anthropic",
                        &format!(
                            "Array constraints like '{}' are not supported by Anthropic structured output. Path: {}",
                            key, path
                        ),
                    );
                    return Err(LLMError::InvalidRequest {
                        message: formatted_error,
                        metadata: None,
                    });
                }
            }
            "additionalProperties" => {
                if let Some(additional_props) = value.as_bool()
                    && additional_props
                {
                    let formatted_error = error_display::format_llm_error(
                        "Anthropic",
                        &format!(
                            "additionalProperties must be set to false, got {}, path: {}",
                            additional_props, path
                        ),
                    );
                    return Err(LLMError::InvalidRequest {
                        message: formatted_error,
                        metadata: None,
                    });
                }
            }
            "properties" => {
                if let Value::Object(props) = value {
                    for (prop_name, prop_value) in props {
                        let prop_path = format!("{}.properties.{}", path, prop_name);
                        if let Value::Object(prop_obj) = prop_value {
                            validate_schema_object(prop_obj, &prop_path)?;
                        }
                    }
                }
            }
            "items" => {
                if let Value::Object(items_obj) = value {
                    let items_path = format!("{}.items", path);
                    validate_schema_object(items_obj, &items_path)?;
                }
            }
            "anyOf" | "allOf" | "oneOf" => {
                if let Value::Array(options) = value {
                    for (i, option) in options.iter().enumerate() {
                        if let Value::Object(option_obj) = option {
                            let option_path = format!("{}.{}[{}]", path, key, i);
                            validate_schema_object(option_obj, &option_path)?;
                        }
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}
