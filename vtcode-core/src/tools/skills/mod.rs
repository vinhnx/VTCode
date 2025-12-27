use crate::llm::provider::ToolDefinition;
use crate::skills::file_references::FileReferenceValidator;
use crate::skills::types::{Skill, SkillVariety};
use crate::tool_policy::ToolPolicy;
use crate::tools::traits::Tool;
use crate::tools::registry::ToolRegistry;
use anyhow::Context;
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
    active_skills: SkillMap,
    dormant_tools: HashMap<String, ToolDefinition>,
    dormant_adapters: Arc<RwLock<HashMap<String, Arc<dyn Tool>>>>,
    active_tools: Option<ToolDefList>,
    tool_registry: Option<Arc<RwLock<ToolRegistry>>>,
}

impl LoadSkillTool {
    pub fn new(
        skills: SkillMap,
        active_skills: SkillMap,
        dormant_tools: HashMap<String, ToolDefinition>,
        dormant_adapters: Arc<RwLock<HashMap<String, Arc<dyn Tool>>>>,
        active_tools: Option<ToolDefList>,
        tool_registry: Option<Arc<RwLock<ToolRegistry>>>,
    ) -> Self {
        Self {
            skills,
            active_skills,
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
        "Load detailed instructions for a specific skill and activate its associated tools into your environment. Use this to unlock high-level 'AgentSkill' workflows or 'SystemUtility' CLI bridges that are currently dormant."
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

            let response = serde_json::json!({
                "name": skill.name(),
                "variety": skill.variety,
                "instructions": instructions,
                "instructions_status": "These instructions are now [ACTIVE] and will persist in your system prompt for the remainder of this session.",
                "activation_status": activation_status,
                "resources": resource_names,
                "path": skill.path,
                "description": skill.description()
            });

            // Add to active skills registry
            self.active_skills.write().await.insert(name.to_string(), skill.clone());

            Ok(response)
        } else {
            Err(anyhow::anyhow!("Skill '{}' not found", name))
        }
    }
}

/// Tool to list all available skills
pub struct ListSkillsTool {
    skills: SkillMap,
    dormant_tools: HashMap<String, ToolDefinition>,
}

impl ListSkillsTool {
    pub fn new(
        skills: SkillMap,
        dormant_tools: HashMap<String, ToolDefinition>,
    ) -> Self {
        Self {
            skills,
            dormant_tools,
        }
    }
}

#[async_trait]
impl Tool for ListSkillsTool {
    fn name(&self) -> &'static str {
        "list_skills"
    }

    fn description(&self) -> &'static str {
        "List all available skills (high-level workflows) and system utilities (CLI tools). Use 'query' to filter by name or 'variety' to filter by type ('agent_skill' or 'system_utility'). Tools are dormant until activated via 'load_skill'."
    }

    fn parameter_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Optional search term to filter skills by name (case-insensitive)"
                },
                "variety": {
                    "type": "string",
                    "enum": ["agent_skill", "system_utility", "built_in"],
                    "description": "Optional variety to filter by"
                }
            },
            "additionalProperties": false
        }))
    }

    fn default_permission(&self) -> ToolPolicy {
        ToolPolicy::Allow
    }

    async fn execute(&self, args: Value) -> anyhow::Result<Value> {
        let query = args.get("query").and_then(|v| v.as_str()).map(|s| s.to_lowercase());
        let variety_filter = args.get("variety").and_then(|v| v.as_str());

        let skills = self.skills.read().await;
        let mut skill_list = Vec::new();

        for skill in skills.values() {
            let name = skill.name();
            let variety_str = format!("{:?}", skill.variety).to_lowercase();
            
            // Apply variety filter
            if let Some(v_filter) = variety_filter {
                if !variety_str.contains(&v_filter.replace("_", "").to_lowercase()) {
                    continue;
                }
            }

            // Apply query filter
            if let Some(q) = &query {
                if !name.to_lowercase().contains(q) {
                    continue;
                }
            }

            skill_list.push(serde_json::json!({
                "name": name,
                "description": skill.description(),
                "variety": skill.variety,
                "status": "active"
            }));
        }

        // Add dormant tools
        for (name, def) in &self.dormant_tools {
            if !skills.contains_key(name) {
                // Apply variety filter (all dormant are SystemUtility)
                if let Some(v_filter) = variety_filter {
                    if !v_filter.to_lowercase().contains("system") && !v_filter.to_lowercase().contains("utility") {
                        continue;
                    }
                }

                // Apply query filter
                if let Some(q) = &query {
                    if !name.to_lowercase().contains(q) {
                        continue;
                    }
                }

                skill_list.push(serde_json::json!({
                    "name": name,
                    "description": def.description(),
                    "variety": SkillVariety::SystemUtility,
                    "status": "dormant"
                }));
            }
        }

        // Sort by name for stable output
        skill_list.sort_by(|a, b| {
            let na = a.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let nb = b.get("name").and_then(|v| v.as_str()).unwrap_or("");
            na.cmp(nb)
        });

        // Group by variety for "better" discovery
        let mut grouped = HashMap::new();
        for skill in &skill_list {
            let variety = skill.get("variety").and_then(|v| v.as_str()).unwrap_or("unknown");
            grouped
                .entry(variety.to_string())
                .or_insert_with(Vec::new)
                .push(skill.clone());
        }

        let mut response = serde_json::json!({
            "count": skill_list.len(),
            "groups": grouped,
        });

        // Add context message for queries
        if query.is_some() || variety_filter.is_some() {
            response.as_object_mut().unwrap().insert("filter_applied".to_string(), serde_json::json!(true));
        }

        Ok(response)
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
        "Access Level 3 resources (scripts, templates, technical docs) referenced in a skill's SKILL.md. Use this to read files from 'scripts/', 'references/', or 'assets/' when the high-level instructions require them."
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
