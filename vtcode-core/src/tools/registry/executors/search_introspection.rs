use super::ToolRegistry;
use crate::mcp::{DetailLevel, ToolDiscovery};
use crate::tools::mcp::model_visible_mcp_tool_name;
use crate::tools::registry::tool_catalog_facade::tool_groups;
use anyhow::{Result, anyhow, bail};
use hashbrown::HashMap;
use serde_json::{Value, json};
use std::collections::BTreeSet;

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
        let agent_type = self
            .agent_type
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        Ok(json!({
            "tools_registered": available_tools,
            "workspace_root": self.workspace_root_str(),
            "available_tools_count": available_tools.len(),
            "agent_type": agent_type,
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

        let limit = args
            .get("limit")
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .unwrap_or(10)
            .clamp(1, 25);

        let mut results = Vec::new();
        let mut matched_names: BTreeSet<String> = BTreeSet::new();
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
                matched_names.insert(tool_name.clone());
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

        // Client-side BM25-backed catalog search plus deferred-tool un-defer
        // wiring. Local search reads directly from the session's attached
        // model-facing tool list (see `ToolRegistry::attach_session_model_tools`),
        // which includes tools that were marked `defer_loading` at catalog-build
        // time and are otherwise invisible in the wire payload sent to the
        // provider. When a search hit references one of those tools by name,
        // `note_tool_references` records the reference so the *next* turn's
        // tool snapshot exposes the full definition. Headless or
        // pre-attachment contexts never call `attach_session_model_tools`, so
        // `session_model_tools()` returns `None` there and this block is
        // skipped entirely -- behavior degrades to exactly today's substring
        // search.
        let mut expanded_tools: Vec<String> = Vec::new();
        let mut by_group: Vec<Value> = Vec::new();

        if !keyword.is_empty() {
            if let Some(session_tools) = self.session_model_tools() {
                let catalog_state = self.tool_catalog_state();
                let bm25_hits = catalog_state
                    .search_tools(&session_tools, keyword, limit)
                    .await;

                let deferred_names: BTreeSet<String> = {
                    let defs = session_tools.read().await;
                    let groups = tool_groups(&defs);
                    if !groups.is_empty() {
                        by_group = groups
                            .into_iter()
                            .map(|group| {
                                json!({
                                    "name": group.name,
                                    "description": group.description,
                                    "tool_count": group.tool_count,
                                    "deferred_count": group.deferred_count,
                                })
                            })
                            .collect();
                    }
                    defs.iter()
                        .filter(|tool| tool.defer_loading == Some(true))
                        .map(|tool| tool.function_name().to_string())
                        .collect()
                };

                for hit in &bm25_hits {
                    // BM25 hit wins on a name conflict: drop any substring
                    // match already recorded for this name before pushing
                    // the catalog-ranked entry.
                    if !matched_names.insert(hit.name.clone()) {
                        results.retain(|existing| {
                            existing.get("name").and_then(Value::as_str) != Some(hit.name.as_str())
                        });
                    }
                    results.push(json!({
                        "name": hit.name,
                        "provider": "catalog",
                        "description": hit.description,
                        "score": hit.score,
                    }));
                }

                if !matched_names.is_empty() {
                    let references: Vec<String> = matched_names.iter().cloned().collect();
                    catalog_state
                        .note_tool_references(&session_tools, &references)
                        .await;
                    expanded_tools = matched_names
                        .iter()
                        .filter(|name| deferred_names.contains(name.as_str()))
                        .cloned()
                        .collect();
                }
            }
        }

        let mut response = json!({ "tools": results });
        if let Some(obj) = response.as_object_mut() {
            if !expanded_tools.is_empty() {
                obj.insert(
                    "note".to_string(),
                    json!(
                        "The listed expanded_tools were discoverable-only and \
                         are now expanded; their full definitions will be \
                         available on the next turn."
                    ),
                );
                obj.insert("expanded_tools".to_string(), json!(expanded_tools));
            }
            if !by_group.is_empty() {
                obj.insert("by_group".to_string(), json!(by_group));
            }
        }

        Ok(response)
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

        if let Some(session_tools) = self.session_model_tools() {
            let references = mcp_results
                .iter()
                .map(|result| model_visible_mcp_tool_name(&result.provider, &result.name))
                .collect::<Vec<_>>();
            self.tool_catalog_state()
                .note_tool_references(&session_tools, &references)
                .await;
        }

        let mut grouped: HashMap<String, Vec<Value>> = HashMap::new();
        let mut provider_order = Vec::new();

        let tools = mcp_results
            .iter()
            .map(|result| {
                match grouped.entry(result.provider.clone()) {
                    hashbrown::hash_map::Entry::Vacant(ve) => {
                        provider_order.push(ve.key().clone());
                        ve.insert(Vec::new()).push(result.to_json(detail_level));
                    }
                    hashbrown::hash_map::Entry::Occupied(oe) => {
                        oe.into_mut().push(result.to_json(detail_level));
                    }
                }
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
        Box::pin(mcp_client.connect_server(server_name)).await?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::provider::ToolDefinition;
    use crate::tools::registry::ToolRegistry;
    use std::path::PathBuf;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    #[tokio::test]
    async fn execute_search_tools_expands_matched_deferred_tool_via_bm25() {
        let registry = ToolRegistry::new(PathBuf::from("/tmp/test")).await;

        let deferred_tool = ToolDefinition::function(
            "grimoire_incantation".to_string(),
            "Cast a rare arcane incantation to summon a grimoire spirit".to_string(),
            serde_json::json!({}),
        )
        .with_defer_loading(true);

        let session_tools = Arc::new(RwLock::new(vec![deferred_tool]));
        registry.attach_session_model_tools(Arc::clone(&session_tools));

        let catalog_state = registry.tool_catalog_state();
        let epoch_before = catalog_state.current_epoch();

        let result = registry
            .execute_search_tools(json!({ "keyword": "grimoire incantation" }))
            .await
            .expect("execute_search_tools should succeed");

        let expanded = result
            .get("expanded_tools")
            .and_then(Value::as_array)
            .expect("expanded_tools should be present in the result");
        assert!(
            expanded
                .iter()
                .any(|value| value.as_str() == Some("grimoire_incantation")),
            "expected grimoire_incantation in expanded_tools, got {expanded:?}"
        );
        assert!(
            result.get("note").is_some(),
            "a note explaining the expansion should be present"
        );

        let epoch_after = catalog_state.current_epoch();
        assert!(
            epoch_after > epoch_before,
            "catalog state should record the new tool expansion by bumping its epoch"
        );
    }

    #[tokio::test]
    async fn execute_search_tools_undefer_round_trip_passes_client_local_wire_filter() {
        // Exercises the full round trip that client-local deferral
        // (`client_tool_search`) depends on: a deferred tool is omitted from
        // the wire payload until the local search index surfaces it, at
        // which point `note_tool_references` bumps the catalog epoch and the
        // *next* snapshot un-defers it (`defer_loading` cleared to `None`).
        // The request builder's wire filter (`client_local_wire_tools` in
        // `src/agent/runloop/unified/turn/turn_processing/llm_request/request_builder.rs`)
        // only drops tools where `defer_loading == Some(true)`, so once this
        // snapshot clears the flag the tool passes through unfiltered.
        let registry = ToolRegistry::new(PathBuf::from("/tmp/test")).await;

        let deferred_tool = ToolDefinition::function(
            "grimoire_incantation".to_string(),
            "Cast a rare arcane incantation to summon a grimoire spirit".to_string(),
            serde_json::json!({}),
        )
        .with_defer_loading(true);

        let session_tools = Arc::new(RwLock::new(vec![deferred_tool]));
        registry.attach_session_model_tools(Arc::clone(&session_tools));
        let catalog_state = registry.tool_catalog_state();

        // Before the search hit, the client-local wire filter would drop
        // this tool: `defer_loading == Some(true)`.
        {
            let defs = session_tools.read().await;
            assert_eq!(defs[0].defer_loading, Some(true));
        }

        registry
            .execute_search_tools(json!({ "keyword": "grimoire incantation" }))
            .await
            .expect("execute_search_tools should succeed");

        // The next turn's snapshot un-defers the tool the search referenced.
        let snapshot = catalog_state
            .filtered_snapshot_with_stats(&session_tools, false, false)
            .await;
        let defs = snapshot
            .snapshot
            .as_deref()
            .expect("snapshot should contain tool definitions");
        let expanded = defs
            .iter()
            .find(|tool| tool.function_name() == "grimoire_incantation")
            .expect("expanded tool should still be present in the snapshot");

        // `defer_loading != Some(true)` is exactly the predicate the
        // client-local wire filter keeps; this tool now satisfies it.
        assert_ne!(expanded.defer_loading, Some(true));
    }

    #[tokio::test]
    async fn execute_search_tools_degrades_gracefully_without_attached_session_tools() {
        let registry = ToolRegistry::new(PathBuf::from("/tmp/test")).await;

        let result = registry
            .execute_search_tools(json!({ "keyword": "nonexistent_keyword_zzz" }))
            .await
            .expect("execute_search_tools should succeed even without session tools");

        assert!(result.get("expanded_tools").is_none());
        assert!(result.get("by_group").is_none());
        assert!(result.get("note").is_none());
        assert!(result.get("tools").is_some());
    }
}
