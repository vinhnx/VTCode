use serde::{Deserialize, Serialize};

use crate::config::loader::VTCodeConfig;
use crate::config::models::Provider;
use crate::config::router::{HeuristicSettings, RouterConfig};
use crate::config::types::AgentConfig as CoreAgentConfig;
use crate::llm::{
    factory::{create_provider_with_config, infer_provider},
    provider as uni,
};

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
        TaskClassifier::new(&HeuristicSettings::default()).classify(input)
    }

    pub fn route(vt_cfg: &VTCodeConfig, core: &CoreAgentConfig, input: &str) -> RouteDecision {
        let router_cfg = &vt_cfg.router;
        let classifier = TaskClassifier::new(&router_cfg.heuristics);
        let selector = ModelSelector::new(&router_cfg, &core.model);

        let class = if router_cfg.heuristic_classification {
            classifier.classify(input)
        } else {
            TaskClass::Standard
        };

        RouteDecision {
            class,
            selected_model: selector.select(class),
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
        let classifier = TaskClassifier::new(&router_cfg.heuristics);
        let selector = ModelSelector::new(router_cfg, &core.model);

        let mut class = if router_cfg.heuristic_classification {
            classifier.classify(input)
        } else {
            TaskClass::Standard
        };

        if !router_cfg.llm_router_model.trim().is_empty() {
            let provider_override = if core.provider.trim().is_empty() {
                None
            } else {
                Some(core.provider.as_str())
            };
            let provider =
                infer_provider(provider_override, &core.model).unwrap_or(Provider::Gemini);
            if let Ok(provider) = create_provider_with_config(
                &provider.to_string(),
                Some(api_key.to_string()),
                None,
                Some(router_cfg.llm_router_model.clone()),
                Some(core.prompt_cache.clone()),
                None,
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
                    verbosity: None,
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

        RouteDecision {
            class,
            selected_model: selector.select(class),
        }
    }
}

pub struct TaskClassifier<'a> {
    settings: &'a HeuristicSettings,
}

impl<'a> TaskClassifier<'a> {
    pub fn new(settings: &'a HeuristicSettings) -> Self {
        Self { settings }
    }

    pub fn classify(&self, input: &str) -> TaskClass {
        let text = input.to_lowercase();
        if self.contains_any(&text, &self.settings.code_patch_markers) {
            return TaskClass::CodegenHeavy;
        }
        if self.contains_any(&text, &self.settings.retrieval_markers) {
            return TaskClass::RetrievalHeavy;
        }
        if text.len() > self.settings.long_request_min_chars
            || self.contains_any(&text, &self.settings.complex_markers)
        {
            return TaskClass::Complex;
        }
        if text.len() < self.settings.short_request_max_chars {
            return TaskClass::Simple;
        }
        TaskClass::Standard
    }

    fn contains_any(&self, haystack: &str, needles: &[String]) -> bool {
        needles.iter().any(|marker| {
            let trimmed = marker.trim();
            if trimmed.is_empty() {
                false
            } else {
                let lowered = trimmed.to_lowercase();
                haystack.contains(lowered.as_str())
            }
        })
    }
}

pub struct ModelSelector<'a> {
    router_cfg: &'a RouterConfig,
    fallback: &'a str,
}

impl<'a> ModelSelector<'a> {
    pub fn new(router_cfg: &'a RouterConfig, fallback: &'a str) -> Self {
        Self {
            router_cfg,
            fallback,
        }
    }

    pub fn select(&self, class: TaskClass) -> String {
        non_empty_or(self.raw_model_for(class), self.fallback).to_string()
    }

    fn raw_model_for(&self, class: TaskClass) -> &str {
        match class {
            TaskClass::Simple => &self.router_cfg.models.simple,
            TaskClass::Standard => &self.router_cfg.models.standard,
            TaskClass::Complex => &self.router_cfg.models.complex,
            TaskClass::CodegenHeavy => &self.router_cfg.models.codegen_heavy,
            TaskClass::RetrievalHeavy => &self.router_cfg.models.retrieval_heavy,
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
