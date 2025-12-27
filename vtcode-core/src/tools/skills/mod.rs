use crate::llm::provider::ToolDefinition;
use crate::skills::file_references::FileReferenceValidator;
use crate::skills::types::Skill;
use crate::tool_policy::ToolPolicy;
use crate::tools::traits::Tool;
use crate::tools::registry::ToolRegistry;
use anyhow::{Context, anyhow};
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

type SkillMap = Arc<RwLock<HashMap<String, Skill>>>;
type ToolDefList = Arc<RwLock<Vec<ToolDefinition>>>;

/// Tool to load skill instructions on demand (Progressive Disclosure)
pub struct LoadSkillTool {
    skills: SkillMap,
    dormant_tools: HashMap<String, ToolDefinition>,
    dormant_adapters: Arc<RwLock<HashMap<String, Arc<dyn Tool>>>>,
    active_tools: Option<ToolDefList>,
    tool_registry: Option<Arc<RwLock<ToolRegistry>>>,
}

impl LoadSkillTool {
    pub fn new(
        skills: SkillMap,
        dormant_tools: HashMap<String, ToolDefinition>,
        dormant_adapters: Arc<RwLock<HashMap<String, Arc<dyn Tool>>>>,
        active_tools: Option<ToolDefList>,
        tool_registry: Option<Arc<RwLock<ToolRegistry>>>,
    ) -> Self {
        Self {
            skills,
            dormant_tools,
            dormant_adapters,
            active_tools,
            tool_registry,
        }
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
                let mut active = tool_list.write().await;
                // Check if already active to avoid duplicates
                if !active
                    .iter()
                    .any(|t| t.function_name() == def.function_name())
                {
                    active.push(def.clone());
                    
                    // Also register the tool in the ToolRegistry if provided
                    if let Some(registry_arc) = &self.tool_registry {
                        let mut adapters = self.dormant_adapters.write().await;
                        if let Some(adapter) = adapters.remove(name) {
                            let mut registry = registry_arc.write().await;
                            let reg = crate::tools::registry::ToolRegistration::from_tool(
                                Box::leak(name.to_string().into_boxed_str()), 
                                crate::config::types::CapabilityLevel::Basic,
                                adapter,
                            );
                            let _ = registry.register_tool(reg);
                        }
                        activation_status = "Associated tools activated and added to context.";
                    } else {
                        activation_status = "Associated tools activated and added to context.";
                    }
                } else {
                    activation_status = "Associated tools were already active.";
                }
            }
        }

        // 2. Load instructions and discover resources
        let skills = self.skills.read().await;
        if let Some(skill) = skills.get(name) {
            let instructions = if skill.instructions.is_empty() {
                // Determine path to SKILL.md
                let skill_file = skill.path.join("SKILL.md");
                if skill_file.exists() {
                    match std::fs::read_to_string(&skill_file) {
                        Ok(content) => content,
                        Err(e) => format!("Error reading skill file: {}", e),
                    }
                } else {
                    format!(
                        "No detailed instructions available for {}. {}",
                        name, activation_status
                    )
                }
            } else {
                skill.instructions.clone()
            };

            // Discover Level 3 Resources
            let validator = FileReferenceValidator::new(skill.path.clone());
            let resources = validator.list_valid_references();
            let resource_names: Vec<String> = resources
                .iter()
                .map(|p| p.to_string_lossy().to_string())
                .collect();

            Ok(serde_json::json!({
                "name": skill.name(),
                "instructions": instructions,
                "activation_status": activation_status,
                "resources": resource_names,
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
        let skills = self.skills.read().await;
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

/// Tool to load a specific resource from a skill (Level 3)
pub struct LoadSkillResourceTool {
    skills: SkillMap,
}

impl LoadSkillResourceTool {
    pub fn new(skills: SkillMap) -> Self {
        Self { skills }
    }
}

#[async_trait]
impl Tool for LoadSkillResourceTool {
    fn name(&self) -> &'static str {
        "load_skill_resource"
    }

    fn description(&self) -> &'static str {
        "Load the content of a specific resource (script, template, data) belonging to a skill. Use this when instructed by a skill's SKILL.md."
    }

    fn parameter_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": {
                "skill_name": {
                    "type": "string",
                    "description": "The name of the skill"
                },
                "resource_path": {
                    "type": "string",
                    "description": "The relative path of the resource (e.g. 'scripts/helper.py')"
                }
            },
            "required": ["skill_name", "resource_path"]
        }))
    }

    fn default_permission(&self) -> ToolPolicy {
        ToolPolicy::Allow
    }

    async fn execute(&self, args: Value) -> anyhow::Result<Value> {
        let skill_name = args
            .get("skill_name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'skill_name' argument"))?;
        let resource_path = args
            .get("resource_path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'resource_path' argument"))?;

        let skills = self.skills.read().await;
        if let Some(skill) = skills.get(skill_name) {
            // Security check: must be relative and within skill path
            let full_path = skill.path.join(resource_path);
            if !full_path.exists() {
                return Err(anyhow::anyhow!(
                    "Resource '{}' not found in skill '{}'",
                    resource_path,
                    skill_name
                ));
            }

            // Read content (limit size for safety)
            let content = std::fs::read_to_string(&full_path).context(format!(
                "Failed to read resource at {}",
                full_path.display()
            ))?;

            Ok(serde_json::json!({
                "skill_name": skill_name,
                "resource_path": resource_path,
                "content": content
            }))
        } else {
            Err(anyhow::anyhow!("Skill '{}' not found", skill_name))
        }
    }
}
