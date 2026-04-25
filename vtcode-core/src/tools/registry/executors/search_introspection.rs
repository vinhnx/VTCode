use super::ToolRegistry;
use crate::mcp::{DetailLevel, ToolDiscovery};
use anyhow::{Result, anyhow, bail};
use hashbrown::HashMap;
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

        let detail_level = parse_detail_level(
            args.get("detail_level")
                .and_then(Value::as_str)
                .unwrap_or(""),
        );

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

    pub(super) async fn execute_mcp_search_tools(&self, args: Value) -> Result<Value> {
        let query = args
            .get("query")
            .or_else(|| args.get("keyword"))
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("query is required"))?;
        let detail_level = parse_detail_level(
            args.get("detail_level")
                .and_then(Value::as_str)
                .unwrap_or(""),
        );
        let max_results = args
            .get("limit")
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .unwrap_or(5)
            .clamp(1, 25);

        let mcp_client = self
            .mcp_client()
            .ok_or_else(|| anyhow!("MCP client not available"))?;
        let discovery = ToolDiscovery::new(mcp_client.clone());
        let mut mcp_results = discovery.search_tools(query, detail_level).await?;
        if mcp_results.len() > max_results {
            mcp_results.truncate(max_results);
        }

        let mut grouped: HashMap<String, Vec<Value>> = HashMap::new();
        let mut provider_order = Vec::new();

        let tools = mcp_results
            .iter()
            .map(|result| {
                if !grouped.contains_key(&result.provider) {
                    provider_order.push(result.provider.clone());
                }
                grouped
                    .entry(result.provider.clone())
                    .or_default()
                    .push(result.to_json(detail_level));
                result.to_json(detail_level)
            })
            .collect::<Vec<_>>();

        let by_provider = provider_order
            .into_iter()
            .map(|provider| {
                let tools = grouped.remove(&provider).unwrap_or_default();
                json!({
                    "provider": provider,
                    "tools": tools
                })
            })
            .collect::<Vec<_>>();
        let available_servers = mcp_client
            .list_servers()
            .into_iter()
            .filter(|server| server["connected"].as_bool() == Some(false))
            .collect::<Vec<_>>();

        Ok(json!({
            "query": query,
            "detail_level": detail_level.as_str(),
            "count": tools.len(),
            "tools": tools,
            "by_provider": by_provider,
            "available_servers": available_servers
        }))
    }

    pub(super) async fn execute_mcp_get_tool_details(&self, args: Value) -> Result<Value> {
        let tool_name = args
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("name is required"))?;

        let mcp_client = self
            .mcp_client()
            .ok_or_else(|| anyhow!("MCP client not available"))?;
        let discovery = ToolDiscovery::new(mcp_client);
        let detail = discovery.get_tool_detail(tool_name).await?;

        Ok(match detail {
            Some(tool) => json!({
                "found": true,
                "tool": tool.to_json(DetailLevel::Full),
            }),
            None => json!({
                "found": false,
                "tool": Value::Null,
            }),
        })
    }

    pub(super) async fn execute_mcp_list_servers(&self, _args: Value) -> Result<Value> {
        let mcp_client = self
            .mcp_client()
            .ok_or_else(|| anyhow!("MCP client not available"))?;
        let servers = mcp_client.list_servers();
        Ok(json!({
            "count": servers.len(),
            "servers": servers,
        }))
    }

    pub(super) async fn execute_mcp_connect_server(&self, args: Value) -> Result<Value> {
        let server_name = args
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("name is required"))?;
        let mcp_client = self
            .mcp_client()
            .ok_or_else(|| anyhow!("MCP client not available"))?;
        if !mcp_client.allow_model_lifecycle_control() {
            bail!(
                "mcp_connect_server is disabled by config. Set [mcp.lifecycle].allow_model_control = true to enable."
            );
        }
        mcp_client.connect_server(server_name).await?;
        self.refresh_mcp_tools().await?;
        Ok(json!({
            "connected": true,
            "name": server_name,
        }))
    }

    pub(super) async fn execute_mcp_disconnect_server(&self, args: Value) -> Result<Value> {
        let server_name = args
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("name is required"))?;
        let mcp_client = self
            .mcp_client()
            .ok_or_else(|| anyhow!("MCP client not available"))?;
        if !mcp_client.allow_model_lifecycle_control() {
            bail!(
                "mcp_disconnect_server is disabled by config. Set [mcp.lifecycle].allow_model_control = true to enable."
            );
        }
        mcp_client.disconnect_server(server_name).await?;
        self.refresh_mcp_tools().await?;
        Ok(json!({
            "disconnected": true,
            "name": server_name,
        }))
    }
}

fn matches_keyword(text: &str, keyword_lower: Option<&str>) -> bool {
    let Some(keyword_lower) = keyword_lower else {
        return true;
    };

    text.to_lowercase().contains(keyword_lower)
}

fn parse_detail_level(raw: &str) -> DetailLevel {
    match raw {
        "name" | "name-only" => DetailLevel::NameOnly,
        "name_description" | "name-and-description" => DetailLevel::NameAndDescription,
        "full" => DetailLevel::Full,
        _ => DetailLevel::NameAndDescription,
    }
}
