use std::fmt::Write as FmtWrite;
use std::str::FromStr;

use anyhow::Result;
use tracing::warn;

use vtcode_core::config::constants::ui;
use vtcode_core::config::models::Provider;
use vtcode_core::config::types::UiSurfacePreference;
use vtcode_core::core::context_curator::{
    ConversationPhase, CuratedContext, Message as CuratorMessage,
    ToolDefinition as CuratorToolDefinition,
};
use vtcode_core::core::token_budget::{ContextComponent, TokenBudgetManager};
use vtcode_core::llm::provider as uni;

pub(crate) struct CuratedPromptSection {
    pub(crate) heading: &'static str,
    pub(crate) component: ContextComponent,
    pub(crate) body: String,
}

pub(crate) fn map_role_to_component(role: &uni::MessageRole) -> ContextComponent {
    match role {
        uni::MessageRole::System => ContextComponent::SystemPrompt,
        uni::MessageRole::User => ContextComponent::UserMessage,
        uni::MessageRole::Assistant => ContextComponent::AssistantMessage,
        uni::MessageRole::Tool => ContextComponent::ToolResult,
    }
}

pub(crate) fn describe_phase(phase: ConversationPhase) -> Option<String> {
    match phase {
        ConversationPhase::Exploration => Some("Exploration – gathering context".to_string()),
        ConversationPhase::Implementation => {
            Some("Implementation – applying code changes".to_string())
        }
        ConversationPhase::Validation => {
            Some("Validation – executing tests and checks".to_string())
        }
        ConversationPhase::Debugging => {
            Some("Debugging – addressing failures or regressions".to_string())
        }
        ConversationPhase::Unknown => None,
    }
}

pub(crate) fn resolve_mode_label(preference: UiSurfacePreference, full_auto: bool) -> String {
    let base = match preference {
        UiSurfacePreference::Alternate => ui::HEADER_MODE_ALTERNATE,
        UiSurfacePreference::Inline => ui::HEADER_MODE_INLINE,
        UiSurfacePreference::Auto => ui::HEADER_MODE_AUTO,
    };
    if full_auto {
        format!("{}{}", base, ui::HEADER_MODE_FULL_AUTO_SUFFIX)
    } else {
        base.to_string()
    }
}

pub(crate) fn format_provider_label(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    Provider::from_str(trimmed)
        .map(|provider| provider.label().to_string())
        .unwrap_or_else(|_| {
            let mut chars = trimmed.chars();
            let Some(first) = chars.next() else {
                return String::new();
            };
            let mut formatted: String = first.to_uppercase().collect();
            formatted.push_str(chars.as_str());
            formatted
        })
}

pub(crate) fn build_curated_sections(context: &CuratedContext) -> Vec<CuratedPromptSection> {
    let mut sections = Vec::new();

    if let Some(summary) = &context.ledger_summary {
        if !summary.trim().is_empty() {
            sections.push(CuratedPromptSection {
                heading: "Decision Ledger",
                component: ContextComponent::DecisionLedger,
                body: summary.trim().to_string(),
            });
        }
    }

    if !context.active_files.is_empty() {
        let mut body = String::new();
        for file in &context.active_files {
            let _ = writeln!(body, "{} ({} lines)", file.path, file.size_lines);
            if !file.summary.trim().is_empty() {
                let _ = writeln!(body, "  {}", file.summary.trim());
            }
        }
        sections.push(CuratedPromptSection {
            heading: "Active Files",
            component: ContextComponent::FileContent,
            body,
        });
    }

    if !context.recent_errors.is_empty() {
        let mut body = String::new();
        for error in &context.recent_errors {
            let mut line = error.error_message.trim().to_string();
            if let Some(tool) = &error.tool_name {
                line.push_str(&format!(" (tool: {})", tool));
            }
            let _ = writeln!(body, "{}", line);
            if let Some(resolution) = &error.resolution {
                if !resolution.trim().is_empty() {
                    let _ = writeln!(body, "  resolution: {}", resolution.trim());
                }
            }
        }
        sections.push(CuratedPromptSection {
            heading: "Recent Errors",
            component: ContextComponent::ToolResult,
            body,
        });
    }

    if !context.relevant_tools.is_empty() {
        let mut body = String::new();
        for tool in &context.relevant_tools {
            let description = tool.description.trim();
            if description.is_empty() {
                let _ = writeln!(body, "{}", tool.name);
            } else {
                let _ = writeln!(body, "{} – {}", tool.name, description);
            }
        }
        sections.push(CuratedPromptSection {
            heading: "Relevant Tools",
            component: ContextComponent::ProjectGuidelines,
            body,
        });
    }

    if let Some(phase_text) = describe_phase(context.phase) {
        sections.push(CuratedPromptSection {
            heading: "Conversation Phase",
            component: ContextComponent::ProjectGuidelines,
            body: phase_text,
        });
    }

    sections
}

pub(crate) async fn build_curator_messages(
    history: &[uni::Message],
    token_budget: &TokenBudgetManager,
    token_budget_enabled: bool,
) -> Result<Vec<CuratorMessage>> {
    let mut messages = Vec::with_capacity(history.len());

    for (index, message) in history.iter().enumerate() {
        let mut materialized = message.content.clone();
        if let Some(tool_calls) = &message.tool_calls {
            if !tool_calls.is_empty() {
                let serialized =
                    serde_json::to_string(tool_calls).unwrap_or_else(|_| "[]".to_string());
                if !serialized.is_empty() {
                    if !materialized.is_empty() {
                        materialized.push('\n');
                    }
                    materialized.push_str(&serialized);
                }
            }
        }

        let component = map_role_to_component(&message.role);
        let component_id = format!("msg_{}", index);
        let component_id_ref = Some(component_id.as_str());
        let estimated_tokens = if token_budget_enabled {
            match token_budget
                .count_tokens_for_component(&materialized, component, component_id_ref)
                .await
            {
                Ok(count) => count,
                Err(err) => {
                    warn!(
                        ?err,
                        "Failed to count tokens for conversation message; using rough estimate"
                    );
                    let estimate = materialized.len() / 4;
                    token_budget
                        .record_tokens_for_component(component, estimate, component_id_ref)
                        .await;
                    estimate
                }
            }
        } else {
            materialized.len() / 4
        };

        messages.push(CuratorMessage {
            role: message.role.as_generic_str().to_string(),
            content: materialized,
            estimated_tokens,
        });
    }

    Ok(messages)
}

pub(crate) fn build_curator_tools(tools: &[uni::ToolDefinition]) -> Vec<CuratorToolDefinition> {
    tools
        .iter()
        .map(|tool| {
            let parameters_repr = tool.function.parameters.to_string();
            let estimated_tokens = tool.function.description.len() / 4 + parameters_repr.len() / 4;
            CuratorToolDefinition {
                name: tool.function.name.clone(),
                description: tool.function.description.clone(),
                estimated_tokens,
            }
        })
        .collect()
}
