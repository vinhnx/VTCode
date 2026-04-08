use super::ToolRegistry;
use crate::mcp::{DetailLevel, ToolDiscovery};
use anyhow::Result;
use serde_json::{Value, json};

impl ToolRegistry {
    pub(super) async fn execute_get_errors(&self, args: Value) -> Result<Value> {
        let scope = args
            .get("scope")
            .and_then(|v| v.as_str())
            .unwrap_or("archive");
        let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(5) as usize;
        let error_patterns = crate::tools::constants::ERROR_DETECTION_PATTERNS;

        let mut error_report = json!({
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "scope": scope,
            "total_errors": 0,
            "recent_errors": Vec::<Value>::new(),
        });

        if scope == "archive" || scope == "all" {
            let sessions = crate::utils::session_archive::list_recent_sessions(limit).await?;
            let mut issues = Vec::new();
            let mut total_errors = 0usize;

            for listing in sessions {
                for message in listing.snapshot.messages {
                    if message.role == crate::llm::provider::MessageRole::Assistant {
                        let text = message.content.as_text();
                        let lower = text.to_lowercase();

                        if error_patterns.iter().any(|&pat| lower.contains(pat)) {
                            total_errors += 1;
                            issues.push(json!({
                                "type": "session_error",
                                "message": text.trim(),
                                "timestamp": listing.snapshot.ended_at.to_rfc3339(),
                            }));
                        }
                    }
                }
            }

            error_report["recent_errors"] = json!(issues);
            error_report["total_errors"] = json!(total_errors);
        }

        Ok(error_report)
    }

    pub(super) async fn execute_agent_info(&self) -> Result<Value> {
        let available_tools = self.available_tools().await;
        Ok(json!({
            "tools_registered": available_tools,
            "workspace_root": self.workspace_root_str(),
            "available_tools_count": available_tools.len(),
            "agent_type": self.agent_type,
        }))
    }

    pub(super) async fn execute_search_tools(&self, args: Value) -> Result<Value> {
        let keyword = args
            .get("keyword")
            .or_else(|| args.get("query"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let keyword_lower = (!keyword.is_empty()).then(|| keyword.to_lowercase());

        let detail_level_str = args
            .get("detail_level")
            .and_then(|v| v.as_str())
            .unwrap_or("name-and-description");
        let detail_level = match detail_level_str {
            "name-only" => DetailLevel::NameOnly,
            "full" => DetailLevel::Full,
            _ => DetailLevel::NameAndDescription,
        };

        let mut results = Vec::new();
        let available_tools = self.available_tools().await;

        for tool_name in available_tools {
            if tool_name.starts_with("mcp_") {
                continue;
            }

            let description = if let Some(reg) = self.inventory.get_registration(&tool_name) {
                reg.metadata().description().unwrap_or("").to_string()
            } else {
                "".to_string()
            };

            if matches_keyword(tool_name.as_str(), keyword_lower.as_deref())
                || matches_keyword(description.as_str(), keyword_lower.as_deref())
            {
                results.push(json!({
                    "name": tool_name,
                    "provider": "builtin",
                    "description": description,
                }));
            }
        }

        if let Some(mcp_client) = self.mcp_client() {
            let discovery = ToolDiscovery::new(mcp_client);
            if let Ok(mcp_results) = discovery.search_tools(keyword, detail_level).await {
                for r in mcp_results {
                    results.push(r.to_json(detail_level));
                }
            }
        }

        let skill_manager = self.inventory.skill_manager();
        if let Ok(skills) = skill_manager.list_skills().await {
            for skill in skills {
                if matches_keyword(skill.name.as_str(), keyword_lower.as_deref())
                    || matches_keyword(skill.description.as_str(), keyword_lower.as_deref())
                {
                    results.push(json!({
                        "name": skill.name,
                        "provider": "skill",
                        "description": skill.description,
                    }));
                }
            }
        }

        Ok(json!({ "tools": results }))
    }
}

fn matches_keyword(text: &str, keyword_lower: Option<&str>) -> bool {
    let Some(keyword_lower) = keyword_lower else {
        return true;
    };

    text.to_lowercase().contains(keyword_lower)
}
