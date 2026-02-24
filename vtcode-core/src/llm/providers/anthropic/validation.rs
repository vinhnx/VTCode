//! Request validation for Anthropic Claude API
//!
//! Validates:
//! - Message requirements
//! - Structured output schema compliance
//! - Extended thinking parameter constraints

use crate::config::core::AnthropicConfig;
use crate::llm::error_display;
use crate::llm::provider::{LLMError, LLMRequest, MessageRole, ToolChoice};

use super::capabilities::{supports_effort, supports_reasoning_effort, supports_structured_output};

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
                "Structured output is not supported for model '{}'. Structured outputs are only available for Claude Sonnet 4.5, Claude Opus 4.1, Claude 3.7 Sonnet, and Claude 3.5 Sonnet models.",
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

    if let Some(budget) = request.thinking_budget
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

    let has_reasoning = request.reasoning_effort.is_some() || request.thinking_budget.is_some();
    if has_reasoning {
        validate_reasoning_constraints(request, default_model, anthropic_config)?;
    }

    if request.prefill.is_some() && has_reasoning {
        let formatted_error = error_display::format_llm_error(
            "Anthropic",
            "Pre-filling assistant responses is not supported when extended thinking is enabled. Use 'prefill' only for non-reasoning requests.",
        );
        return Err(LLMError::InvalidRequest {
            message: formatted_error,
            metadata: None,
        });
    }

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

fn validate_effort_setting(effort: &str, model: &str, default_model: &str) -> Result<(), LLMError> {
    let normalized = effort.trim().to_ascii_lowercase();
    let is_supported = supports_effort(model, default_model);

    if !is_supported {
        let formatted_error = error_display::format_llm_error(
            "Anthropic",
            &format!(
                "effort is not supported for model '{}'. Supported models are Claude Opus 4.5 and Claude Opus 4.6.",
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

    let allowed = ["low", "medium", "high", "max"];
    if !allowed.contains(&normalized.as_str()) {
        let formatted_error = error_display::format_llm_error(
            "Anthropic",
            &format!(
                "effort must be one of low, medium, high, or max (got '{}').",
                effort
            ),
        );
        return Err(LLMError::InvalidRequest {
            message: formatted_error,
            metadata: None,
        });
    }

    let resolved_model = if model.trim().is_empty() {
        default_model
    } else {
        model
    };
    if normalized == "max"
        && resolved_model != crate::config::constants::models::anthropic::CLAUDE_OPUS_4_6
    {
        let formatted_error = error_display::format_llm_error(
            "Anthropic",
            "effort='max' is only supported by Claude Opus 4.6.",
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
    use crate::config::types::ReasoningEffortLevel;

    let budget = if let Some(b) = request.thinking_budget {
        b
    } else if let Some(effort) = request.reasoning_effort {
        match effort {
            ReasoningEffortLevel::None => 0,
            ReasoningEffortLevel::Minimal => 1024,
            ReasoningEffortLevel::Low => 4096,
            ReasoningEffortLevel::Medium => 8192,
            ReasoningEffortLevel::High => 16384,
            ReasoningEffortLevel::XHigh => 32768,
        }
    } else {
        anthropic_config.interleaved_thinking_budget_tokens
    };

    let max_tokens = request.max_tokens.unwrap_or(4096);
    if budget >= max_tokens && !supports_reasoning_effort(&request.model, default_model) {
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

    if let Some(top_p) = request.top_p {
        if !(0.95..=1.0).contains(&top_p) {
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
    }

    if let Some(last_msg) = request.messages.last() {
        if last_msg.role == MessageRole::Assistant {
            let formatted_error = error_display::format_llm_error(
                "Anthropic",
                "Pre-filling assistant responses is not supported when extended thinking is enabled.",
            );
            return Err(LLMError::InvalidRequest {
                message: formatted_error,
                metadata: None,
            });
        }
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
                    if let Some(min_items) = value.as_u64() {
                        if min_items > 1 {
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
                if let Some(additional_props) = value.as_bool() {
                    if additional_props {
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
