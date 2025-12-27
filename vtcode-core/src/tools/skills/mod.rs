use crate::skills::types::Skill;
use crate::tool_policy::ToolPolicy;
use crate::tools::traits::Tool;
use crate::llm::provider::ToolDefinition;
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

type SkillMap = Arc<RwLock<HashMap<String, Skill>>>;
type ToolDefList = Arc<RwLock<Vec<ToolDefinition>>>;

/// Tool to load skill instructions on demand (Progressive Disclosure)
pub struct LoadSkillTool {
    skills: SkillMap,
    dormant_tools: HashMap<String, ToolDefinition>,
    active_tools: Option<ToolDefList>,
}

impl LoadSkillTool {
    pub fn new(skills: SkillMap, dormant_tools: HashMap<String, ToolDefinition>, active_tools: Option<ToolDefList>) -> Self {
        Self { skills, dormant_tools, active_tools }
    }
}

#[async_trait]
impl Tool for LoadSkillTool {
    fn name(&self) -> &'static str {
        "load_skill"
    }

    fn description(&self) -> &'static str {
        "Load detailed instructions for a specific skill and activate its associated tools. Use this when you want to understand/use a skill listed in your system prompt."
    }

    fn parameter_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "The name of the skill to load"
                }
            },
            "required": ["name"]
        }))
    }

    fn default_permission(&self) -> ToolPolicy {
        // Loading instructions is safe and read-only
        ToolPolicy::Allow
    }

    async fn execute(&self, args: Value) -> anyhow::Result<Value> {
        let name = args
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'name' argument"))?;

        // 1. Activate tool definition if it exists in dormant set
        let mut activation_status = "No associated tools to activate.";
        if let Some(tool_list) = &self.active_tools {
            if let Some(def) = self.dormant_tools.get(name) {
                let mut active = tool_list.write().unwrap();
                // Check if already active to avoid duplicates
                if !active.iter().any(|t| t.name == def.name) {
                    active.push(def.clone());
                    activation_status = "Associated tools activated and added to context.";
                } else {
                    activation_status = "Associated tools were already active.";
                }
            }
        }

        // 2. Load instructions
        let skills = self.skills.read().unwrap();
        if let Some(skill) = skills.get(name) {
            let instructions = if skill.instructions.is_empty() {
                // Determine path to SKILL.md
                let skill_file = skill.path.join("SKILL.md");
                if skill_file.exists() {
                     match std::fs::read_to_string(&skill_file) {
                        Ok(content) => content,
                        Err(e) => format!("Error reading skill file: {}", e)
                    }
                } else {
                    format!("No detailed instructions available for {}. {}", name, activation_status)
                }
            } else {
                skill.instructions.clone()
            };

            Ok(serde_json::json!({
                "name": skill.name(),
                "instructions": instructions,
                "activation_status": activation_status,
                "path": skill.path,
                "description": skill.description()
            }))
        } else {
            Err(anyhow::anyhow!("Skill '{}' not found", name))
        }
    }
}

/// Tool to list all available skills
pub struct ListSkillsTool {
    skills: SkillMap,
}

impl ListSkillsTool {
    pub fn new(skills: SkillMap) -> Self {
        Self { skills }
    }
}

#[async_trait]
impl Tool for ListSkillsTool {
    fn name(&self) -> &'static str {
        "list_skills"
    }

    fn description(&self) -> &'static str {
        "List all available skills that can be loaded. Use this to discover capabilities before loading them."
    }

    fn parameter_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        }))
    }

    fn default_permission(&self) -> ToolPolicy {
        ToolPolicy::Allow
    }

    async fn execute(&self, _args: Value) -> anyhow::Result<Value> {
        let skills = self.skills.read().unwrap();
        let mut skill_list = Vec::new();

        for skill in skills.values() {
            skill_list.push(serde_json::json!({
                "name": skill.name(),
                "description": skill.description(),
            }));
        }
        
        // Sort by name for stable output
        skill_list.sort_by(|a, b| {
            let na = a.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let nb = b.get("name").and_then(|v| v.as_str()).unwrap_or("");
            na.cmp(nb)
        });

        Ok(serde_json::json!({
            "count": skill_list.len(),
            "skills": skill_list
        }))
    }
}
