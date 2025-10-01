use anyhow::{Result, anyhow};
use reqwest::Url;
use serde_json::{Value, json};

use crate::config::constants::tools;
use crate::tool_policy::{ToolPolicy, ToolPolicyManager};

use super::ToolRegistry;

impl ToolRegistry {
    pub(super) fn sync_policy_available_tools(&mut self) {
        let mut available = self.available_tools();
        available.extend(self.mcp_policy_keys());
        available.sort();
        available.dedup();
        if let Some(ref mut pm) = self.tool_policy
            && let Err(err) = pm.update_available_tools(available)
        {
            eprintln!("Warning: Failed to update tool policies: {}", err);
        }
    }

    pub(super) fn apply_policy_constraints(&self, name: &str, mut args: Value) -> Result<Value> {
        if let Some(constraints) = self
            .tool_policy
            .as_ref()
            .and_then(|tp| tp.get_constraints(name))
            .cloned()
        {
            let obj = args
                .as_object_mut()
                .ok_or_else(|| anyhow!("Error: tool arguments must be an object"))?;

            if let Some(fmt) = constraints.default_response_format {
                obj.entry("response_format").or_insert(json!(fmt));
            }

            if let Some(allowed) = constraints.allowed_modes
                && let Some(mode) = obj.get("mode").and_then(|v| v.as_str())
                && !allowed.iter().any(|m| m == mode)
            {
                return Err(anyhow!(format!(
                    "Mode '{}' not allowed by policy for '{}'. Allowed: {}",
                    mode,
                    name,
                    allowed.join(", ")
                )));
            }

            match name {
                n if n == tools::LIST_FILES => {
                    if let Some(cap) = constraints.max_items_per_call {
                        let requested = obj
                            .get("max_items")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(cap as u64) as usize;
                        if requested > cap {
                            obj.insert("max_items".to_string(), json!(cap));
                            obj.insert(
                                "_policy_note".to_string(),
                                json!(format!("Capped max_items to {} by policy", cap)),
                            );
                        }
                    }
                }
                n if n == tools::GREP_SEARCH => {
                    if let Some(cap) = constraints.max_results_per_call {
                        let requested = obj
                            .get("max_results")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(cap as u64) as usize;
                        if requested > cap {
                            obj.insert("max_results".to_string(), json!(cap));
                            obj.insert(
                                "_policy_note".to_string(),
                                json!(format!("Capped max_results to {} by policy", cap)),
                            );
                        }
                    }
                }
                n if n == tools::READ_FILE => {
                    if let Some(cap) = constraints.max_bytes_per_read {
                        let requested = obj
                            .get("max_bytes")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(cap as u64) as usize;
                        if requested > cap {
                            obj.insert("max_bytes".to_string(), json!(cap));
                            obj.insert(
                                "_policy_note".to_string(),
                                json!(format!("Capped max_bytes to {} by policy", cap)),
                            );
                        }
                    }
                }
                n if n == tools::CURL => {
                    if let Some(cap) = constraints.max_response_bytes {
                        let requested = obj
                            .get("max_bytes")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(cap as u64) as usize;
                        if requested > cap {
                            obj.insert("max_bytes".to_string(), json!(cap));
                            obj.insert(
                                "_policy_note".to_string(),
                                json!(format!("Capped max_bytes to {} bytes by policy", cap)),
                            );
                        }
                    }

                    let url_value = obj
                        .get("url")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| anyhow!("curl tool requires a 'url' parameter"))?;

                    let parsed = Url::parse(url_value)
                        .map_err(|err| anyhow!(format!("Invalid URL '{}': {}", url_value, err)))?;

                    if let Some(allowed) = constraints.allowed_url_schemes.as_ref() {
                        let scheme = parsed.scheme();
                        if !allowed
                            .iter()
                            .any(|candidate| candidate.eq_ignore_ascii_case(scheme))
                        {
                            return Err(anyhow!(format!(
                                "Scheme '{}' is not allowed for curl tool. Allowed schemes: {}",
                                scheme,
                                allowed.join(", ")
                            )));
                        }
                    }

                    if let Some(denied_hosts) = constraints.denied_url_hosts.as_ref()
                        && let Some(host_str) = parsed.host_str()
                    {
                        let lowered = host_str.to_lowercase();
                        let blocked = denied_hosts.iter().any(|pattern| {
                            let normalized = pattern.to_lowercase();
                            if normalized.starts_with('.') {
                                lowered.ends_with(&normalized)
                            } else {
                                lowered == normalized
                                    || lowered.ends_with(&format!(".{}", normalized))
                            }
                        });
                        if blocked {
                            return Err(anyhow!(format!(
                                "URL host '{}' is blocked by policy",
                                host_str
                            )));
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(args)
    }

    pub fn policy_manager_mut(&mut self) -> Result<&mut ToolPolicyManager> {
        self.tool_policy
            .as_mut()
            .ok_or_else(|| anyhow!("Tool policy manager not available"))
    }

    pub fn policy_manager(&self) -> Result<&ToolPolicyManager> {
        self.tool_policy
            .as_ref()
            .ok_or_else(|| anyhow!("Tool policy manager not available"))
    }

    pub fn set_policy_manager(&mut self, manager: ToolPolicyManager) {
        self.tool_policy = Some(manager);
        self.sync_policy_available_tools();
    }

    pub fn set_tool_policy(&mut self, tool_name: &str, policy: ToolPolicy) -> Result<()> {
        self.tool_policy
            .as_mut()
            .expect("Tool policy manager not initialized")
            .set_policy(tool_name, policy)
    }

    pub fn get_tool_policy(&self, tool_name: &str) -> ToolPolicy {
        self.tool_policy
            .as_ref()
            .map(|tp| tp.get_policy(tool_name))
            .unwrap_or(ToolPolicy::Allow)
    }

    pub fn reset_tool_policies(&mut self) -> Result<()> {
        if let Some(tp) = self.tool_policy.as_mut() {
            tp.reset_all_to_prompt()
        } else {
            Err(anyhow!("Tool policy manager not available"))
        }
    }

    pub fn allow_all_tools(&mut self) -> Result<()> {
        if let Some(tp) = self.tool_policy.as_mut() {
            tp.allow_all_tools()
        } else {
            Err(anyhow!("Tool policy manager not available"))
        }
    }

    pub fn deny_all_tools(&mut self) -> Result<()> {
        if let Some(tp) = self.tool_policy.as_mut() {
            tp.deny_all_tools()
        } else {
            Err(anyhow!("Tool policy manager not available"))
        }
    }

    pub fn print_policy_status(&self) {
        if let Some(tp) = self.tool_policy.as_ref() {
            tp.print_status();
        } else {
            eprintln!("Tool policy manager not available");
        }
    }
}
