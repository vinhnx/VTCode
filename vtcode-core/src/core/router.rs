use serde::{Deserialize, Serialize};

use crate::config::loader::VTCodeConfig;
use crate::config::types::AgentConfig as CoreAgentConfig;
use crate::llm::{
    factory::{create_provider_with_config, get_factory},
    provider as uni,
};
use crate::models::ModelId;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
pub enum TaskClass {
    Simple,
    Standard,
    Complex,
    CodegenHeavy,
    RetrievalHeavy,
}

impl std::fmt::Display for TaskClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TaskClass::Simple => write!(f, "simple"),
            TaskClass::Standard => write!(f, "standard"),
            TaskClass::Complex => write!(f, "complex"),
            TaskClass::CodegenHeavy => write!(f, "codegen_heavy"),
            TaskClass::RetrievalHeavy => write!(f, "retrieval_heavy"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteDecision {
    pub class: TaskClass,
    pub selected_model: String,
}

pub struct Router;

impl Router {
    pub fn classify_heuristic(input: &str) -> TaskClass {
        let text = input.to_lowercase();
        let has_code_fence = text.contains("```") || text.contains("diff --git");
        let has_patch_keywords = [
            "apply_patch",
            "unified diff",
            "patch",
            "edit_file",
            "create_file",
        ]
        .iter()
        .any(|k| text.contains(k));
        let retrieval = [
            "search",
            "web",
            "google",
            "docs",
            "cite",
            "source",
            "up-to-date",
        ]
        .iter()
        .any(|k| text.contains(k));
        let complex_markers = [
            "plan",
            "multi-step",
            "decompose",
            "orchestrate",
            "architecture",
            "benchmark",
            "implement end-to-end",
            "design api",
            "refactor module",
            "evaluate",
            "tests suite",
        ];
        let complex = complex_markers.iter().any(|k| text.contains(k));
        let long = text.len() > 1200;

        if has_code_fence || has_patch_keywords {
            return TaskClass::CodegenHeavy;
        }
        if retrieval {
            return TaskClass::RetrievalHeavy;
        }
        if complex || long {
            return TaskClass::Complex;
        }
        if text.len() < 120 {
            return TaskClass::Simple;
        }
        TaskClass::Standard
    }

    pub fn route(vt_cfg: &VTCodeConfig, core: &CoreAgentConfig, input: &str) -> RouteDecision {
        let router_cfg = &vt_cfg.router;
        let class = if router_cfg.heuristic_classification {
            Self::classify_heuristic(input)
        } else {
            // fallback: treat as standard
            TaskClass::Standard
        };

        let model = match class {
            TaskClass::Simple => non_empty_or(&router_cfg.models.simple, &core.model),
            TaskClass::Standard => non_empty_or(&router_cfg.models.standard, &core.model),
            TaskClass::Complex => non_empty_or(&router_cfg.models.complex, &core.model),
            TaskClass::CodegenHeavy => non_empty_or(&router_cfg.models.codegen_heavy, &core.model),
            TaskClass::RetrievalHeavy => {
                non_empty_or(&router_cfg.models.retrieval_heavy, &core.model)
            }
        };

        RouteDecision {
            class,
            selected_model: model.to_string(),
        }
    }

    /// Optional LLM-based classification when `router.llm_router_model` is set.
    /// Falls back to heuristics on any error.
    pub async fn route_async(
        vt_cfg: &VTCodeConfig,
        core: &CoreAgentConfig,
        api_key: &str,
        input: &str,
    ) -> RouteDecision {
        let router_cfg = &vt_cfg.router;
        let mut class = if router_cfg.heuristic_classification {
            Self::classify_heuristic(input)
        } else {
            TaskClass::Standard
        };

        if !router_cfg.llm_router_model.trim().is_empty() {
            let provider_name = if core.provider.trim().is_empty() {
                core.model
                    .parse::<ModelId>()
                    .ok()
                    .map(|model| model.provider().to_string())
                    .or_else(|| {
                        let factory = get_factory().lock().unwrap();
                        factory.provider_from_model(core.model.as_str())
                    })
                    .unwrap_or_else(|| "gemini".to_string())
            } else {
                core.provider.to_lowercase()
            };
            if let Ok(provider) = create_provider_with_config(
                &provider_name,
                Some(api_key.to_string()),
                None,
                Some(router_cfg.llm_router_model.clone()),
                Some(core.prompt_cache.clone()),
            ) {
                let sys = "You are a routing classifier. Output only one label: simple | standard | complex | codegen_heavy | retrieval_heavy. Choose the best class for the user's last message. No prose.".to_string();
                let supports_effort =
                    provider.supports_reasoning_effort(&router_cfg.llm_router_model);
                let reasoning_effort = if supports_effort {
                    Some(vt_cfg.agent.reasoning_effort)
                } else {
                    None
                };
                let req = uni::LLMRequest {
                    messages: vec![uni::Message::user(input.to_string())],
                    system_prompt: Some(sys),
                    tools: None,
                    model: router_cfg.llm_router_model.clone(),
                    max_tokens: Some(8),
                    temperature: Some(0.0),
                    stream: false,
                    tool_choice: Some(uni::ToolChoice::none()),
                    parallel_tool_calls: None,
                    parallel_tool_config: None,
                    reasoning_effort,
                };
                if let Ok(resp) = provider.generate(req).await {
                    if let Some(text) = resp.content {
                        let t = text.trim().to_lowercase();
                        class = match t {
                            x if x.contains("codegen") => TaskClass::CodegenHeavy,
                            x if x.contains("retrieval") => TaskClass::RetrievalHeavy,
                            x if x.contains("complex") => TaskClass::Complex,
                            x if x.contains("simple") => TaskClass::Simple,
                            _ => TaskClass::Standard,
                        };
                    }
                }
            }
        }

        let model = match class {
            TaskClass::Simple => non_empty_or(&router_cfg.models.simple, &core.model),
            TaskClass::Standard => non_empty_or(&router_cfg.models.standard, &core.model),
            TaskClass::Complex => non_empty_or(&router_cfg.models.complex, &core.model),
            TaskClass::CodegenHeavy => non_empty_or(&router_cfg.models.codegen_heavy, &core.model),
            TaskClass::RetrievalHeavy => {
                non_empty_or(&router_cfg.models.retrieval_heavy, &core.model)
            }
        };

        RouteDecision {
            class,
            selected_model: model.to_string(),
        }
    }
}

fn non_empty_or<'a>(value: &'a str, fallback: &'a str) -> &'a str {
    if value.trim().is_empty() {
        fallback
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_class_display() {
        assert_eq!(format!("{}", TaskClass::Simple), "simple");
        assert_eq!(format!("{}", TaskClass::Standard), "standard");
        assert_eq!(format!("{}", TaskClass::Complex), "complex");
        assert_eq!(format!("{}", TaskClass::CodegenHeavy), "codegen_heavy");
        assert_eq!(format!("{}", TaskClass::RetrievalHeavy), "retrieval_heavy");
    }
}
